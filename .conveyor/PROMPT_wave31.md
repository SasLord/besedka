Ты работаешь в проекте **Besedka** (Rust/Axum бэкенд + Svelte 5 UI). Конвейер: строго по срезам, гейты обязательны, в конце отчёт в Telegram.
Если доступен скилл `besedka` — загрузи его (workflow + гейты). Иначе стек/правила — ТОЛЬКО в `.conveyor/CONVENTIONS.md` (NEVER доверять памяти).

=== ЗАДАЧА: Wave 3.1 — загрузка/скачивание файлов (лимиты размера/типа) ===
Уже сделано и на это опирайся:
- **Волна 1 ✅**: AD mock + парольный auth, комнаты + join/leave, бейджи ролей, сайдбар комнат (ui/src/App.svelte, Svelte 5 runes: `$state`/`$derived`).
- **2.1 ✅** (коммит `89b7b39`): таблица `messages` (V4), `Message` (время — RFC 3339 UTC с миллисекундами), API сообщений (пост/чтение, курсорная пагинация `limit`/`before_id`).
- **2.2 ✅** (коммит `08827a0`): `GET /api/v1/users`; UI-чат `RoomChat.svelte` (пост, история 50, «Загрузить ещё», live-поллинг 3 с с паузой на скрытой вкладке, имена из каталога).
- **2.3 ✅** (коммит `a97e1ed`, отчёт `.conveyor/reports/wave23-report.md`, коммит `9b58d21`):
  - Миграция `V5__edit_reads.sql`: `messages.edited_at`/`deleted_at` + таблица `message_readers` + курсор `room_read_cursors`.
  - `PATCH /rooms/{id}/messages/{mid}` (только автор), `DELETE` (только автор, **204 без тела** через хелпер `JsonOrEmpty`), `POST /rooms/{id}/read`, `GET /rooms/{id}/messages/{mid}/readers`, пагинация `?after_id=`.
  - UI: инлайн-edit («(изменено)»), двухклик-удаление («Удалить»→«Точно?»), read receipts «✓ N» + «Прочитали: …», отметка прочтения.
  - Гейты зелёные: **177 тестов** (app 71 / core 12 / infra 94), clippy/fmt/svelte-check/build чистые.
- UI-паттерны: api-клиент `ui/src/lib/api.ts` (типы + `fetch*`-функции, `ApiError`), стили — ТОЛЬКО токены `ui/src/app.css`.
- **PITFALL (важно):** НЕ чистить `ui/node_modules`, НЕ переустанавливать зависимости (два node: hermes x64 / nvm arm64 — нужны ОБА бинарника @rollup/@esbuild darwin-x64+arm64).

=== Срез (Accept из PLAN.md 3.1) ===
Загрузка/скачивание файлов (лимиты размера/типа). **Accept:** upload → download → превью.
Срез **бэкенд** ([infra+app]): файл можно загрузить через API и скачать; браузер может открыть картинку/PDF напрямую (inline-serve). UI-превью в чате = 3.2.

ШАГИ (строго по порядку):

1. Контекст (прочитать ВСЁ прежде чем писать):
   - ~/Projects/besedka/.conveyor/CONVENTIONS.md · STATE.md (2.3 DONE, 3.1 — дальше) · PLAN.md (секция Wave 3)
   - crates/besedka-app/src/http/mod.rs + lib.rs (роуты, хелпер `JsonOrEmpty`, паттерны ролевых гейтов) · crates/besedka-core/ (как устроены доменные типы) · crates/besedka-infra/src/db.rs (методы + тесты, как писать) · migrations/V1..V5 (формат миграций)
   - config.toml + как читаются env-оверрайды `BESEDKA_*` (CONVENTIONS.md)
   - ui/src/lib/api.ts + RoomChat.svelte (на будущее для 3.2, в 3.1 UI почти не трогаем)

2. CORE (`besedka-core`):
   - Тип `File { id, uploader_id, filename, content_type, size_bytes, created_at }` + доменные правила:
     * `MAX_FILE_SIZE` = 25 MiB (26214400) — константа, но из конфига (см. шаг 4);
     * допустимые `content_type`: `image/png`, `image/jpeg`, `image/gif`, `image/webp`, `application/pdf`;
     * sanitize filename (убрать пути/спецсимволы, пустое → `"file"`), максимум 255 символов.
   - Тесты правил: граница размера (ровно 25 MiB ок, +1 байт — reject), допустимый/недопустимый тип, sanitize (примеры: `../../etc/passwd`, `a b/c.txt`, пустая строка).

3. INFRA (`besedka-infra`):
   - **Миграция V6** `migrations/V6__files.sql`:
     `CREATE TABLE files (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        uploader_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        filename TEXT NOT NULL,
        content_type TEXT NOT NULL,
        size_bytes INTEGER NOT NULL,
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
     );` + индекс по `uploader_id`.
   - Методы + тесты в db.rs: `Db::create_file(uploader_id, filename, content_type, size_bytes) -> File` · `Db::get_file(file_id) -> Option<File>` · `Db::delete_file(file_id) -> bool`.

