Ты работаешь в проекте **Besedka** (Rust/Axum бэкенд + Svelte 5 UI). Конвейер: строго по срезам, гейты обязательны, в конце отчёт в Telegram.
Если доступен скилл `besedka` — загрузи его (workflow + гейты + питфоллы). Иначе — ТОЛЬКО `.conveyor/CONVENTIONS.md` (NEVER доверять памяти).

=== ЗАДАЧА: Исправить/дописать Wave 3.1 (файлы: upload/download) по отчёту верификации ===
Верификация (`wave31-verify-report.md`) завершилась **FAIL**: core/infra сделаны и зелёные, но **воркспейс не компилируется** (`E0433: cannot find 'files' in 'http'`), HTTP-слой 3.1 отсутствует, UI-шаг не выполнен, конвейерные шаги (гейты/живая проверка/коммит/STATE/PROMPT_wave32) пропущены. Отчёт: **`.conveyor/reports/wave31-verify-report.md`** — прочитай ВПЕРВЫХ, это источник правды. Исходная спека: `.conveyor/PROMPT_wave31.md`.

## Что УЖЕ есть и НЕ ТРОГАТЬ (проверено, зелёное):
- `crates/besedka-core/src/file.rs` — тип `File`, `MAX_FILE_SIZE_DEFAULT` (25 MiB), `ALLOWED_CONTENT_TYPES`, `validate_upload(size_bytes, max_size_bytes, content_type)`, `sanitize_filename`, 13 тестов. **Правка:** только убрать `#[serde(rename_all = "camelCase")]` с `File` (FAIL-5) — остальное как есть.
- `migrations/V6__files.sql` — таблица `files` + `idx_files_uploader`. **НЕ править** (FK-решение — см. ниже, миграция остаётся как есть).
- `crates/besedka-infra/src/db.rs` — `create_file/get_file/delete_file` + 3 теста. Не трогать.
- `http/mod.rs` — хелперы `HttpError::payload_too_large` / `unsupported_media_type` (использовать их; комментарий «Wave 3.4» поправить на «Wave 3.1», FAIL-7).
- `Cargo.toml` — axum feature `multipart` + multer в Cargo.lock. Не трогать.
- Базовые числа тестов: core 28, infra 99 (оба зелёные); app — сейчас не запускается.

## FAIL из отчёта и РЕШЕНИЕ по каждому (выполнять именно так):

