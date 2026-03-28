# Minigram2 (Rust monorepo)

Монорепозиторий содержит клиент и сервер мессенджера на Rust.

- `crates/minigram-server` — gRPC сервер синхронизации, хранит данные в PostgreSQL и отправляет уведомления о новых/обновлённых сообщениях в NATS.
- `crates/minigram-client-core` — общий движок синхронизации и локального SQLite-хранилища (используется CLI и Tauri клиентами), включая вложения файлов/фото.
- `crates/minigram-client` — CLI клиент на базе `minigram-client-core`.
- `crates/minigram-tauri` — Tauri backend.
- `crates/minigram-web` — Vue 3 (Composition API) + Vite frontend для Tauri.
- `crates/minigram-proto` — protobuf/gRPC контракт для обмена данными.

## Запуск сервера

```bash
cargo run -p minigram-server -- --listen 127.0.0.1:50051 --postgres-url postgres://postgres:postgres@127.0.0.1:5432/minigram --nats-url nats://127.0.0.1:4222 --nats-subject minigram.messages --jwt-secret minigram-dev-secret
```


## JWT авторизация

Сервер проверяет JWT в metadata `authorization: Bearer <token>` для RPC `PushMessages` и `PullMessages`.

- Секрет подписи на сервере: `--jwt-secret` (HS256).
- Токен для клиентов: `MINIGRAM_JWT_TOKEN` (или `--jwt-token` для CLI).

Пример payload токена:

```json
{ "sub": "alice", "exp": 1893456000 }
```

> `exp` должен быть в будущем (unix timestamp в секундах).

## CLI клиент

```bash
cargo run -p minigram-client -- --db ./client_store.db send --chat general --author alice --text "Привет" --photo ./cat.jpg --file ./spec.pdf
MINIGRAM_JWT_TOKEN=<ваш_jwt> cargo run -p minigram-client -- --db ./client_store.db sync
cargo run -p minigram-client -- --db ./client_store.db list --chat general
```

## Tauri + Vue3 (Composition API) клиент

Tauri backend команды:
- `send_message({ chat, author, text, attachments })`
- `list_chats()`
- `list_messages({ chat })`
- `sync_messages()`

Vue интерфейс (`crates/minigram-web`) предоставляет:
- левую панель с чатами (как в Telegram),
- отдельное окно текущего чата,
- маршрутизацию через Vue Router (`/chats`, `/chats/:chatId`),
- разбиение UI на компоненты (`SidebarChats`, `ChatHeader`, `MessageList`, `ComposerBar`),
- обращение к Tauri-командам через composable (`useMessenger` + `useMinigramApi`),
- отправку сообщений и sync по кнопке,
- вложение файлов и фотографий (превью изображений в чате).

Конфигурация через переменные окружения:
- `MINIGRAM_SERVER_URL` (по умолчанию `http://127.0.0.1:50051`)
- `MINIGRAM_DB_PATH` (по умолчанию `minigram_tauri.db`)
- `MINIGRAM_JWT_TOKEN` (JWT для авторизации gRPC sync запросов)

### Запуск Tauri dev режима

```bash
cd crates/minigram-web
npm install

cd ../minigram-tauri
cargo tauri dev
```
