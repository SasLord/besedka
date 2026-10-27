<script lang="ts">
  import { onMount } from "svelte";
  import EmptyState from "./lib/components/EmptyState.svelte";
  import Login from "./lib/components/Login.svelte";
  import {
    ApiError,
    fetchPing,
    fetchRooms,
    joinRoom,
    leaveRoom,
    logout,
    me,
    ssoStatus,
    type Room,
    type SsoStatus,
    type User,
  } from "./lib/api";

  let user = $state<User | null>(null);
  let authChecked = $state(false);
  let rooms = $state<Room[]>([]);
  let sso = $state<SsoStatus | null>(null);
  let error = $state<string | null>(null);
  let busyRoom = $state<number | null>(null);
  let serverOk = $state(false);

  function toMessage(e: unknown): string {
    return e instanceof ApiError
      ? e.message
      : e instanceof Error
        ? e.message
        : String(e);
  }

  async function refreshRooms() {
    rooms = await fetchRooms();
  }

  onMount(async () => {
    // Server liveness (footer dot) and the SSO feature probe are best-effort.
    try {
      const ping = await fetchPing();
      serverOk = ping.status === "ok";
    } catch {
      serverOk = false;
    }
    try {
      sso = await ssoStatus();
    } catch {
      sso = null;
    }
    // Who are we? 401 => anonymous (login screen), anything else is an error.
    try {
      user = await me();
    } catch (e) {
      if (!(e instanceof ApiError && e.status === 401)) {
        error = toMessage(e);
      }
      user = null;
    }
    try {
      await refreshRooms();
    } catch (e) {
      error = toMessage(e);
    } finally {
      authChecked = true;
    }
  });

  async function handleAuthed(authedUser: User) {
    user = authedUser;
    error = null;
    try {
      await refreshRooms();
    } catch (e) {
      error = toMessage(e);
    }
  }

  async function handleLogout() {
    try {
      await logout();
    } catch {
      /* the local state resets anyway */
    }
    user = null;
    try {
      await refreshRooms();
    } catch (e) {
      error = toMessage(e);
    }
  }

  async function toggleMembership(room: Room) {
    if (busyRoom !== null) return;
    busyRoom = room.id;
    error = null;
    try {
      if (room.joined) {
        await leaveRoom(room.id);
      } else {
        await joinRoom(room.id);
      }
      await refreshRooms();
    } catch (e) {
      error = toMessage(e);
    } finally {
      busyRoom = null;
    }
  }

  const isAdmin = $derived(user?.role === "admin");
  const firstJoined = $derived(rooms.find((r) => r.joined) ?? null);
  const displayName = (u: User) => u.display_name?.trim() || u.username;
</script>

