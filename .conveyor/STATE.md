# STATE — Besedka
_Обновлено: 2026-10-26 · Source of truth «где мы»._

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
| **Wave 1 (аутентификация + комнаты)** | ⬜ **ДАЛЬШЕ — не начато** |

## Wave 0 — что сделано (принято)
- Cargo-воркспейс: `besedka-core` (типы users/roles/rooms) · `besedka-infra` (rusqlite bundled + refinery) · `besedka-app` (axum + бинарник).
- `besedka-app`: `/api/v1/ping`, `/api/v1/rooms` (пустой список из БД), конфиг из `config.toml` (+ env-оверрайды `BESEDKA_*`), tracing, tower-sessions (cookie `besedka_session`, TTL 7д).
- `besedka-infra`: миграция `migrations/V1__init.sql` — таблицы `users`, `rooms`; `Db::open`/`open_in_memory` + `migrate`.
- `ui/`: Svelte 5 + Vite, дизайн-токены в `app.css`, layout-оболочка (сайдбар комнат + контент), empty-state, индикатор статуса (ping).
- **Гейты:** `cargo check/clippy/test` (13 passed)/`build` — зелёные; `ping` = 200 JSON; `rooms` = 200 `{"rooms":[]}`; UI рендерится (проверено в браузере, скриншот); отчёт в Telegram доставлен.
- **Питфолл (зафиксировать в HANDOFF):** в этой машине два node (hermes x64 v26 / nvm arm64 v24) — `node_modules` нужен и `@rollup/rollup-darwin-{x64,arm64}`, и `@esbuild/darwin-{x64,arm64}`; npm `install-scripts` гейт может блокировать postinstall esbuild.

## Дальше
**WAVE 1 — аутентификация + комнаты** (PLAN.md §Wave 1):
1. **1.1 🟡** register/login/logout (argon2) + сессии + `users`/`roles`/`rooms` в core — локальная 27B.
2. **1.2 🔴** AD/Kerberos SSO — перенос `trackly` `keytab.rs`/`sso.rs`/`http/sso.rs`/`mock.rs` (READ-ONLY!) → **делегировать DeepSeek** (по правилу гибрид B).
3. **1.3 🟡** UI: список комнат + join/leave + ролевые гейты — локальная 27B.

## Указатели контекста
- Конвенции/стек/роли: `CONVENTIONS.md`
- Срезы: `PLAN.md` · Майлстоны: `MILESTONES.md` · Запуск: `HANDOFF.md`
- Требования: `../CORP_CHAT_PLAN.md`
- AD/Kerberos-код для переноса: `trackly/crates/trackly-infra/src/ad/{sso,keytab,mock,real,discovery}.rs`
  + `trackly/crates/trackly-app/src/http/sso.rs`
