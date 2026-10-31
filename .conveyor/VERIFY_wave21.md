Ты работаешь в проекте **Besedka** (Rust/Axum бэкенд + Svelte 5 UI). Это **верификационный прогон** (read-only по коду): ты НЕ чинишь и НЕ дорабатываешь — только проверяешь и фиксируешь PASS/FAIL.
Если доступен скилл `besedka` — загрузи (стек/гейты). Иначе — ТОЛЬКО `.conveyor/CONVENTIONS.md`.

=== ЗАДАЧА: Независимая проверка Wave 2.1 (каналы + сообщения append: core + infra + API) ===
Контекст: 2.1 делалась по `.conveyor/PROMPT_wave21.md`, отчёт выполнения — `.conveyor/reports/wave21-report.md`. На HEAD уже лежат 2.2 и 2.3 (UI-чат, edit/delete/receipts) — это НОРМАЛЬНО. Задача: доказать, что **функциональность 2.1 на текущем HEAD** реально на месте: код, миграция V4, тесты, гейты, коммиты, push.
Важно: точные числа тестов из отчёта 2.1 (166) устарели — текущий прогон будет больше. НЕ привязывайся к старому числу; требуй: (а) все текущие тесты зелёные, (б) конкретные тесты 2.1 присутствуют (ищи по именам/содержимому: create→read, пагинация/курсор/границы лимитов, FK-каскад, пустая комната, приёмочный поток admin→пост→чтение, 403 не-участник, 403 не-админ, 400 текст, пагинация 2 страницы) и проходят.

ОЖИДАЕМОЕ СОСТОЯНИЕ (что проверяем):
- Коммиты: `89b7b39` (feat messages) и `7f52668` (state) — видны в `git log`.
- Core: `Message { id, room_id, sender_id, text, created_at }` (`crates/besedka-core/src/message.rs`), время — RFC 3339 UTC с миллисекундами; «канал» = `RoomKind::Channel` (отдельной таблицы каналов нет — это по дизайну, не FAIL).
- Migrations: `V4__messages.sql` → `messages(id PK AUTOINCREMENT, room_id FK→rooms CASCADE, sender_id FK→users CASCADE, text NOT NULL, created_at)` + индексы `(room_id, id)` и `sender_id`.
- Infra: `Db::create_message`, `Db::list_messages(room_id, limit, before_id)` (свежие N, курсор `before_id`, лимит 50/200), `Db::get_message`.
- API: `POST /api/v1/rooms` (admin; `{kind: room|channel, title 1..128}` → 201, не-админ → 403, создатель — участник) · `POST /rooms/{id}/messages` (участник; text 1..4096 trim → 201; не-участник → 403; комнаты нет → 404; пустой/длинный → 400) · `GET /rooms/{id}/messages?limit=&before_id=` (участник → `{"messages":[...]}`, свежие N; пустая комната → `{"messages":[]}`).
- Гейты: build/test/clippy/fmt зелёные (текущее состояние), UI в 2.1 не менялся → npm-гейты не обязательны, но svelte-check/build на HEAD должны быть зелёными (2.2/2.3 их гоняли).

ШАГИ (строго по порядку):

1. **Git — ничего не потеряно:**
   - `git log --oneline -12` → видны `89b7b39`, `7f52668` (позже — `08827a0`, `a97e1ed` и новые);
   - `git status --short` → пусто (чистое дерево);
   - `git remote -v` → origin = `https://github.com/SasLord/besedka.git`;
   - `git rev-parse HEAD` == `git ls-remote origin main | awk '{print $1}'` → **локаль = GitHub**;
   - `git show --stat 89b7b39` → в составе: core message.rs, V4-миграция, infra db.rs, app http — сверить с ожидаемым.

2. **Статические гейты (перезапуск, всё зелёное):**
   - `cargo build` · `cargo test --workspace` → **0 failed** (число passed — любое ≥ 166; приложить вывод);
   - `cargo clippy --all-targets -- -D warnings` → 0 · `cargo fmt --check` → чистый;
   - `cd ui && npm run check` → 0 errors · `npm run build` → OK (Node 24 через nvm);
   - **PITFALL:** НЕ чистить `ui/node_modules`, НЕ переустанавливать зависимости (два node: hermes x64 / nvm arm64 — нужны ОБА бинарника @rollup/@esbuild).