<div class="app">
  {#if !authChecked}
    <div class="boot">
      <EmptyState icon="⏳" title="Загрузка…" />
    </div>
  {:else if !user}
    <Login sso={sso} onAuthed={handleAuthed} />
  {:else}
    <aside class="sidebar">
      <header class="sidebar__header">
        <span class="brand">Besedka</span>
        <span class="role-badge" class:admin={isAdmin} title="Роль">{user.role}</span>
      </header>

      <nav class="rooms" aria-label="Комнаты">
        {#each rooms as room (room.id)}
          <div class="room" class:joined={room.joined}>
            <div class="room__info">
              <span class="room__title">{room.title}</span>
              <span class="room__meta">
                <span class="room__kind">{room.kind}</span>
                {#if room.joined}<span class="you-here">вы здесь</span>{/if}
              </span>
            </div>
            <span class="room__count" title="Участников">{room.member_count}</span>
            <button
              class="room__btn"
              type="button"
              disabled={busyRoom === room.id}
              onclick={() => toggleMembership(room)}
            >
              {busyRoom === room.id ? "…" : room.joined ? "Выйти" : "Войти"}
            </button>
          </div>
        {:else}
          <p class="rooms__empty">Комнат пока нет</p>
        {/each}
      </nav>

      {#if isAdmin}
        <!-- Role gate: visible to admins only (Wave 1.3) -->
        <section class="admin" aria-label="Админ-панель">
          <h2 class="admin__title">Админ</h2>
          <p class="admin__hint">Управление комнатами и AD-диагностика — со следующей волны.</p>
        </section>
      {/if}

      <footer class="sidebar__footer">
        <span class="dot" class:ok={serverOk} aria-hidden="true"></span>
        <span class="footer-user">{displayName(user)}</span>
        <button class="link" type="button" onclick={handleLogout}>Выйти</button>
      </footer>
    </aside>

    <main class="main">
      {#if error}
        <EmptyState icon="⚠️" title="Ошибка" text={error} />
      {:else if firstJoined}
        <div class="chat">
          <header class="chat__header">
            <span class="chat__title">{firstJoined.title}</span>
            <span class="chat__status">
              вы здесь · {firstJoined.member_count}
              {firstJoined.member_count % 10 === 1 && firstJoined.member_count % 100 !== 11
                ? "участник"
                : [2, 3, 4].includes(firstJoined.member_count % 10) &&
                    ![12, 13, 14].includes(firstJoined.member_count % 100)
                  ? "участника"
                  : "участников"}
            </span>
          </header>
          <div class="chat__body">
            <EmptyState
              icon="💬"
              title="Сообщения"
              text="Переписка появится во второй волне."
            />
          </div>
        </div>
      {:else if isAdmin}
        <EmptyState
          icon="🛡️"
          title="Вы не в комнатах"
          text="Заходите в комнату кнопкой «Войти» в левой панели."
        />
      {:else}
        <EmptyState
          icon="💬"
          title="Вы не в комнатах"
          text="Заходите в комнату кнопкой «Войти» в левой панели."
        />
      {/if}
    </main>
  {/if}
</div>

<style>
  .app {
    display: flex;
    height: 100%;
  }

  .boot {
    flex: 1;
    min-width: 0;
    display: flex;
  }

  .sidebar {
    display: flex;
    flex-direction: column;
    width: 300px;
    min-width: 240px;
    background: var(--color-bg-sidebar);
    border-right: 1px solid var(--color-border);
  }

  .sidebar__header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-4);
  }

  .brand {
    font-size: var(--text-large);
    font-weight: 700;
    letter-spacing: 0.2px;
  }

  .role-badge {
    padding: 2px 8px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.4px;
    color: var(--color-text-muted);
    background: var(--color-bg-panel);
    border: 1px solid var(--color-border);
    border-radius: 999px;
  }

  .role-badge.admin {
    color: var(--color-accent);
    border-color: var(--color-accent);
    background: var(--color-accent-soft);
  }

  .rooms {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-2) var(--space-3);
    overflow-y: auto;
  }

  .rooms__empty {
    margin: var(--space-2);
    color: var(--color-text-muted);
    font-size: var(--text-small);
  }

  .room {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-md);
  }

  .room:hover {
    background: var(--color-accent-soft);
  }

  .room.joined {
    background: var(--color-accent-soft);
  }

  .room__info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .room__title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .room__meta {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .room__kind {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.4px;
    color: var(--color-text-muted);
  }

  .you-here {
    font-size: 11px;
    font-weight: 600;
    color: var(--color-accent);
  }

  .room__count {
    flex-shrink: 0;
    min-width: 22px;
    padding: 1px 7px;
    text-align: center;
    font-size: 12px;
    color: var(--color-text-muted);
    background: var(--color-bg-panel);
    border: 1px solid var(--color-border);
    border-radius: 999px;
  }

  .room__btn {
    flex-shrink: 0;
    padding: 4px 10px;
    font-size: 12px;
    font-weight: 600;
    color: var(--color-accent);
    background: transparent;
    border: 1px solid var(--color-accent);
    border-radius: var(--radius-sm);
    cursor: pointer;
  }

  .room__btn:hover {
    background: var(--color-accent);
    color: var(--color-bg-panel);
  }

  .room__btn:disabled {
    opacity: 0.6;
    cursor: default;
  }

  .admin {
    padding: var(--space-3) var(--space-4);
    border-top: 1px solid var(--color-border);
  }

  .admin__title {
    margin: 0 0 4px;
    font-size: var(--text-small);
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.4px;
    color: var(--color-accent);
  }

  .admin__hint {
    margin: 0;
    font-size: var(--text-small);
    color: var(--color-text-muted);
  }

  .sidebar__footer {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
    border-top: 1px solid var(--color-border);
    color: var(--color-text-muted);
    font-size: var(--text-small);
  }

  .footer-user {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-text);
  }

  .dot {
    width: 8px;
    height: 8px;
    flex-shrink: 0;
    border-radius: 50%;
    background: var(--color-danger);
  }

  .dot.ok {
    background: var(--color-accent);
  }

  .link {
    flex-shrink: 0;
    padding: 2px 6px;
    color: var(--color-accent);
    background: none;
    border: 0;
    cursor: pointer;
    font-size: var(--text-small);
  }

  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  .chat {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .chat__header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-5);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-bg-panel);
  }

  .chat__title {
    font-size: var(--text-large);
    font-weight: 600;
  }

  .chat__status {
    color: var(--color-text-muted);
    font-size: var(--text-small);
  }

  .chat__body {
    flex: 1;
    min-height: 0;
    display: flex;
  }
</style>
