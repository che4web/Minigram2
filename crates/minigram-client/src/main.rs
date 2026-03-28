use std::{fs, path::PathBuf};

use clap::{Parser, Subcommand};
use minigram_client_core::{connect, run_sync, AttachmentRecord, SqliteStore};
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(about = "Minigram CLI client with SQLite local storage + sync")]
struct Args {
    #[arg(long, default_value = "http://127.0.0.1:50051")]
    server: String,
    #[arg(long, default_value = "client_store.db")]
    db: PathBuf,
    #[arg(long)]
    jwt_token: Option<String>,

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
        #[arg(long = "file")]
        files: Vec<PathBuf>,
        #[arg(long = "photo")]
        photos: Vec<PathBuf>,
    },
    List {
        #[arg(long)]
        chat: Option<String>,
    },
    Sync,
}

fn guess_mime(path: &std::path::Path, default: &str) -> String {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("jpg" | "jpeg") => "image/jpeg".to_string(),
        Some("png") => "image/png".to_string(),
        Some("gif") => "image/gif".to_string(),
        Some("webp") => "image/webp".to_string(),
        Some("pdf") => "application/pdf".to_string(),
        Some("txt") => "text/plain".to_string(),
        _ => default.to_string(),
    }
}

fn attachment_from_path(
    path: PathBuf,
    kind: &str,
    default_mime: &str,
) -> anyhow::Result<AttachmentRecord> {
    let data = fs::read(&path)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file.bin")
        .to_string();

    Ok(AttachmentRecord {
        id: Uuid::new_v4().to_string(),
        kind: kind.to_string(),
        file_name,
        mime_type: guess_mime(&path, default_mime),
        data,
    })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(false)
        .init();

    let args = Args::parse();
    let store = SqliteStore::open(&args.db)?;
    let jwt_token = args
        .jwt_token
        .or_else(|| std::env::var("MINIGRAM_JWT_TOKEN").ok());

    match args.command {
        Command::Send {
            chat,
            author,
            text,
            files,
            photos,
        } => {
            let mut attachments = Vec::new();
            for file in files {
                attachments.push(attachment_from_path(
                    file,
                    "file",
                    "application/octet-stream",
                )?);
            }
            for photo in photos {
                attachments.push(attachment_from_path(photo, "photo", "image/jpeg")?);
            }

            store.add_local_message_with_attachments(chat, author, text, attachments)?;
            println!("Message stored locally in SQLite and queued for sync.");
        }
        Command::List { chat } => {
            let messages = store.list_messages(chat.as_deref())?;
            for msg in messages {
                println!(
                    "[{}] {} {}: {} (attachments={})",
                    msg.chat_id,
                    msg.created_at,
                    msg.author,
                    msg.text,
                    msg.attachments.len()
                );
                for att in msg.attachments {
                    println!(
                        "  - [{}] {} ({}, {} bytes)",
                        att.kind,
                        att.file_name,
                        att.mime_type,
                        att.data.len()
                    );
                }
            }
            println!("pending_uploads={}", store.pending_count()?);
            println!("last_sync_timestamp={}", store.last_sync_timestamp()?);
        }
        Command::Sync => {
            let mut client = connect(&args.server).await?;
            let stats = run_sync(&store, &mut client, jwt_token.as_deref()).await?;
            println!(
                "Sync complete (pushed={}, pulled={}, server_timestamp={}).",
                stats.pushed, stats.pulled, stats.server_timestamp
            );
        }
    }

    Ok(())
}
