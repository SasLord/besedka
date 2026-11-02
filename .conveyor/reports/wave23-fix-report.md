# Wave 2.3 — FIX (по отчёту верификации)

Дата: 2026-11-02. Источник правды — `.conveyor/reports/wave23-verify-report.md` (FAIL, частичная
реализация: 7 пунктов). Решение по каждому — по `.conveyor/PROMPT_wave23-fix.md`:
**5 пунктов закрыты кодом** (FAIL 1, 4, 5, 6, 7), **2 — осознанный дизайн, задокументирован**
(FAIL 2, 3). `wave23-report.md` и `wave23-verify-report.md` не переписаны (история).

## Таблица: FAIL → что сделано

| FAIL # | было | стало (код / решение) | доказательство (обрезанный вывод) |
|---|---|---|---|
| 1 | `?after_id=` отсутствует (grep 0; live — полный список) | **КОД:** `Db::list_messages_after(room_id, after_id, limit)` (infra) + параметр `after_id` в `GET /api/v1/rooms/{id}/messages` (app): только `id > after_id`, порядок по возрастанию id, лимит 50/200 как у `before_id`; `before_id` не тронут. Тесты: infra `list_messages_after_returns_only_newer_in_ascending_order`, app `list_messages_after_id_returns_only_newer_ascending` | live: `GET /rooms/1/messages?after_id=2` → `[200] {"messages":[{"id":3,...,"text":"tretie",...}]}` (только id>2); `after_id=999999` → `[200] {"messages":[]}` |
| 2 | таблицы `message_readers` нет (есть `room_read_cursors`) | **РЕШЕНИЕ (дизайн):** `room_read_cursors` — фактическая схема; «readers» вычисляются запросом по курсорам (`last_read_id >= message_id`). Функционально эквивалентно заявленному поведению API, покрыто тестами (курсor, дедуп, unread). Новую таблицу не выдумывать: дублирующая `message_readers` создала бы второй источник истины для того же факта. **Этот пункт — задокументированный дизайн, не «забыли»** | live: `bob POST /rooms/1/read` → `[200] {"last_read_id":3}`; `GET /rooms/1/messages/2/readers` → `[200] {"users":[{"id":2,"display_name":"bob"}]}`; 403 для не-участника |
| 3 | `deleted_at`/tombstone нет (hard `DELETE`) | **РЕШЕНИЕ (дизайн):** hard delete — фактический дизайн: `DELETE …/messages/{mid}` → **204**, после — сообщение недоступно (GET → 404, в списке отсутствует, в БД 0 строк). Приёмочные критерии 2.3 выполнены без tombstone; «мягкое удаление» — отдельный срез, если потребуется аудит. **Этот пункт — задокументированный дизайн, не «забыли»** | live: `alice DELETE /rooms/1/messages/1` → `[204]` (0 байт); затем `GET /rooms/1/messages/1` → `[404]` |
| 4 | `unread_count()` отсутствует (grep 0) | **КОД:** `Db::unread_count(room_id, user_id) -> i64` (infra): кол-во сообщений комнаты с `id > last_read_id` участника; **0, если курсора нет**. Тест: infra `unread_count_tracks_messages_past_the_cursor` (с/без курсора). UI-бейдж — опционально по промпту, в этом срезе UI не тронут | live (реплика SQL, room=1, bob, курсор=3, пришло сообщение id=4): `sqlite3 … SELECT … END AS unread_for_bob` → `1`; carol (курсор нет) → `0` |
| 5 | core-тип `ReadReceipt` отсутствует | **КОД:** `pub struct ReadReceipt { user_id, display_name }` в `besedka-core::message`, экспортирован из `lib.rs`; используется в app-слое `message_readers` (infra отдаёт строки `ReadReceipt`). **JSON-форма API НЕ изменилась:** `{"users":[{"id","display_name"},...]}` — UI зависит от неё. Тест: core `read_receipt_serde_roundtrip` | live: `GET /rooms/1/messages/2/readers` → `[200] {"users":[{"id":2,"display_name":"bob"}]}` (форма без изменений) |
| 6 | core-функции `edit_message()`/`delete_message()` отсутствуют (правила дублировались) | **КОД:** `impl Message { can_edit_by(&self, actor_id) -> bool }` + `can_delete_by` в `besedka-core` (чистые проверяемые правила: только автор); `Db::edit_message`/`Db::delete_message` (infra) теперь **вызывают их** вместо дублирования проверки авторства. Тесты: core `can_edit_by_author_only`, `can_delete_by_author_only` | live: `bob PATCH …/messages/2` → `[403] {"error":"you can only edit your own messages"}`; `bob DELETE …/messages/2` → `[403] {"error":"you can only delete your own messages"}` |
| 7 | `GET /rooms/{id}/messages/{mid}` отсутствует (405) | **КОД:** новый `get_message` (app) + роут: 200 + сообщение (участник); 403 если не участник; 404 если сообщения нет; 404 если комнаты нет. Тест: app `get_single_message_by_id_membership_and_lifecycle` (включая «после DELETE → 404») | live: `alice GET …/messages/1` → `[200] {"message":{...}}`; `carol GET …/messages/1` → `[403] {"error":"you are not a member of this room"}`; `GET …/messages/999999` → `[404]`; `GET /rooms/999999/messages/1` → `[404]` |

## Гейты (все до commit)

| Команда | Результат |
|---|---|
| `cargo build` | ok |
| `cargo test --workspace` | **184 passed / 0 failed** (app **73** — было 71, core **15** — было 12, infra **96** — было 94; +7 новых, старые 177 зелёные) |
| `cargo clippy --all-targets -- -D warnings` | 0 |
| `cargo fmt --check` | чистый |
| `npm run check` / `npm run build` | не требовались — **UI в срезе не тронут** (JSON-форма API не изменилась; `ui/node_modules` не чистили) |

## Живая перепроверка (scratch-БД, порт 8080)

`BESEDKA_DB_PATH=…/scratch/w23fix/fresh.db BESEDKA_PORT=8080 cargo run -p besedka-app`;
миграции V1–V5 применились без ошибок. Сценарий: register alice/bob/carol (201; пароли —
`[REDACTED]`, не фиксируются), alice → admin (sqlite), alice создаёт канал, alice+bob
присоединились, carol вне; alice постит 4 сообщения. Прогнаны все 7 пунктов — **все PASS**
(детали в таблице выше). Фоновые процессы после проверки остановлены, порт 8080 освобождён.

## Итог

**Wave 2.3 закрыт полностью: 5 пунктов кодом (1, 4, 5, 6, 7), 2 — задокументированный дизайн (2, 3).**
Отчёт волны (`wave23-report.md`) и отчёт верификации (`wave23-verify-report.md`) остаются историей;
STATE.md у 2.3 пометка **FIXED** + список изменений.

## Telegram

**Не доставлено (сеть):** 2 попытки (1 + 1 повтор) — `hermes send: Telegram send failed: Timed out`
(оба раза, 2026-11-02). По промпту это НЕ FAIL; отчёт доставляется здесь, в чат.
