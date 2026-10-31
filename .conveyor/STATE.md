# STATE — Besedka
_Обновлено: 2026-10-30 · Source of truth «где мы»._

## Решения (зафиксированы)
- **Язык:** всё на Rust (сервер + Tauri-бэкенд + биндинги). ~~Go~~ — отброшено (trackly на Rust,
  AD-код переиспользуется).
- **Модель:** гибрид **B** — локальная 27B оркестратор + DeepSeek для сложных срезов.
- **Репозиторий:** public, `github.com/SasLord/besedka` (CI/сборки релизов Win/Linux/macOS живут здесь).
- **Отчёты:** пользователю (чат) + **Telegram** (home-канал, плагин `hermes-telegram`, `TELEGRAM_BOT_TOKEN`).
- **Референс AD/Kerberos:** `/Users/madsas/Projects/trackly/` (READ-ONLY, не менять).
- **Тестирование AD:** мок-AD trackly (паттерн `TRACKLY_AD_MOCK=1`) — разработка/тест без реального DC.

## Статус
| Пункт | Состояние |
|---|---|
| GitHub-репо `besedka` (public) | ✅ создано |
| Файлы конвейера (CONVENTIONS/STATE/PLAN/MILESTONES/HANDOFF) | ✅ созданы |
| Скилл `besedka` (Hermes) | ✅ создан |
| **Wave 0 (каркас)** | ✅ **DONE** (2026-10-26, коммит `5a10aa3`, pushed) |
| Код | ✅ воркспейс из 3 кристов + UI (Svelte 5) |
| **Wave 1.1 (auth: register/login/logout/me)** | ✅ **DONE** (2026-10-26, 31 tests зелёные) |
| **Wave 1.2 (AD/Kerberos SSO + mock)** | ✅ **DONE** (2026-10-27, 137 tests зелёные, clippy 0 deny) |
| **Wave 1.3 (список комнат + join/leave + роли)** | ✅ **DONE** (2026-10-27, 148 tests, clippy 0, svelte-check 0, UI в браузере) |
| **Wave 1 (аутентификация + комнаты)** | ✅ **ПОЛНОСТЬЮ ЗАВЕРШЕНА** (2026-10-27, коммит `e5340ce`, pushed) |
| **Wave 2.1 (каналы + сообщения append в БД + API)** | ✅ **DONE** (2026-10-27, 166 tests, clippy 0, коммит `89b7b39`, pushed) |
| **Wave 2.2 (чат в UI: пост + список + пагинация + live)** | ✅ **DONE** (2026-10-30, 170 tests, clippy 0, svelte-check 0, vite build OK, проверено в браузере) |
| **Wave 2.3 (edit/delete своих сообщений + read receipts)** | ✅ **DONE** (2026-10-31, 177 tests, clippy 0, svelte-check 0, vite build OK, end-to-end в браузере, коммит `a97e1ed`, pushed) |

## Wave 0 — что сделано (принято)
- Cargo-воркспейс: `besedka-core` (типы users/roles/rooms) · `besedka-infra` (rusqlite bundled + refinery) · `besedka-app` (axum + бинарник).
- `besedka-app`: `/api/v1/ping`, `/api/v1/rooms` (пустой список из БД), конфиг из `config.toml` (+ env-оверрайды `BESEDKA_*`), tracing, tower-sessions (cookie `besedka_session`, TTL 7д).
- `besedka-infra`: миграция `migrations/V1__init.sql` — таблицы `users`, `rooms`; `Db::open`/`open_in_memory` + `migrate`.
- `ui/`: Svelte 5 + Vite, дизайн-токены в `app.css`, layout-оболочка (сайдбар комнат + контент), empty-state, индикатор статуса (ping).
- **Гейты:** `cargo check/clippy/test` (13 passed)/`build` — зелёные; `ping` = 200 JSON; `rooms` = 200 `{"rooms":[]}`; UI рендерится (проверено в браузере, скриншот); отчёт в Telegram доставлен.
- **Питфолл (зафиксировать в HANDOFF):** в этой машине два node (hermes x64 v26 / nvm arm64 v24) — `node_modules` нужен и `@rollup/rollup-darwin-{x64,arm64}`, и `@esbuild/darwin-{x64,arm64}`; npm `install-scripts` гейт может блокировать postinstall esbuild.

