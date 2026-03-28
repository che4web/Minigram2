use std::path::Path;

use anyhow::Context;
use chrono::Utc;
use minigram_proto::minigram::{
    sync_service_client::SyncServiceClient, Attachment as ProtoAttachment, Message as ProtoMessage,
    PullMessagesRequest, PushMessagesRequest,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use tonic::{metadata::MetadataValue, transport::Channel, Request};
use tracing::info;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize)]
pub struct AttachmentRecord {
    pub id: String,
    pub kind: String,
    pub file_name: String,
    pub mime_type: String,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MessageRecord {
    pub id: String,
    pub chat_id: String,
    pub author: String,
    pub text: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub deleted: bool,
    pub attachments: Vec<AttachmentRecord>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SyncStats {
    pub pushed: usize,
    pub pulled: usize,
    pub server_timestamp: i64,
}

pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
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

            CREATE TABLE IF NOT EXISTS message_attachments (
                id TEXT PRIMARY KEY,
                message_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                file_name TEXT NOT NULL,
                mime_type TEXT NOT NULL,
                data BLOB NOT NULL,
                position INTEGER NOT NULL,
                FOREIGN KEY(message_id) REFERENCES messages(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_message_attachments_message_id
              ON message_attachments(message_id, position);

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

    pub fn add_local_message(
        &self,
        chat: String,
        author: String,
        text: String,
    ) -> anyhow::Result<()> {
        self.add_local_message_with_attachments(chat, author, text, Vec::new())
    }

    pub fn add_local_message_with_attachments(
        &self,
        chat: String,
        author: String,
        text: String,
        attachments: Vec<AttachmentRecord>,
    ) -> anyhow::Result<()> {
        let now = Utc::now().timestamp();
        let id = Uuid::new_v4().to_string();
        let tx = self.conn.unchecked_transaction()?;

        tx.execute(
            "INSERT INTO messages (id, chat_id, author, text, created_at, updated_at, deleted) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)",
            params![id, chat, author, text, now, now],
        )?;

        for (position, att) in attachments.into_iter().enumerate() {
            tx.execute(
                "INSERT INTO message_attachments (id, message_id, kind, file_name, mime_type, data, position) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    att.id,
                    id,
                    att.kind,
                    att.file_name,
                    att.mime_type,
                    att.data,
                    position as i64
                ],
            )?;
        }

        tx.execute(
            "INSERT OR REPLACE INTO pending_uploads (message_id, created_at) VALUES (?1, ?2)",
            params![id, now],
        )?;

        tx.commit()?;
        Ok(())
    }

    pub fn list_messages(&self, chat: Option<&str>) -> anyhow::Result<Vec<MessageRecord>> {
        let sql = if chat.is_some() {
            "SELECT id, chat_id, author, text, created_at, updated_at, deleted FROM messages WHERE chat_id = ?1 ORDER BY created_at ASC"
        } else {
            "SELECT id, chat_id, author, text, created_at, updated_at, deleted FROM messages ORDER BY created_at ASC"
        };

        let mut stmt = self.conn.prepare(sql)?;
        let mut messages = if let Some(chat_id) = chat {
            stmt.query_map(params![chat_id], row_to_message_without_attachments)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            stmt.query_map([], row_to_message_without_attachments)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };

        for msg in &mut messages {
            msg.attachments = self.attachments_for_message(&msg.id)?;
        }

        Ok(messages)
    }

    pub fn pending_messages(&self) -> anyhow::Result<Vec<MessageRecord>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT m.id, m.chat_id, m.author, m.text, m.created_at, m.updated_at, m.deleted
            FROM pending_uploads p
            JOIN messages m ON m.id = p.message_id
            ORDER BY p.created_at ASC
            "#,
        )?;

        let mut messages = stmt
            .query_map([], row_to_message_without_attachments)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        for msg in &mut messages {
            msg.attachments = self.attachments_for_message(&msg.id)?;
        }

        Ok(messages)
    }

    pub fn clear_pending(&self, message_ids: &[String]) -> anyhow::Result<()> {
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

    pub fn upsert_message(&self, msg: MessageRecord) -> anyhow::Result<()> {
        let message_id = msg.id.clone();
        let existing_updated: Option<i64> = self
            .conn
            .query_row(
                "SELECT updated_at FROM messages WHERE id = ?1",
                params![&message_id],
                |row| row.get(0),
            )
            .optional()?;

        if existing_updated.is_some_and(|local_ts| local_ts > msg.updated_at) {
            return Ok(());
        }

        let tx = self.conn.unchecked_transaction()?;

        tx.execute(
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

        tx.execute(
            "DELETE FROM message_attachments WHERE message_id = ?1",
            params![&message_id],
        )?;

        for (position, att) in msg.attachments.into_iter().enumerate() {
            tx.execute(
                "INSERT INTO message_attachments (id, message_id, kind, file_name, mime_type, data, position) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    att.id,
                    &message_id,
                    att.kind,
                    att.file_name,
                    att.mime_type,
                    att.data,
                    position as i64
                ],
            )?;
        }

        tx.commit()?;

        Ok(())
    }

    pub fn pending_count(&self) -> anyhow::Result<i64> {
        let count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM pending_uploads", [], |row| row.get(0))?;
        Ok(count)
    }

    pub fn last_sync_timestamp(&self) -> anyhow::Result<i64> {
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

    pub fn set_last_sync_timestamp(&self, ts: i64) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT INTO sync_meta (key, value) VALUES ('last_sync_timestamp', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![ts.to_string()],
        )?;
        Ok(())
    }

    fn attachments_for_message(&self, message_id: &str) -> anyhow::Result<Vec<AttachmentRecord>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, kind, file_name, mime_type, data
            FROM message_attachments
            WHERE message_id = ?1
            ORDER BY position ASC
            "#,
        )?;