| # | FAIL | Решение |
|---|---|---|
| 1 | Воркспейс не компилируется: `lib.rs:86` → `http::files::upload_file` (модуля нет); `main.rs` → `cfg.files.file_upload_max_bytes` (поля нет) | **КОД:** создать `crates/besedka-app/src/http/files.rs` (FAIL-2) + `pub mod files;` в `http/mod.rs`; передать конфиг по-честному (FAIL-3). Критерий: `cargo check --workspace` зелёный. |
| 2 | HTTP-слой 3.1 отсутствует (эндпоинты, 4xx, inline-serve, disk, тесты) | **КОД:** реализовать полностью по шагу 4 исходного `PROMPT_wave31.md` (там точные статус-коды, тексты ошибок, RFC 5987, PITFALL про размер до чтения). |
| 3 | Неверный маршрут `/api/v1/rooms/{id}/files` + `AppState.file_upload_max_bytes: usize` + `app()` принимает сырой usize | **КОД:** убрать room-маршрут; зарегистрировать `POST /api/v1/files` и `GET /api/v1/files/{id}` (AuthedUser, ролевых гейтов НЕТ); `AppState` получает `files: FilesConfig`; сигнатура `app(db, sessions, ad, files: FilesConfig)`; `main.rs` — `app(db, cfg.sessions, ad, cfg.files)`. После: `grep -rn "file_upload_max_bytes" crates/` = 0 совпадений. |
| 4 | Конфиг неполный: нет `dir` и env-оверрайдов | **КОД:** `FilesConfig { dir: String (дефолт `"data/files"`), max_size_bytes: u64 (дефолт из core) }`; в `Config::load` оверрайды `BESEDKA_FILES_DIR` / `BESEDKA_FILES_MAX_SIZE_BYTES` — паттерн существующих (config.rs:148-156, `if let Ok(..) = std::env::var(..)`); тесты: дефолты + кастомные значения + env-оверрайт. |
| 5 | Wire-формат: `File` camelCase, проект snake_case | **КОД:** убрать `#[serde(rename_all = "camelCase")]` с `File`; тело 201 = `{"file":{"id","uploader_id","filename","content_type","size_bytes","created_at","url":"..."}}` (snake_case); `File` в api.ts — те же snake_case-поля. |
| 6 | UI-шаг не выполнен: api.ts без `File`/`uploadFile`/`fileUrl` | **КОД:** в `ui/src/lib/api.ts`: тип `File` (snake_case, как на бэкенде), `uploadFile(file: File, onProgress?: (loaded: number) => void): Promise<File>` (multipart-POST на `/api/v1/files`, поле `file`; XHR/fetch — прогресс обязателен), `fileUrl(f: File): string` → `/api/v1/files/{id}`. RoomChat/App — НЕ трогать (превью = 3.2). |
| 7 | «Wave 3.4» и захардкоженные 10 MiB (спека: 25 MiB из конфига) | **КОД:** все комментарии «Wave 3.4» → «Wave 3.1»; тесты lib.rs без хардкода 10 MiB — лимит приходит из `FilesConfig` (в тестах можно закрутить до ~1 KiB, это и есть смысл «лимит — параметр»). Критерий: `grep -rn "Wave 3.4\|10 \* 1024 \* 1024" crates/` = 0 совпадений. |
| 8 | Конвейерные шаги пропущены (гейты/живая/скрин/отчёт/STATE/wave32/Telegram) | **ШАГИ 5-9 ниже.** |
| 9 | Мусор вне среза: `crates/besedka-app/src/ws/` («Wave 2.4» — нет в PLAN.md, `handler.rs` отсутствует) и `.conveyor/patches/uncommitted-direct-room-WIP.patch` | **УДАЛИТЬ** (untracked, `rm -rf`). WS — не срез 3.1 и не в PLAN.md; если понадобится — отдельная волна по PLAN, а не мёртвый WIP в дереве. |

## ⚖️ Решение по отклонению (задокументировать, код НЕ менять):
**FK на `files.uploader_id`:** исходный промпт 3.1 писал `REFERENCES users(id) ON DELETE CASCADE`; реализовано БЕЗ FK — осознанный дизайн (каскад удалил бы метаданные, оставив сиротские байты на диске; байты владеет app-слой). Зафиксировать строкой в `wave31-report.md` (секция «Решения/отклонения») + в STATE.md, чтобы следующий верификатор не ставил это в FAIL (прецедент: wave23-fix FAIL 2/3).

## ШАГИ (строго по порядку):

1. **Контекст (прочитать ВСЁ):**
   - `.conveyor/CONVENTIONS.md` · `.conveyor/STATE.md` · `.conveyor/PLAN.md` (Wave 3)
   - `.conveyor/reports/wave31-verify-report.md` (источник правды) · `.conveyor/PROMPT_wave31.md` (исходная спека: статус-коды, тексты ошибок, RFC 5987, живая проверка)
   - Код: `crates/besedka-core/src/file.rs` (сигнатуры — не переименовывать) · `crates/besedka-app/src/config.rs` + `lib.rs` + `main.rs` + `http/mod.rs` + `http/extract.rs` (`AuthedUser`) · `crates/besedka-infra/src/db.rs` (file-методы) · `ui/src/lib/api.ts`

2. **Реализовать** FAIL 1-7 (код + тесты), **удалить** мусор (FAIL 9), **задокументировать** FK-решение.

3. **Новые HTTP-тесты (обязательны, без них срез не закрыт):**
   - upload→download roundtrip: байты совпадают (сравнивать `sha256`/полный массив), имя/тип/размер совпадают;
   - 413: файл чуть больше лимита (в тесте лимит закрутить до ~1 KiB через `FilesConfig`);
   - 415: `text/plain`;
   - 400: пустой multipart / нет поля `file`;
   - 404: `GET /api/v1/files/999999`;
   - кириллическое имя → `Content-Disposition: inline; filename*=UTF-8''...`;
   - 410: строка есть в БД, файла на диске нет (в тесте удалить файл с диска) — и без паники.
   - Старые тесты core 28 / infra 99 остаются зелёными; число в app растёт.