3. **Свежая БД — миграции без ошибок (scratch-путь, НЕ базовая БД проекта):**
   - `BESEDKA_DB_PATH=<scratch>/fresh21.db BESEDKA_PORT=8081 cargo run -p besedka-app` (background, лог в scratch) → стартует, V1–V5 (V4 обязательно) без ошибок в логе;
   - `sqlite3 <db> ".tables"` → `users`, `rooms`, `room_members`, `messages` (+ `room_read_cursors`, `edited_at` — от 2.3, это НОРМА);
   - `sqlite3 <db> "PRAGMA table_info(messages)"` → обязательные колонки 2.1: `id`, `room_id`, `sender_id`, `text`, `created_at` (`edited_at` от 2.3 — допустим, пометить в отчёте);
   - `sqlite3 <db> "SELECT name, sql FROM sqlite_master WHERE type='index' AND tbl_name='messages';"` → индексы по `(room_id, id)` и `sender_id`;
   - FK-каскад проверить кодом/запросом: создать комнату+сообщение → удалить комнату → `SELECT count(*) FROM messages` = 0.

4. **Живой API-сценарий (curl, пароль в запросе в отчёте писать `[REDACTED]`):**
   - register `alice`, `bob`, `carol` → 201; login всех;
   - admin: `UPDATE users SET role='admin' WHERE username='alice';` (scratch-БД — допустимая установка, зафиксировать в отчёте);
   - alice: `POST /rooms` `{kind: "channel", title: "verify21"}` → **201**; bob: `POST /rooms` → **403** (не-админ);
   - bob: `POST /rooms/1/join` → 200; carol — НЕ join;
   - bob: `POST /rooms/1/messages` `{"text":"hello 21"}` → **201** + id; carol: тот же пост → **403** (не-участник);
   - bob: пост с пустым текстом → **400**; текст > 4096 символов → **400**;
   - bob: `GET /rooms/1/messages` → `{"messages":[...]}` содержит «hello 21» (имя отправителя НЕ ожидается — это ограничение 2.1, зафиксировать как ОК);
   - **пагинация:** скриптом (curl-цикл) создать ещё ~55 сообщений от bob; `GET …?limit=25` → ровно 25, свежие (порядок — сверить с документированным: свежие N); `GET …?limit=25&before_id=<id_старейшего_из_первой_страницы>` → следующие 25, без пересечений id; `limit=999` → не больше 200 (максимум);
   - `GET /rooms/999/messages` → **404** (комнаты нет).

5. **UI:** чата в 2.1 **нет по дизайну** (это 2.2) — UI-смоук НЕ требуется; только гейты из шага 2. Если в UI вдруг найдётся чат от 2.1 — зафиксировать как отклонение от среза.

6. **Отчёт:** `.conveyor/reports/wave21-verify-report.md` — таблица `пункт | ожидалось | факт | PASS/FAIL` + ключевые выводы команд (обрезанные, без паролей). Вывод строкой:
   - все PASS → **«Wave 2.1 верифицирован, ничего не потеряно»**;
   - есть FAIL → перечислить каждый (команда, ожидалось, факт) и **остановиться** (ничего не чинить — фикс в отдельной сессии по отчёту).

7. `git add .conveyor/reports/wave21-verify-report.md` · commit "docs(verify): Wave 2.1 verification — <PASS|FAIL>" · `git push origin main` (проверить локаль == GitHub).

8. **Telegram (попробовать, не навязывать):**
   `hermes send -t telegram --subject "Besedka Wave 2.1 — VERIFIED ✅" --file .conveyor/reports/wave21-verify-report.md`
   Сеть на `api.telegram.org` может быть закрыта (таймаут) — максимум 1 повтор, зафиксировать в отчёте «Telegram: не доставлено (сеть)». Это НЕ считается FAIL.

9. **Перед завершением:** остановить все фоновые процессы (API-сервер, vite, cargo-джобы) — не оставлять висящие.

Начни с шага 1.
