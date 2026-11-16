Ты работаешь в проекте **Besedka** (Rust/Axum бэкенд + Svelte 5 UI). Конвейер: строго по срезам, гейты обязательны.
Если доступен скилл `besedka` — загрузи его (workflow + гейты + питфоллы). Иначе — ТОЛЬКО `.conveyor/CONVENTIONS.md` (NEVER доверять памяти).

=== ЗАДАЧА: Исправить Wave 3.1 (файлы: upload/download) — ЧАСТЬ 1/2: БЭКЕНД ===
Верификация (`wave31-verify-report.md`) завершилась **FAIL**: core/infra сделаны и зелёные, но **воркспейс не компилируется** (`E0433: cannot find 'files' in 'http'`), HTTP-слой 3.1 отсутствует. Отчёт: **`.conveyor/reports/wave31-verify-report.md`** — прочитай ВПЕРВЫХ, это источник правды. Исходная спека: `.conveyor/PROMPT_wave31.md` (статус-коды, тексты ошибок, RFC 5987, PITFALLы — оттуда).

**Граница части 1: ТОЛЬКО бэкенд (Rust).** UI (`ui/`) НЕ трогать, браузер НЕ поднимать, STATE/отчёт/PROMPT_wave32 — НЕ писать. Это всё **ЧАСТЬ 2/2** (`.conveyor/PROMPT_wave31-fix-2.md`) — твоя работа заканчивается зелёным бэкендом + commit + push + стоп фоновых процессов.

## Что УЖЕ есть и НЕ ТРОГАТЬ (проверено, зелёное):
- `crates/besedka-core/src/file.rs` — тип `File`, `MAX_FILE_SIZE_DEFAULT` (25 MiB), `ALLOWED_CONTENT_TYPES`, `validate_upload(size_bytes, max_size_bytes, content_type)`, `sanitize_filename`, 13 тестов. **Единственная правка:** убрать `#[serde(rename_all = "camelCase")]` с `File` (проект — snake_case: Message/User/Room).
- `migrations/V6__files.sql` — таблица `files` + `idx_files_uploader`. **НЕ править** (V6 неприменена в dev-БД, схема на V5; FK без каскада — осознанный дизайн, задокументирует часть 2).
- `crates/besedka-infra/src/db.rs` — `create_file/get_file/delete_file` + 3 теста. Не трогать.
- `http/mod.rs` — хелперы `HttpError::payload_too_large` / `unsupported_media_type` (использовать их; комментарий «Wave 3.4» → «Wave 3.1»).
- `Cargo.toml` — axum feature `multipart` + multer в Cargo.lock. Не трогать.
- Базовые числа: core 28, infra 99 (оба зелёные); app — сейчас не запускается.

## ЧТО ДЕЛАТЬ (всё — бэкенд):

### 1. Конфиг (`config.rs`, `main.rs`)
- `FilesConfig { dir: String (дефолт `"data/files"`), max_size_bytes: u64 (дефолт из `besedka_core::MAX_FILE_SIZE_DEFAULT`) }`.
- В `Config::load` — env-оверрайды `BESEDKA_FILES_DIR` / `BESEDKA_FILES_MAX_SIZE_BYTES`, паттерн существующих (config.rs:148-156: `if let Ok(..) = std::env::var(..)`).
- `main.rs`: `app(db, cfg.sessions, ad, cfg.files)` (текущий `cfg.files.file_upload_max_bytes` — несуществующее поле, убрать).
- Тесты: дефолты (dir + max), кастомные значения из TOML, env-оверрайт (ставить/чистить env в тесте, один за другим).

### 2. HTTP-слой (`crates/besedka-app/src/http/files.rs` — НОВЫЙ)
- В `http/mod.rs`: `pub mod files;`
- `AppState` получает `files: FilesConfig` (убрать `file_upload_max_bytes: usize`); сигнатура `app(db, sessions, ad, files: FilesConfig)`; **удалить** room-маршрут `.route("/api/v1/rooms/{id}/files", ...)`; зарегистрировать:
  - `POST /api/v1/files` (AuthedUser — `http::extract::AuthedUser`; ролевых гейтов НЕТ);
  - `GET /api/v1/files/{id}` (AuthedUser).
