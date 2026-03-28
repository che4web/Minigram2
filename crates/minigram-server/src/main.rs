use std::{net::SocketAddr, sync::Arc};

use anyhow::Context;
use chrono::Utc;
use clap::Parser;
use minigram_proto::minigram::{
    sync_service_server::{SyncService, SyncServiceServer},
    Message as ProtoMessage, PullMessagesRequest, PullMessagesResponse, PushMessagesRequest,
    PushMessagesResponse,
};
use tokio::sync::Mutex;
use tokio_postgres::{types::ToSql, Client, NoTls};
use tonic::{transport::Server, Request, Response, Status};
use tracing::{info, warn};

#[derive(Debug, Parser)]
struct Args {
    #[arg(long, default_value = "127.0.0.1:50051")]
    listen: SocketAddr,
    #[arg(
        long,
        default_value = "postgres://postgres:postgres@127.0.0.1:5432/minigram"
    )]
    postgres_url: String,
    #[arg(long, default_value = "nats://127.0.0.1:4222")]
    nats_url: String,
    #[arg(long, default_value = "minigram.messages")]
    nats_subject: String,
}

#[derive(Clone)]
struct SyncServiceImpl {
    db: Arc<Mutex<Client>>,
    nats: Option<async_nats::Client>,
    nats_subject: String,
}

#[tonic::async_trait]
impl SyncService for SyncServiceImpl {
    async fn push_messages(
        &self,
        request: Request<PushMessagesRequest>,
    ) -> Result<Response<PushMessagesResponse>, Status> {
        let input = request.into_inner();
        let accepted = input.messages.len() as u32;

        let db = self.db.lock().await;
        for msg in input.messages {
            validate_message(&msg)?;
            db.execute(
                r#"
                INSERT INTO messages (id, chat_id, author, text, created_at, updated_at, deleted)
                VALUES ($1, $2, $3, $4, $5, $6, $7)
                ON CONFLICT (id) DO UPDATE SET
                    chat_id = EXCLUDED.chat_id,
                    author = EXCLUDED.author,
                    text = EXCLUDED.text,
                    created_at = EXCLUDED.created_at,
                    updated_at = EXCLUDED.updated_at,
                    deleted = EXCLUDED.deleted
                WHERE messages.updated_at < EXCLUDED.updated_at
                "#,
                &[
                    &msg.id as &(dyn ToSql + Sync),
                    &msg.chat_id,
                    &msg.author,
                    &msg.text,
                    &msg.created_at,
                    &msg.updated_at,
                    &msg.deleted,
                ],
            )
            .await
            .map_err(|err| Status::internal(format!("db write error: {err}")))?;

            publish_message_notification(&self.nats, &self.nats_subject, &msg).await;
        }

        Ok(Response::new(PushMessagesResponse { accepted }))
    }

    async fn pull_messages(
        &self,
        request: Request<PullMessagesRequest>,
    ) -> Result<Response<PullMessagesResponse>, Status> {
        let since_timestamp = request.into_inner().since_timestamp;
        let db = self.db.lock().await;

        let rows = db
            .query(
                r#"
                SELECT id, chat_id, author, text, created_at, updated_at, deleted
                FROM messages
                WHERE updated_at > $1
                ORDER BY updated_at ASC
                "#,
                &[&since_timestamp],
            )
            .await
            .map_err(|err| Status::internal(format!("db read error: {err}")))?;

        let messages = rows
            .into_iter()
            .map(|row| ProtoMessage {
                id: row.get::<_, String>(0),
                chat_id: row.get::<_, String>(1),
                author: row.get::<_, String>(2),
                text: row.get::<_, String>(3),
                created_at: row.get::<_, i64>(4),
                updated_at: row.get::<_, i64>(5),
                deleted: row.get::<_, bool>(6),
            })
            .collect();

        Ok(Response::new(PullMessagesResponse {
            messages,
            server_timestamp: Utc::now().timestamp(),
        }))
    }
}

async fn publish_message_notification(
    nats: &Option<async_nats::Client>,
    subject: &str,
    msg: &ProtoMessage,
) {
    let Some(client) = nats else {
        return;
    };

    let payload = serde_json::json!({
        "id": msg.id,
        "chat_id": msg.chat_id,
        "author": msg.author,
        "text": msg.text,
        "created_at": msg.created_at,
        "updated_at": msg.updated_at,
        "deleted": msg.deleted,
        "event": "message_upserted"
    })
    .to_string();

    if let Err(err) = client.publish(subject.to_string(), payload.into()).await {
        warn!("failed to publish NATS notification: {err}");
    }
}

fn validate_message(msg: &ProtoMessage) -> Result<(), Status> {
    if msg.id.trim().is_empty() {
        return Err(Status::invalid_argument("message id is required"));
    }
    if msg.chat_id.trim().is_empty() {
        return Err(Status::invalid_argument("chat_id is required"));
    }
    if msg.author.trim().is_empty() {
        return Err(Status::invalid_argument("author is required"));
    }
    Ok(())
}

async fn connect_db(postgres_url: &str) -> anyhow::Result<Client> {
    let (client, connection) = tokio_postgres::connect(postgres_url, NoTls)
        .await
        .with_context(|| format!("failed to connect to postgres at {postgres_url}"))?;

    tokio::spawn(async move {
        if let Err(err) = connection.await {
            eprintln!("postgres connection error: {err}");
        }
    });

    client
        .batch_execute(
            r#"
            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                chat_id TEXT NOT NULL,
                author TEXT NOT NULL,
                text TEXT NOT NULL,
                created_at BIGINT NOT NULL,
                updated_at BIGINT NOT NULL,
                deleted BOOLEAN NOT NULL DEFAULT FALSE
            );
            CREATE INDEX IF NOT EXISTS idx_messages_updated_at ON messages(updated_at);
            "#,
        )
        .await
        .context("failed to initialize postgres schema")?;

    Ok(client)
}

async fn connect_nats(url: &str) -> Option<async_nats::Client> {
    match async_nats::connect(url).await {
        Ok(client) => {
            info!("connected to nats at {url}");
            Some(client)
        }
        Err(err) => {
            warn!("failed to connect to nats at {url}: {err}");
            None
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(false)
        .init();

    let args = Args::parse();
    let db = connect_db(&args.postgres_url).await?;
    let nats = connect_nats(&args.nats_url).await;

    let service = SyncServiceImpl {
        db: Arc::new(Mutex::new(db)),
        nats,
        nats_subject: args.nats_subject,
    };

    info!("minigram server listening on {}", args.listen);
    Server::builder()
        .add_service(SyncServiceServer::new(service))
        .serve(args.listen)
        .await?;

    Ok(())
}
