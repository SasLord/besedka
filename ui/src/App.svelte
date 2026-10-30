<script lang="ts">
  import { onMount } from "svelte";
  import EmptyState from "./lib/components/EmptyState.svelte";
  import Login from "./lib/components/Login.svelte";
  import {
    ApiError,
    fetchPing,
    fetchRooms,
    fetchUsers,
    joinRoom,
    leaveRoom,
    logout,
    me,
    ssoStatus,
    type Room,
    type SsoStatus,
    type User,
    type UserRef,
  } from "./lib/api";
  import RoomChat from "./lib/components/RoomChat.svelte";

  let user = $state<User | null>(null);
  let authChecked = $state(false);
  let rooms = $state<Room[]>([]);
  let sso = $state<SsoStatus | null>(null);
  let error = $state<string | null>(null);
  let busyRoom = $state<number | null>(null);
  let serverOk = $state(false);
  // Wave 2.2: user catalog (sender names) + the room whose chat is open.
  let users = $state<UserRef[]>([]);
  let activeRoomId = $state<number | null>(null);
  const activeRoom = $derived(rooms.find((r) => r.id === activeRoomId) ?? null);

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

  // ----- Wave 2.2: chat wiring -----

  /** Sender name: catalog display name, my own name for myself, else a fallback. */
  function userName(id: number): string {
    const ref = users.find((u) => u.id === id);
    if (ref) return ref.display_name;
    if (user && user.id === id) return displayName(user);
    return `Пользователь #${id}`;
  }

  function openRoom(room: Room) {
    activeRoomId = room.id;
    error = null;
  }

  /** First joined room becomes the chat on load/login (if none picked yet). */
  function selectDefaultRoom() {
    if (activeRoomId !== null) return;
    const first = rooms.find((r) => r.joined);
    if (first) activeRoomId = first.id;
  }

  /** The catalog is only for sender names — best effort, never blocks the UI. */
  async function loadUsers() {
    try {
      users = await fetchUsers();
    } catch {
      users = [];
    }
  }

  /** A message may arrive from an id the catalog doesn't know (a user
   *  registered after login) — refresh the catalog once so names render. */
  let usersRefreshing = false;
  async function refreshUsersIfNeeded(newMsgs: { sender_id: number }[]) {
    const me = user;
    if (!me || !newMsgs.length) return;
    const hasUnknown = newMsgs.some(
      (m) => m.sender_id !== me.id && !users.some((u) => u.id === m.sender_id),
    );
    if (!hasUnknown || usersRefreshing) return;
    usersRefreshing = true;
    try {
      users = await fetchUsers();
    } catch {
      // keep the old catalog; the fallback name stays
    } finally {
      usersRefreshing = false;
    }
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
    if (user) {
      await loadUsers();
    }
    try {
      await refreshRooms();
    } catch (e) {
      error = toMessage(e);
    }
    if (user) {
      selectDefaultRoom();
    }
    authChecked = true;
  });

  async function handleAuthed(authedUser: User) {
    user = authedUser;
    error = null;
    await loadUsers();
    try {
      await refreshRooms();
      selectDefaultRoom();
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
    users = [];
    activeRoomId = null;
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
          <div
            class="room"
            class:joined={room.joined}
            class:active={room.id === activeRoomId}
            role="button"
            tabindex="0"
            title={room.joined ? "Открыть чат" : "Открыть комнату"}
            onclick={() => openRoom(room)}
            onkeydown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                openRoom(room);
              }
            }}
          >
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
              onclick={(e) => {
                e.stopPropagation();
                void toggleMembership(room);
              }}
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
      {:else if activeRoom && activeRoom.joined}
        <RoomChat room={activeRoom} meId={user.id} {userName} onNewMessages={(m) => void refreshUsersIfNeeded(m)} />
      {:else if activeRoom}
        <div class="joincta">
          <span class="joincta__icon" aria-hidden="true">🔒</span>
          <h2 class="joincta__title">Вы ещё не в «{activeRoom.title}»</h2>
          <p class="joincta__text">
            Присоединяйтесь к комнате, чтобы видеть историю и отправлять сообщения.
          </p>
          <button
            class="room__btn joincta__btn"
            type="button"
            disabled={busyRoom === activeRoom.id}
            onclick={() => void toggleMembership(activeRoom)}
          >
            {busyRoom === activeRoom.id ? "…" : "Войти в комнату"}
          </button>
        </div>
      {:else}
        <EmptyState
          icon="💬"
          title="Вы не в комнатах"
          text="Выберите комнату в левой панели — её чат откроется здесь."
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
    cursor: pointer;
  }

  .room:hover {
    background: var(--color-accent-soft);
  }

  .room.joined {
    background: var(--color-accent-soft);
  }

  .room.active {
    border-left: 3px solid var(--color-accent);
    padding-left: calc(var(--space-3) - 3px);
  }

  .room.active .room__title {
    color: var(--color-accent);
    font-weight: 600;
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

  .joincta {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-3);
    padding: var(--space-5);
    text-align: center;
  }

  .joincta__icon {
    font-size: 40px;
    line-height: 1;
  }

  .joincta__title {
    margin: 0;
    font-size: var(--text-large);
    font-weight: 600;
  }

  .joincta__text {
    margin: 0 0 var(--space-2);
    max-width: 420px;
    color: var(--color-text-muted);
    font-size: var(--text-small);
  }

  .joincta__btn {
    padding: var(--space-2) var(--space-4);
  }
</style>
