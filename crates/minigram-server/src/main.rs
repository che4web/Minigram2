use std::{net::SocketAddr, sync::Arc};

use anyhow::Context;
use chrono::Utc;
use clap::Parser;
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use minigram_proto::minigram::{
    sync_service_server::{SyncService, SyncServiceServer},
    Attachment as ProtoAttachment, Message as ProtoMessage, PullMessagesRequest,
    PullMessagesResponse, PushMessagesRequest, PushMessagesResponse,
};
use serde::Deserialize;
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
    #[arg(long, default_value = "minigram-dev-secret")]
    jwt_secret: String,
}

#[derive(Debug, Clone, Deserialize)]
struct JwtClaims {
    sub: String,
    exp: usize,
}

#[derive(Clone)]
struct SyncServiceImpl {
    db: Arc<Mutex<Client>>,
    nats: Option<async_nats::Client>,
    nats_subject: String,
    jwt_secret: Arc<String>,
}

#[tonic::async_trait]
impl SyncService for SyncServiceImpl {
    async fn push_messages(
        &self,
        request: Request<PushMessagesRequest>,
    ) -> Result<Response<PushMessagesResponse>, Status> {
        let claims = authorize_request(&request, &self.jwt_secret)?;
        info!(subject = %claims.sub, expires_at = claims.exp, "authorized push_messages");

        let input = request.into_inner();
        let accepted = input.messages.len() as u32;

        let db = self.db.lock().await;
        for msg in input.messages {
            validate_message(&msg)?;
            upsert_message_with_attachments(&db, &msg).await?;
            publish_message_notification(&self.nats, &self.nats_subject, &msg).await;
        }

        Ok(Response::new(PushMessagesResponse { accepted }))
    }

    async fn pull_messages(
        &self,
        request: Request<PullMessagesRequest>,
    ) -> Result<Response<PullMessagesResponse>, Status> {
        let claims = authorize_request(&request, &self.jwt_secret)?;
        info!(subject = %claims.sub, expires_at = claims.exp, "authorized pull_messages");

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

        let mut messages = Vec::with_capacity(rows.len());
        for row in rows {
            let message_id = row.get::<_, String>(0);
            let attachments = load_attachments(&db, &message_id).await?;

            messages.push(ProtoMessage {
                id: message_id,
                chat_id: row.get::<_, String>(1),
                author: row.get::<_, String>(2),
                text: row.get::<_, String>(3),
                created_at: row.get::<_, i64>(4),
                updated_at: row.get::<_, i64>(5),
                deleted: row.get::<_, bool>(6),
                attachments,
            });
        }

        Ok(Response::new(PullMessagesResponse {
            messages,
            server_timestamp: Utc::now().timestamp(),
        }))
    }
}

async fn upsert_message_with_attachments(db: &Client, msg: &ProtoMessage) -> Result<(), Status> {
    let local_updated = db
        .query_opt("SELECT updated_at FROM messages WHERE id = $1", &[&msg.id])
        .await
        .map_err(|err| Status::internal(format!("db read error: {err}")))?
        .map(|row| row.get::<_, i64>(0));

    if local_updated.is_some_and(|ts| ts > msg.updated_at) {
        return Ok(());
    }

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

    db.execute(
        "DELETE FROM message_attachments WHERE message_id = $1",
        &[&msg.id],
    )
    .await
    .map_err(|err| Status::internal(format!("db write error: {err}")))?;

    for (position, att) in msg.attachments.iter().enumerate() {
        db.execute(
            "INSERT INTO message_attachments (id, message_id, kind, file_name, mime_type, data, position) VALUES ($1, $2, $3, $4, $5, $6, $7)",
            &[&att.id, &msg.id, &att.kind, &att.file_name, &att.mime_type, &att.data, &(position as i32)],
        )
        .await
        .map_err(|err| Status::internal(format!("db write error: {err}")))?;
    }

    Ok(())
}

async fn load_attachments(db: &Client, message_id: &str) -> Result<Vec<ProtoAttachment>, Status> {
    let rows = db
        .query(
            "SELECT id, kind, file_name, mime_type, data FROM message_attachments WHERE message_id = $1 ORDER BY position ASC",
            &[&message_id],
        )
        .await
        .map_err(|err| Status::internal(format!("db read error: {err}")))?;

    Ok(rows
        .into_iter()
        .map(|row| ProtoAttachment {
            id: row.get(0),
            kind: row.get(1),
            file_name: row.get(2),
            mime_type: row.get(3),
            data: row.get(4),
        })
        .collect())
}

fn authorize_request<T>(request: &Request<T>, jwt_secret: &str) -> Result<JwtClaims, Status> {
    let raw_auth = request
        .metadata()
        .get("authorization")
        .ok_or_else(|| Status::unauthenticated("authorization metadata is required"))?;

    let auth_header = raw_auth
        .to_str()
        .map_err(|_| Status::unauthenticated("authorization header is invalid"))?;

    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or_else(|| Status::unauthenticated("expected Bearer token"))?;

    decode::<JwtClaims>(
        token,
        &DecodingKey::from_secret(jwt_secret.as_bytes()),
        &Validation::new(Algorithm::HS256),
    )
    .map(|data| data.claims)
    .map_err(|_| Status::unauthenticated("jwt token validation failed"))
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
        "attachment_count": msg.attachments.len(),
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

    for attachment in &msg.attachments {
        if attachment.id.trim().is_empty() {
            return Err(Status::invalid_argument("attachment id is required"));
        }
        if attachment.file_name.trim().is_empty() {
            return Err(Status::invalid_argument("attachment file_name is required"));
        }
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

            CREATE TABLE IF NOT EXISTS message_attachments (
                id TEXT PRIMARY KEY,
                message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
                kind TEXT NOT NULL,
                file_name TEXT NOT NULL,
                mime_type TEXT NOT NULL,
                data BYTEA NOT NULL,
                position INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_message_attachments_message_id
                ON message_attachments(message_id, position);
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
        jwt_secret: Arc::new(args.jwt_secret),
    };

    info!("minigram server listening on {}", args.listen);
    Server::builder()
        .add_service(SyncServiceServer::new(service))
        .serve(args.listen)
        .await?;

    Ok(())
}
