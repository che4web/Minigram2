use crate::api;
use crate::components::{Composer, MessagesView, Sidebar, TopBar};
use crate::models::{ChatSummary, ClientStatus, Message};
use dioxus::prelude::*;

pub fn App() -> Element {
    let mut chats = use_signal(Vec::<ChatSummary>::new);
    let mut messages = use_signal(Vec::<Message>::new);
    let mut selected_chat = use_signal(|| None::<String>);
    let mut status = use_signal(ClientStatus::default);
    let mut loading = use_signal(|| false);
    let mut error = use_signal(String::new);
    let mut author = use_signal(|| "me".to_string());
    let mut draft = use_signal(String::new);
    let mut jwt = use_signal(api::load_jwt_token);

    use_future(move || async move {
        let _ = refresh_chats(chats, status, loading, error, selected_chat).await;
        if let Some(chat_id) = selected_chat() {
            let _ = refresh_messages(messages, status, loading, error, chat_id).await;
        }
    });

    let on_refresh = move |_| {
        spawn(async move {
            let _ = refresh_chats(chats, status, loading, error, selected_chat).await;
        });
    };

    let on_select_chat = move |chat_id: String| {
        selected_chat.set(Some(chat_id.clone()));
        spawn(async move {
            let _ = refresh_messages(messages, status, loading, error, chat_id).await;
        });
    };

    let on_save_jwt = move |_| {
        let token = jwt();
        spawn(async move {
            if let Err(err) = api::set_jwt_token(&token).await {
                error.set(err);
            }
        });
    };

    let on_sync = move |_| {
        spawn(async move {
            loading.set(true);
            error.set(String::new());

            if let Err(err) = api::sync_messages().await {
                error.set(err);
                loading.set(false);
                return;
            }

            let _ = refresh_chats(chats, status, loading, error, selected_chat).await;
            if let Some(chat_id) = selected_chat() {
                let _ = refresh_messages(messages, status, loading, error, chat_id).await;
            }

            loading.set(false);
        });
    };

    let on_send = move |_| {
        let selected = selected_chat();
        let text = draft();
        if selected.is_none() || text.trim().is_empty() {
            return;
        }

        let chat_id = selected.unwrap_or_default();
        let author_name = author();
        spawn(async move {
            loading.set(true);
            error.set(String::new());

            match api::send_message(&chat_id, &author_name, &text).await {
                Ok(_) => {
                    draft.set(String::new());
                    let _ = refresh_chats(chats, status, loading, error, selected_chat).await;
                    let _ = refresh_messages(messages, status, loading, error, chat_id).await;
                }
                Err(err) => error.set(err),
            }

            loading.set(false);
        });
    };

    rsx! {
        style { {include_str!("./styles.css")} }
        div { class: "shell",
            Sidebar {
                chats: chats.into(),
                selected_chat: selected_chat.into(),
                loading: loading(),
                on_refresh,
                on_select_chat
            }
            main { class: "content",
                TopBar {
                    jwt: jwt(),
                    loading: loading(),
                    on_jwt_input: move |v| jwt.set(v),
                    on_save_jwt,
                    on_sync
                }
                p { class: "status", "Pending: {status().pending_uploads}, last sync: {status().last_sync_timestamp}" }
                if !error().is_empty() {
                    p { class: "error", "{error}" }
                }
                MessagesView { messages: messages.into() }
                Composer {
                    author: author(),
                    draft: draft(),
                    loading: loading(),
                    on_author_input: move |v| author.set(v),
                    on_draft_input: move |v| draft.set(v),
                    on_send
                }
            }
        }
    }
}

async fn refresh_chats(
    mut chats: Signal<Vec<ChatSummary>>,
    mut status: Signal<ClientStatus>,
    mut loading: Signal<bool>,
    mut error: Signal<String>,
    mut selected_chat: Signal<Option<String>>,
) -> Result<(), String> {
    loading.set(true);
    error.set(String::new());

    let result = api::list_chats().await?;
    status.set(result.status);
    chats.set(result.chats.clone());

    if selected_chat().is_none() {
        if let Some(first) = result.chats.first() {
            selected_chat.set(Some(first.chat_id.clone()));
        }
    }

    loading.set(false);
    Ok(())
}

async fn refresh_messages(
    mut messages: Signal<Vec<Message>>,
    mut status: Signal<ClientStatus>,
    mut loading: Signal<bool>,
    mut error: Signal<String>,
    chat_id: String,
) -> Result<(), String> {
    loading.set(true);
    error.set(String::new());

    let result = api::list_messages(&chat_id).await?;
    status.set(result.status);
    messages.set(result.messages);

    loading.set(false);
    Ok(())
}
