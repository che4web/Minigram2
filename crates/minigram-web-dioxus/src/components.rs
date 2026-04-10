use crate::models::{ChatSummary, Message};
use dioxus::prelude::*;

#[component]
pub fn Sidebar(
    chats: ReadOnlySignal<Vec<ChatSummary>>,
    selected_chat: ReadOnlySignal<Option<String>>,
    loading: bool,
    on_refresh: EventHandler<()>,
    on_select_chat: EventHandler<String>,
) -> Element {
    rsx! {
        aside { class: "sidebar",
            h1 { "Minigram · Dioxus" }
            button {
                onclick: move |_| on_refresh.call(()),
                disabled: loading,
                "Обновить чаты"
            }
            ul {
                for chat in chats().iter() {
                    li {
                        button {
                            class: if selected_chat().as_deref() == Some(chat.chat_id.as_str()) {"chat selected"} else {"chat"},
                            onclick: {
                                let chat_id = chat.chat_id.clone();
                                move |_| on_select_chat.call(chat_id.clone())
                            },
                            div { class: "chat-title", "{chat.chat_id}" }
                            div { class: "chat-preview", "{chat.last_message_preview}" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn TopBar(
    jwt: String,
    loading: bool,
    on_jwt_input: EventHandler<String>,
    on_save_jwt: EventHandler<()>,
    on_sync: EventHandler<()>,
) -> Element {
    rsx! {
        div { class: "topbar",
            input {
                value: "{jwt}",
                placeholder: "JWT token",
                oninput: move |e| on_jwt_input.call(e.value()),
            }
            button { onclick: move |_| on_save_jwt.call(()), "Сохранить JWT" }
            button { onclick: move |_| on_sync.call(()), disabled: loading, "Sync" }
        }
    }
}

#[component]
pub fn MessagesView(messages: ReadOnlySignal<Vec<Message>>) -> Element {
    rsx! {
        section { class: "messages",
            for message in messages().iter() {
                article { class: "message",
                    header { "{message.author} · {format_ts(message.created_at)}" }
                    p { "{message.text}" }
                    if !message.attachments.is_empty() {
                        small { "Вложений: {message.attachments.len()}" }
                    }
                }
            }
        }
    }
}

#[component]
pub fn Composer(
    author: String,
    draft: String,
    loading: bool,
    on_author_input: EventHandler<String>,
    on_draft_input: EventHandler<String>,
    on_send: EventHandler<()>,
) -> Element {
    rsx! {
        div { class: "composer",
            input {
                value: "{author}",
                oninput: move |e| on_author_input.call(e.value()),
                placeholder: "Author"
            }
            textarea {
                value: "{draft}",
                oninput: move |e| on_draft_input.call(e.value()),
                placeholder: "Введите сообщение..."
            }
            button { onclick: move |_| on_send.call(()), disabled: loading, "Отправить" }
        }
    }
}

fn format_ts(ts: i64) -> String {
    let date = js_sys::Date::new(&(ts as f64 * 1000.0).into());
    date.to_locale_string("ru-RU")
        .as_string()
        .unwrap_or_else(|| ts.to_string())
}
