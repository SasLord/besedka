# Besedka UI (Svelte 5 + Vite)

Wave 0: дизайн-токены (`src/app.css`), layout-оболочка (сайдбар + главная
панель), список комнат из `GET /api/v1/rooms`, empty-state и индикатор
доступности сервера (`GET /api/v1/ping`).

```sh
npm install
npm run dev     # http://localhost:5173 (прокси /api → 127.0.0.1:8080)
npm run build   # production-сборка в ui/build/
```
