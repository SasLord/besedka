# Wave 2.2 — independent verification report

- **Дата:** 2026-11-02 (UTC+7)
- **HEAD:** `main`, локаль == GitHub (`git rev-parse HEAD` == `git ls-remote origin main`)
- **Верифицировано по:** `.conveyor/VERIFY_wave22.md` (read-only по коду — ничего не чинили)
- **Среда:** macOS arm64, Node 24 (nvm), Rust (cargo workspace), БД в scratch: `~/.hermes/cache/scratch/besedka-verify22/fresh22.db`

## Итог

**Wave 2.2 верифицирован, ничего не потеряно.** Все пункты — PASS. Отдельные расхождения с
формулировками чек-листа (не дефекты, а неточности описаний) — в разделе «Замечания».

## Таблица результатов

| # | Пункт | Ожидалось | Факт | PASS/FAIL |
|---|-------|-----------|------|-----------|
| 1a | Git: коммиты 2.2 в истории | `08827a0`, `45ef2a5` видны в `git log` | Оба в истории `main`, `git branch --contains` → main | **PASS** |
| 1b | Git: чистый статус, локаль = GitHub | `git status --short` пусто; HEAD == origin/main | Пусто; SHA совпадают | **PASS** |
| 1c | Git: состав `08827a0` | RoomChat.svelte, App.svelte, api.ts, db.rs, http/mod.rs | `git show --stat` — все файлы в составе | **PASS** |
| 2a | `cargo build` | OK | OK | **PASS** |
| 2b | `cargo test --workspace` | 0 failed, ≥ 170 passed | **184 passed, 0 failed** (тесты `list_users` присутствуют и проходят: `list_users_returns_all_with_name_fallback_sorted`, `list_users_empty_when_no_users`) | **PASS** |
| 2c | `cargo clippy --all-targets -- -D warnings` | 0 | 0 | **PASS** |
| 2d | `cargo fmt --check` | чистый | чистый | **PASS** |
| 2e | `npm run check` (ui) | 0 errors | 0 errors | **PASS** |
| 2f | `npm run build` (ui) | OK | OK | **PASS** |
| 3a | Свежая БД + старт бэкенда :8080 | старт, миграции без ошибок | старт OK, миграции применились | **PASS** |
| 3b | Таблицы БД | users, rooms, room_members, messages (+room_read_cursors) | `messages, room_members, rooms, room_read_cursors, users` (+ refinery_schema_history) | **PASS** |
| 3c | `Db::list_users` в infra | SELECT id, display_name (fallback), ORDER BY display_name | `db.rs:157`: `SELECT id, COALESCE(display_name, username) FROM users ORDER BY display_name` — fallback на месте | **PASS** |
| 3d | Маршрут `GET /users` | в http/mod.rs, AuthedUser | `http/mod.rs:406` — `list_users` (AuthedUser, «any logged-in user») | **PASS** |
| 3e | register alice/bob → 201, login | 201 / кука сессии | 201, логин OK (сессия = сессионная кука, не Bearer) | **PASS** |
| 3f | alice → admin (UPDATE в scratch) | роль admin | `SELECT username, role FROM users` → alice=admin, bob=user | **PASS** |
| 3g | alice: POST /rooms (channel «verify22») | 201 | 201 (поле запроса — `title`, room id=1) | **PASS** |
| 3h | bob: POST /rooms/1/join | 200 | 200 | **PASS** |
| 3i | bob: GET /users → 200, есть alice и bob; без аутентификации → 401 | 200 {users:[…]} / 401 | `200 {"users":[{id:1,"alice"},{id:2,"bob"},{id:3,"carol"}]}`; без куки `401 {"error":"not logged in"}` | **PASS** |
| 4a | UI: комната видна, клик → чат | заголовок + список (пустой — ОК) | «verify22», бейдж «канал · N участник(ов)», «Пока пусто — напишите первое сообщение.» | **PASS** |
| 4b | Отправка «привет 2.2» | имя «Вы», время, текст | «Вы 22:46 … привет 2.2» | **PASS** |
| 4c | 2-е сообщение, дублей по id нет | 2 разных сообщения | «привет 2.2» и «второе от alice» — по одному, дублей нет | **PASS** |
| 4d | Пустое поле не шлёт | кнопка disabled, Enter не шлёт | «Отправить» disabled при пустом поле; Enter пустого поля → сообщений не появилось | **PASS** |
| 4e | Имена + live-поллинг | пост bob через API появляется сам за ~3–5 с без перезагрузки, имя «bob» (не «Пользователь #N»/«Unknown») | маркер `window.__pollmark` жив (перезагрузки не было); «от боба» появилось с именем «bob»; интервал поллинга 5 с (POLL_MS=5000) | **PASS** |
| 4f | Пагинация: 50 в списке + кнопка | ровно 50 + «Загрузить ещё» | после 53 сообщений в БД: свежая загрузка = ровно 50 + кнопка | **PASS** |
| 4g | Клик «Загрузить ещё» | список растёт, новые (старшие) вверху, свои/чужие не теряются | 50→53: старейшие 3 (привет 2.2, второе от alice, от боба) вставились вверху, бот-сообщения 1..50 на месте, кнопка исчезла (всё загружено) | **PASS** |
| 4h | Не-участник carol | комната в списке; клик → состояние «Присоединиться» (не чат); кнопка → чат с постами | комната в списке (joined:false); клик → гейт «Вы ещё не в «verify22» / Войти в комнату»; после join — чат: поле ввода, 50 постов, кнопка «Загрузить ещё», в БД carol участник | **PASS** |
| 4i | Поллинг не гоняется на скрытой вкладке | (опционально) | не проверялось (опциональный пункт; в коде `visibilitychange` + `onDestroy` гасит таймер) | **н/п** |
| 5 | api.ts | Message, listMessages(roomId, limit?, beforeId?), postMessage(roomId, text), fetchUsers() | все 4 функции + интерфейс Message на месте (api.ts:35/163/172/215) | **PASS** |
| 6 | RoomChat подключён в App.svelte | импорт + использование | 2 вхождения в App.svelte | **PASS** |

## Ключевые выводы команд (обрезано, без паролей)

```
$ git log --oneline -12
… 08827a0 feat: RoomChat (post + list + pagination + live polling) + users catalog …
… 45ef2a5 docs(2.2): verification report …

$ cargo test --workspace
… 184 passed; 0 failed; 0 ignored …

$ sqlite3 fresh22.db ".tables"
messages  room_members  rooms  room_read_cursors  users  refinery_schema_history

$ sqlite3 fresh22.db "SELECT username, role FROM users ORDER BY id;"
alice|admin
bob|user
carol|user

$ curl -s -w '\n%{http_code}\n' http://127.0.0.1:8080/api/v1/users          # без куки
{"error":"not logged in"}
401

$ curl -s -w '\n%{http_code}\n' -b jar_bob.txt http://127.0.0.1:8080/api/v1/users
{"users":[{"id":1,"display_name":"alice"},{"id":2,"display_name":"bob"},{"id":3,"display_name":"carol"}]}
200
```

Браузерные наблюдения (vite :5173, логин alice / carol):

- Отправка: `«Вы» 22:46 → «привет 2.2»`, `«Вы» → «второе от alice»`; пустое поле — кнопка
  «Отправить» disabled, Enter не шлёт.
- Live: `window.__pollmark='alive-22'` установлен ДО поста bob → после 5–6 с сообщение
  «от боба» (имя «bob») в чате, маркер жив → перезагрузки не было.
- Пагинация: до клика `бот-сообщение 1..50` (ровно 50), alice-сообщений нет; после клика —
  `привет 2.2 → второе от alice → от боба → бот-сообщение 1..50` (всего 53), кнопка исчезла.
- Carol: `joined:false` в списке; гейт-экран вместо чата; после «Войти в комнату» — чат с 50
  постами и кнопкой «Загрузить ещё»; в БД: carol в `room_members`.

## Замечания (расхождения с формулировками чек-листа, НЕ дефекты)

1. **«новые вставляются сверху»** (п. 4e): реализация (тот же код, что и в оригинальном
   `08827a0`) — хронологический список, новые дописываются **вниз** (низ = новое, автоскролл
   при приближении к низу); «Загрузить ещё» вставляет старые — **вверху** (это и есть
   «новые вверху» для пагинации). Формулировка чек-листа унаследована из
   `wave22-report.md` и не совпадает с его же реализацией. Функциональное ядро (live без
   перезагрузки, без дублей, имена резолвятся) — работает.
2. **«GET /users (Bearer)»** (п. 3i): аутентификация в проекте — сессионные куки, Bearer
   нигде не поддерживается (extractor `AuthedUser`, `crates/besedka-app/src/http/extract.rs`).
   Семантика «с аутентификацией 200 / без — 401» соблюдена.
3. **«поллинг каждые ~3 с»** (п. 4e): в коде `POLL_MS = 5000` (5 с) — это соответствует
   исходной спецификации `PROMPT_wave22.md` («каждые 5 сек»). Пост bob появился за 5–6 с.
4. Кнопка join у не-участника называется **«Войти в комнату»** (гейт: «Присоединяйтесь к
   комнате…») — состояние эквивалентно ожидаемому «Присоединиться».
5. `POST /rooms` принимает поле **`title`** (не `name`) — контракт API, UI использует то же
   поле.
6. Опциональный пункт (скрытая вкладка) не проверялся — помечено «н/п».

## Вывод

**Wave 2.2 верифицирован, ничего не потеряно** — все обязательные пункты PASS.

## Telegram

(заполняется после попытки отправки)
