# Верификация волны 2.3 — edit + delete + read-receipts (read-only, host terminal)

Дата: 2026-10-31. Проведено строго по `.conveyor/VERIFY_wave23.md`, без единой
изменённой строки кода (только чтение, curl, sqlite3 в scratch-БД, UI smoke в браузере).

## Итог: **FAIL (частичная реализация)**

Ядро пользовательского функционала (inline-редактирование, двухклик-удаление,
✓N-рецепты, 403-гейты, тесты, гейты, коммиты, push) **на месте и работает** —
проверено live. Но несколько пунктов, которые отчёт волны и STATE.md
заявляют как сделанные, **в коде отсутствуют**: `after_id`-пагинация,
`unread_count()`, core-тип `ReadReceipt`, core-функции `edit_message`/
`delete_message`, таблица `message_readers`, колонка `deleted_at`.

## Шаг 1 — git (PASS)

| Проверка | Результат |
|---|---|
| `git log --oneline -6` | `9b58d21` feat(http,ui): edit, delete, read receipts, после `a97e1ed` feat(db,core): V5 edit+reads, после `9f23962` (2.1), `a9f8f49`, `5942818` |
| `git status --short` | пусто (чисто) |
| `git rev-parse HEAD` vs `git ls-remote origin main` | оба `9b58d219…` — локаль == GitHub |

## Шаг 2 — гейты (PASS)

| Команда | Результат |
|---|---|
| `cargo build` | ok |
| `cargo test --workspace` | **177 passed / 0 failed** (core 12, app 94, infra 71 — совпадает с ожидаемой суммой) |
| `cargo clippy --all-targets -- -D warnings` | 0 замечаний |
| `cargo fmt --check` | чистый |
| `ui: npm run check` (Node 24) | svelte-check 0 ошибок / 0 предупреждений |
| `ui: npm run build` | vite build ok (555 ms dev / production build ok) |

## Шаг 3 — свежая БД (PARTIAL)

Запуск: `BESEDKA_DB_PATH=…/scratch/verify23/fresh.db BESEDKA_PORT=… cargo run -p
besedka-app`; миграции V1–V5 применились без ошибок (log: `applying migration:
V5__edit_reads`, `sqlite ready`).

| Ожидалось | Факт |
|---|---|
| Таблицы users, rooms, room_members, messages, message_readers, room_read_cursors | users, rooms, room_members, messages, room_read_cursors — есть; **message_readers НЕТ** |
| `roles` | нет таблицы; эквивалент — колонка `users.role` (default 'user') |
| messages.edited_at | **есть** |
| messages.deleted_at | **НЕТ** (удаление — hard `DELETE FROM messages`) |

## Шаг 4 — живой API-сценарий (PASS по поведению, 1 FAIL по API surface)

Регистрация alice/bob/carol (201), alice→admin (scratch-БД, `UPDATE users SET
role='admin'`), комната создана (201), alice+bob в комнате, carol вне.

| Проверка | Ожидалось | Факт |
|---|---|---|
| alice `POST /rooms/1/messages` | 201 + id | **201** `{"message":{"id":1,…,"edited_at":null}}` |
| bob `POST /rooms/1/read` | 200 | **200** `{"last_read_id":1}` |
| alice `GET /rooms/1/messages/1/readers` | alice видит bob | **200** `{"users":[bob]}` |
| alice `PATCH …/messages/1` | 200 + edited_at | **200**, `edited_at=2026-10-31T14:48:50.839Z` |
| bob `PATCH` чужое | 403 | **403** "you can only edit your own messages" |
| bob `DELETE` чужое | 403 | **403** "you can only delete your own messages" |
| alice `DELETE` своё | 204, пустой body | **204**, 0 байт |
| после DELETE сообщение недоступно | 404 | см. FAIL-7 (GET-эндпоинта нет; удалено подтверждено списком + БД: 0 строк) |
| carol `POST /rooms/1/read` | 403 | **403** "you are not a member of this room" |
| carol `GET …/readers` | 403 | **403** |
| `GET /rooms/1/messages?after_id=N` | только сообщения с id > N | **FAIL: параметр игнорируется** — `after_id=2` и `after_id=999999` вернули весь список (оба сообщения) |