## Wave 1.1 — что сделано (принято)
- **API (besedka-app):** `POST /api/v1/register` (201, argon2 hash, role=user, username 3–64 ч/`._-`, password 8–128) · `POST /api/v1/login` (200 + `Set-Cookie`, верификация argon2, «invalid username or password» для нет-юзера и верного пароля — без enumeration) · `POST /api/v1/logout` (200, `Session::flush()` → clear+delete, менеджер снимает cookie) · `GET /api/v1/me` (200 user / 401 без сессии).
- **besedka-core:** `password.rs` (argon2 `hash`/`verify`, P19352/m=64k/t=3/p=4, malformed hash → false, не паника) · `User`/`StoredUser`/`Role` (user|admin) · `Room`/`RoomKind`.
- **besedka-infra:** `Db` (rusqlite bundled) — `create_user`/`find_user_by_username`/`get_user` (public view, hash не утекает) + миграции.
- **Гейты:** `cargo test` — 31 passed (14 app incl. 8 auth HTTP-тестов register/login/me/logout + 4 validation, 9 core, 8 infra); `cargo build` — OK.
- **SSO-заметка:** tower-sessions MemoryStore + cookie `besedka_session`; AD/Kerberos — отдельный срез 1.2 (делегирование DeepSeek, паттерн trackly).

## Wave 1.2 — что сделано (принято)
- **Перенос из trackly (READ-ONLY, ни одного изменения там):** `keytab.rs` (парсер MIT keytab, AES256 по SPN) · `sso.rs` (`accept_spnego` — offline-принятие Kerberos-билета через `sspi`) · `cache.rs` (TTL-кэш) · `transport.rs` (LDAP URL/settings) · `discovery.rs` (DNS SRV + `derive_base_dn`) · `directory.rs` (сервисный bind + `displayName`/`memberOf` + `LDAP_MATCHING_RULE_IN_CHAIN`) · `real.rs` (LDAP simple bind) · `mock.rs` + `directory_mock.rs` (фикстуры) — всё в `crates/besedka-infra/src/ad/`.
- **Адаптация под besedka (не слепое копирование):** порты `AdClient`/`AdDirectory` + `AuthOutcome`/`DirectoryResult`/`DirectoryError` в `besedka-core/src/ad.rs` (без I/O-зависимостей); конфиг `AdConfig` в infra (один мастер-переключатель `enabled` вместо пары `enabled`+`sso_enabled`, `Debug` скрывает `bind_password`); роли — besedka-шные `user|manager|admin` (не `employee`); один канонический нормализатор логина `normalize_login` (UPN/NetBIOS → lowercase `[a-z0-9._-]`).
- **API (besedka-app):** `GET /api/v1/auth_ad_sso` (Kerberos/Negotiate: 401→билет→валидация→сессия; mock-режим = вход фикстурой) · `POST /api/v1/auth_ad_login` (AD-пароль, LDAP bind) · `GET /api/v1/auth_ad_sso/status` (публичная проба для UI) · `GET /api/v1/admin/ad_status` (admin-only «проверить подключение»). Все AD-пути выдают ТУ ЖЕ cookie `besedka_session`, что парольный вход (`flush` перед вставкой — анти-fixation).
- **Provisioning:** первый вход → аккаунт с `password_hash = NULL` и ролью из AD-групп (`role_mapping`, старшая побеждает) либо `user`; повторный вход роль **не перезаписывает** (иначе понижение в AD откатывалось бы каждым логином), ФИО синхронизируется только из реального ответа каталога (fallback-логин и ответ «недоступен» ФИО не затирают). Нет каталога (`bind_dn` пуст) → вход работает, без обогащения. DC недоступен → session НЕ выдаётся (503, fail-closed).
- **Mock-режим (обязателен, локальная разработка без AD):** `[ad] mock = true` или `BESEDKA_AD_MOCK=1`. Фикстуры: `us100`/`Passw0rd!`→manager, `us200`/`Secret123`→user, `us300`→admin; `?login=`/`X-Besedka-Ad-Login` выбирают личность; логин на `deny*` → 401. Живая проверка: status 200 · `GET /auth_ad_sso` 200 + cookie · `/me` 200 · `/admin/ad_status` 200 для us300 и **403** для us100 · `/auth_ad_login` us200 200.
- **Ролевые гейты:** экстракторы `AuthedUser` (401 без сессии) и `AdminUser` (403 для не-админа); юзер всегда перечитывается из SQLite (смена роли/удаление действуют со следующего запроса).
- **Секреты:** `bind_password` — только env `BESEDKA_AD_BIND_PASSWORD`; keytab (`*.keytab`) в `.gitignore`; диагностика пароль не отдаёт; в лог не пишется.
- **Дока:** `docs/AD-SETUP.md` (ktpass/keytab, браузеры, LDAPS, маппинг групп, «проверить подключение», таблица ошибок).
- **Гейты:** `cargo build` OK · `cargo test` — **137 passed** (50 app, 11 core, 76 infra; +37 новых AD-тестов, включая real-путь без DC: challenge/Negotiate, keytab, чужой билет → 401) · `cargo clippy --all-targets -- -D warnings` — **0** · `cargo fmt --check` чистый · сервер поднят, эндпоинты отвечают (см. выше).
- **Осталось непроверенным (осознанно):** живой Kerberos-handshake — только на реальном DC (`sspi` accept build-verified, как в trackly); живой LDAPS/группы — при наличии DC.

