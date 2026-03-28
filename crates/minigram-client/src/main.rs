use std::path::PathBuf;

use anyhow::Context;
use chrono::Utc;
use clap::{Parser, Subcommand};
use minigram_proto::minigram::{
    sync_service_client::SyncServiceClient, Message as ProtoMessage, PullMessagesRequest,
    PushMessagesRequest,
};
use rusqlite::{params, Connection, OptionalExtension};
use tonic::transport::Channel;
use tracing::info;
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(about = "Minigram gRPC client with SQLite local storage + sync")]
struct Args {
    #[arg(long, default_value = "http://127.0.0.1:50051")]
    server: String,
    #[arg(long, default_value = "client_store.db")]
    db: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Send {
        #[arg(long)]
        chat: String,
        #[arg(long)]
        author: String,
        #[arg(long)]
        text: String,
    },
    List {
        #[arg(long)]
        chat: Option<String>,
    },
    Sync,
}

#[derive(Clone, Debug)]
struct MessageRecord {
    id: String,
    chat_id: String,
    author: String,
    text: String,
    created_at: i64,
    updated_at: i64,
    deleted: bool,
}

struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    fn open(path: &PathBuf) -> anyhow::Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("failed to open sqlite db {}", path.display()))?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> anyhow::Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                chat_id TEXT NOT NULL,
                author TEXT NOT NULL,
                text TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                deleted INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS pending_uploads (
                message_id TEXT PRIMARY KEY,
                created_at INTEGER NOT NULL,
                FOREIGN KEY(message_id) REFERENCES messages(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS sync_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            "#,
        )?;
        Ok(())
    }

    fn add_local_message(&self, chat: String, author: String, text: String) -> anyhow::Result<()> {
        let now = Utc::now().timestamp();
        let id = Uuid::new_v4().to_string();
        let tx = self.conn.unchecked_transaction()?;

        tx.execute(
            "INSERT INTO messages (id, chat_id, author, text, created_at, updated_at, deleted) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)",
            params![id, chat, author, text, now, now],
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO pending_uploads (message_id, created_at) VALUES (?1, ?2)",
            params![id, now],
        )?;

        tx.commit()?;
        Ok(())
    }

    fn list_messages(&self, chat: Option<&str>) -> anyhow::Result<Vec<MessageRecord>> {
        let sql = if chat.is_some() {
            "SELECT id, chat_id, author, text, created_at, updated_at, deleted FROM messages WHERE chat_id = ?1 ORDER BY created_at ASC"
        } else {
            "SELECT id, chat_id, author, text, created_at, updated_at, deleted FROM messages ORDER BY created_at ASC"
        };

        let mut stmt = self.conn.prepare(sql)?;
        let rows = if let Some(chat_id) = chat {
            stmt.query_map(params![chat_id], row_to_message)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            stmt.query_map([], row_to_message)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };

        Ok(rows)
    }

    fn pending_messages(&self) -> anyhow::Result<Vec<MessageRecord>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT m.id, m.chat_id, m.author, m.text, m.created_at, m.updated_at, m.deleted
            FROM pending_uploads p
            JOIN messages m ON m.id = p.message_id
            ORDER BY p.created_at ASC
            "#,
        )?;

        let rows = stmt
            .query_map([], row_to_message)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    fn clear_pending(&self, message_ids: &[String]) -> anyhow::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for id in message_ids {
            tx.execute(
                "DELETE FROM pending_uploads WHERE message_id = ?1",
                params![id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn upsert_message(&self, msg: MessageRecord) -> anyhow::Result<()> {
        let existing_updated: Option<i64> = self
            .conn
            .query_row(
                "SELECT updated_at FROM messages WHERE id = ?1",
                params![msg.id],
                |row| row.get(0),
            )
            .optional()?;

        if existing_updated.is_some_and(|local_ts| local_ts > msg.updated_at) {
            return Ok(());
        }

        self.conn.execute(
            r#"
            INSERT INTO messages (id, chat_id, author, text, created_at, updated_at, deleted)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(id) DO UPDATE SET
                chat_id = excluded.chat_id,
                author = excluded.author,
                text = excluded.text,
                created_at = excluded.created_at,
                updated_at = excluded.updated_at,
                deleted = excluded.deleted
            "#,
            params![
                msg.id,
                msg.chat_id,
                msg.author,
                msg.text,
                msg.created_at,
                msg.updated_at,
                msg.deleted as i64
            ],
        )?;

        Ok(())
    }

    fn pending_count(&self) -> anyhow::Result<i64> {
        let count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM pending_uploads", [], |row| row.get(0))?;
        Ok(count)
    }

    fn last_sync_timestamp(&self) -> anyhow::Result<i64> {
        let val: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM sync_meta WHERE key = 'last_sync_timestamp'",
                [],
                |row| row.get(0),
            )
            .optional()?;

        Ok(val.as_deref().unwrap_or("0").parse::<i64>().unwrap_or(0))
    }

    fn set_last_sync_timestamp(&self, ts: i64) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT INTO sync_meta (key, value) VALUES ('last_sync_timestamp', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![ts.to_string()],
        )?;
        Ok(())
    }
}

