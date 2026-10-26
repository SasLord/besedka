# HANDOFF — как запустить свежую сессию
_Вставь промпт ниже в НОВУЮ сессию Hermes (профиль default). Она прочитает файлы и продолжит — контекст
из прошлого чата НЕ нужен._

## Прогресс
- ✅ **Wave 0 (каркас)** — done 2026-10-26, коммит `5a10aa3`, pushed. Все гейты зелёные.
- ⬜ **Wave 1 (аутентификация + комнаты)** — дальше. Промпт ниже.

## Промпт для запуска (скопируй) — WAVE 1
```
Продолжай работу над проектом Besedka (конвейер разработки).

Сделай строго по порядку:
1. Прочитай /Users/madsas/Projects/besedka/.conveyor/CONVENTIONS.md — конвенции, стек, роли, правила.
2. Прочитай /Users/madsas/Projects/besedka/.conveyor/STATE.md — где мы и что дальше.
3. Прочитай /Users/madsas/Projects/besedka/.conveyor/PLAN.md и MILESTONES.md.
4. Выполни WAVE 1 (аутентификация + комнаты) по PLAN.md, срезы по одному:
   - 1.1 register/login/logout (argon2, tower-sessions) + роли/комнаты в besedka-core;
     GET /api/v1/me; логин/логаут меняют сессию. Локальная 27B.
   - 1.2 AD/Kerberos SSO — 🔴 СЛОЖНЫЙ: делегируй DeepSeek (delegation.model / подзадача),
     НЕ мучай локальную модель. Референс READ-ONLY: trackly crates trackly-infra/src/ad/
     (keytab.rs, sso.rs, mock.rs, real.rs, discovery.rs) + trackly-app/src/http/sso.rs.
     Мок-режим: BESEDKA_AD_MOCK=1 (паттерн trackly TRACKLY_AD_MOCK=1).
   - 1.3 UI: форма логина (пароль + «вход через AD»), список комнат в сайдбаре,
     join/leave, ролевые гейты (admin/user/guest). Локальная 27B.
5. Проверь гейты на КАЖДОМ срезе: cargo check + clippy (0 deny) + test + build зелёные;
   новые маршруты отвечают (register/login/me/rooms); UI рендерится;
   git commit (per срез) + push в SasLord/besedka.
6. Отправь отчёт в Telegram: hermes send -t telegram --subject "..." --file report.md
   (home-канал) + сводку в чат простым русским.
7. Обнови .conveyor/STATE.md (Wave 1 = done, дальше Wave 2) и назови следующий срез.

Правила:
- Trackly (/Users/madsas/Projects/trackly/) — READ-ONLY референс AD/Kerberos, НЕ менять.
- Модель — гибрид (B): простые срезы — локальная 27B; сложные (AD/Kerberos, WS, тяжёлый UI) —
  DeepSeek (delegation.model) или подзадача. Не мучай локальную модель на сложном.
- Стек, версии, роли — в CONVENTIONS.md. Никаких секретов в репо.
- Порты: API 8080, UI 5173 (vite strictPort). Если заняты — сначала убей старые процессы.
- Node-питфолл: в ui/node_modules нужны ОБА бинарника @rollup/rollup-darwin-{x64,arm64} и
  @esbuild/darwin-{x64,arm64} (два node: x64 v26 / arm64 v24).
```

## Что должно получиться после запуска (Wave 1)
- register/login/logout работают, сессия живёт в cookie `besedka_session`; `/api/v1/me` отдаёт
  текущего пользователя; комнаты: список + join/leave; UI: логин + комнаты в сайдбаре.
- AD/SSO: мок-режим работает (BESEDKA_AD_MOCK=1), real-код перенесён из trackly (READ-ONLY).
- Push в GitHub, отчёт в Telegram + чат, STATE.md на Wave 2.

## Эскалация (если срез слишком сложный для локальной 27B)
Subagent с `delegation.model`, закреплённой на DeepSeek, или спросить пользователя. Не мучай локальную
модель на сложном срезе.

## Telegram-отчёт
Конфиг: `TELEGRAM_BOT_TOKEN` в `~/.hermes/.env`, плагин `hermes-telegram`, home-канал. Короткий отчёт
на каждую волну: что собрано, гейты, ссылка на коммит/репо, что дальше. Проверенная команда:
`hermes send -t telegram --subject "..." --file report.md`.
