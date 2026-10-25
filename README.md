# Besedka

**Беседа** — self-hosted корпоративный чат: сервер + desktop-клиент + (опционально) веб.
Фирменная фича: **вход по Active Directory через SAML/Kerberos (SPNEGO)** — пользователь
логинится доменным аккаунтом без пароля, роли и комнаты назначаются из AD.

- **Стек:** Rust (сервер: `axum` + `rusqlite`/`refinery` + `sspi`/`ldap3` для AD) +
  `Tauri 2` + `Svelte 5` (UI). Типы Rust↔TS генерируются автоматически (`specta`).
- **Архитектура:** 3 криста — `besedka-core` (домен) / `besedka-infra` (адаптеры, AD/Kerberos)
  / `besedka-app` (HTTP/WS, Tauri-команды) + `ui/` (Svelte).
- **Статус:** greenfield. Каркас (Wave 0) ещё не написан.

## Для людей
Прочитай [`.conveyor/CONVENTIONS.md`](.conveyor/CONVENTIONS.md) — архитектура, стек, правила, референс
AD/Kerberos (trackly). План срезов: [`.conveyor/PLAN.md`](.conveyor/PLAN.md). Где мы:
[`.conveyor/STATE.md`](.conveyor/STATE.md).

## Для ИИ-агентов
**Стартовая точка — `.conveyor/CONVENTIONS.md`.** Там правила, стек, роли, гейты верификации и ссылка на
read-only референс AD/Kerberos (`/Users/madsas/Projects/trackly/`, **не менять**).
