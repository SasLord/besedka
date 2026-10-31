Ты работаешь в проекте **Besedka** (Rust/Axum бэкенд + Svelte 5 UI). Конвейер: строго по срезам, гейты обязательны.
Если доступен скилл `besedka` — загрузи его (стек/гейты). Иначе — ТОЛЬКО `.conveyor/CONVENTIONS.md` (NEVER доверять памяти).

=== ЗАДАЧА: Исправить/дописать Wave 2.3 по отчёту верификации ===
Верификация (`VERIFY_wave23.md`) завершилась **FAIL (частичная реализация)**: пользовательские сценарии работают (177/177 тестов, гейты зелёные), но отчёт волны и STATE.md заявляют больше, чем есть в коде. Отчёт: **`.conveyor/reports/wave23-verify-report.md`** — прочитай ВПЕРВЫХ, это источник правды.

## 7 FAIL из отчёта и РЕШЕНИЕ по каждому (выполнять именно так):

| # | FAIL | Решение |
|---|---|---|
| 1 | `?after_id=` пагинация отсутствует (grep 0; live игнорируется) | **КОД:** реализовать в `GET /api/v1/rooms/{id}/messages`: параметр `after_id` → только сообщения с `id > after_id`, порядок по возрастанию id (вперёд-курсор для поллинга), лимит как у `before_id` (50/200). `before_id` остаётся как есть. Метод в infra + HTTP-тесты (включая «after_id=2 вернул только id>2», «after_id больше всех → []»). |
| 2 | таблицы `message_readers` нет (есть `room_read_cursors`) | **РЕШЕНИЕ (задокументировать):** курсоры — фактический дизайн, функционально эквивалентны и покрыты тестами; новую таблицу НЕ выдумывать. Зафиксировать в отчёте фикса + STATE.md. |
| 3 | `deleted_at`/tombstone нет (hard DELETE) | **РЕШЕНИЕ (задокументировать):** hard delete — фактический дизайн, поведение соответствует приёмке (204 → недоступно). Зафиксировать в отчёте фикса + STATE.md. |
| 4 | `unread_count()` отсутствует | **КОД:** `Db::unread_count(room_id, user_id) -> i64` в infra (кол-во сообщений комнаты с `id > last_read_id` участника; 0, если курсора нет) + тесты (с/без курсора, пустая комната). UI-бейдж — если удобно, опционально; API/тесты — обязательно. |
| 5 | core-тип `ReadReceipt` отсутствует | **КОД:** `pub struct ReadReceipt { pub user_id: i64, pub display_name: String }` в `besedka-core`, использовать в app-слое readers-эндпоинта. **JSON-форма API НЕ менять**: `{users:[{id, display_name}]}` — от неё зависит UI. |
| 6 | core-функции `edit_message()`/`delete_message()` отсутствуют | **КОД:** правила авторства вынести в core как чистые проверяемые функции (например `impl Message { pub fn can_edit_by(&self, actor_id: i64) -> bool }` + `can_delete_by`), infra/app обязаны вызывать их (а не дублировать проверку) + unit-тесты в core. |
| 7 | `GET /rooms/{id}/messages/{mid}` отсутствует (405) | **КОД:** реализовать: 200 + сообщение (участник); 404 если сообщения нет; 403 если не участник; 404 если комнаты нет. HTTP-тесты, включая «после DELETE → 404». |

## ШАГИ (строго по порядку):

1. **Контекст (прочитать ВСЁ):**
   - `.conveyor/CONVENTIONS.md` · `.conveyor/STATE.md` · `.conveyor/PLAN.md`
   - `.conveyor/reports/wave23-verify-report.md` (источник правды) · `.conveyor/reports/wave23-report.md`
   - Код: `crates/besedka-core/src/` (message.rs, lib.rs) · `crates/besedka-infra/src/db.rs` + `migrations/` · `crates/besedka-app/src/http/mod.rs` + `lib.rs` · `ui/src/lib/api.ts` + `RoomChat.svelte`

2. **Реализовать** FAIL 1, 4, 5, 6, 7 (код + тесты) и **зафиксировать решения** по FAIL 2, 3 (это осознанный дизайн, а не «забыли» — написать так, чтобы следующий верификатор не ставил их в FAIL повторно).

3. **Тесты по отчёту (новые, не только зелёные старые):**
   - `after_id`: live-сценарий из отчёта — `after_id=2` вернул только id>2, `after_id=999999` → `[]` (было: полный список);
   - `GET …/messages/{mid}`: 200 → 404 после DELETE;
   - core: can_edit_by/can_delete_by (свой/чужой), ReadReceipt;
   - unread_count: с/без курсора.
   - Старые 177 тестов остаются зелёными (число только вырастает).

4. **ГЕЙТЫ (все ДО commit):**
   - `cargo build` OK · `cargo test --workspace` — 0 failed (все новые тесты на месте) · `cargo clippy --all-targets -- -D warnings` = 0 · `cargo fmt --check` чистый;
   - UI трогали (хотя бы api.ts/бейдж) → `cd ui && npm run check` = 0 · `npm run build` OK;
   - **PITFALL:** НЕ чистить `ui/node_modules`, НЕ переустанавливать зависимости (два node: hermes x64 / nvm arm64 — нужны ОБА бинарника @rollup/@esbuild).

5. **Живая перепроверка (scratch-БД, порт 8080):** register alice/bob/carol (пароли в curl — в отчёте писать `[REDACTED]`), alice→admin (sqlite), комната, пост; затем точечно прогнать 7 пунктов отчёта: `after_id=2` / `after_id=999999`, GET single (200 → после DELETE 404), readers, unread. Скриншоты — необязательно.

6. **Отчёт:** `.conveyor/reports/wave23-fix-report.md` — таблица `FAIL # | было | стало (код/решение) | доказательство (обрезанный вывод)` + гейты + «Wave 2.3 закрыт полностью (5 пунктов кодом, 2 — задокументированный дизайн)». `wave23-report.md` и `wave23-verify-report.md` **НЕ переписывать** (это история). `STATE.md`: у 2.3 добавить пометку **FIXED** + список изменений.

7. **commit** (2-3 коммита по смыслу: `feat(...)` для кода, `docs(conveyor): wave 2.3 fix — report + STATE`) · `git push origin main` (проверить, что `git rev-parse HEAD` == `git ls-remote origin main | awk '{print $1}'`).

8. **Telegram (попробовать, не навязывать):**
   `hermes send -t telegram --subject "Besedka Wave 2.3 — FIXED ✅" --file .conveyor/reports/wave23-fix-report.md`
   Сеть на `api.telegram.org` может быть закрыта (таймаут) — максимум 1 повтор, зафиксировать в отчёте «Telegram: не доставлено (сеть)». Это НЕ считается FAIL.

9. **Перед завершением:** остановить все фоновые процессы (API-сервер, vite, cargo-джобы) — не оставлять висящие.

Начни с шага 1.
