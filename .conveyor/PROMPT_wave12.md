Ты работаешь в проекте **Besedka** (Rust, Axum, SQLite, AD-SSO). Это конвейер
разработки: строго по срезам, гейты обязательны, в конце отчёт в Telegram.
Стек и правила — только в .conveyor/CONVENTIONS.md (NEVER доверять памяти).

=== ЗАДАЧА: Wave 1.2 — AD/Kerberos SSO ===
Перенос ГОТОВОГО SSO-кода из trackly в besedka.
trackly = READ-ONLY: НИ ОДНОГО изменения в ~/Projects/trackly/ (только чтение).

ШАГИ (строго по порядку):

1. Контекст (прочитать всё, прежде чем писать код):
   - ~/Projects/besedka/.conveyor/CONVENTIONS.md   (конвенции, стек, гейты, правило гибрид-B)
   - ~/Projects/besedka/.conveyor/STATE.md          (сейчас: 1.1 DONE, 1.2 — дальше)
   - ~/Projects/besedka/.conveyor/PLAN.md           (секция Wave 1.2)

2. SSO-исходник в trackly (только чтение):
   - ~/Projects/trackly/crates/trackly-infra/src/ad/keytab.rs
   - ~/Projects/trackly/crates/trackly-infra/src/ad/sso.rs
   - ~/Projects/trackly/crates/trackly-infra/src/ad/mock.rs
   - ~/Projects/trackly/crates/trackly-infra/src/ad/{mod,real,directory,discovery,transport,cache}.rs
   - ~/Projects/trackly/crates/trackly-app/src/http/sso.rs
   - тест-референс: ~/Projects/trackly/crates/trackly-app/tests/ad_directory_sso.rs

3. Реализация в besedka (по PLAN.md §Wave 1.2):
   - адаптировать под стек besedka: Axum + tower-sessions + аутентификация 1.1 (argon2)
   - SSO-эндпоинты (login через AD/Kerberos, mock-режим для локал. разработки без AD-сервера)
   - ролевые гейты user/admin
   - НЕ копировать слепо: следуй конвенциям besedka, а не trackly

4. ГЕЙТЫ (все обязательны, ДО commit):
   - cargo build              → OK
   - cargo test               → зелёные (включая новые SSO-тесты)
   - cargo clippy --all-targets → 0 deny
   (cargo clippy -D warnings, как в CONVENTIONS.md)

5. commit: "feat(sso): Wave 1.2 — AD/Kerberos SSO (port from trackly, READ-ONLY)"
   push:  git push origin main

6. STATE.md: отметить 1.2 DONE, зафиксировать что сделано (эндпоинты, mock, гейты).

7. Отчёт в Telegram:
   hermes send -t telegram --subject "Besedka Wave 1.2 ✅" --file <путь_к_отчёту>

ОБЯЗАТЕЛЬНЫЕ ПРАВИЛА:
- trackly — READ-ONLY (никаких изменений в ~/Projects/trackly/).
- follow CONVENTIONS.md besedka, не trackly.
- mock-режим обязателен (локальная разработка без AD-сервера).
- Если застрял на гейтах — не коммить, довести до зелёных.

Начни с шага 1.
