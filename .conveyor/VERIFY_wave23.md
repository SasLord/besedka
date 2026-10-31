Ты работаешь в проекте **Besedka** (Rust/Axum бэкенд + Svelte 5 UI). Это **верификационный прогон** (read-only по коду): ты НЕ чинишь и НЕ дорабатываешь — только проверяешь и фиксируешь PASS/FAIL.
Если доступен скилл `besedka` — загрузи (стек/гейты). Иначе — ТОЛЬКО `.conveyor/CONVENTIONS.md`.

=== ЗАДАЧА: Независимая проверка Wave 2.3 (edit/delete + read receipts) ===
Контекст: волна 2.3 делалась в сессии с обрывами (модель/агент) и перезагрузкой ноутбука. Нужно доказать, что всё реально на месте: код, миграции, тесты, гейты, коммиты, push.

ОЖИДАЕМОЕ СОСТОЯНИЕ (что проверяем):
- Коммиты (HEAD должен быть `e2cbe78` или новее):
  * `a97e1ed` — фича 2.3: миграция `V5__edit_reads.sql`, `db.rs` (edit/delete/record_read/readers/unread_count), `http/mod.rs` (PATCH/DELETE/POST read/GET readers/after_id, хелпер `JsonOrEmpty`), core-типы `edited_at`/`deleted_at`/`ReadReceipt`, `RoomChat.svelte` (инлайн-edit, двухклик-delete, ✓ receipts), `api.ts`.
  * `9b58d21` — отчёт `.conveyor/reports/wave23-report.md` + STATE.md (2.3 DONE).
  * `e2cbe78` — промпт следующего среза (позже переименован в `PROMPT_wave31.md`).
- API: `PATCH/DELETE /api/v1/rooms/{id}/messages/{mid}` (только автор → иначе 403; нет → 404; DELETE → **204 без тела**), `POST /rooms/{id}/read`, `GET /rooms/{id}/messages/{mid}/readers`, `GET /rooms/{id}/messages?after_id=`.
- UI: на своих сообщениях — edit/delete, «(изменено)», «✓ N» + «Прочитали: …»; на чужих — ничего.
- Гейты: `cargo test --workspace` = **177 passed / 0 failed** (app 71, core 12, infra 94); clippy 0; fmt чистый; svelte-check 0; vite build OK.

ШАГИ (строго по порядку):

1. **Git — ничего не потеряно:**
   - `git log --oneline -6` → видны `a97e1ed`, `9b58d21`, `e2cbe78`;
   - `git status --short` → пусто (чистое дерево);
   - `git remote -v` → origin = `https://github.com/SasLord/besedka.git`;
   - `git rev-parse HEAD` == `git ls-remote origin main | awk '{print $1}'` → **локаль = GitHub** (push состоялся);
   - `git show --stat a97e1ed` → список файлов совпадает с ожидаемым (V5-миграция, db.rs, http, core, RoomChat.svelte, api.ts).

2. **Статические гейты — перезапуск с нуля (всё должно быть зелёным):**
   - `cargo build` · `cargo test --workspace` → ровно **177 passed / 0 failed** (app 71 / core 12 / infra 94; если другое число — FAIL + приложить вывод);
   - `cargo clippy --all-targets -- -D warnings` → 0 · `cargo fmt --check` → чистый;
   - `cd ui && npm run check` → 0 errors · `npm run build` → OK;
   - **PITFALL:** НЕ чистить `ui/node_modules`, НЕ переустанавливать зависимости (два node: hermes x64 / nvm arm64 — нужны ОБА бинарника @rollup/@esbuild).

3. **Свежая БД — миграции применяются без ошибок:**
   - dev-БД в scratch (новый путь), `cargo run -p besedka-app` → стартует, миграции V1–V5 без ошибок в логе;
   - `sqlite3 <db> ".tables"` → `users`, `roles`, `rooms`, `room_members` (или эквивалент), `messages`, `message_readers`, `room_read_cursors`;
   - `sqlite3 <db> "PRAGMA table_info(messages)"` → есть колонки `edited_at`, `deleted_at`.

4. **Живой сценарий по API (curl):**
   - register + login: `alice`, `bob`, `carol`; alice и bob — join в комнату 1, carol — НЕ join;
   - alice: `POST /rooms/1/messages` → 201, запоминаем `id=M`;
   - bob: `POST /rooms/1/read` → 200; alice: `GET /rooms/1/messages/M/readers` → в списке **bob**;
   - alice: `PATCH /rooms/1/messages/M` `{"text":"новый текст"}` → 200 + `edited_at` заполнен;
   - bob: `PATCH` того же M → **403**; bob: `DELETE` того же M → **403**;
   - alice: `DELETE /rooms/1/messages/M` → **204, пустое тело**; `GET /rooms/1/messages/M` → **404**;
   - carol: `POST /rooms/1/read` → **403**; `GET /rooms/1/messages/<любое>/readers` → **403**.

5. **UI-смоук (визуально):**
   - `cd ui && npm run dev` → http://localhost:5173; залогиниться как alice;
   - комната открывается, пост работает, на своих сообщениях — кнопки edit/delete и «✓», на чужих — нет;
   - инлайн-edit сохраняет текст + «(изменено)»; двухклик-delete «Удалить»→«Точно?» → сообщение исчезло;
   - (если удобно) скриншоты → `.conveyor/screenshots/verify23-*.png`.

6. **Отчёт:** `.conveyor/reports/wave23-verify-report.md` — таблица: `пункт | ожидалось | факт | PASS/FAIL` + ключевые выводы команд (обрезанные). Вывод строкой:
   - все PASS → **«Wave 2.3 верифицирован, ничего не потеряно»**;
   - есть FAIL → перечислить каждый (команда, ожидалось, факт) и **остановиться** (ничего не чинить).

7. `git add .conveyor/reports/wave23-verify-report.md` (+ скриншоты, если были) · commit "docs(verify): Wave 2.3 verification — <PASS|FAIL>" · `git push origin main`.

8. **Telegram (попробовать, не навязывать):**
   `hermes send -t telegram --subject "Besedka Wave 2.3 — VERIFIED ✅" --file .conveyor/reports/wave23-verify-report.md`
   Сеть на `api.telegram.org` может быть закрыта (таймаут) — тогда максимум 1 повтор, зафиксировать в отчёте «Telegram: не доставлено (сеть)» и двигаться дальше. Это НЕ считается FAIL.

9. **Перед завершением:** остановить все фоновые процессы (API-сервер, vite, cargo-джобы) — не оставлять висящие.

Начни с шага 1.
