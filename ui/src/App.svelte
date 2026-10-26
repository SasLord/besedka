<script lang="ts">
  import { onMount } from "svelte";
  import EmptyState from "./lib/components/EmptyState.svelte";
  import { fetchPing, fetchRooms, type Room } from "./lib/api";

  let rooms = $state<Room[]>([]);
  let loaded = $state(false);
  let error = $state<string | null>(null);
  let serverOk = $state(false);

  async function load() {
    try {
      const ping = await fetchPing();
      serverOk = ping.status === "ok";
      rooms = await fetchRooms();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loaded = true;
    }
  }

  onMount(load);
</script>

<div class="app">
  <aside class="sidebar">
    <header class="sidebar__header">
      <span class="brand">Besedka</span>
    </header>

    <nav class="rooms" aria-label="Комнаты">
      {#each rooms as room (room.id)}
        <button class="room" type="button" title={room.title}>
          <span class="room__title">{room.title}</span>
          <span class="room__kind">{room.kind}</span>
        </button>
      {:else}
        <p class="rooms__empty">Комнат пока нет</p>
      {/each}
    </nav>

    <footer class="sidebar__footer">
      <span class="dot" class:ok={serverOk} aria-hidden="true"></span>
      <span>{serverOk ? "сервер на связи" : "сервер недоступен"}</span>
    </footer>
  </aside>

  <main class="main">
    {#if error}
      <EmptyState icon="⚠️" title="Нет связи с сервером" text={error} />
    {:else if !loaded}
      <EmptyState icon="⏳" title="Загрузка…" />
    {:else}
      <EmptyState
        title="Комнат пока нет"
        text="Создайте первую комнату — сообщения появятся здесь."
      />
    {/if}
  </main>
</div>

<style>
  .app {
    display: flex;
    height: 100%;
  }

  .sidebar {
    display: flex;
    flex-direction: column;
    width: 280px;
    min-width: 220px;
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
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border: 0;
    border-radius: var(--radius-md);
    background: transparent;
    cursor: pointer;
    text-align: left;
  }

  .room:hover {
    background: var(--color-accent-soft);
  }

  .room__title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .room__kind {
    flex-shrink: 0;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.4px;
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

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--color-danger);
  }

  .dot.ok {
    background: var(--color-accent);
  }

  .main {
    flex: 1;
    min-width: 0;
    display: flex;
  }
</style>