## Шаг 5 — UI smoke (PASS)

Vite :5173 (прокси → :8080), логин alice, комната «obshchii».

| Проверка | Факт |
|---|---|
| inline-редактирование | «✎ Изменить» → инлайн-поле + «Сохранить/Отмена»; текст сменился на «smoke test EDITED-OK», маркер «(изменено)» появился |
| двухклик-удаление | «🗑 Удалить» → «⚠ Точно?» → сообщение исчезло из списка и из БД |
| ✓N-рецепты | «✓ 1» на своих сообщениях; после `bob POST /read` UI live обновился до **«✓ 2»**, список «**Прочитали: alice, bob**» (polling работает) |

Примечание: счётчик ✓N включает автора (его собственный read-cursor), т.к.
запрос `message_readers` считает всех участников с `last_read_id >= message_id`
без исключения автора — согласованно в API и UI, но стоит иметь в виду.

## Шаг 6 — FAIL-список (команда, ожидалось, факт)

1. **`after_id`-пагинация отсутствует.**
   Команда: `grep -rn "after_id" crates/ ui/src/` → exit 1 (0 совпадений); live
   `GET /rooms/1/messages?after_id=2` → вернул оба сообщения (id 2 и 3).
   Ожидалось (report/STATE/промпт): «поддержка `GET /rooms/{id}/messages?after_id=`
   — „старше id"… для поллинга». Факт: `ListMessagesQuery` содержит только
   `limit` + `before_id` (пагинация 2.1).
2. **`message_readers`-таблица отсутствует.**
   Ожидалось (report/промпт): миграция V5 создаёт `message_readers`.
   Факт: `migrations/V5__edit_reads.sql` создаёт `room_read_cursors`
   (курсоры); «readers» вычисляются запросом по курсорам. Функционально
   эквивалентно для API — но схема не та, что заявлена.
3. **`deleted_at` отсутствует.**
   Ожидалось (report/промпт): `deleted_at` колонка + `Message.deleted_at`.
   Факт: репо-wide grep `deleted_at` → 0 совпадений; `delete_message` делает
   hard `DELETE`. Поведенчески соответствует критериям (204, недоступно) — но
   tombstone-схема не реализована.
4. **`unread_count()` отсутствует.**
   Ожидалось (report 2.3: «`unread_count()` для бейджа»). Факт: grep по всему
   репо → 0 совпадений.
5. **Core-тип `ReadReceipt` отсутствует.**
   Ожидалось (report: «тип ReadReceipt в core»). Факт: `besedka-core`
   экспортирует только `Message, Role, Room, RoomKind, User`.
6. **Core-функции `edit_message()`/`delete_message()` отсутствуют.**
   Ожидалось (report: «правила edit_message()/delete_message() в core»).
   Факт: `besedka-core/src/message.rs` — только `struct Message` (+ `edited_at`);
   правила авторства живут в `besedka-infra::db` / `besedka-app::http`.
7. **`GET /rooms/{id}/messages/{mid}` отсутствует.**
   Ожидалось (промпт шаг 4): после DELETE `GET …/messages/M` → 404.
   Факт: **405 Method Not Allowed** — роут существует только для PATCH/DELETE
   (в коде прямо: «any other method on this path is answered 405 by the router»).
   Удаление само по себе подтверждено: список пуст, в БД 0 строк.

## Вывод

Волна **частично реализована**: все пользовательские сценарии из приёмочных
критериев работают (edit/delete/✓N/403), 177/177 тестов, все гейты зелёные,
коммиты на GitHub. Однако отчёт волны и STATE.md описывают реализацию шире,
чем она есть: `after_id`, `unread_count()`, `ReadReceipt`, core-функции
редактирования, `message_readers` и `deleted_at` в коде отсутствуют.
Ничего не чинить — требуется решение: (а) доделать недостающее, или (б)
скорректировать report/STATE под фактическую схему (курсоры + hard delete).
