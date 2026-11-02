# Верификация волны 2.1 — сообщения (read-only, host terminal)

Дата: 2026-11-02. Проведено строго по `.conveyor/VERIFY_wave21.md`, без единой
изменённой строки кода (только чтение, curl, sqlite3 в scratch-БД). UI smoke не
требуется — 2.1 не добавляет UI (чат в UI — волна 2.2).

## Итог: **PASS — Волна 2.1 верифицирована, ничего не потеряно**

Все пункты волны на месте и работают: миграция V4, core-тип, infra-функции,
HTTP API (create/read/join/403/400/404/пагинация), тесты 2.1, все гейты,
коммиты на GitHub. Ничего не отсутствует. Отсутствующая в списке имён
отправителя — задокументированное ограничение 2.1 (заполняется позже),
отмечено как OK.

## Итоговая таблица (item | expected | actual | PASS/FAIL)

| item | expected | actual | PASS/FAIL |
|---|---|---|---|
| **Шаг 1 — git** | | | |
| 4 коммита (2.1→2.2→state→2.3) | `89b7b39`, `7f52668`, `08827a0`, `a97e1ed` в этом порядке | все 4 есть, порядок 89b7b39→7f52668→08827a0→a97e1ed→новее | PASS |
| рабочее дерево чистое | `git status --short` пусто | пусто | PASS |
| HEAD == origin/main | совпадение | оба `55d8a574…` | PASS |
| Node + нативные deps | arm64/x64 binaries @rollup/@esbuild | оба имеют arm64+x64 (nvm v24.21.0) | PASS |
| **Шаг 2 — гейты** | | | |
| `cargo build` | ok | ok (exit 0) | PASS |
| `cargo test --workspace` | ≥166 passed, 0 failed | **184 passed / 0 failed** (app 73, core 15, infra 96) | PASS |
| `cargo clippy --all-targets -- -D warnings` | 0 замечаний | 0 замечаний (exit 0) | PASS |
| `cargo fmt --check` | чистый | чистый (exit 0) | PASS |
| `ui: npm run check` (Node 24) | 0 ошибок | 0 ошибок | PASS |
| `ui: npm run build` | build ok | vite build ok | PASS |
| **Шаг 3 — свежая БД** | | | |
| миграции V1–V5 | применились без ошибок | V1__users, V3__rooms, V4__messages, V5__edit_reads — ok, sqlite ready | PASS |
| таблицы | users, rooms, room_members, messages | все есть (+ room_read_cursors из 2.3) | PASS |
| колонки messages | id, room_id, sender_id, text, created_at | есть + edited_at (из 2.3) | PASS |
| индексы | (room_id, id) и sender_id | `idx_messages_room_id (room_id, id)`, `idx_messages_sender (sender_id)` | PASS |
| FK-каскад | room_id→rooms, sender_id→users CASCADE | оба `ON DELETE CASCADE` | PASS |
| **Шаг 4 — живой API (13/13 + admin)** | | | |
| регистрация alice/bob/carol | 201 | 201 | PASS |
| alice (admin) `POST /rooms` | 201 | **201** | PASS |
| bob `POST /rooms` (не-админ) | 403 | **403** | PASS |
| bob `POST /rooms/1/messages` (участник) | 201 + id | **201** | PASS |
| carol `POST /rooms/1/messages` (не-участник) | 403 | **403** «not a member» | PASS |
| bob пустой текст | 400 | **400** | PASS |
| bob текст > 4096 | 400 | **400** | PASS |
| bob `GET /rooms/1/messages` | 200 + содержит «hello 21» | **200**, текст на месте | PASS |
| `GET ?limit=25` | ровно 25 | **25** (ids 33–57) | PASS |
| `GET ?limit=25&before_id=` | ещё 25, без дублей id | **25**, overlap=0 | PASS |
| `GET ?limit=999` | 400 ИЛИ ≤200 позиций | **400** «limit must be between 1 and 200» | PASS |
| `GET /rooms/999/messages` | 404 | **404** | PASS |
| границы limit (1..=200) | валидные → 200, невалидные → 400 | 1→200(n=1); 200→200(n=57); 201/999/0/−5/abc→400 | PASS |
| **Шаг 5 — UI** | | | |
| smoke-тест | не нужен (UI-чат — 2.2) | гейты шага 2 зелёные; чат-UI ещё нет (ожидалось) | PASS |
| **Пункт 6 — конкретные тесты 2.1** | | | |
| create→read | тест есть и проходит | `create_message_then_read_back` 1 passed | PASS |
| пагинация/курсор/границы limit | тест есть и проходит | `list_messages_pagination_and_cursor`, `list_messages_rejects_bad_limit_and_cursor` — passed | PASS |
| FK-каскад | тест есть и проходит | `deleting_room_cascades_to_messages`, `deleting_user_cascades_to_messages` — passed | PASS |
| пустая комната | тест есть и проходит | `empty_room_lists_no_messages` — passed | PASS |
| приёмочный поток admin→post→read | тест есть и проходит | `admin_creates_channel_posts_and_reads` 1 passed | PASS |
| 403 не-участник | тест есть и проходит | `post_message_requires_membership` 1 passed | PASS |
| 403 не-админ | тест есть и проходит | `create_room_requires_admin` 1 passed | PASS |
| 400 текст | тест есть и проходит | `post_message_rejects_empty_and_long_text` 1 passed | PASS |
| пагинация 2 страницы | тест есть и проходит | `messages_pagination_two_pages` 1 passed | PASS |
| 404 неизвестная комната | тест есть и проходит | `messages_unknown_room_is_404` 1 passed | PASS |
| **Ожидаемое состояние** | | | |
| core `Message {id,room_id,sender_id,text,created_at}` | есть в `message.rs` | есть (+ `edited_at` из 2.3 — допустимо) | PASS |
| время RFC 3339 UTC ms | `created_at` в этом формате | задокументировано, дефолт `strftime('%Y-%m-%dT%H:%M:%fZ','now')` | PASS |
| «канал» = `RoomKind::Channel` | нет отдельной таблицы | подтверждено (комната typed) | PASS |
| `V4__messages.sql` | PK AUTOINCREMENT, 2×FK CASCADE, text NOT NULL, created_at, 2 индекса | в точности как в спеке | PASS |
| infra `create_message`/`list_messages`/`get_message` | есть в `db.rs` | все три присутствуют | PASS |
| API-контракт | GET/POST `/rooms/{id}/messages`, 403/404/400, `before_id` | работает; **имя отправителя не отдаётся** (2.1-лимит — OK) | PASS |