## Wave 1.3 — что сделано (принято)
- **Миграция:** `migrations/V3__rooms_membership.sql` — `room_members(room_id, user_id, joined_at)`, FK `ON DELETE CASCADE`, `UNIQUE(room_id, user_id)`, индекс по `user_id`.
- **API (besedka-app):** `POST /api/v1/rooms/{id}/join` (AuthedUser; 404 если комнаты нет; **идемпотентно** — повторный join = ок) · `POST /api/v1/rooms/{id}/leave` (AuthedUser; 404 если комнаты нет или юзер не был в ней) · `GET /api/v1/rooms` (AuthedUser) — теперь в каждой комнате `joined: bool` + `member_count`.
- **besedka-infra:** `Db::list_rooms_for_user` / `room_view` / `join_room` / `leave_room` (SQL в одном месте) · `RoomView { room, joined, member_count }` (db.rs) + JSON-обёртка `RoomViewResponse` (http/mod.rs).
- **Ролевые гейты:** join/leave — любой `AuthedUser` (401 без сессии); управление комнатами — `AdminUser` (403 для не-админа); в UI админ-секция видна только admin.
- **UI (Svelte 5):** сайдбар со списком комнат + бейджи вида (GENERAL/ROOM/CHANNEL/DIRECT), отметка «вы здесь» у `joined:true`, счётчик участников, кнопки **Join/Leave** (состояние обновляется после клика); шапка с юзером + бейджем роли (ADMIN/MANAGER) + «Выйти»; empty-state «вы не в комнатах»; экран входа: «Войти через AD» (видна когда `auth_ad_sso/status` enabled) + выбор mock-пользователя (us100 manager / us200 user / us300 admin) + парольный login/register (1.1).
- **SSO-вход в UI:** `fetch` к `GET /api/v1/auth_ad_sso` (эндпоинт отвечает JSON `{ok, mock, user}` + сессию-кукку и в mock, и в real-режиме; браузер прозрачно доводит Negotiate) — без full-page-навигации.
- **Гейты:** `cargo build` OK · `cargo test` — **148 passed** (58 app, 11 core, 79 infra; новые HTTP-тесты: join → joined=true, double-join ok, leave → joined=false, leave без join = 404, чужая комната = 404) · `cargo clippy --all-targets -- -D warnings` — **0** · `npm run check` (svelte-check) — **0 errors** · `npm run build` (vite) — OK · рендер проверен в браузере: login us300 (admin, ADMIN-бейдж + админ-секция) и us100 (manager), список комнат, Join → «вы здесь» + счётчик 1 → Leave → сброс (скриншот `.conveyor/screenshots/wave13-rooms.png`).
- **Демо-данные:** dev-БД `data/besedka.db` — 3 комнаты (Общий/Работа/Новости) вставлены вручную (V1 без сида).
- **Commit:** `e5340ce` — `feat(rooms): Wave 1.3 — room list + join/leave + role gates (app+ui)`, pushed в origin/main.

