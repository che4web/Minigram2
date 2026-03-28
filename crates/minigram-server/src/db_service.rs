use std::{process::Command, sync::Arc};

use anyhow::Context;
use minigram_proto::minigram::{Attachment as ProtoAttachment, Message as ProtoMessage};
use tokio::sync::Mutex;
use tokio_postgres::{types::ToSql, Client, NoTls};
use tonic::Status;

#[derive(Clone)]
pub struct DbService {
    client: Arc<Mutex<Client>>,
}

impl DbService {
    pub async fn connect_with_migrations(postgres_url: &str) -> anyhow::Result<Self> {
        ensure_schema_with_atlas(postgres_url)
            .context("failed to apply postgres migrations with Atlas")?;

        let (client, connection) = tokio_postgres::connect(postgres_url, NoTls)
            .await
            .with_context(|| format!("failed to connect to postgres at {postgres_url}"))?;

        tokio::spawn(async move {
            if let Err(err) = connection.await {
                eprintln!("postgres connection error: {err}");
            }
        });

        Ok(Self {
            client: Arc::new(Mutex::new(client)),
        })
    }

    pub async fn upsert_message_with_attachments(&self, msg: &ProtoMessage) -> Result<(), Status> {
        let db = self.client.lock().await;

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

    pub async fn pull_messages_since(
        &self,
        since_timestamp: i64,
    ) -> Result<Vec<ProtoMessage>, Status> {
        let db = self.client.lock().await;

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

        Ok(messages)
    }
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

fn ensure_schema_with_atlas(postgres_url: &str) -> anyhow::Result<()> {
    let migrations_dir = format!("file://{}/migrations", env!("CARGO_MANIFEST_DIR"));

    let output = Command::new("atlas")
        .args([
            "migrate",
            "apply",
            "--dir",
            &migrations_dir,
            "--url",
            postgres_url,
            "--revisions-schema",
            "minigram_atlas",
        ])
        .output()
        .context("failed to run atlas cli; ensure Atlas is installed and available in PATH")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        anyhow::bail!(
            "atlas migrate apply failed with status {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            stdout,
            stderr
        );
    }

    Ok(())
}