        let attachments = stmt
            .query_map(params![message_id], |row| {
                Ok(AttachmentRecord {
                    id: row.get(0)?,
                    kind: row.get(1)?,
                    file_name: row.get(2)?,
                    mime_type: row.get(3)?,
                    data: row.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(attachments)
    }
}

fn row_to_message_without_attachments(row: &rusqlite::Row<'_>) -> rusqlite::Result<MessageRecord> {
    Ok(MessageRecord {
        id: row.get(0)?,
        chat_id: row.get(1)?,
        author: row.get(2)?,
        text: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        deleted: row.get::<_, i64>(6)? != 0,
        attachments: Vec::new(),
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
        attachments: msg
            .attachments
            .into_iter()
            .map(proto_attachment_to_record)
            .collect(),
    }
}

fn proto_attachment_to_record(att: ProtoAttachment) -> AttachmentRecord {
    AttachmentRecord {
        id: att.id,
        kind: att.kind,
        file_name: att.file_name,
        mime_type: att.mime_type,
        data: att.data,
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
        attachments: msg
            .attachments
            .iter()
            .map(record_attachment_to_proto)
            .collect(),
    }
}

fn record_attachment_to_proto(att: &AttachmentRecord) -> ProtoAttachment {
    ProtoAttachment {
        id: att.id.clone(),
        kind: att.kind.clone(),
        file_name: att.file_name.clone(),
        mime_type: att.mime_type.clone(),
        data: att.data.clone(),
    }
}

pub async fn connect(server: &str) -> anyhow::Result<SyncServiceClient<Channel>> {
    let client = SyncServiceClient::connect(server.to_string())
        .await
        .with_context(|| format!("failed to connect to {server}"))?;
    Ok(client)
}

fn with_auth<T>(request: &mut Request<T>, jwt_token: Option<&str>) -> anyhow::Result<()> {
    if let Some(token) = jwt_token {
        let header = format!("Bearer {token}");
        let metadata = MetadataValue::try_from(header)
            .context("failed to construct authorization metadata")?;
        request.metadata_mut().insert("authorization", metadata);
    }
    Ok(())
}

pub async fn run_sync(
    store: &SqliteStore,
    client: &mut SyncServiceClient<Channel>,
    jwt_token: Option<&str>,
) -> anyhow::Result<SyncStats> {
    let pending = store.pending_messages()?;
    let mut pushed = 0usize;

    if !pending.is_empty() {
        let ids: Vec<String> = pending.iter().map(|m| m.id.clone()).collect();
        let push_batch: Vec<_> = pending.iter().map(record_to_proto).collect();
        pushed = push_batch.len();

        let mut request = Request::new(PushMessagesRequest {
            messages: push_batch,
        });
        with_auth(&mut request, jwt_token)?;

        let response = client.push_messages(request).await?.into_inner();

        info!(
            "uploaded {} messages (accepted={})",
            pushed, response.accepted
        );
        store.clear_pending(&ids)?;
    }

    let since_ts = store.last_sync_timestamp()?;
    let mut request = Request::new(PullMessagesRequest {
        since_timestamp: since_ts,
    });
    with_auth(&mut request, jwt_token)?;

    let pulled = client.pull_messages(request).await?.into_inner();

    let pulled_count = pulled.messages.len();
    for msg in pulled.messages {
        store.upsert_message(proto_to_record(msg))?;
    }
    store.set_last_sync_timestamp(pulled.server_timestamp)?;

    Ok(SyncStats {
        pushed,
        pulled: pulled_count,
        server_timestamp: pulled.server_timestamp,
    })
}

pub async fn run_sync_db(
    path: impl AsRef<Path>,
    client: &mut SyncServiceClient<Channel>,
    jwt_token: Option<&str>,
) -> anyhow::Result<SyncStats> {
    let pending = {
        let store = SqliteStore::open(path.as_ref())?;
        store.pending_messages()?
    };
    let since_ts = {
        let store = SqliteStore::open(path.as_ref())?;
        store.last_sync_timestamp()?
    };
    let mut pushed = 0usize;

    if !pending.is_empty() {
        let ids: Vec<String> = pending.iter().map(|m| m.id.clone()).collect();
        let push_batch: Vec<_> = pending.iter().map(record_to_proto).collect();
        pushed = push_batch.len();

        let mut request = Request::new(PushMessagesRequest {
            messages: push_batch,
        });
        with_auth(&mut request, jwt_token)?;

        let response = client.push_messages(request).await?.into_inner();

        info!(
            "uploaded {} messages (accepted={})",
            pushed, response.accepted
        );

        let store = SqliteStore::open(path.as_ref())?;
        store.clear_pending(&ids)?;
    }

    let mut request = Request::new(PullMessagesRequest {
        since_timestamp: since_ts,
    });
    with_auth(&mut request, jwt_token)?;

    let pulled = client.pull_messages(request).await?.into_inner();
    let pulled_count = pulled.messages.len();
    let server_timestamp = pulled.server_timestamp;

    let store = SqliteStore::open(path.as_ref())?;
    for msg in pulled.messages {
        store.upsert_message(proto_to_record(msg))?;
    }
    store.set_last_sync_timestamp(server_timestamp)?;

    Ok(SyncStats {
        pushed,
        pulled: pulled_count,
        server_timestamp,
    })
}
