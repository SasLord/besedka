# Besedka — Wave 2.1 ✅ Каналы + сообщения (append) в БД

**Дата:** 2026-10-27 · **Коммиты:** `89b7b39` (feat) + `7f52668` (state) — оба pushed в `origin/main`

## Что сделано
1. **CORE (`besedka-core/src/message.rs`)** — `Message { id, room_id, sender_id, text, created_at }`.
   Формат времени (единый для проекта): **RFC 3339 UTC с миллисекундами** (`2026-10-27T09:12:33.456Z`), строка в SQLite.
   «Канал» = комната `RoomKind::Channel` — отдельной таблицы каналов нет.
   Полей `edited_at`/`deleted_by` в 2.1 **нет намеренно** — появятся в 2.3 (помечено в комментариях).
2. **INFRA (`besedka-infra`)** — миграция `V4__messages.sql`:
   `messages(id PK AUTOINCREMENT, room_id FK→rooms CASCADE, sender_id FK→users CASCADE, text NOT NULL, created_at)`
   + индексы `(room_id, id)` для пагинации и `sender_id` для аудита.
   Методы: `create_message`, `list_messages(room_id, limit, before_id)` (свежие N, курсор `before_id`, лимит 50 по умолчанию / 200 максимум), `get_message`.
3. **API (`besedka-app`)** — только бэкенд (UI-чат = 2.2):
   - `POST /api/v1/rooms` — AdminUser, `{kind: room|channel, title 1–128}` → 201; не-админ → 403; создатель становится участником.
   - `POST /api/v1/rooms/{id}/messages` — участник комнаты, текст 1–4096 (trim) → 201; не-участник → 403; комнаты нет → 404; пустой/длинный → 400.
   - `GET /api/v1/rooms/{id}/messages?limit=&before_id=` — участник → `{"messages": [...]}`; пустая комната → `{"messages": []}`.

## Тесты (166 passed / 0 failed)
- **infra 87:** create→read · пагинация/курсор/границы лимитов · FK-каскад (удаление комнаты и пользователя гаснет сообщения) · пустая комната → [].
- **app 67:** полный приёмочный поток (admin создаёт канал → пост → чтение) · пост не-участника → 403 · создание комнаты не-админом → 403 · пустой/длинный текст → 400 · пустая комната → `[]` · пагинация на 2 страницы.
- **core 12.**

## Гейты (все зелёные, до commit)
- `cargo build` — OK
- `cargo test --workspace` — **166 passed / 0 failed**
- `cargo clippy --all-targets -- -D warnings` — **0**
- UI в срезе не менялся → npm-гейты не требовались

## Дальше — Wave 2.2
[app+ui] Пост/список сообщений (WS или поллинг) + пагинация. **Accept:** обмен в UI.
