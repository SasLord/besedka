# STATE — Besedka
_Обновлено: 2026-10-26 · Source of truth «где мы»._

## Решения (зафиксированы)
- **Язык:** всё на Rust (сервер + Tauri-бэкенд + биндинги). ~~Go~~ — отброшено (trackly на Rust,
  AD-код переиспользуется).
- **Модель:** гибрид **B** — локальная 27B оркестратор + DeepSeek для сложных срезов.
- **Репозиторий:** public, `github.com/SasLord/besedka` (CI/сборки релизов Win/Linux/macOS живут здесь).
- **Отчёты:** пользователю (чат) + **Telegram** (home-канал, плагин `hermes-telegram`, `TELEGRAM_BOT_TOKEN`).
- **Референс AD/Kerberos:** `/Users/madsas/Projects/trackly/` (READ-ONLY, не менять).
- **Тестирование AD:** мок-AD trackly (паттерн `TRACKLY_AD_MOCK=1`) — разработка/тест без реального DC.

## Статус
| Пункт | Состояние |
|---|---|
| GitHub-репо `besedka` (public) | ✅ создано |
| Файлы конвейера (CONVENTIONS/STATE/PLAN/MILESTONES/HANDOFF) | ✅ созданы |
| Скилл `besedka` (Hermes) | ✅ создан |
| **Wave 0 (каркас)** | ⬜ **ДАЛЬШЕ — не начато** |
| Код | ⬜ ещё нет |

## Дальше
**WAVE 0 — каркас.** Спецификация + acceptance criteria — в `HANDOFF.md` и `PLAN.md`.

## Указатели контекста
- Конвенции/стек/роли: `CONVENTIONS.md`
- Срезы: `PLAN.md` · Майлстоны: `MILESTONES.md` · Запуск: `HANDOFF.md`
- Требования: `../CORP_CHAT_PLAN.md`
- AD/Kerberos-код для переноса: `trackly/crates/trackly-infra/src/ad/{sso,keytab,mock,real,discovery}.rs`
  + `trackly/crates/trackly-app/src/http/sso.rs`
