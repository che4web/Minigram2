mod db_service;

use std::{net::SocketAddr, sync::Arc};

use chrono::Utc;
use clap::Parser;
use db_service::DbService;
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use minigram_proto::minigram::{
    sync_service_server::{SyncService, SyncServiceServer},
    Message as ProtoMessage, PullMessagesRequest, PullMessagesResponse, PushMessagesRequest,
    PushMessagesResponse,
};
use serde::Deserialize;
use tonic::{transport::Server, Request, Response, Status};
use tracing::{info, warn};

#[derive(Debug, Parser)]
struct Args {
    #[arg(
        long,
        env = "MINIGRAM_SERVER_LISTEN",
        default_value = "127.0.0.1:50051"
    )]
    listen: SocketAddr,
    #[arg(
        long,
        env = "MINIGRAM_POSTGRES_URL",
        default_value = "postgres://postgres:postgres@127.0.0.1:5432/minigram"
    )]
    postgres_url: String,
    #[arg(
        long,
        env = "MINIGRAM_NATS_URL",
        default_value = "nats://127.0.0.1:4222"
    )]
    nats_url: String,
    #[arg(
        long,
        env = "MINIGRAM_NATS_SUBJECT",
        default_value = "minigram.messages"
    )]
    nats_subject: String,
    #[arg(
        long,
        env = "MINIGRAM_JWT_SECRET",
        default_value = "minigram-dev-secret"
    )]
    jwt_secret: String,
}

#[derive(Debug, Clone, Deserialize)]
struct JwtClaims {
    sub: String,
    exp: usize,
}

#[derive(Clone)]
struct SyncServiceImpl {
    db: DbService,
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

        for msg in input.messages {
            validate_message(&msg)?;
            self.db.upsert_message_with_attachments(&msg).await?;
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
        let messages = self.db.pull_messages_since(since_timestamp).await?;

        Ok(Response::new(PullMessagesResponse {
            messages,
            server_timestamp: Utc::now().timestamp(),
        }))
    }
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

    dotenvy::dotenv().ok();
    let args = Args::parse();
    let db = DbService::connect_with_migrations(&args.postgres_url).await?;
    let nats = connect_nats(&args.nats_url).await;

    let service = SyncServiceImpl {
        db,
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