## Ключевые выходы команд (обрезано, без паролей)

```
=== cargo build ===
   Compiling besedka-core v0.1.0 (...)
   Compiling besedka-infra v0.1.0 (...)
   Compiling besedka-app v0.1.0 (...)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.00s     (BUILD_EXIT=0)

=== cargo test --workspace ===
   Compiling besedka-app v0.1.0 (...)
running 73 tests ... test result: ok. 73 passed; 0 failed; 0 ignored
   Doc-tests besedka_app ... test result: ok. 0 passed; 0 failed
   Compiling besedka-infra v0.1.0 (...)
running 96 tests ... test result: ok. 96 passed; 0 failed; 0 ignored
   Doc-tests besedka_infra ... test result: ok. 0 passed; 0 failed
   Compiling besedka-core v0.1.0 (...)
running 15 tests ... test result: ok. 15 passed; 0 failed; 0 ignored
   Doc-tests besedka_core ... test result: ok. 0 passed; 0 failed
   (ИТОГО: 184 passed / 0 failed — ≥166)

=== cargo clippy --all-targets -- -D warnings ===
    Checking besedka-core ... Checking besedka-infra ... Checking besedka-app ...
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.77s     (CLIPPY_EXIT=0, 0 warnings)

=== cargo fmt --check ===
(FMT_EXIT=0 — чистый)

=== ui (nvm Node 24.21.0) ===
$ npm run check   → 0 errors
$ npm run build   → vite build ok

=== свежая БД ===
BESEDKA_DB_PATH=.../scratch/verify21/fresh.db BESEDKA_PORT=8081 cargo run -p besedka-app
  applying migration: V1__users / V3__rooms / V4__messages / V5__edit_reads  → sqlite ready

=== живой API (bob/carl/alice, scratch) ===
alice POST /rooms                      → 201
bob   POST /rooms                      → 403
bob   POST /rooms/1/messages           → 201 {id:…}
carol POST /rooms/1/messages           → 403 "not a member"
bob   POST  (empty)                    → 400
bob   POST  (5000 chars)               → 400
bob   GET /rooms/1/messages            → 200  (содержит "hello 21")
bob   GET ?limit=25                    → 200  n=25
bob   GET ?limit=25&before_id=<min>    → 200  n=25  overlap=0
bob   GET ?limit=999                   → 400  "limit must be between 1 and 200"
bob   GET /rooms/999/messages          → 404
```

## Вывод

**Волна 2.1 верифицирована, ничего не потеряно.** Все пункты на месте и работают
live: миграция V4 (таблица `messages` + индексы + FK-каскад), core-тип `Message`,
infra-функции (`create_message`/`list_messages`/`get_message`), HTTP API
(create/read/join, 403 не-участник, 403 не-админ, 400 текст, 404 комната,
пагинация `before_id`/`limit` 1..=200), все конкретные тесты 2.1 (1 passed /
0 failed каждый), 184/184 теста workspace, все гейты зелёные, коммиты на GitHub.

Единственное отличие от «полной» картины — **имя отправителя не возвращается**
в `GET /rooms/{id}/messages` (в теле — только `sender_id`). Это задокументированное
ограничение 2.1 («заполняется позже»), зафиксировано как OK, не FAIL.

FAIL-пунктов нет. Чинить не требуется — отчёт готов, можно фиксировать результат.
