# Minigram2 (Rust monorepo)

Монорепозиторий содержит клиент и сервер мессенджера на Rust.

- `crates/minigram-server` — gRPC сервер синхронизации, хранит данные в PostgreSQL и отправляет уведомления о новых/обновлённых сообщениях в NATS.
- `crates/minigram-client` — CLI клиент с локальным SQLite-хранилищем и синхронизацией с сервером.
- `crates/minigram-proto` — protobuf/gRPC контракт для обмена данными.

## Запуск

```bash
cargo run -p minigram-server -- --listen 127.0.0.1:50051 --postgres-url postgres://postgres:postgres@127.0.0.1:5432/minigram --nats-url nats://127.0.0.1:4222 --nats-subject minigram.messages
```

В другом терминале:

```bash
cargo run -p minigram-client -- --db ./client_store.db send --chat general --author alice --text "Привет"
cargo run -p minigram-client -- --db ./client_store.db sync
cargo run -p minigram-client -- --db ./client_store.db list --chat general
```
