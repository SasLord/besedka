# HANDOFF — как запустить свежую сессию
_Вставь промпт ниже в НОВУЮ сессию Hermes (профиль default). Она прочитает файлы и продолжит — контекст
из прошлого чата НЕ нужен._

## Промпт для запуска (скопируй)
```
Продолжай работу над проектом Besedka (конвейер разработки).

Сделай строго по порядку:
1. Прочитай /Users/madsas/Projects/besedka/.conveyor/CONVENTIONS.md — конвенции, стек, роли, правила.
2. Прочитай /Users/madsas/Projects/besedka/.conveyor/STATE.md — где мы и что дальше.
3. Прочитай /Users/madsas/Projects/besedka/.conveyor/PLAN.md и MILESTONES.md.
4. Выполни верхнюю волну (сейчас WAVE 0) по PLAN.md:
   - Cargo-воркспейс из 3 кристов (besedka-core / besedka-infra / besedka-app),
   - besedka-app на axum: /api/v1/ping + конфиг из TOML + tracing + tower-sessions,
   - besedka-infra: SQLite (rusqlite bundled) + первая миграция refinery (таблица users),
   - ui/ на Svelte 5: дизайн-токены + layout-оболочка,
   - плюс 0.2: GET /api/v1/rooms (пустой список) + empty-state в UI.
5. Проверь гейты: cargo check + clippy + test + build зелёные; GET /api/v1/ping = 200; UI рендерится;
   git commit + push в SasLord/besedka.
6. Отправь отчёт в Telegram (home-канал, плагин hermes-telegram) + сводку в чат простым русским.
7. Обнови .conveyor/STATE.md (Wave 0 = done, дальше Wave 1) и назови следующий срез.

Правила:
- Trackly (/Users/madsas/Projects/trackly/) — READ-ONLY референс AD/Kerberos, НЕ менять.
- Модель — гибрид (B): простые срезы — локальная 27B; сложные (AD/Kerberos, WS, тяжёлый UI) — DeepSeek
  (delegation.model) или подзадача. Не мучай локальную модель на сложном.
- Стек, версии, роли — в CONVENTIONS.md. Никаких секретов в репо.
```

## Что должно получиться после запуска (Wave 0)
- Build зелёный (check/clippy/test/build), `GET /api/v1/ping` = 200, UI рендерит layout, `rooms` пустой
  список. Push в GitHub. Отчёт в Telegram + чат. STATE.md обновлён на Wave 1.

## Эскалация (если срез слишком сложный для локальной 27B)
Subagent с `delegation.model`, закреплённой на DeepSeek, или спросить пользователя. Не мучай локальную
модель на сложном срезе.

## Telegram-отчёт
Конфиг: `TELEGRAM_BOT_TOKEN` в `~/.hermes/.env`, плагин `hermes-telegram`, home-канал. Короткий отчёт
на каждую волну: что собрано, гейты, ссылка на коммит/репо, что дальше.
