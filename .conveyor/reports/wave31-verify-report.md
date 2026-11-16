# Верификация Wave 3.1 — FAIL (частичная реализация, воркспейс не компилируется)

Дата: 2026-11-16. Проверялось по `.conveyor/PROMPT_wave31.md` (коммит `dbcbf16`) против
фактического кода (`git status` — вся волна НЕЗАКОММИЧЕНА: modified + untracked).

## Итог

**FAIL.** Core и infra слои сделаны и зелёные, но app-слой не существует как код:
`cargo check --workspace` падает (E0433), HTTP-эндпоинты 3.1 отсутствуют, UI-шаг не
выполнен, гейты/живая проверка/отчёт/STATE/PROMPT_wave32 — всё пропущено.

## ✅ Реализовано (проверено кодом и тестами)

| Часть | Статус |
|---|---|
| core: `File`, `MAX_FILE_SIZE_DEFAULT` (25 MiB), `ALLOWED_CONTENT_TYPES`, `validate_upload(size, max, type)` (лимит — аргумент), `sanitize_filename` (пути/`""`→`"file"`/255/мультбайт-безопасно) | ✅ `crates/besedka-core/src/file.rs`, 13 тестов |
| core: тесты границ (ровно max ок, +1 — reject, лимит 8 байт; типы; sanitize `../../etc/passwd`, `a b/c.txt`, пустое) | ✅ все в file.rs |
| infra: миграция V6 `files` + `idx_files_uploader` | ✅ `migrations/V6__files.sql` (V6 неприменена в dev-БД, схема на V5) |
| infra: `Db::create_file/get_file/delete_file` + 3 теста | ✅ `db.rs` |
| app: `HttpError::payload_too_large` / `unsupported_media_type` (плоский формат) | ✅ `http/mod.rs` |
| app: секция `[files] max_size_bytes` + 2 теста конфига | ✅ `config.rs` (но без `dir`, см. FAIL-4) |
| workspace: axum `multipart`, multer в Cargo.lock | ✅ |
| Тесты core/infra | ✅ core 28 passed (база 15 +13), infra 99 passed (база 96 +3), 0 failed |

## ❌ FAIL (проверено фактом)

1. **Воркспейс НЕ компилируется.** `cargo check --workspace` → `error[E0433]: cannot find
   'files' in 'http'` (lib.rs:86 ссылается на `http::files::upload_file`, модуля нет; в
   http/mod.rs объявлены только `extract` и `sso`). Второй закладной брак: `main.rs`
   передаёт `cfg.files.file_upload_max_bytes` — поля нет в `FilesConfig` (есть
   `max_size_bytes: u64`, а аргумент `app()` — `usize`).
2. **HTTP-слой 3.1 отсутствует целиком.** `http/files.rs` нет: нет `POST /api/v1/files`
   (multipart), нет `GET /api/v1/files/{id}` (inline-serve), нет 400/413/415/404/410,
   нет RFC 5987 `filename*`, нет Content-Length, нет хранения байтов на диске
   (`{dir}/<id>_<safe>`), нет проверки размера до полного чтения, нет HTTP-тестов
   (roundtrip/413/415/400/404/кириллица).
3. **Неверный маршрут и форма API.** В lib.rs зарегистрирован
   `/api/v1/rooms/{id}/files` (room-scoped; спека — глобальный `/api/v1/files`);
   `AppState.file_upload_max_bytes: usize` + `app()` принимает сырой usize вместо
   секции конфига.
4. **Конфиг неполный.** Нет `dir` (дефолт `data/files`) и env-оверрайдов
   `BESEDKA_FILES_DIR` / `BESEDKA_FILES_MAX_SIZE_BYTES` (паттерн — config.rs:148-156).
5. **Wire-формат.** `File` имеет `#[serde(rename_all = "camelCase")]`, а весь проект —
   snake_case (Message/User/Room, пример тела 201 в промпте тоже snake_case). Ломает
   контракт с api.ts.
6. **UI-шаг не выполнен.** `ui/src/lib/api.ts` не тронут: нет типа `File`,
   `uploadFile(file, onProgress?)`, `fileUrl(file)`.
7. **Мусор/чужой scope.** `crates/besedka-app/src/ws/` (untracked, «Wave 2.4» — волны
   нет в PLAN.md; `handler.rs` отсутствует → не скомпилируется, если объявить) и
   `.conveyor/patches/uncommitted-direct-room-WIP.patch` (WIP чужого среза).
8. **Конвейерные шаги пропущены.** Коммит волны нет (всё dirty); гейты не проходили
   (некомпилируется); живая проверка (curl 201/415/413/404, превью, sha256) нет;
   скриншот `31-file-preview.png` нет; `.conveyor/reports/wave31-report.md` нет;
   STATE.md без «3.1 DONE»; `PROMPT_wave32.md` не написан; Telegram-отчёт не отправлен.
9. **Номер волны и цифры в комментариях.** «Wave 3.4» в config.rs/lib.rs/http/mod.rs;
   «default cap 10 MiB» и захардкоженные 10 MiB в тестах lib.rs — в спеке дефолт
   25 MiB из конфига.

## ⚖️ Отклонение от спеки (решение зафиксировано в fix-промпте)

- **FK на `uploader_id`**: промпт 3.1 требовал `REFERENCES users(id) ON DELETE CASCADE`;
  реализовано БЕЗ FK (комментарий в миграции: каскад удалит метаданные, но оставит
  сиротские байты на диске — очистка байтов на app-слое). V6 неприменена → безопасно
  править. **Решение fix-промпта: оставить без FK и задокументировать** (осознанный
  дизайн, как в wave23-fix FAIL 2/3).

## Команда для повторной проверки после фикса

```
cargo check --workspace && cargo test --workspace
cargo clippy --all-targets -- -D warnings && cargo fmt --check
grep -n "file_upload_max_bytes\|Wave 3.4" -r crates/   # ожидаем 0 совпадений
sqlite3 data/besedka.db "SELECT version FROM refinery_schema_history"  # после live — 6
```
