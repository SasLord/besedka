# STATE — Besedka
_Обновлено: 2026-10-27 · Source of truth «где мы»._

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
| **Wave 1 (аутентификация + комнаты)** | 🟡 **ЧАСТИЧНО** — 1.1 ✅, 1.2 ✅, 1.3 ⬜ |

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

## Дальше
**WAVE 1 — аутентификация + комнаты** (PLAN.md §Wave 1):
1. **1.1 ✅ DONE** register/login/logout/me (argon2) + сессии + `users`/`roles` в core.
2. **1.2 ✅ DONE** AD/Kerberos SSO (`GET /auth_ad_sso`, `POST /auth_ad_login`, status, admin-диагностика) + mock-режим + ролевые гейты.
3. **1.3 🟡** UI: список комнат + join/leave + ролевые гейты — локальная 27B. Точка входа в UI: `GET /api/v1/auth_ad_sso/status` (показать/скрыть кнопку «Войти через AD»), затем `GET /api/v1/auth_ad_sso` (редирект-вход).

## Указатели контекста
- Конвенции/стек/роли: `CONVENTIONS.md`
- Срезы: `PLAN.md` · Майлстоны: `MILESTONES.md` · Запуск: `HANDOFF.md`
- Требования: `../CORP_CHAT_PLAN.md`
- AD/Kerberos-код для переноса: `trackly/crates/trackly-infra/src/ad/{sso,keytab,mock,real,discovery}.rs`
  + `trackly/crates/trackly-app/src/http/sso.rs`
