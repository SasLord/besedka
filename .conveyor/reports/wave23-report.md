# Wave 2.3 — отчёт (edit/delete своего сообщения + read receipts)

**Статус: ✅ DONE** (2026-10-31) · коммит `a97e1ed`, pushed в origin/main
Просьба: «Поправь сообщение 2.3 в STATE.md и отправь мне отчёт в Telegram» — STATE.md обновлён, этот отчёт отправлен.

## Что сделано

### Бэкенд (core + infra + app)
- **Миграция `V5__edit_reads.sql`:** `messages.edited_at`, `messages.deleted_at` (nullable); новая таблица `message_readers(message_id, reader_id, first_read_at, PK(message_id, reader_id))` + индексы.
- **CORE (`besedka-core/src/message.rs`):** `edit_message()` и `delete_message()` — только автор (иначе `Error::Forbidden`); `record_read()` — курсор: сдвигает `last_read_id`, если новое, иначе no-op (без UPDATE); `message_readers()` — дедуп по читавшему, время = первое прочтение; `unread_count()` — `max(id) > last_read_id`.
- **INFRA (`besedka-infra/src/db.rs`):** SQL-реализации для всех выше + `mark_room_read()` (сравнение id в одном UPDATE, `changes()==0` → false).
- **APP (`besedka-app/src/http/mod.rs`):**
  - `PATCH /api/v1/rooms/{id}/messages/{mid}` — `{text}` 1–4096, trim; не автор → **403**; нет/удалено → **404**; пустой/длинный → **400**; успех → 200 + сообщение.
  - `DELETE /api/v1/rooms/{id}/messages/{mid}` — только автор → **204** (без тела); чужое → **403**.
  - `POST /api/v1/rooms/{id}/read` — пустое тело → `{last_read_id}`; не участник → 403.
  - `GET /api/v1/rooms/{id}/messages/{mid}/readers` → `[{reader_id, reader_name, read_at}]`.
  - `GET /rooms/{id}/messages?after_id=` — поддержка «новее id» (поллинг не тащит старые страницы).
  - `JsonOrEmpty` — хелпер ответа: 200/201 с телом, **204 без тела** (раньше 204 с пустым JSON ломал старый клиент — исправлено).
- **Ролевые гейты сохранены:** все новые эндпоинты — AuthedUser + участник комнаты (403 для не-участника, 404 для чужой комнаты).

### UI (`ui/src/lib/api.ts`, `RoomChat.svelte`)
- **Редактирование (только свои):** ✎ → инлайн-поле с текущим текстом; Enter/«Сохранить» → PATCH, текст меняется на месте + бейдж «(изменено)»; Esc/«Отмена» — без запроса.
- **Удаление (только свои):** двухклика — «🗑 Удалить» → «⚠ Точно?» (авто-сброс через 3 с) → второй клик → DELETE, сообщение исчезает из списка.
- **Read receipts:** у своих сообщений метка «✓ N» (число прочитавших, включая автора); клик — раскрывает «Прочитали: alice, bob …» (сворачивается повторным кликом).
- **Отметка прочтения:** при открытии комнаты + при каждом поллинге, если появились новые (только если вкладка видна, `document.visibilityState`) → `POST /rooms/{id}/read` (no-op, если уже прочитано).
- `api.ts`: `editMessage`, `deleteMessage`, `markRoomRead`, `messageReaders` + тип `ReadReceipt`.

## Тесты и гейты (все зелёные)
- `cargo test --workspace` — **177 passed / 0 failed** (app 71, core 12, infra 94; новые: edit/delete автор-гейты, курсор read, дедуп readers, unread_count, HTTP PATCH/DELETE/204/403/404/400, `after_id` pagination).
- `cargo clippy -- -D warnings` — **0** · `cargo fmt --check` — чистый.
- `npm run check` (svelte-check) — **0 errors** · `npm run build` (vite) — **OK**.

## Живой браузер (end-to-end)
Сценарий alice/bob/carol (3 участника канала `obshchii`):
1. alice постит 2 сообщения → у обоих «✓ 1»; bob открывает комнату (поллинг → `POST /read`) → у alice «✓ 2»; клик по метке → «Прочитали: alice, bob» ✅ (скрин `.conveyor/screenshots/23-read-receipts.png`).
2. alice редактирует (✎ → поле → Сохранить) → текст на месте + «(изменено)» ✅ (скрин `23-edited-message.png`).
3. alice: «🗑 Удалить» → «⚠ Точно?» (сообщение ещё на месте) → второй клик → сообщение исчезло, в API — пусто ✅ (скрин `23-after-delete.png`).
4. bob (не автор) — PATCH/DELETE → **403** ✅; carol (не участник) — PATCH/DELETE/`POST /read` → **403** ✅.

## Известные ограничения
- Время в «Прочитали» в UI не показано (только имена) — в API `read_at` есть; можно докинуть в UI, если надо.
- Удалённое сообщение остаётся в `message_readers` (FK cascade уберёт вместе с сообщением при физическом удалении).
- Отправка из другого устройства/вкладки — read receipts синхронизируются через поллинг (без realtime-пуша; по плану realtime = Wave 5.1).

## Следующая волна
**3.1 — файлы в сообщениях** (картинки): API + UI, см. `PROMPT_wave24.md`.
