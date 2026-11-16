Ты работаешь в проекте **Besedka** (Rust/Axum бэкенд + Svelte 5 UI). Конвейер: гейты обязательны, в конце отчёт в Telegram.
Если доступен скилл `besedka` — загрузи его (workflow + гейты + питфоллы). Иначе — ТОЛЬКО `.conveyor/CONVENTIONS.md` (NEVER доверять памяти).

=== ЗАДАЧА: Закрыть Wave 3.1 (файлы: upload/download) — ЧАСТЬ 2/2: UI + живое превью + конвейерное закрытие ===
**Предыстория:** верификация 3.1 была **FAIL** (отчёт: `.conveyor/reports/wave31-verify-report.md`). **ЧАСТЬ 1/2 (`.conveyor/PROMPT_wave31-fix-1.md`) уже выполнена**: бэкенд зелёный — HTTP-слой `http/files.rs` (POST/GET `/api/v1/files`), конфиг `[files] dir + max_size_bytes` + env-оверрайды, wire snake_case, cleanup WIP, curl-проверки пройдены, коммит на main. Твоя часть — всё остальное, чтобы волна стала DONE.

**Пресуппозиция (проверь первым делом, 1 команда):** `git log --oneline -3` — коммит part 1 есть; `cargo test --workspace 2>&1 | tail -5` — зелёный. Если part 1 НЕ сделана — **СТОП**, доложи, не продолжай.

## ЧТО ДЕЛАТЬ (всё — UI + живое превью + бумаги):

### 1. UI: `ui/src/lib/api.ts` (единственный UI-файл; RoomChat/App НЕ трогать — превью в чате это Wave 3.2)
Добавить (snake_case, как на бэкенде):
```ts
export type File = {
  id: string;
  uploader_id: string;
  filename: string;
  content_type: string;
  size_bytes: number;
  created_at: string;
  url: string;
};
```
- `uploadFile(file: File, onProgress?: (loaded: number) => void): Promise<File>` — multipart-POST на `/api/v1/files`, поле `file`; прогресс обязателен (XHR `upload.onprogress` или fetch-стрим);
- `fileUrl(f: File): string` → `/api/v1/files/{id}` (или `f.url` — выбрать одно и зафиксировать);
- `npm run check` = 0 ошибок · `npm run build` OK.
- **PITFALL:** НЕ чистить `ui/node_modules`, НЕ переустанавливать зависимости (два node: hermes x64 / nvm arm64 — нужны ОБА бинарника @rollup/@esbuild darwin-x64+arm64).

### 2. Живая проверка (обязательна)
- `cargo run -p besedka-app` (dev-БД); curl-создать тестовый PNG → **201** → открыть url **в браузере** → картинка отображается (превью);
- `sqlite3 data/besedka.db "SELECT version FROM refinery_schema_history"` → на последнем месте **6**;
- скриншот превью → **`.conveyor/screenshots/31-file-preview.png`**.

### 3. STATE.md
- 3.1 → **DONE**: эндпоинты (`POST/GET /api/v1/files`), V6 `files`, конфиг `[files] dir+max_size_bytes` + `BESEDKA_FILES_DIR`/`BESEDKA_FILES_MAX_SIZE_BYTES`, гейты (part 1), живая проверка (part 2), коммиты (оба), UI: api.ts `File/uploadFile/fileUrl`; секция «Решения/отклонения»: **FK на `files.uploader_id` — намеренно без `REFERENCES users ON DELETE CASCADE`** (каскад удалил бы метаданные, оставив сиротские байты; байты владеет app-слой) — осознанный дизайн, не баг;
- «Дальше»: WAVE 3: 3.1 ✅ · 3.2 → UI-превью (картинка инлайн, PDF-ссылка);
- commit `docs(state): Wave 3.1 DONE (fix parts 1+2) — file upload/download` → push.

### 4. Отчёт + Telegram
- `.conveyor/reports/wave31-report.md` — формат как `wave23-report.md` (сделано / отклонения / гейты / живая проверка / что дальше). Обязательные пункты: обе части фикса (part 1 = код, part 2 = UI+живая); **секция «Решения/отклонения»**: FK без каскада (см. выше); «Wave 3.4»-комментарии и WIP `ws/`/`.conveyor/patches/` — удалено (мусор вне среза);
- `hermes send -t telegram --subject "Besedka Wave 3.1 ✅ (fix: файлы upload/download)" --file .conveyor/reports/wave31-report.md`
  (таймаут доставки — повторить 1 раз; всё равно не ушло → зафиксировать в отчёте, это НЕ FAIL).

### 5. PROMPT_wave32.md (ОБЯЗАТЕЛЬНО)
Написать `.conveyor/PROMPT_wave32.md` — **Wave 3.2 [ui]: Превью файлов в чате (image/PDF)** (Accept из PLAN.md 3.2: картинка инлайн, PDF-ссылка): привязка file↔message (колонка/таблица — решить по PLAN.md), кнопка «📎» в RoomChat, drag&drop (бонус), инлайн-картинка (клик → полный размер), PDF — ссылка; опираться на готовый API 3.1 (POST/GET `/api/v1/files`, `uploadFile`/`fileUrl` из api.ts). Формат — как `PROMPT_wave31.md`/`PROMPT_wave23.md` (сделано, шаги, гейты, commit, STATE, Telegram, шаг «написать PROMPT_wave41.md» — 4.1 поиск FTS5).

### 6. Финальные коммиты
- отчёт + скриншот + PROMPT_wave32: `docs(conveyor): Wave 3.1 report + screenshots + PROMPT_wave32 (3.2 UI preview)` → `git push origin main`.

### 7. Перед завершением
- **остановить все фоновые процессы** (API-сервер, vite, cargo-джобы) — не оставлять висящие;
- финальный отчёт в чате: UI-гейты, живое превью (скриншот), STATE, Telegram (status), коммиты.

Начни с проверки пресуппозиции, потом шага 1.
