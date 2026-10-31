Ты работаешь в проекте **Besedka** (Rust/Axum бэкенд + Svelte 5 UI). Это **верификационный прогон** (read-only по коду): ты НЕ чинишь и НЕ дорабатываешь — только проверяешь и фиксируешь PASS/FAIL.
Если доступен скилл `besedka` — загрузи (стек/гейты). Иначе — ТОЛЬКО `.conveyor/CONVENTIONS.md`.

=== ЗАДАЧА: Независимая проверка Wave 2.2 (чат в UI: пост + список + пагинация + live-поллинг, каталог пользователей) ===
Контекст: 2.2 делалась по `.conveyor/PROMPT_wave22.md`, отчёт выполнения — `.conveyor/reports/wave22-report.md`. На HEAD уже лежит 2.3 (edit/delete/receipts) — это НОРМАЛЬНО и не мешает: базовые функции 2.2 (отправка, список, имена, пагинация, поллинг) должны работать как есть.
Важно: точные числа тестов из отчёта 2.2 (170) устарели — текущий прогон больше. НЕ привязывайся к старому числу; требуй: (а) все текущие тесты зелёные, (б) тесты 2.2 присутствуют (ищи: `list_users`, HTTP `GET /users`) и проходят.

ОЖИДАЕМОЕ СОСТОЯНИЕ (что проверяем):
- Коммиты: `08827a0` (feat: RoomChat + users) и `45ef2a5` (report) — видны в `git log`.
- Бэкенд: `GET /api/v1/users` (AuthedUser) → `{"users":[{"id","display_name"},...]}`; `Db::list_users()` в infra (`SELECT id, display_name FROM users ORDER BY display_name`, fallback display_name→username).
- UI: компонент `ui/src/lib/components/RoomChat.svelte` существует и подключён в `App.svelte`:
  - клик по комнате → заголовок (название + бейдж вида room/channel) + список сообщений (имя отправителя, время, текст);
  - имена: `fetchUsers()` один раз после логина (карта id→display_name), свои — «Вы»; live-перечитывание каталога при незнакомом sender_id;
  - отправка: поле + Enter/кнопка, trim, пустое не шлём; после 201 сообщение в списке, дубликаты по id исключены;
  - пагинация: свежие 50; если ровно 50 — кнопка «Загрузить ещё» (`before_id` = id старейшего в списке);
  - live: поллинг `GET /rooms/{id}/messages?limit=…&before_id=<новейшее>` каждые ~3 с, только пока чат открыт и вкладка видна (`visibilitychange`); новые вставляются сверху;
  - не-участник: состояние «Присоединиться» + кнопка (join → 200 → чат открывается);
  - `ui/src/lib/api.ts`: тип `Message`, `listMessages(roomId, limit?, beforeId?)`, `postMessage(roomId, text)`, `fetchUsers()`.

ШАГИ (строго по порядку):

1. **Git — ничего не потеряно:**
   - `git log --oneline -12` → видны `08827a0`, `45ef2a5`;
   - `git status --short` → пусто; `git remote -v` → origin = `https://github.com/SasLord/besedka.git`;
   - `git rev-parse HEAD` == `git ls-remote origin main | awk '{print $1}'` → **локаль = GitHub**;
   - `git show --stat 08827a0` → в составе: `ui/src/lib/components/RoomChat.svelte`, `App.svelte`, `api.ts`, `db.rs` (list_users), `http/mod.rs` (GET /users) — сверить.

2. **Статические гейты (перезапуск, всё зелёное):**
   - `cargo build` · `cargo test --workspace` → **0 failed** (число passed — любое ≥ 170; приложить вывод);
   - `cargo clippy --all-targets -- -D warnings` → 0 · `cargo fmt --check` → чистый;
   - `cd ui && npm run check` → 0 errors · `npm run build` → OK (Node 24 через nvm);
   - **PITFALL:** НЕ чистить `ui/node_modules`, НЕ переустанавливать зависимости (два node: hermes x64 / nvm arm64 — нужны ОБА бинарника @rollup/@esbuild).

3. **Свежая БД + бэкенд (scratch-путь, НЕ базовая БД проекта; порт 8080):**
   - `BESEDKA_DB_PATH=<scratch>/fresh22.db BESEDKA_PORT=8080 cargo run -p besedka-app` (background, лог в scratch) → старт, миграции без ошибок;
   - `sqlite3 <db> ".tables"` → `users`, `rooms`, `room_members`, `messages` (+ `room_read_cursors` от 2.3 — НОРМА);
   - grep: `Db::list_users` в `crates/besedka-infra/src/db.rs` — на месте; `GET /users` в `crates/besedka-app/src/http/mod.rs` — на месте;
   - register `alice`, `bob` → 201; login; alice → admin (`UPDATE users SET role='admin'`; scratch — допустимая установка); alice: `POST /rooms` (channel «verify22») → 201; bob: `POST /rooms/1/join` → 200;
   - bob: `GET /api/v1/users` (Bearer) → **200** `{"users":[…]}` содержит alice и bob; без токена → 401.

4. **Живая проверка в браузере (vite :5173, логин alice):**
   - комната «verify22» видна; клик → чат (пустой список — ОК);
   - **отправка:** написать «привет 2.2», Enter → сообщение появляется (имя «Вы», время, текст); отправить ещё раз → **дублей по id нет** (2 разных сообщения);
   - пустое поле → отправить не шлёт (нет нового сообщения, нет ошибки «послано»);
   - **имена + live-поллинг:** bob (API): `POST /rooms/1/messages` `{"text":"от боба"}` → 201; в браузере alice сообщение появляется **само за ~3–5 с без перезагрузки**, имя — «bob» (или display_name), а НЕ «Пользователь #N»/«Unknown»;
   - **пагинация:** скриптом (curl от bob) создать ещё ~50 сообщений; в UI должна появиться кнопка **«Загрузить ещё»** (в списке ровно 50); клик → список растёт (+50), новые вверху, свои/чужие не теряются;
   - **не-участник:** новый пользователь `carol` (API), login в отдельном окне/вкладке — комната в списке, клик → состояние **«Присоединиться»** (не чат); кнопка → комната открывается, посты видны;
   - поллинг не гоняется на скрытой вкладке (проверить по сетевой вкладке/логам — опционально, пометить).

5. **Отчёт:** `.conveyor/reports/wave22-verify-report.md` — таблица `пункт | ожидалось | факт | PASS/FAIL` + выводы команд (обрезанные, без паролей). Скриншоты — опционально. Вывод строкой:
   - все PASS → **«Wave 2.2 верифицирован, ничего не потеряно»**;
   - есть FAIL → перечислить каждый (команда/шаг, ожидалось, факт) и **остановиться** (ничего не чинить — фикс в отдельной сессии по отчёту).

6. `git add .conveyor/reports/wave22-verify-report.md` · commit "docs(verify): Wave 2.2 verification — <PASS|FAIL>" · `git push origin main` (проверить локаль == GitHub).

7. **Telegram (попробовать, не навязывать):**
   `hermes send -t telegram --subject "Besedka Wave 2.2 — VERIFIED ✅" --file .conveyor/reports/wave22-verify-report.md`
   Сеть на `api.telegram.org` может быть закрыта (таймаут) — максимум 1 повтор, зафиксировать в отчёте «Telegram: не доставлено (сеть)». Это НЕ считается FAIL.

8. **Перед завершением:** остановить все фоновые процессы (API-сервер, vite, cargo-джобы) — не оставлять висящие.

Начни с шага 1.
