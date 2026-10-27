<script lang="ts">
  import {
    login,
    register,
    ssoLogin,
    ApiError,
    type SsoStatus,
    type User,
  } from "../api";

  let {
    sso,
    onAuthed,
  }: {
    sso: SsoStatus | null;
    onAuthed: (user: User) => void;
  } = $props();

  let mode = $state<"login" | "register">("login");
  let username = $state("");
  let password = $state("");
  let error = $state<string | null>(null);
  let busy = $state(false);
  let mockLogin = $state("us100");

  function switchMode(next: "login" | "register") {
    mode = next;
    error = null;
    password = "";
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (busy) return;
    error = null;
    busy = true;
    try {
      const user =
        mode === "login"
          ? await login(username.trim(), password)
          : await register(username.trim(), password);
      onAuthed(user);
    } catch (e) {
      error = e instanceof ApiError ? e.message : "Не удалось выполнить вход";
    } finally {
      busy = false;
    }
  }

  async function ssoSignIn() {
    if (busy) return;
    error = null;
    busy = true;
    try {
      const login = sso?.mock === true ? mockLogin : undefined;
      onAuthed(await ssoLogin(login));
    } catch (e) {
      error = e instanceof ApiError ? e.message : "Не удалось выполнить вход через AD";
    } finally {
      busy = false;
    }
  }

  const adAvailable = $derived(sso?.enabled === true && sso?.sso === true);
</script>

<div class="auth" role="main" aria-label="Вход">
  <div class="card">
    <div class="brand">Besedka</div>
    <p class="tagline">Корпоративный мессенджер</p>

    {#if adAvailable}
      <button
        class="ad-button"
        type="button"
        disabled={busy}
        onclick={ssoSignIn}
      >
        Войти через AD
        {#if sso?.mock === true}<span class="ad-button__mock">(mock)</span>{/if}
      </button>
      {#if sso?.mock === true}
        <label class="field ad-mock">
          <span class="field__label">Mock-пользователь (демо)</span>
          <select class="field__input" bind:value={mockLogin}>
            <option value="us100">us100 — Иванов (manager)</option>
            <option value="us200">us200 — Петрова (user)</option>
            <option value="us300">us300 — Сидоров (admin)</option>
          </select>
        </label>
      {/if}
      <div class="divider" role="separator" aria-label="или">
        <span>или по паролю</span>
      </div>
    {/if}

    <form class="form" onsubmit={submit} novalidate>
      <label class="field">
        <span class="field__label">Логин</span>
        <input
          class="field__input"
          name="username"
          type="text"
          autocomplete="username"
          bind:value={username}
          required
          minlength="3"
        />
      </label>

      <label class="field">
        <span class="field__label">Пароль</span>
        <input
          class="field__input"
          name="password"
          type="password"
          autocomplete={mode === "login" ? "current-password" : "new-password"}
          bind:value={password}
          required
          minlength="8"
        />
      </label>

      {#if error}
        <p class="form__error" role="alert">{error}</p>
      {/if}

      <button class="primary" type="submit" disabled={busy}>
        {busy ? "Подождите…" : mode === "login" ? "Войти" : "Создать аккаунт"}
      </button>
    </form>

    <button
      class="link"
      type="button"
      disabled={busy}
      onclick={() => switchMode(mode === "login" ? "register" : "login")}
    >
      {mode === "login" ? "Нет аккаунта? Зарегистрироваться" : "Уже есть аккаунт? Войти"}
    </button>
  </div>
</div>

<style>
  .auth {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: var(--space-6);
  }

  .card {
    width: 100%;
    max-width: 360px;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-6);
    background: var(--color-bg-panel);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-sm);
  }

  .brand {
    font-size: 22px;
    font-weight: 700;
  }

  .tagline {
    margin: -8px 0 4px;
    color: var(--color-text-muted);
    font-size: var(--text-small);
  }

  .ad-button {
    padding: 10px 14px;
    font-weight: 600;
    color: var(--color-bg-panel);
    background: var(--color-accent);
    border: 0;
    border-radius: var(--radius-md);
    cursor: pointer;
  }

  .ad-button:hover {
    filter: brightness(1.08);
  }

  .ad-button:disabled {
    opacity: 0.6;
    cursor: default;
  }

  .ad-button__mock {
    font-weight: 400;
    opacity: 0.8;
  }

  .divider {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--color-text-muted);
    font-size: 12px;
  }

  .divider::before,
  .divider::after {
    content: "";
    flex: 1;
    height: 1px;
    background: var(--color-border);
  }

  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .field__label {
    font-size: var(--text-small);
    color: var(--color-text-muted);
  }

  .field__input {
    padding: 9px 12px;
    font: inherit;
    color: var(--color-text);
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
  }

  .field__input:focus {
    outline: 2px solid var(--color-accent);
    outline-offset: -1px;
  }

  .form__error {
    margin: 0;
    color: var(--color-danger);
    font-size: var(--text-small);
  }

  .primary {
    padding: 10px 14px;
    font-weight: 600;
    color: var(--color-bg-panel);
    background: var(--color-accent);
    border: 0;
    border-radius: var(--radius-md);
    cursor: pointer;
  }

  .primary:disabled {
    opacity: 0.6;
    cursor: default;
  }

  .link {
    padding: 4px;
    color: var(--color-accent);
    background: none;
    border: 0;
    cursor: pointer;
    font-size: var(--text-small);
  }
</style>
