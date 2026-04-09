use crate::models::{ChatsResponse, MessagesResponse};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

const JWT_STORAGE_KEY: &str = "minigram.jwt_token";

pub async fn list_chats() -> Result<ChatsResponse, String> {
    invoke("list_chats", None).await
}

pub async fn list_messages(chat_id: &str) -> Result<MessagesResponse, String> {
    let payload = json!({ "payload": { "chat": chat_id } });
    invoke("list_messages", Some(payload)).await
}

pub async fn send_message(chat_id: &str, author: &str, text: &str) -> Result<(), String> {
    let payload = json!({
        "payload": {
            "chat": chat_id,
            "author": author,
            "text": text,
            "attachments": []
        }
    });
    let _: Value = invoke("send_message", Some(payload)).await?;
    Ok(())
}

pub async fn sync_messages() -> Result<(), String> {
    let _: Value = invoke("sync_messages", None).await?;
    Ok(())
}

pub async fn set_jwt_token(token: &str) -> Result<(), String> {
    let normalized = token.trim();
    let payload = json!({
        "token": if normalized.is_empty() { Value::Null } else { Value::String(normalized.to_string()) }
    });
    let _: Value = invoke("set_jwt_token", Some(payload)).await?;
    persist_jwt_token(normalized);
    Ok(())
}

pub fn load_jwt_token() -> String {
    web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item(JWT_STORAGE_KEY).ok().flatten())
        .unwrap_or_default()
}

fn persist_jwt_token(value: &str) {
    if let Some(storage) =
        web_sys::window().and_then(|window| window.local_storage().ok().flatten())
    {
        if value.is_empty() {
            let _ = storage.remove_item(JWT_STORAGE_KEY);
        } else {
            let _ = storage.set_item(JWT_STORAGE_KEY, value);
        }
    }
}

async fn invoke<T>(command: &str, args: Option<Value>) -> Result<T, String>
where
    T: DeserializeOwned,
{
    let core = tauri_core();
    if core.is_none() {
        return Err("Tauri runtime недоступен. Запустите клиент внутри Tauri shell.".to_string());
    }

    let core = core.expect("checked is_some");
    let invoke_fn = js_sys::Reflect::get(&core, &JsValue::from_str("invoke"))
        .map_err(|_| "Tauri invoke API is unavailable".to_string())?
        .dyn_into::<js_sys::Function>()
        .map_err(|_| "Tauri invoke has invalid type".to_string())?;

    let parsed_args = match args {
        Some(v) => js_sys::JSON::parse(&v.to_string())
            .map_err(|_| "Failed to serialize invoke args".to_string())?,
        None => JsValue::UNDEFINED,
    };

    let promise = invoke_fn
        .call2(&core, &JsValue::from_str(command), &parsed_args)
        .map_err(|e| format!("invoke call failed: {e:?}"))?;

    let result = JsFuture::from(js_sys::Promise::from(promise))
        .await
        .map_err(|e| format!("invoke rejected: {e:?}"))?;

    serde_wasm_bindgen::from_value(result).map_err(|e| e.to_string())
}

fn tauri_core() -> Option<JsValue> {
    let window = web_sys::window()?;
    let tauri = js_sys::Reflect::get(&window, &JsValue::from_str("__TAURI__")).ok()?;
    js_sys::Reflect::get(&tauri, &JsValue::from_str("core")).ok()
}