4. APP (`besedka-app`) — только API (UI-превью = 3.2):
   - **Конфиг:** секция `[files]` в `config.toml`: `dir` (путь к хранилищу, дефолт `data/files`) + `max_size_bytes = 26214400`; env-оверрайды `BESEDKA_FILES_DIR` / `BESEDKA_FILES_MAX_SIZE_BYTES` (по паттерну существующих).
   - **Хранилище:** бинарники на диске в `{dir}/<id>_<safe_filename>` (id в имени — коллизии исключены; каталог создавать при запуске, если нет). В БД — только метаданные.
   - **HTTP** (AuthedUser; ролевых гейтов по комнатам НЕТ — файлы пока не привязаны к комнатам):
     * `POST /api/v1/files` — multipart, поле `file` (axum `Multipart`). Валидация: пустое/нет поля → 400; `size_bytes > max_size_bytes` → **413** `{"error":{"code":"file_too_large","message":...}}`; content_type вне списка → **415** `{"error":{"code":"type_not_allowed",...}}`; успех → **201** `{"file":{"id","filename","content_type","size_bytes","url":"/api/v1/files/{id}"}}`.
       * PITFALL: размер проверять ДО полного чтения в память (ограничить reader / считать на лету, прервать при превышении) — не грузить гигабайты в RAM.
     * `GET /api/v1/files/{id}` — отдаёт бинарник: `Content-Type` из БД, `Content-Disposition: inline; filename="<safe>"` (для кириллицы — `filename*=UTF-8''...` по RFC 5987), `Content-Length`; нет файла → 404; файл в БД есть, но на диске отсутствует → 410 (и не паниковать).
   - **HTTP-тесты:** upload→download roundtrip (байты совпадают, имя/тип/размер совпадают), 413 (файл чуть больше лимита — в тесте лимит закрутить в конфиге до ~1 KiB), 415 (text/plain), 400 (пустой multipart), 404 (нет id), кириллическое имя filename → `filename*` в Content-Disposition.
   - Больше бэкенд НЕ трогаем (привязка file↔message, UI — в 3.2).

5. UI — МИНИМАЛЬНО (полное превью = 3.2):
   - `ui/src/lib/api.ts`: тип `File`, `uploadFile(file: File, onProgress?: (loaded: number) => void) -> Promise<File>` (multipart, XHR/fetch для progress), `fileUrl(file: File)`.
   - RoomChat/App UI в этом срезе НЕ меняем (в 3.2 добавим кнопку «📎» и превью).

6. ГЕЙТЫ (все ДО commit):
   - бэкенд: `cargo build` · `cargo test --workspace` (зелёные, включая новые) · `cargo clippy --all-targets -- -D warnings` = 0 · `cargo fmt --check`
   - ui: `npm run check` = 0 ошибок · `npm run build` OK (api.ts меняем — гейты ОБЯЗАТЕЛЬНЫ)
   - **PITFALL:** НЕ чистить `ui/node_modules`, НЕ переустанавливать зависимости.
   - Живая проверка (обязательна): поднять API (`cargo run -p besedka-app`, dev-БД в scratch), в curl:
     * `curl -F file=@test.png` (создать тестовый PNG) → 201 + url;
     * открыть url в браузере → картинка отображается (превью);
     * тот же curl на .txt → 415; большой файл (dd 30 MiB) → 413;
     * скачать → байты идентичны исходнику (sha256).
     Скриншот превью в браузере → `.conveyor/screenshots/31-file-preview.png` (если удобно).

7. commit: "feat(files): Wave 3.1 — file upload/download (limits + inline serve)"
   push:  git push origin main

8. STATE.md: отметить **3.1 DONE** (эндпоинты, миграция V6, конфиг, гейты, коммит) + в «Дальше» — WAVE 3: 3.1 ✅, 3.2 → (UI-превью). → commit "docs(state): Wave 3.1 DONE — file upload/download" → push.

9. Отчёт в Telegram:
   hermes send -t telegram --subject "Besedka Wave 3.1 ✅ (файлы: upload/download)" --file .conveyor/reports/wave31-report.md
   (отчёт — в `.conveyor/reports/wave31-report.md`, формат как wave23-report.md)

10. **ОБЯЗАТЕЛЬНО (правило конвейера):** написать `.conveyor/PROMPT_wave32.md` для следующего среза — **Wave 3.2 [ui]: Превью файлов в чате (image/PDF)** (Accept из PLAN.md 3.2: картинка инлайн, PDF-ссылка). Смысл: привязка file↔message (новая колонка/таблица, если надо), кнопка «📎» в RoomChat (drag&drop — бонус), инлайн-картинка (клик → полный размер), PDF — ссылка-превью, в списке сообщений. Формат — как PROMPT_wave23.md / PROMPT_wave31.md: что уже сделано (включая 3.1), шаги, гейты, commit, STATE.md, Telegram, и шаг «написать PROMPT_wave41.md» (4.1 поиск FTS5). Закоммитить и запушить.

11. Перед завершением: **остановить все фоновые процессы** (API-сервер, vite, cargo-джобы) — не оставлять висящие.

Начни с шага 1.