fn row_to_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<MessageRecord> {
    Ok(MessageRecord {
        id: row.get(0)?,
        chat_id: row.get(1)?,
        author: row.get(2)?,
        text: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        deleted: row.get::<_, i64>(6)? != 0,
    })
}

fn proto_to_record(msg: ProtoMessage) -> MessageRecord {
    MessageRecord {
        id: msg.id,
        chat_id: msg.chat_id,
        author: msg.author,
        text: msg.text,
        created_at: msg.created_at,
        updated_at: msg.updated_at,
        deleted: msg.deleted,
    }
}

fn record_to_proto(msg: &MessageRecord) -> ProtoMessage {
    ProtoMessage {
        id: msg.id.clone(),
        chat_id: msg.chat_id.clone(),
        author: msg.author.clone(),
        text: msg.text.clone(),
        created_at: msg.created_at,
        updated_at: msg.updated_at,
        deleted: msg.deleted,
    }
}

async fn connect(server: &str) -> anyhow::Result<SyncServiceClient<Channel>> {
    let client = SyncServiceClient::connect(server.to_string())
        .await
        .with_context(|| format!("failed to connect to {server}"))?;
    Ok(client)
}

async fn run_sync(
    store: &SqliteStore,
    client: &mut SyncServiceClient<Channel>,
) -> anyhow::Result<()> {
    let pending = store.pending_messages()?;
    if !pending.is_empty() {
        let ids: Vec<String> = pending.iter().map(|m| m.id.clone()).collect();
        let push_batch: Vec<_> = pending.iter().map(record_to_proto).collect();
        let pushed = push_batch.len();

        let response = client
            .push_messages(PushMessagesRequest {
                messages: push_batch,
            })
            .await?
            .into_inner();

        info!(
            "uploaded {} messages (accepted={})",
            pushed, response.accepted
        );
        store.clear_pending(&ids)?;
    }

    let since_ts = store.last_sync_timestamp()?;
    let pulled = client
        .pull_messages(PullMessagesRequest {
            since_timestamp: since_ts,
        })
        .await?
        .into_inner();

    for msg in pulled.messages {
        store.upsert_message(proto_to_record(msg))?;
    }
    store.set_last_sync_timestamp(pulled.server_timestamp)?;

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(false)
        .init();

    let args = Args::parse();
    let store = SqliteStore::open(&args.db)?;

    match args.command {
        Command::Send { chat, author, text } => {
            store.add_local_message(chat, author, text)?;
            println!("Message stored locally in SQLite and queued for sync.");
        }
        Command::List { chat } => {
            let messages = store.list_messages(chat.as_deref())?;
            for msg in messages {
                println!(
                    "[{}] {} {}: {}",
                    msg.chat_id, msg.created_at, msg.author, msg.text
                );
            }
            println!("pending_uploads={}", store.pending_count()?);
            println!("last_sync_timestamp={}", store.last_sync_timestamp()?);
        }
        Command::Sync => {
            let mut client = connect(&args.server).await?;
            run_sync(&store, &mut client).await?;
            println!("Sync complete.");
        }
    }

    Ok(())
}
