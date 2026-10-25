# PLAN — Besedka (сliced, всё на Rust)
_Сложность: 🟢 simple · 🟡 medium · 🔴 hard. Каждый срез = одна роль + acceptance criteria._

## Wave 0 — Каркас (доказываем, что конвейер крутится)
- **0.1 🟡 [app+infra+ui]** Cargo-воркспейс (3 криста) + `besedka-app`: axum, `/api/v1/ping`, конфиг из
  TOML, tracing, `tower-sessions`. `besedka-infra`: SQLite (rusqlite bundled) + первая миграция refinery
  (напр. таблица `users`). `ui/`: Svelte 5 + дизайн-токены (из CORP_CHAT_PLAN.md) + layout-оболочка.
  - **Accept:** `cargo check/clippy/test/build` зелёные; `cargo run` → `GET /api/v1/ping` = 200 JSON;
    UI рендерит layout; committed+pushed; отчёт доставлен.
- **0.2 🟢 [app+ui]** `GET /api/v1/rooms` (пустой список из БД) + UI показывает «нет комнат».
  - **Accept:** ping+rooms работают, empty-state UI виден.

## Wave 1 — Аутентификация + комнаты
- **1.1 🟡 [core+infra+app]** Парольная авторизация (argon2) + сессии + `users`/`roles`/`rooms` в core.
  - **Accept:** register/login/logout через API, сессионный куки.
- **1.2 🔴 [infra+app]** **AD/Kerberos SSO** — перенести trackly `keytab.rs`+`sso.rs`+`http/sso.rs`+`mock.rs`
  в besedka.
  - **Accept:** с мок-AD `GET /api/v1/auth_ad_sso` аутентифицирует тестового юзера; real-AD-путь
    компилируется (live-тест при наличии DC).
- **1.3 🟡 [app+ui]** Список комнат + join/leave + роли.
  - **Accept:** UI показывает комнаты, юзер заходит, ролевые гейты видны.

## Wave 2 — Каналы + сообщения
- **2.1 🟡 [core+infra]** Каналы + сообщения (append) в БД. **Accept:** создать канал, пост, чтение.
- **2.2 🟡 [app+ui]** Пост/список сообщений (WS или поллинг) + пагинация. **Accept:** обмен в UI.
- **2.3 🟢 [app+ui]** Edit/delete своего сообщения + read receipts. **Accept:** edit/delete, отметки.

## Wave 3 — Файлы
- **3.1 🟡 [infra+app]** Загрузка/скачивание файлов (лимиты размера/типа). **Accept:** upload→download→превью.
- **3.2 🟢 [ui]** Превью файлов в чате (image/PDF). **Accept:** картинка инлайн, PDF-ссылка.

## Wave 4 — Поиск + уведомления
- **4.1 🟢 [core+app]** Глобальный текстовый поиск (SQLite FTS5). **Accept:** поиск находит сообщения.
- **4.2 🟢 [app+ui]** Уведомления (упоминание/новое сообщение). **Accept:** mention → уведомление в UI.

## Wave 5 — Голосование + админ
- **5.1 🟡 [core+app]** Голосование (создание/опции/голос/результат). **Accept:** провести голосование.
- **5.2 🟡 [app+ui]** Админ-панель (CRUD users/roles/rooms). **Accept:** админ управляет.
- **5.3 🟢 [app]** Аудит-лог. **Accept:** действия логируются, выводятся.

## Wave 6 — i18n + полировка
- **6.1 🟢 [ui]** i18n RU/EN. **Accept:** переключение языка, все строки локализованы.
- **6.2 🟢 [ui]** Полировка (empty states, ошибки, loading). **Accept:** консистентный UX.

## Wave 7 — Упаковка + деплой
- **7.1 🔴 [app]** Tauri-сборки Windows + macOS + Linux (GitHub Actions). **Accept:** инсталляторы через CI.
- **7.2 🟡 [infra]** Docker compose (сервер + SQLite volume). **Accept:** `docker compose up` поднимает сервер.

## Wave 8 — (опционально) веб-клиент
- **8.1 🟡 [ui+app]** Веб-клиент, отдаваемый сервером. **Accept:** чат в браузере работает.