- **POST** (axum `Multipart`, поле `file`), по спеке PROMPT_wave31.md шаг 4:
  - нет поля / пустое → **400** (плоский `HttpError`);
  - `size > files.max_size_bytes` → **413** `{"error":"file too large (max <N> bytes)"}` через `HttpError::payload_too_large`;
  - content_type вне списка → **415** `{"error":"content type not allowed: <type>"}` через `HttpError::unsupported_media_type`;
  - успех → **201** `{"file":{"id","uploader_id","filename","content_type","size_bytes","created_at","url":"/api/v1/files/{id}"}}` (snake_case!);
  - **PITFALL из спеки:** размер проверять ДО полного чтения в память (считать на лете, прервать при превышении) — не грузить гигабайты в RAM;
  - хранение: байты → `{dir}/<id>_<sanitized_filename>` (каталог создавать при запуске, если нет); метаданные — `Db::create_file(uploader_id, filename, content_type, size_bytes)`; имя через `sanitize_filename`.
- **GET**: бинарник с диска; `Content-Type` из БД; `Content-Disposition: inline; filename="<safe>"` + для кириллицы `filename*=UTF-8''...` (RFC 5987); `Content-Length`; файла в БД нет → **404**; строка есть, а байтов на диске нет → **410** (и не паниковать).
- **Новые HTTP-тесты (обязательны, без них часть не закрыта):**
  - upload→download roundtrip: байты совпадают (сравнить весь массив / sha256), имя/тип/размер совпадают;
  - 413: лимит в тесте закручен до ~1 KiB через `FilesConfig`, файл чуть больше;
  - 415: `text/plain`; 400: пустой multipart / нет поля `file`;
  - 404: `GET /api/v1/files/999999`;
  - кириллическое имя → `filename*=UTF-8''` в Content-Disposition;
  - 410: в БД есть, на диске нет (в тесте удалить файл с диска).
  - Старые core 28 / infra 99 остаются зелёными; тесты lib.rs, где `app(...)` — поправить под новую сигнатуру (без хардкода 10 MiB — `FilesConfig::default()` или маленький лимит).

### 3. Cleanup
- `rm -rf crates/besedka-app/src/ws .conveyor/patches` (untracked WIP вне среза; «Wave 2.4» нет в PLAN.md).
- Все комментарии «Wave 3.4» → «Wave 3.1».
- Проверки: `grep -rn "file_upload_max_bytes\|Wave 3.4\|10 \* 1024 \* 1024" crates/` = **0 совпадений**.

### 4. ГЕЙТЫ (бэкенд, все ДО commit)
- `cargo build` OK · `cargo test --workspace` — 0 failed (все новые тесты на месте) · `cargo clippy --all-targets -- -D warnings` = 0 · `cargo fmt --check` чистый.
- UI НЕ трогать → npm-гейты НЕ нужны (это часть 2).

### 5. Живая curl-проверка (без браузера)
- `cargo run -p besedka-app` (dev-БД); curl (логин + cookie — см. `references/dev-env.md` скилла):
  - тестовый PNG (1×1) → `curl -F file=@test.png` → **201** + url;
  - скачать url → байты идентичны исходнику (`shasum -a 256` на обоих);
  - `.txt` → **415**; `dd` 30 MiB → **413**; пустой multipart → **400**; `GET .../999999` → **404**;
  - `sqlite3 data/besedka.db "SELECT version FROM refinery_schema_history"` → на последнем месте **6**.
- Браузерное превью + скриншот — **не сейчас**, это часть 2.

### 6. Завершение части 1
- commit: `fix(files): Wave 3.1 part 1 — HTTP upload/download layer, config dir, snake_case wire, WIP cleanup` → `git push origin main`.
- **Остановить все фоновые процессы** (API-сервер, cargo-джобы) — не оставлять висящие.
- Короткий отчёт в чате: что сделано, гейты, живые curl-статусы, номера тестов. **Не** STATE.md, **не** Telegram, **не** PROMPT_wave32 — ждём часть 2.

Начни с чтения контекста (CONVENTIONS/STATE/PLAN/отчёт верификации/PROMPT_wave31/перечисленные файлы), потом пиши код одним проходом, не перечитывай files, которые уже записал (write_file верифицирует).
