use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Attachment {
    pub kind: String,
    pub file_name: String,
    pub mime_type: String,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub id: String,
    pub chat_id: String,
    pub author: String,
    pub text: String,
    pub created_at: i64,
    pub attachments: Vec<Attachment>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ChatSummary {
    pub chat_id: String,
    pub last_message_preview: String,
    pub last_message_at: i64,
    pub message_count: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ClientStatus {
    pub pending_uploads: i64,
    pub last_sync_timestamp: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ChatsResponse {
    pub chats: Vec<ChatSummary>,
    pub status: ClientStatus,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct MessagesResponse {
    pub messages: Vec<Message>,
    pub status: ClientStatus,
}
