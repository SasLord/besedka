# Вход через Active Directory (Kerberos SSO) — настройка

Два способа входа через AD:

| Способ | Кто использует | Что нужно серверу |
|---|---|---|
| **Kerberos / SPNEGO** (`GET /api/v1/auth_ad_sso`) | браузер или desktop-приложение на **доменной** машине. Пароль не вводится вообще | SPN + `.keytab` |
| **AD-пароль** (`POST /api/v1/auth_ad_login`) | личный ноутбук вне домена, веб-клиент | адрес DC (LDAP) |

Оба выдают **ту же** сессионную cookie (`besedka_session`), что и обычный парольный вход.

---

## 0. Быстрая проверка без домена (mock-режим)

Mock-режим — это полноценный путь входа, только вместо DC и Kerberos работают фикстуры.
Ни keytab, ни DC не нужны.

```bash
# в config.toml уже стоит: [ad] enabled = true, mock = true
cargo run
curl -i 'http://127.0.0.1:8080/api/v1/auth_ad_sso'            # 200 + Set-Cookie
curl -i 'http://127.0.0.1:8080/api/v1/auth_ad_sso?login=us300' # вошли как админ
curl -b 'besedka_session=<id>' http://127.0.0.1:8080/api/v1/me
```

Фикстуры (`crates/besedka-infra/src/ad/{mock,directory_mock}.rs`):

| логин | пароль (только для `/auth_ad_login`) | ФИО | роль |
|---|---|---|---|
| `us100` | `Passw0rd!` | Иванов Иван Иванович | manager |
| `us200` | `Secret123` | Петрова Анна Сергеевна | user |
| `us300` | — | Сидоров Пётр Петрович | admin |

Логин, начинающийся на `deny`, всегда получает 401 — чтобы можно было проверить отказ.

Включается либо `[ad] mock = true`, либо переменной окружения `BESEDKA_AD_MOCK=1`
(паттерн trackly `TRACKLY_AD_MOCK=1`). **В production mock обязан быть выключен.**

---

## 1. Production: Kerberos (вход без пароля)

### 1.1 SPN и keytab (на контроллере домена, Windows)

Сервисный аккаунт в AD, затем:

```powershell
ktpass -princ HTTP/chat.corp.example@CORP.EXAMPLE `
       -mapuser CORP\svc-besedka-sso `
       -crypto AES256-SHA1 `
       -ptype KRB5_NT_PRINCIPAL `
       -pass * `
       -out HTTP-chat.keytab
```

`HTTP/chat.corp.example` — это SPN: `HTTP/` + имя, **по которому пользователи открывают Besedka**
(ровно как в адресной строке). Сменили адрес — перегенерируйте keytab.

`*.keytab` в `.gitignore` — файл кладётся рядом с сервером, в репозиторий не попадает.

### 1.2 Конфиг

```toml
[ad]
enabled = true
mock = false
domain = "corp.example"
spn = "HTTP/chat.corp.example"
keytab_path = "secrets/HTTP-chat.keytab"
host = "dc1.corp.example"          # или пусто → автоопределение по DNS SRV
base_dn = "dc=corp,dc=example"
```

### 1.3 Клиенты

- **Windows, доменная машина:** Chrome/Edge/IE отправляют Kerberos-билет на сервер из
  зоны Local intranet. Если браузер не отвечает на `401 Negotiate`, добавьте адрес в
  *Internet Options → Security → Local intranet → Sites → Advanced*, либо в политику
  `AuthServerAllowlist` для Chrome.
- **Firefox:** `network.negotiate-auth.trusted-uris = chat.corp.example`.
- **curl (проверка):** `curl --negotiate -u : -i http://chat.corp.example:8080/api/v1/auth_ad_sso`

---

## 2. Production: AD-пароль (LDAP bind)

Нужен только доступ по LDAP(S) к DC — Kerberos не участвует.

```toml
[ad]
enabled = true
mock = false
domain = "corp.example"
host = "dc1.corp.example"
ldap_tls_mode = "ldaps"   # ldaps | starttls | plain (plain — пароли открытым текстом!)
```

Пароль сервисного аккаунта (для обогащения ФИО/роли) — **только через окружение**:

```bash
export BESEDKA_AD_BIND_PASSWORD='...'
```

---

## 3. Роли из групп AD

```toml
[[ad.role_mapping]]
group_dn = "CN=Besedka-Admins,OU=Groups,DC=corp,DC=example"
role = "admin"

[[ad.role_mapping]]
group_dn = "CN=Besedka-Managers,OU=Groups,DC=corp,DC=example"
role = "manager"
```

- Вложенные группы учитываются (`LDAP_MATCHING_RULE_IN_CHAIN`).
- Побеждает **старшая** роль: admin > manager > user, порядок строк не важен.
- Роль назначается **при первом входе**. Дальше её меняет админ — вход из AD больше роль
  не перезаписывает (иначе понижение в AD незаметно откатывалось бы при каждом логине).
- ФИО обновляется при каждом входе, но **только** из реального ответа каталога: если DC
  недоступен, сохранённое ФИО не затирается логином.

Разрешения:

| роль | что может |
|---|---|
| `user` | чат, комнаты |
| `manager` | + управление комнатами и каналами (Wave 1.3) |
| `admin` | + `GET /api/v1/admin/ad_status`, управление пользователями |

---

## 4. Проверка подключения (админ)

```bash
curl -b 'besedka_session=<admin-id>' http://127.0.0.1:8080/api/v1/admin/ad_status
```

```json
{
  "enabled": true, "mock": false, "sso": true, "password_login": true,
  "domain": "corp.example",
  "spn_configured": true, "keytab_configured": true, "directory_configured": true,
  "resolved_host": "dc1.corp.example",
  "reachable": "ok"
}
```

`reachable`: `ok` — DC отвечает; `unreachable` — сеть/TLS; `unknown` — странный ответ.
Пароль сервисного аккаунта в ответе не появляется никогда.

Публичный (без сессии) статус для UI: `GET /api/v1/auth_ad_sso/status` →
`{"enabled":…, "mock":…, "sso":…, "password_login":…}`.

---

## 5. Частые ошибки

| Симптом | Причина |
|---|---|
| `503 AD login is disabled` | `[ad] enabled = false` |
| `503 Kerberos SSO is not configured` | пустые `spn` или `keytab_path` (и mock выключен) |
| `503 keytab is not readable` | файл не найден/нет прав чтения |
| `401` после билета | SPN не совпадает с тем, под который сделан keytab, либо адрес в браузере другой |
| `401` на `/auth_ad_login` при верном пароле | логин без домена при пустом `domain`; проверьте `host`/`ldap_tls_mode` |
| `503 AD is unreachable` | нет сети до DC, неверный порт, TLS-сертификат (для лаборатории: `no_tls_verify = true`) |
| ФИО = логин | `bind_dn` пуст (обогащение не настроено) или пользователь не найден в `base_dn` |