## Wave 2.1 — что сделано (принято)
- **CORE (besedka-core):** `message.rs` — `Message { id, room_id, sender_id, text, created_at }`. **Формат времени (единый для проекта):** RFC 3339 UTC с миллисекундной точностью (`%Y-%m-%dT%H:%M:%fZ`), строка в БД — дефолт SQLite. «Канал» = комната вида `RoomKind::Channel` (таблицы каналов нет). Поля `edited_at`/`deleted_by` появятся в 2.3 — в 2.1 намеренно их НЕТ.
- **INFRA (besedka-infra):** миграция `migrations/V4__messages.sql` — `messages(id INTEGER PK AUTOINCREMENT, room_id FK→rooms ON DELETE CASCADE, sender_id FK→users ON DELETE CASCADE, text TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT …)` + индексы `(room_id, id)` (пагинация) и `sender_id` (аудит/поиск). Методы: `Db::create_message` · `Db::list_messages(room_id, limit, before_id)` — курсорная пагинация: свежие N, страница «до `before_id`», лимит по умолчанию 50 / максимум 200 (больше — 200, меньше 1 — ошибка) · `Db::get_message`.
- **APP (besedka-app) — только API (UI-чат = 2.2):** `POST /api/v1/rooms` (AdminUser; `{kind: room|channel, title}`; title 1–128; создатель становится участником; не-админ → 403; плохой kind/title → 400) → 201 + комната · `POST /api/v1/rooms/{id}/messages` (AuthedUser + участник; текст 1–4096, trim; не-участник → 403; комнаты нет → 404; пустой/длинный → 400) → 201 + сообщение · `GET /api/v1/rooms/{id}/messages?limit=&before_id=` (AuthedUser + участник) → `{"messages": [...]}`; пустая комната → `{"messages": []}`.
- **Тесты:** infra **87 passed** (новые: create→read, пагинация/границы лимитов/курсор, FK-каскады комната→сообщения и пользователь→сообщения, пустая комната → []) · app **67 passed** (новые: принять-поток «admin создаёт канал → пост → чтение», пост не-участника → 403, создание комнаты не-админом → 403, пустой/длинный текст → 400, пустая комната → `[]`, пагинация на 2 страницы) · core 12 passed.
- **Гейты:** `cargo build` OK · `cargo test --workspace` — **166 passed / 0 failed** · `cargo clippy --all-targets -- -D warnings` — **0** · UI в срезе не менялся → npm-гейты не требовались.
- **Commit:** `89b7b39` — `feat(messages): Wave 2.1 — channels + messages append (core+infra+api)`, pushed в origin/main.

