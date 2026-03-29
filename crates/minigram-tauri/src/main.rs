use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use minigram_client_core::{
    connect, run_sync_db, AttachmentRecord, MessageRecord, SqliteStore, SyncStats,
};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

struct AppState {
    server_url: String,
    db_path: PathBuf,
    jwt_token: Mutex<Option<String>>,
}

#[derive(Debug, Deserialize)]
struct AttachmentPayload {
    kind: String,
    file_name: String,
    mime_type: String,
    data: Vec<u8>,
}

#[derive(Debug, Deserialize)]
struct SendMessagePayload {
    chat: String,
    author: String,
    text: String,
    attachments: Option<Vec<AttachmentPayload>>,
}

#[derive(Debug, Deserialize)]
struct ListMessagesPayload {
    chat: String,
}

#[derive(Debug, Serialize)]
struct ClientStatus {
    pending_uploads: i64,
    last_sync_timestamp: i64,
}

#[derive(Debug, Serialize)]
struct ListMessagesResult {
    messages: Vec<MessageRecord>,
    status: ClientStatus,
}

#[derive(Debug, Serialize)]
struct ChatSummary {
    chat_id: String,
    last_message_preview: String,
    last_message_at: i64,
    message_count: usize,
}

#[derive(Debug, Serialize)]
struct ChatsResult {
    chats: Vec<ChatSummary>,
    status: ClientStatus,
}

#[tauri::command]
async fn send_message(
    state: State<'_, AppState>,
    payload: SendMessagePayload,
) -> Result<(), String> {
    let store = SqliteStore::open(&state.db_path).map_err(|e| e.to_string())?;

    let attachments = payload
        .attachments
        .unwrap_or_default()
        .into_iter()
        .map(|att| AttachmentRecord {
            id: Uuid::new_v4().to_string(),
            kind: att.kind,
            file_name: att.file_name,
            mime_type: att.mime_type,
            data: att.data,
        })
        .collect();

    store
        .add_local_message_with_attachments(payload.chat, payload.author, payload.text, attachments)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_chats(state: State<'_, AppState>) -> Result<ChatsResult, String> {
    let store = SqliteStore::open(&state.db_path).map_err(|e| e.to_string())?;
    let messages = store.list_messages(None).map_err(|e| e.to_string())?;

    let mut grouped: HashMap<String, ChatSummary> = HashMap::new();
    for msg in messages {
        let preview = if msg.text.trim().is_empty() && !msg.attachments.is_empty() {
            format!("📎 {} влож.", msg.attachments.len())
        } else {
            msg.text.clone()
        };

        let entry = grouped.entry(msg.chat_id.clone()).or_insert(ChatSummary {
            chat_id: msg.chat_id.clone(),
            last_message_preview: preview.clone(),
            last_message_at: msg.created_at,
            message_count: 0,
        });

        entry.message_count += 1;
        if msg.created_at >= entry.last_message_at {
            entry.last_message_at = msg.created_at;
            entry.last_message_preview = preview;
        }
    }

    let mut chats: Vec<ChatSummary> = grouped.into_values().collect();
    chats.sort_by(|a, b| b.last_message_at.cmp(&a.last_message_at));

    Ok(ChatsResult {
        chats,
        status: ClientStatus {
            pending_uploads: store.pending_count().map_err(|e| e.to_string())?,
            last_sync_timestamp: store.last_sync_timestamp().map_err(|e| e.to_string())?,
        },
    })
}

#[tauri::command]
async fn list_messages(
    state: State<'_, AppState>,
    payload: ListMessagesPayload,
) -> Result<ListMessagesResult, String> {
    let store = SqliteStore::open(&state.db_path).map_err(|e| e.to_string())?;
    let messages = store
        .list_messages(Some(&payload.chat))
        .map_err(|e| e.to_string())?;

    Ok(ListMessagesResult {
        messages,
        status: ClientStatus {
            pending_uploads: store.pending_count().map_err(|e| e.to_string())?,
            last_sync_timestamp: store.last_sync_timestamp().map_err(|e| e.to_string())?,
        },
    })
}

#[tauri::command]
async fn sync_messages(state: State<'_, AppState>) -> Result<SyncStats, String> {
    let mut client = connect(&state.server_url)
        .await
        .map_err(|e| e.to_string())?;
    let jwt_token = state
        .jwt_token
        .lock()
        .map_err(|e| e.to_string())?
        .clone();

    run_sync_db(&state.db_path, &mut client, jwt_token.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_jwt_token(state: State<'_, AppState>, token: Option<String>) -> Result<(), String> {
    let normalized = token.and_then(|value| {
        let trimmed = value.trim().to_owned();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    });

    let mut jwt_token = state.jwt_token.lock().map_err(|e| e.to_string())?;
    *jwt_token = normalized;
    Ok(())
}

fn default_db_path() -> PathBuf {
    if let Ok(path) = std::env::var("MINIGRAM_DB_PATH") {
        return PathBuf::from(path);
    }

    if let Ok(data_dir) = std::env::var("XDG_DATA_HOME") {
        return PathBuf::from(data_dir)
            .join("minigram")
            .join("minigram_tauri.db");
    }

    if let Ok(home_dir) = std::env::var("HOME") {
        return PathBuf::from(home_dir)
            .join(".local")
            .join("share")
            .join("minigram")
            .join("minigram_tauri.db");
    }

    std::env::temp_dir().join("minigram_tauri.db")
}

fn main() {
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(false)
        .init();

    let server_url = std::env::var("MINIGRAM_SERVER_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:50051".to_string());
    let db_path = default_db_path();
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent).expect("failed to create application data directory");
    }
    let jwt_token = std::env::var("MINIGRAM_JWT_TOKEN").ok();

    tauri::Builder::default()
        .manage(AppState {
            server_url,
            db_path,
            jwt_token: Mutex::new(jwt_token),
        })
        .invoke_handler(tauri::generate_handler![
            send_message,
            list_chats,
            list_messages,
            sync_messages,
            set_jwt_token
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
