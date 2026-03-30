# Minigram Android (Jetpack Compose + Rust sync core)

Прототип Android-клиента на Jetpack Compose, который использует общую Rust-логику синхронизации из `minigram-client-core` через JNI-обёртку `minigram-mobile-ffi`.

## Что уже подключено

- Jetpack Compose UI (`MainActivity`, `MainViewModel`)
- Native bridge `MinigramNative` (`System.loadLibrary("minigram_mobile_ffi")`)
- JNI-функции в Rust:
  - `syncOnce(serverUrl, dbPath, jwtToken)`
  - `addLocalMessage(dbPath, chatId, author, text)`
- Использование общего sync-движка `run_sync_db` из `minigram-client-core`

## Сборка Rust-библиотеки для Android

Ниже пример через `cargo-ndk`:

```bash
cargo install cargo-ndk
cd /workspace/Minigram2
cargo ndk -t arm64-v8a -o crates/minigram-android/app/src/main/jniLibs build -p minigram-mobile-ffi --release
```

После команды появится `libminigram_mobile_ffi.so` в `app/src/main/jniLibs/arm64-v8a`.

## Запуск Android приложения

1. Открыть `crates/minigram-android` в Android Studio.
2. Дождаться синхронизации Gradle.
3. Убедиться, что `.so` уже собрана и лежит в `jniLibs`.
4. Запустить на эмуляторе/устройстве.

> По умолчанию в `MainViewModel` указан сервер `http://10.0.2.2:50051` (доступ к localhost хоста из Android-эмулятора).
