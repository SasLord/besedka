Ты работаешь в проекте **Besedka** (Rust/Axum бэкенд + Svelte 5 UI). Конвейер: строго по срезам, гейты обязательны, в конце отчёт в Telegram.
Если доступен скилл `besedka` — загрузи его (workflow + гейты). Иначе стек/правила — ТОЛЬКО в `.conveyor/CONVENTIONS.md` (NEVER доверять памяти).

=== ЗАДАЧА: Wave 1.3 — список комнат + join/leave + роли (app + ui) ===
Уже сделано и на это опирайся: **1.1** auth (register/login/logout/me, cookie `besedka_session`) ✅ · **1.2** AD/Kerberos SSO + mock + ролевые экстракторы `AuthedUser`/`AdminUser` + `/api/v1/me` ✅.
Сейчас есть: `GET /api/v1/rooms` (возвращает `RoomsResponse{rooms}` из `db.list_rooms()`, БЕЗ флага `joined`), Room-структура в `besedka-core/src/room.rs`. **join/leave и `room_members` ЕЩЁ НЕТ — это и есть 1.3.**

ШАГИ (строго по порядку):

1. Контекст (прочитать ВСЁ прежде чем писать):
   - ~/Projects/besedka/.conveyor/CONVENTIONS.md
   - ~/Projects/besedka/.conveyor/STATE.md    (1.1/1.2 DONE, 1.3 — дальше)
   - ~/Projects/besedka/.conveyor/PLAN.md     (секция 1.3: «UI показывает комнаты, юзер заходит, ролевые гейты видны»)
   - Код: crates/besedka-app/src/http/mod.rs (rooms, auth) · crates/besedka-infra/src/ (Db, migrations/) · crates/besedka-core/src/ (User/Room/Role) · ui/src/ (App.svelte, lib/api.ts, lib/components/)

2. БЭКЕНД (app):
   - миграция `V3__rooms_membership.sql` → таблица `room_members(room_id, user_id, joined_at)` (FK + уникальная пара)
   - `POST /api/v1/rooms/:id/join` (AuthedUser) — встать в комнату; 404 если комнаты нет; идемпотентно (повторный join = ок)
   - `POST /api/v1/rooms/:id/leave` (AuthedUser) — выйти; 404 если комнаты нет / не был в ней
   - `GET /api/v1/rooms` — добавить в каждую комнату флаг `joined: bool` (был ли текущий юзер в ней) + `member_count`
   - роли: join/leave — любой AuthedUser; админ-управление (создание/удаление комнат) — `AdminUser` (если PLAN требует)
   - тесты HTTP: join → joined=true · double-join ok · leave → joined=false · leave без join = 404 · чужая комната 404

3. UI (Svelte 5, `ui/`):
   - сайдбар: список комнат из `/api/v1/rooms`; бейдж/отметка «вы здесь» у `joined:true`; счётчик участников
   - кнопки **Join / Leave** у каждой комнаты (вызов бэкенда, обновить состояние после)
   - ролевые гейты в UI: показать роль юзера (из `/api/v1/me`); admin-элементы видны только admin
   - логин: `GET /api/v1/auth_ad_sso/status` → показать/скрыть кнопку «Войти через AD»; клик → `GET /api/v1/auth_ad_sso` (редирект-вход). Рядом — парольный login (уже есть в 1.1).
   - после входа: перечитать `/me` → показать юзера + его комнаты (joined)

4. ГЕЙТЫ (все ДО commit):
   - `cargo build` OK · `cargo test` зелёные (включая новые join/leave) · `cargo clippy --all-targets -- -D warnings` = 0
   - UI: `cd ui && npm run build` (vite build) без ошибок + `npm run check` (svelte-check); рендер проверен в браузере (скриншот)
   - **PITFALL (Wave 0):** два node (hermes x64 / nvm arm64) — в `ui/node_modules` нужны `@rollup/rollup-darwin-{x64,arm64}` и `@esbuild/darwin-{x64,arm64}`; НЕ чисти node_modules без нужды.

5. commit: "feat(rooms): Wave 1.3 — room list + join/leave + role gates (app+ui)"
   push:  git push origin main

6. STATE.md: отметить **1.3 DONE** → **Wave 1 ПОЛНОСТЬЮ завершена**. Зафиксировать что сделано (эндпоинты, UI, гейты).

7. Отчёт в Telegram:
   hermes send -t telegram --subject "Besedka Wave 1.3 ✅ (Волна 1 завершена)" --file <путь_к_отчёту>

Начни с шага 1.
