Ты работаешь в проекте **Besedka** (Rust/Axum бэкенд + Svelte 5 UI). Конвейер: строго по срезам, гейты обязательны, в конце отчёт в Telegram.
Если доступен скилл `besedka` — загрузи его (workflow + гейты). Иначе стек/правила — ТОЛЬКО в `.conveyor/CONVENTIONS.md` (NEVER доверять памяти).

=== ЗАДАЧА: Wave 2.1 — каналы + сообщения (append) в БД (core + infra + минимум app) ===
Уже сделано и на это опирайся:
- **Волна 1 ПОЛНОСТЬЮ завершена** ✅: парольный auth (1.1) · AD/Kerberos SSO + mock (1.2) · список комнат + join/leave + ролевые гейты `AuthedUser`/`AdminUser` (1.3) · UI: сайдбар комнат, логин (AD mock + пароль), бейджи ролей.
- БД: `users`, `rooms` (V1), auth (V2), `room_members` (V3). **Сообщений и HTTP-создания комнат НЕТ — это и есть 2.1.**
- `Db::create_room(kind, title)` в infra УЖЕ есть (используется в тестах) — HTTP-эндпоинта над ней ещё нет.

ШАГИ (строго по порядку):

1. Контекст (прочитать ВСЁ прежде чем писать):
   - ~/Projects/besedka/.conveyor/CONVENTIONS.md
   - ~/Projects/besedka/.conveyor/STATE.md    (Wave 1 DONE, 2.1 — дальше)
   - ~/Projects/besedka/.conveyor/PLAN.md     (секция 2.1: «Каналы + сообщения (append) в БД. Accept: создать канал, пост, чтение»)
   - Код: crates/besedka-core/src/ (User/Room/Role — куда положить Message) · crates/besedka-infra/src/db.rs + migrations/ (паттерны V1..V3) · crates/besedka-app/src/http/mod.rs (паттерны: AuthedUser/AdminUser, spawn_blocking, HttpError) · crates/besedka-app/src/lib.rs (роуты, тесты)

2. CORE (besedka-core):
   - `Message` struct: `id, room_id, sender_id, text, created_at` (формат времени — решить и задокументировать в core; единый для всего проекта)
   - «Канал» = комната вида `RoomKind::Channel`/`Room` (таблицу каналов НЕ выдумывать — комнаты уже типизированы)
   - (на будущее 2.3: пометь в комментарии поля, которые появятся — edited_at, deleted_by — но в 2.1 их НЕТ)

3. INFRA (besedka-infra):
   - миграция `V4__messages.sql`: `messages(id INTEGER PK AUTOINCREMENT, room_id NOT NULL REFERENCES rooms(id) ON DELETE CASCADE, sender_id NOT NULL REFERENCES users(id) ON DELETE CASCADE, text TEXT NOT NULL, created_at ...)` + индексы: `(room_id, id)` (пагинация), `sender_id`
   - `Db::create_message(room_id, sender_id, text) -> i64`
   - `Db::list_messages(room_id, limit, before_id) -> Vec<Message>` — курсорная пагинация (direction и порядок — решить и задокументировать; лимит по умолчанию 50, max 200)
   - `Db::get_message(id) -> Option<Message>`
   - тесты: create→read · пагинация (границы лимитов, курсор) · FK (удалили комнату → сообщения пропали) · пустая комната → []

4. APP (besedka-app) — только API (чат в UI — это 2.2, НЕ смешивать):
   - `POST /api/v1/rooms` (AdminUser; body `{kind, title}`; kind ∈ room|channel; title 1..128) → 201 + комната; не-админ → 403
   - `POST /api/v1/rooms/{id}/messages` (AuthedUser, **должен быть в комнате** — иначе 403; текст 1..4096, trim) → 201 + сообщение; комнаты нет → 404
   - `GET /api/v1/rooms/{id}/messages?limit=&before_id=` (AuthedUser, участник) → `{messages: [...]}`; комнаты нет → 404
   - HTTP-тесты: создать канал (admin) → пост → чтение · пост не-участника → 403 · создать комнату не-админом → 403 · текст пустой/слишком длинный → 400 · пустая комната → `{messages: []}` · пагинация (2 страницы)

5. ГЕЙТЫ (все ДО commit):
   - `cargo build` OK · `cargo test` зелёные (включая новые message-тесты) · `cargo clippy --all-targets -- -D warnings` = 0
   - UI в этом срезе НЕ трогаем → npm-гейты не нужны (если вдруг изменишь ui/ — тогда `npm run build` + `npm run check`)
   - **PITFALL:** НЕ чисти `ui/node_modules` (два node: hermes x64 / nvm arm64 — нужны оба rollup/esbuild бинарника)

6. commit: "feat(messages): Wave 2.1 — channels + messages append (core+infra+api)"
   push:  git push origin main

7. STATE.md: отметить **2.1 DONE** → зафиксировать (эндпоинты, таблица, тесты, гейты). Дальше — 2.2.

8. Отчёт в Telegram:
   hermes send -t telegram --subject "Besedka Wave 2.1 ✅ (каналы + сообщения в БД)" --file <путь_к_отчёту>

Начни с шага 1.
