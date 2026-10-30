Ты работаешь в проекте **Besedka** (Rust/Axum бэкенд + Svelte 5 UI). Конвейер: строго по срезам, гейты обязательны, в конце отчёт в Telegram.
Если доступен скилл `besedka` — загрузи его (workflow + гейты). Иначе стек/правила — ТОЛЬКО в `.conveyor/CONVENTIONS.md` (NEVER доверять памяти).

=== ЗАДАЧА: Wave 2.3 — edit/delete своего сообщения + read receipts ===
Уже сделано и на это опирайся:
- **Волна 1 ✅**: AD mock + парольный auth, комнаты + join/leave, бейджи ролей, сайдбар комнат (ui/src/App.svelte, Svelte 5 runes: `$state`/`$derived`).
- **2.1 ✅** (коммиты `89b7b39`, `7f52668`): таблица `messages` (V4), `Message {id, room_id, sender_id, text, created_at}` (время — RFC 3339 UTC с миллисекундами). API сообщений есть: пост `POST /rooms/{id}/messages` `{text 1..4096}` → 201; чтение `GET /rooms/{id}/messages?limit=&before_id=` → `{"messages":[...]}` (лимит 50 по умолч., макс 200).
- **2.2 ✅** (коммиты `08827a0`, отчёт `45ef2a5`):
  - `GET /api/v1/users` (AuthedUser) → `{"users":[{"id","display_name"},...]}` — каталог имён.
  - UI: `ui/src/lib/components/RoomChat.svelte` — чат комнаты: пост (Enter/кнопка, trim, без дублей), история 50, «Загрузить ещё» (курсор `before_id`), live-поллинг 3 с (пауза при скрытой вкладке), имена из каталога с live-перечитыванием, не-участник → «Присоединиться».
  - `api.ts`: тип `Message`, `listMessages(roomId, limit?, beforeId?)`, `postMessage`, `fetchUsers`.
  - Гейты зелёные: **170 тестов** (app 69 / core 12 / infra 89), clippy/fmt/svelte-check/build чистые.
- UI-паттерны: api-клиент `ui/src/lib/api.ts` (типы + `fetch*`-функции, `ApiError`), стили — ТОЛЬКО токены `ui/src/app.css`.

=== Срез (Accept из PLAN.md 2.3) ===
Свой пост можно отредактировать и удалить; на своих сообщениях видны отметки прочтения (кто прочитал).

ШАГИ (строго по порядку):

1. Контекст (прочитать ВСЁ прежде чем писать):
   - ~/Projects/besedka/.conveyor/CONVENTIONS.md · STATE.md (2.2 DONE, 2.3 — дальше) · PLAN.md (секция 2.3)
   - ui/src/App.svelte · ui/src/lib/components/RoomChat.svelte · ui/src/lib/api.ts · ui/src/app.css (токены)
   - crates/besedka-app/src/http/mod.rs + lib.rs (точные JSON-формы, роуты) · crates/besedka-core/src/message.rs · crates/besedka-infra/src/db.rs (методы list_messages/get_message — как считать) · migrations/V1..V4 (формат миграций)

2. APP (бэкенд, МИНИМАЛЬНО):
   - **Миграция V5** `migrations/V5__edit_reads.sql`:
     * `ALTER TABLE messages ADD COLUMN edited_at TEXT;` (NULL = не редактировалось)
     * `CREATE TABLE room_read_cursors (room_id INTEGER NOT NULL REFERENCES rooms(id) ON DELETE CASCADE, user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE, last_read_id INTEGER NOT NULL, PRIMARY KEY (room_id, user_id));`
   - **Ядро:** в `Message` поле `edited_at: Option<...>` (serde: `skip_serializing_if = "Option::is_none"`, `#[serde(default)]` — старые JSON без поля читаются).
   - **Инфра (методы + тесты в db.rs):**
     * `Db::update_message_text(message_id, room_id, new_text) -> Option<Message>` (UPDATE ... SET text=..., edited_at=now WHERE id=? AND room_id=?; возвращает обновлённое)
     * `Db::delete_message(message_id, room_id) -> bool` (DELETE WHERE id=? AND room_id=?)
     * `Db::set_read_cursor(room_id, user_id, last_read_id)` (UPSERT: курсор = max(текущий, переданный))
     * `Db::latest_message_id(room_id) -> Option<i64>`
     * `Db::message_readers(room_id, message_id) -> Vec<(i64, String)>` (участники комнаты, чей курсор ≥ message_id; JOIN users → display_name; ORDER BY display_name)
   - **HTTP (только участник комнаты; иначе 403):**
     * `PATCH /api/v1/rooms/{rid}/messages/{mid}` `{text 1..4096}` → 200 `{"message": {...}}` — ТОЛЬКО отправитель (иначе 403; чужое сообщение → 403; нет сообщения/чужая комната → 404; пустой text → 400).
     * `DELETE /api/v1/rooms/{rid}/messages/{mid}` → 204 — ТОЛЬКО отправитель (403/404 аналогично).
     * `POST /api/v1/rooms/{rid}/read` → 200 `{"last_read_id": N}` — ставит курсор = max(текущий, latest_message_id); комната без сообщений → last_read_id = текущий курсор (или 0).
     * `GET /api/v1/rooms/{rid}/messages/{mid}/readers` → `{"users":[{"id","display_name"},...]}` — кто прочитал (курсор ≥ mid).
     * HTTP-тесты на каждый: сценарии «свой/чужой/нет сообщения/не-участник» + тест «readers видят второго юзера после POST /read».
   - Больше бэкенд НЕ трогаем.

