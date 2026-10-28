Ты работаешь в проекте **Besedka** (Rust/Axum бэкенд + Svelte 5 UI). Конвейер: строго по срезам, гейты обязательны, в конце отчёт в Telegram.
Если доступен скилл `besedka` — загрузи его (workflow + гейты). Иначе стек/правила — ТОЛЬКО в `.conveyor/CONVENTIONS.md` (NEVER доверять памяти).

=== ЗАДАЧА: Wave 2.2 — чат в UI: пост + список сообщений + пагинация + live (polling) ===
Уже сделано и на это опирайся:
- **Волна 1 ✅**: AD mock + парольный auth, комнаты + join/leave, бейджи ролей, сайдбар комнат (ui/src/App.svelte, Svelte 5 runes: `$state`/`$derived`).
- **2.1 ✅** (коммиты `89b7b39`, `7f52668`): таблица `messages` (V4), `Message {id, room_id, sender_id, text, created_at}` (время — RFC 3339 UTC с миллисекундами). API УЖЕ есть и протестировано (166 тестов):
  - `POST /api/v1/rooms` `{kind: "room"|"channel", title 1..128}` → 201 (только admin)
  - `POST /api/v1/rooms/{id}/messages` `{text 1..4096}` → 201 (только участник; пустой → 400)
  - `GET /api/v1/rooms/{id}/messages?limit=&before_id=` → `{"messages":[...]}` (участник; свежие N, курсор `before_id` = id сообщения, к которым «уходят назад»; лимит по умолчанию 50, максимум 200)
  - **НО:** сообщений про ИМЯ отправителя нет (`sender_id` — число), эндпоинта «список пользователей» НЕТ — это и есть маленькая app-часть 2.2 (см. шаг 2).
- UI-паттерны: api-клиент `ui/src/lib/api.ts` (типы + `fetch*`-функции, `ApiError`), стили — ТОЛЬКО токены `ui/src/app.css`.

=== Срез: обмен в UI (Accept из PLAN.md 2.2) ===
Пользователь открывает комнату → видит историю (свежие), печатает и отправляет, старые сообщения подгружаются (пагинация), новые сообщения других видны без перезагрузки страницы (polling).

ШАГИ (строго по порядку):

1. Контекст (прочитать ВСЁ прежде чем писать):
   - ~/Projects/besedka/.conveyor/CONVENTIONS.md · STATE.md (2.1 DONE, 2.2 — дальше) · PLAN.md (секция 2.2)
   - ui/src/App.svelte (структура: сайдбар, логин, админ-панель) · ui/src/lib/api.ts · ui/src/app.css (токены)
   - crates/besedka-app/src/http/mod.rs + lib.rs (точные JSON-формы ответов, роуты) · crates/besedka-core/src/message.rs (serde-поля Message)
   - crates/besedka-infra/src/db.rs (методы list_messages/get_message — как считать)

2. APP (бэкенд, МИНИМАЛЬНО — только недостающее):
   - `GET /api/v1/users` (AuthedUser) → `{"users": [{"id", "display_name"}, ...]}` — каталог пользователей (для имён отправителей; пригодится и в 2.3 read receipts). Инфра: `Db::list_users() -> Vec<(i64, String)>` (SELECT id, display_name FROM users ORDER BY display_name) + тест.
   - HTTP-тест: авторизованный видит список, содержит себя и других зарегистрированных.
   - Больше бэкенд НЕ трогаем: сообщения уже полностью на месте.

3. UI (главная часть среза):
   - `ui/src/lib/api.ts`: тип `Message` (id, room_id, sender_id, text, created_at) + `listMessages(roomId, limit?, beforeId?)`, `postMessage(roomId, text)`, `fetchUsers()`.
   - Чат комнаты (новый компонент `ui/src/lib/components/RoomChat.svelte` или секция в App.svelte — решай по структуре App):
     * клик по комнате в сайдбаре открывает чат: заголовок (название + тип), список сообщений (имя отправителя, время, текст).
     * **Имена:** загрузить `fetchUsers()` один раз (после логина), карта id→display_name; своя — можно «Вы».
     * **Ввод:** поле + Enter или кнопка; trim; пустое не шлём; после 201 добавляем возвращённое сообщение в список (дубликаты по id не плодить).
     * **Пагинация:** начальная загрузка — свежие 50; если их 50 — показать кнопку «Загрузить ещё» (или подгрузка при скролле к верху): `before_id` = id самого старого в списке. Скролл не сбрасывать при подгрузке.
     * **Live-обновления — POLLING (решено: WS в этом срезе НЕ делаем):** пока комната открыта, каждые 5 сек `GET messages`, в список добавляем только НОВЫЕ (id > последнего). Таймер гасим при закрытии/переключении комнаты/логауте.
     * **Не-участник комнаты:** чат не открывать (403) — показать состояние «Присоединиться к комнате» с кнопкой join (join уже есть в App).
     * Не-админ/админ — разница в этом срезе только в создании комнат (админка уже есть, не трогай).
   - Дизайн: только токены `ui/src/app.css`, никаких новых цветов/шрифтов.
   - TypeScript: `npm run check` без ошибок.

4. ГЕЙТЫ (все ДО commit; UI меняем → npm-гейты ОБЯЗАТЕЛЬНЫ):
   - в ui/: `npm run check` = 0 ошибок · `npm run build` OK
   - бэкенд: `cargo build` · `cargo test --workspace` (зелёные, включая новый users-тест) · `cargo clippy --all-targets -- -D warnings` = 0
   - **PITFALL:** НЕ чистить `ui/node_modules`, НЕ переустанавливать зависимости (два node: hermes x64 / nvm arm64 — нужны ОБА бинарника @rollup/@esbuild darwin-x64+arm64)
   - Рендер-проверка: поднять API (`cargo run -p besedka-app`) + `npm run dev` (в ui/), руками: логин → создать комнату (admin) → пост → увидеть сообщение → «загрузить ещё» → закрыть. Скриншот в .conveyor/screenshots/ (если удобно).

5. commit: "feat(chat-ui): Wave 2.2 — chat view: post + list + pagination + polling"
   push:  git push origin main

6. STATE.md: отметить **2.2 DONE** (эндпоинт users, UI-компонент, polling, гейты, коммит) → commit "docs(state): Wave 2.2 DONE — chat in UI" → push.

7. Отчёт в Telegram:
   hermes send -t telegram --subject "Besedka Wave 2.2 ✅ (чат в UI)" --file <путь_к_отчёту.md>

8. **ОБЯЗАТЕЛЬНО (правило конвейера):** написать `.conveyor/PROMPT_wave23.md` для следующего среза — **2.3 [app+ui]: Edit/delete своего сообщения + read receipts** (Accept: edit/delete, отметки). Формат — как PROMPT_wave21.md / PROMPT_wave22.md: что уже сделано (включая 2.2), шаги, гейты, commit, STATE.md, Telegram, и шаг «написать PROMPT_wave24.md» (следующая волна — Wave 3, файлы: см. PLAN.md). Закоммитить и запушить.

9. Перед завершением: **остановить все фоновые процессы** (API-сервер, vite, cargo-джобы) — не оставлять висящие.

Начни с шага 1.
