# Besedka — Guide for AI agents (конвенции + мозг проекта)

**What:** Besedka (беседа = «разговор») — self-hosted корпоративный чат: сервер + desktop-клиент
(+ веб). Киллер-фича: **вход по Active Directory через Kerberos (SPNEGO/Negotiate)** — пользователь
логинится доменным аккаунтом без пароля, роли/комнаты назначаются из AD.

**Status:** greenfield. Репозиторий создан (public, `SasLord/besedka`). Первый срез (Wave 0) ещё не
написан. Где мы — `STATE.md`.

## Non-negotiable rules
1. **`/Users/madsas/Projects/trackly/` — READ-ONLY референс.** Это *другое, рабочее* приложение
   (мониторинг сети), но там **проверенная реализация AD/Kerberos на Rust**, которую мы переносим.
   Никогда не редактировать, коммитить или удалять что-либо в trackly. Только копировать/адаптировать
   паттерны в besedka.
2. **Никаких секретов в репозитории.** `.env`, keytab, сертификаты — в gitignore. Конфиг через TOML +
   env-переменные.
3. **Гейты верификации обязательны** до того, как срез «сделан»: `cargo check` + `cargo clippy`
   (0 deny) + `cargo test` + `cargo build` зелёные; UI рендерится (скриншот); `git commit` + `git push`
   в `SasLord/besedka`. Нет гейта — не сделано.
4. **Состояние живёт в файлах.** `STATE.md` — source of truth «где мы». Обновлять после каждой волны.
5. **Простой русский, «на пальцах»** в отчётах пользователю (новичок в мехатронике/3D-печати, программирует
   «нормально»). Новые термины объяснять. Без воды.

## Architecture (all Rust)
```
besedka/
├── crates/
│   ├── besedka-core/    # домен: комнаты, каналы, роли, голосования, сообщения (без I/O)
│   ├── besedka-infra/   # SQLite+refinery, AD/Kerberos (из trackly), WS, файлы
│   └── besedka-app/     # axum HTTP/WS, SSO-эндпоинт, Tauri-команды, wiring
├── ui/                  # Svelte 5 (TipTap rich text, дизайн-токены, i18n ru/en)
├── .conveyor/           # STATE, PLAN, MILESTONES, HANDOFF, CONVENTIONS (конвейер)
└── README.md
```
Зеркали 3-кристовый сплит trackly (core=домен, infra=адаптеры, app=HTTP/Tauri).

## Tech stack (пinned — копируй из trackly)
- Rust 2021 (workspace), `rust-version = "1.92"` (на машине 1.95 — ок)
- HTTP: `axum 0.8` (http1, json, ws) + `tower 0.5` + `tower-http 0.6`
- БД: `rusqlite 0.38` (bundled) + `refinery 0.9` (миграции)
- AD/Kerberos: `sspi 0.21` (SPNEGO server-accept) + `ldap3 0.12` (tls-rustls-ring) + `hickory-resolver 0.26` (DNS SRV)
- Аутентификация: `argon2 0.5` (пароли) + `tower-sessions 0.15` (сессии)
- TLS: `tokio-rustls 0.26` + `rcgen 0.14` (self-signed) + `rustls-pemfile`
- Desktop: `tauri 2` + `tauri-specta` + `specta` (авто Rust↔TS типы)
- UI: Svelte 5 + TipTap (rich text) + i18n (ru/en)
- Lints: `clippy disallowed_methods/types = deny`

## Референс AD/Kerberos (trackly — читать эти, переносить паттерн)
| Файл (trackly) | Что делает |
|---|---|
| `crates/trackly-infra/src/ad/keytab.rs` | Чисто-Rust парсер MIT keytab (юнит-тестится, macOS-safe). Достаёт AES256-ключ по SPN |
| `crates/trackly-infra/src/ad/sso.rs` | `accept_spnego(spn, keytab, computer, token)` → AD-юзернейм. `sspi` offline-принятие |
| `crates/trackly-infra/src/ad/mock.rs` | **МОК AD** — локальные тестовые юзеры (`us100`/`Passw0rd!`). Вкл. `TRACKLY_AD_MOCK=1` → **разрабатывать/тестить весь путь входа без реального DC** |
| `crates/trackly-infra/src/ad/real.rs` | Реальный AD (ldap3, LDAPS) |
| `crates/trackly-infra/src/ad/discovery.rs` | Автоопределение: DNS SRV + Windows env |
| `crates/trackly-app/src/http/sso.rs` | `GET /api/v1/auth_ad_sso`: 401→`Negotiate`→билет→валидация→сессия |
| `crates/trackly-core/src/ports/ad.rs` | Доменный трейт AD (портируй в besedka-core) |
| `docs/AD-SETUP.md` | Дока для админа + МОК + паттерн «Проверить подключение» |
| `crates/trackly-infra/Cargo.toml` | Точные версии зависимостей (sspi/ldap3/hickory) |
| `CLAUDE.md` | Конвенции trackly (образец) |

**Дери́ск:** самая сложная часть (Kerberos) уже решена на Rust и **мокается локально**. Переноси, а не
изобретай заново.

## Роли (одна фокус-направленность на срез)
- **core** — доменная логика в `besedka-core` (без I/O, юнит-тестится).
- **infra** — адаптеры: БД, AD/Kerberos, WS, файлы (перенос из trackly).
- **app** — HTTP/WS-эндпоинты, Tauri-команды, wiring.
- **ui** — Svelte (только через сгенерированные `specta`-типы).
Срез называет ОДНУ роль; правила этой роли доминируют.

## Модель (гибрид B)
- **Оркестратор** = main-сессия (локальная 27B, `qwen3.8-27b`) — планирует, ревьюит, мержит, репортит.
- **Простые/средние срезы** → локальная 27B (оркестратор или subagent).
- **Сложные срезы** (перенос AD/Kerberos, WS-мост, тяжёлый UI) → **DeepSeek** (пинь `delegation.model`
  на DeepSeek, или subagent с этой моделью). Если локальная не тянет — эскалируй, не мучай её.

## Workflow (конвейер)
1. Прочитай `STATE.md` → что дальше.
2. Возьми верхний срез из `PLAN.md` (одна роль).
3. Реализуй → проверь по **acceptance criteria** + **гейтам** выше.
4. `git commit` (conventional message) + `git push`.
5. Обнови `STATE.md` (done + next).
6. **Отчёт:** сводка пользователю (простой русский) + Telegram (home-канал, плагин `hermes-telegram`,
   `TELEGRAM_BOT_TOKEN`).
7. Следующая волна.

## Гейты верификации (на каждый срез)
- `cargo check` / `cargo clippy` (0 deny) / `cargo test` / `cargo build` — всё зелёное.
- UI-срез: приложение запускается, скриншот сделан, layout корректен.
- `git status` чистый, запушено в `SasLord/besedka`.
- Отчёт доставлен (чат + Telegram).