4. **ГЕЙТЫ (все ДО commit):**
   - `cargo build` OK · `cargo test --workspace` — 0 failed · `cargo clippy --all-targets -- -D warnings` = 0 · `cargo fmt --check` чистый;
   - UI тронут (api.ts) → `cd ui && npm run check` = 0 ошибок · `npm run build` OK;
   - **PITFALL:** НЕ чистить `ui/node_modules`, НЕ переустанавливать зависимости (два node: hermes x64 / nvm arm64 — нужны ОБА бинарника @rollup/@esbuild darwin-x64+arm64).
   - **PITFALL:** миграция V6 НЕ применена в dev-БД (схема на V5) — НЕ переименовывать/пересоздавать; правки (если потребуются) только в unapplied-миграциях.

5. **Живая проверка (обязательна):** `cargo run -p besedka-app` (dev-БД), затем curl:
   - создать тестовый PNG (например, 1×1, `base64 -d` или `sips`); `curl -F file=@test.png` (с cookie логина — см. `references/dev-env.md` скилла) → **201** + `url`;
   - открыть `url` в браузере → картинка отображается (превью);
   - тот же запрос на `.txt` → **415**; большой файл (`dd` 30 MiB) → **413**; пустой multipart → **400**; `GET .../999999` → **404**;
   - скачать загруженный файл → байты идентичны исходнику (`shasum -a 256` на обоих);
   - скриншот превью → `.conveyor/screenshots/31-file-preview.png`;
   - после: `sqlite3 data/besedka.db "SELECT version FROM refinery_schema_history"` → на последнем месте **6**.

6. **commit** (всё разом, включая cleanup): `fix(files): Wave 3.1 — HTTP upload/download layer, config dir, snake_case wire, WIP cleanup` → `git push origin main`.

7. **STATE.md:** 3.1 → **DONE** (эндпоинты, V6, конфиг `[files] dir+max_size_bytes` + env-оверрайды, гейты, живая проверка, коммит; секция «Решения»: FK без каскада — осознанный дизайн) + «Дальше»: WAVE 3: 3.1 ✅, 3.2 → UI-превью. → commit `docs(state): Wave 3.1 DONE (fix) — file upload/download` → push.

8. **Отчёт:** `.conveyor/reports/wave31-report.md` (формат как `wave23-report.md`; обязательна секция «Решения/отклонения» с FK + «Wave 3.4 → 3.1» cleanup) → Telegram:
   `hermes send -t telegram --subject "Besedka Wave 3.1 ✅ (fix: файлы upload/download)" --file .conveyor/reports/wave31-report.md`
   (таймаут доставки — повторить 1 раз; всё равно не ушло → зафиксировать в отчёте, это НЕ FAIL).

9. **ПROMPT_wave32.md (ОБЯЗАТЕЛЬНО):** написать `.conveyor/PROMPT_wave32.md` — **Wave 3.2 [ui]: Превью файлов в чате (image/PDF)** (Accept из PLAN.md 3.2: картинка инлайн, PDF-ссылка): привязка file↔message (колонка/таблица — решить по PLAN), кнопка «📎» в RoomChat, drag&drop (бонус), инлайн-картинка (клик → полный размер), PDF — ссылка; опираться на готовый API 3.1 (POST/GET `/api/v1/files`, `uploadFile`/`fileUrl` из api.ts). Формат — как `PROMPT_wave31.md`/`PROMPT_wave23.md` (сделано, шаги, гейты, commit, STATE, Telegram, шаг «написать PROMPT_wave41.md» — 4.1 поиск FTS5). Закоммитить и запушить.

10. **Перед завершением: остановить все фоновые процессы** (API-сервер, vite, cargo-джобы) — не оставлять висящие.

Начни с шага 1.