3. UI (главная часть):
   - `ui/src/lib/api.ts`: `editMessage(roomId, msgId, text)`, `deleteMessage(roomId, msgId)` (204), `markRoomRead(roomId)`, `messageReaders(roomId, msgId)`.
   - `RoomChat.svelte`:
     * **Edit (только свои сообщения):** при наведении — мелкие кнопки «изменить»/«удалить». Edit → инлайн-редактор (поле с текущим текстом): Enter/Сохранить → PATCH; Esc/Отмена — без запроса. После 200 заменить текст на месте + метка «изменено» (edited_at != null). Ошибку (403/400) — через ApiError в ту же error-полосу, что и пост.
     * **Delete (только свои):** подтверждение в 2 клика (кнопка → «Точно?»), после 204 убрать из списка.
     * **Read receipts (только НА своих сообщениях):** маленькая отметка (✓) + счётчик; клик → список «Прочитали: Имя1, Имя2…» (GET readers лениво, по клику). Чужие сообщения — отметок НЕТ.
     * **Ставить прочтение:** при открытии комнаты И при каждом поллинге, если пришли новые сообщения (комната открыта и вкладка видна — пользователь «в чате») → `POST /rooms/{rid}/read`.
     * Чужие сообщения: без кнопок edit/delete вообще (не показывать).
   - Дизайн: только токены `ui/src/app.css`, никаких новых цветов/шрифтов.
   - TypeScript: `npm run check` без ошибок.

4. ГЕЙТЫ (все ДО commit; UI меняем → npm-гейты ОБЯЗАТЕЛЬНЫ):
   - в ui/: `npm run check` = 0 ошибок · `npm run build` OK
   - бэкенд: `cargo build` · `cargo test --workspace` (зелёные, включая новые) · `cargo clippy --all-targets -- -D warnings` = 0 · `cargo fmt --check`
   - **PITFALL:** НЕ чистить `ui/node_modules`, НЕ переустанавливать зависимости (два node: hermes x64 / nvm arm64 — нужны ОБА бинарника @rollup/@esbuild darwin-x64+arm64)
   - Рендер-проверка: поднять API (`cargo run -p besedka-app`, dev-БД в scratch) + `npm run dev` (в ui/), двумя сессиями/юзерами:
     * юзер A постит → юзер B открывает комнату → у A на сообщении появляется отметка с именем B;
     * A редактирует свой пост → текст сменился + «изменено»; A удаляет → пропало;
     * B пытается PATCH/DELETE чужое → 403 (проверить в консоли/Network);
     * не-участник → 403 на всех четырёх эндпоинтах.
     Скриншоты в .conveyor/screenshots/ (если удобно).

5. commit: "feat(chat): Wave 2.3 — edit/delete own message + read receipts"
   push:  git push origin main

6. STATE.md: отметить **2.3 DONE** (эндпоинты, миграция V5, UI, гейты, коммит) → commit "docs(state): Wave 2.3 DONE — edit/delete + read receipts" → push.

7. Отчёт в Telegram:
   hermes send -t telegram --subject "Besedka Wave 2.3 ✅ (edit/delete + read receipts)" --file <путь_к_отчёту.md>

8. **ОБЯЗАТЕЛЬНО (правило конвейера):** написать `.conveyor/PROMPT_wave24.md` для следующего среза — **Wave 3.1 [infra+app]: Загрузка/скачивание файлов (лимиты размера/типа)** (Accept из PLAN.md 3.1: upload→download→превью). Формат — как PROMPT_wave22.md / PROMPT_wave23.md: что уже сделано (включая 2.3), шаги, гейты, commit, STATE.md, Telegram, и шаг «написать PROMPT_wave25.md» (3.2 UI-превью). Закоммитить и запушить.

9. Перед завершением: **остановить все фоновые процессы** (API-сервер, vite, cargo-джобы) — не оставлять висящие.

Начни с шага 1.
