use anyhow::Context;
use jni::objects::{JClass, JString};
use jni::sys::jstring;
use jni::JNIEnv;
use minigram_client_core::{connect, run_sync_db, SqliteStore, SyncStats};
use serde::Serialize;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Serialize)]
struct FfiResult<T: Serialize> {
    ok: bool,
    data: Option<T>,
    error: Option<String>,
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
}

fn json_ok<T: Serialize>(data: T) -> String {
    serde_json::to_string(&FfiResult {
        ok: true,
        data: Some(data),
        error: None,
    })
    .unwrap_or_else(|err| {
        format!(
            r#"{{\"ok\":false,\"data\":null,\"error\":\"serialization failed: {}\"}}"#,
            err
        )
    })
}

fn json_err(error: anyhow::Error) -> String {
    serde_json::to_string(&FfiResult::<()> {
        ok: false,
        data: None,
        error: Some(format!("{error:#}")),
    })
    .unwrap_or_else(|err| {
        format!(
            r#"{{\"ok\":false,\"data\":null,\"error\":\"serialization failed: {}\"}}"#,
            err
        )
    })
}

fn into_jstring(env: &mut JNIEnv<'_>, value: String) -> jstring {
    env.new_string(value)
        .expect("failed to create java string")
        .into_raw()
}

fn read_java_string(
    env: &mut JNIEnv<'_>,
    value: JString<'_>,
    field: &str,
) -> anyhow::Result<String> {
    Ok(env
        .get_string(&value)
        .with_context(|| format!("failed to read {field}"))?
        .into())
}

#[no_mangle]
pub extern "system" fn Java_com_minigram_mobile_rust_MinigramNative_syncOnce(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    server_url: JString<'_>,
    db_path: JString<'_>,
    jwt_token: JString<'_>,
) -> jstring {
    init_tracing();

    let result = (|| -> anyhow::Result<SyncStats> {
        let server_url = read_java_string(&mut env, server_url, "serverUrl")?;
        let db_path = read_java_string(&mut env, db_path, "dbPath")?;
        let jwt = read_java_string(&mut env, jwt_token, "jwtToken")?;
        let jwt_opt = if jwt.trim().is_empty() {
            None
        } else {
            Some(jwt.as_str())
        };

        let runtime = tokio::runtime::Runtime::new().context("failed to create tokio runtime")?;

        runtime.block_on(async {
            let mut client = connect(&server_url).await?;
            run_sync_db(&db_path, &mut client, jwt_opt).await
        })
    })();

    let payload = match result {
        Ok(stats) => json_ok(stats),
        Err(err) => json_err(err),
    };

    into_jstring(&mut env, payload)
}

#[no_mangle]
pub extern "system" fn Java_com_minigram_mobile_rust_MinigramNative_addLocalMessage(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    db_path: JString<'_>,
    chat_id: JString<'_>,
    author: JString<'_>,
    text: JString<'_>,
) -> jstring {
    let result = (|| -> anyhow::Result<()> {
        let db_path = read_java_string(&mut env, db_path, "dbPath")?;
        let chat_id = read_java_string(&mut env, chat_id, "chatId")?;
        let author = read_java_string(&mut env, author, "author")?;
        let text = read_java_string(&mut env, text, "text")?;

        let store = SqliteStore::open(&db_path)?;
        store.add_local_message(chat_id, author, text)
    })();

    let payload = match result {
        Ok(()) => json_ok("ok"),
        Err(err) => json_err(err),
    };

    into_jstring(&mut env, payload)
}