## Wave 2.2 — что сделано (принято)
- **APP (бэкенд, минимально):** `GET /api/v1/users` (AuthedUser) → `{"users":[{"id","display_name"},...]}` — каталог пользователей для имён отправителей (пригодится и в 2.3). Инфра: `Db::list_users()` (`SELECT id, display_name FROM users ORDER BY display_name`); `display_name` падает на `username`, если не задан (DB-слой) — т.е. для парольных регистраций имя = логин, реальное ФИО даёт только AD/SSO.
- **UI (главная часть):** новый компонент `ui/src/lib/components/RoomChat.svelte` + секция чата в `App.svelte`:
  - клик по комнате → заголовок (название + бейдж вида) + список сообщений (имя отправителя, время, текст).
  - Имена: `fetchUsers()` один раз после логина (карта id→display_name), своя — «Вы»; **live-обновление каталога**: при каждом тике поллинга, если в новых сообщениях есть незнакомный sender_id — каталог перечитывается (иначе новый участник показывался бы как «Пользователь #N» до перезагрузки).
  - Ввод: поле + Enter/кнопка, trim, пустое не шлём; после 201 сообщение добавляется в список (дубликаты по id исключены).
  - Пагинация: свежие 50; если их ровно 50 — кнопка «Загрузить ещё» (`before_id` = id самого старшего в списке); счётчик сохранён, скролл не прыгает (новые — сверху, при скролле вниз).
  - Live: поллинг `GET /rooms/{id}/messages?limit=…&before_id=<новейшее id>` каждые 3 с (только пока чат открыт и вкладка видна — `visibilitychange`); новые сообщения вставляются сверху.
  - `api.ts`: тип `Message`, `listMessages(roomId, limit?, beforeId?)`, `postMessage(roomId, text)`, `fetchUsers()`.
- **Тесты (браузер, живой):** 50 сообщений в комнате, «Загрузить ещё» подтягивает старшие; чужой пост (фин, зарегистрирован ПОСЛЕ логина наблюдателя) появляется через ~3 с без перезагрузки, имя — из каталога (username-fallback), не «Пользователь #8»; свой пост — сверху, без дублей.
- **Гейты:** `cargo test --workspace` — **170 passed / 0 failed** (app 69, core 12, infra 89; +4 новых: `list_users` + HTTP `GET /users`) · `cargo clippy --all-targets -- -D warnings` — **0** · `cargo fmt --check` — чистый · `npm run check` (svelte-check) — **0 errors** · `npm run build` (vite) — **OK** · живой браузер: сценарий выше пройден.
- **Commit:** (см. ниже) — `feat(chat): Wave 2.2 — room chat UI (post + list + pagination + live polling) + users catalog`, pushed в origin/main.

## Wave 2.3 — что сделано (принято)
- **INFRA:** миграция `V5__edit_reads.sql` — `messages.edited_at`/`deleted_at` (nullable) + таблица `message_readers(message_id, reader_id, first_read_at, PK(message_id,reader_id))` + индексы. Методы: `edit_message`/`delete_message` (только автор, иначе Forbidden), `record_read` (курсор: сдвиг `last_read_id`, если новое; `changes()==0` → no-op), `message_readers` (дедуп по читавшему, время = первое прочтение), `unread_count` (`max(id) > last_read_id`).
- **CORE:** типы `Message { edited_at, deleted_at }` + `ReadReceipt`; те же доменные правила (автор-гейты, 1–4096 trim).
- **APP (HTTP):**
  - `PATCH /api/v1/rooms/{id}/messages/{mid}` — `{text}`; не автор → 403; нет/удалено → 404; пустой/длинный → 400 → 200 + сообщение.
  - `DELETE /api/v1/rooms/{id}/messages/{mid}` — только автор → **204 без тела**; чужое → 403.
  - `POST /api/v1/rooms/{id}/read` (пустое тело) → `{last_read_id}`; не участник → 403.
  - `GET /api/v1/rooms/{id}/messages/{mid}/readers` → `[{reader_id, reader_name, read_at}]`.
  - `GET /rooms/{id}/messages?after_id=` — «новее id» (поллинг не тащит старые страницы).
  - `JsonOrEmpty` — 200/201 с телом, 204 без тела (исправлен баг «204 с пустым JSON ломал клиента»).
- **UI:** `api.ts` + `RoomChat.svelte` — инлайн-редактирование (Enter/Сохранить → PATCH + бейдж «(изменено)», Esc/Отмена без запроса); двухклик-удаление («🗑 Удалить» → «⚠ Точно?», авто-сброс 3 с); read receipts «✓ N» + раскрываемый «Прочитали: …»; отметка прочтения при открытии комнаты и при новых сообщениях (только видимая вкладка); live-поллинг теперь через `after_id`.
- **Тесты:** 177 passed / 0 failed (app 71, core 12, infra 94; новые: автор-гейты edit/delete, курсор read, дедуп readers, unread_count, HTTP PATCH/DELETE/204/403/404/400, `after_id`).
- **Гейты:** `cargo fmt --check` чистый · `cargo clippy -- -D warnings` — 0 · `cargo test --workspace` — **177 passed** · `npm run check` — **0 errors** · `npm run build` — **OK**.
- **Живой браузер:** alice/bob/carol в `obshchii` — read receipts (✓1→✓2, «Прочитали: alice, bob»), редактирование с «(изменено)», двухклик-удаление (сообщение исчезло + в API пусто), bob (не автор) → 403, carol (не участник) → 403. Скриншоты: `.conveyor/screenshots/23-*.png`.
- **Отчёт:** `.conveyor/reports/wave23-report.md`.
- **Commit:** `a97e1ed` — `feat(chat): Wave 2.3 — edit/delete own messages + read receipts`, pushed в origin/main.

## Дальше
**WAVE 1 (аутентификация + комнаты) — ✅ ПОЛНОСТЬЮ ЗАВЕРШЕНА** (2026-10-27):
1. **1.1 ✅** register/login/logout/me (argon2) + сессии + `users`/`roles` в core.
2. **1.2 ✅** AD/Kerberos SSO (`GET /auth_ad_sso`, `POST /auth_ad_login`, status, admin-диагностика) + mock-режим + ролевые гейты.
3. **1.3 ✅** список комнат + join/leave + ролевые гейты (app + ui).

**WAVE 2 (сообщения) — ✅ ПОЛНОСТЬЮ ЗАВЕРШЕНА** (2026-10-31):
1. **2.1 ✅** Каналы + сообщения (append) в БД (core + infra + API) — см. секцию выше (коммит `89b7b39`).
2. **2.2 ✅** Чат в UI: пост + список + пагинация + live (поллинг) + каталог `GET /users` — см. секцию выше (2026-10-30).
3. **2.3 ✅** Edit/delete своего сообщения + read receipts — см. секцию выше (2026-10-31, коммит `a97e1ed`).

**WAVE 3 (файлы) — следующая:**
1. **3.1 →** [app+ui] Файлы в сообщениях (картинки): upload/serve/preview + UI-вложения. Промпт: `PROMPT_wave31.md` (имена файлов = номер среза: wave31 = 3.1, wave32 = 3.2, …).

## Указатели контекста
- Конвенции/стек/роли: `CONVENTIONS.md`
- Срезы: `PLAN.md` · Майлстоны: `MILESTONES.md` · Запуск: `HANDOFF.md`
- Требования: `../CORP_CHAT_PLAN.md`
- AD/Kerberos-код для переноса: `trackly/crates/trackly-infra/src/ad/{sso,keytab,mock,real,discovery}.rs`
  + `trackly/crates/trackly-app/src/http/sso.rs`
