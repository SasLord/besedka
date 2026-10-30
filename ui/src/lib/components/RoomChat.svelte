<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import { ApiError, listMessages, postMessage } from "../api";
  import type { Message, Room } from "../api";

  let {
    room,
    meId,
    userName,
    onNewMessages,
  }: {
    room: Room;
    meId: number;
    /** Maps a sender id to a display name (fallback handled by the caller). */
    userName: (id: number) => string;
    /** Fired with newly learned messages (initial, older page, polled) so the
     *  caller can refresh its sender-name catalog for unknown ids. Optional. */
    onNewMessages?: (msgs: Message[]) => void;
  } = $props();

  const PAGE = 50;
  const POLL_MS = 5000;

  let messages = $state<Message[]>([]);
  let hasMore = $state(false);
  let loading = $state(true);
  let loadingMore = $state(false);
  let sending = $state(false);
  let error = $state<string | null>(null);
  let draft = $state("");
  let listEl: HTMLElement | undefined = $state();
  let pollTimer: ReturnType<typeof setInterval> | undefined;

  function toMessage(e: unknown): string {
    if (e instanceof ApiError) return `Ошибка ${e.status}: ${e.message}`;
    return e instanceof Error ? e.message : String(e);
  }

  function lastId(): number {
    return messages.length ? messages[messages.length - 1].id : 0;
  }

  function nearBottom(): boolean {
    const el = listEl;
    return el ? el.scrollHeight - el.scrollTop - el.clientHeight < 160 : true;
  }

  function scrollBottom() {
    const el = listEl;
    if (el) el.scrollTop = el.scrollHeight;
  }

  async function loadInitial() {
    loading = true;
    error = null;
    try {
      const page = await listMessages(room.id, PAGE);
      messages = page;
      hasMore = page.length === PAGE;
      onNewMessages?.(page);
    } catch (e) {
      error = toMessage(e);
    } finally {
      loading = false;
    }
  }

  /** Older page via the `before_id` cursor (id of the oldest in the list). */
  async function loadMore() {
    if (loadingMore || !hasMore || !messages.length) return;
    const el = listEl;
    const prevHeight = el?.scrollHeight ?? 0;
    const prevTop = el?.scrollTop ?? 0;
    loadingMore = true;
    error = null;
    try {
      const older = await listMessages(room.id, PAGE, messages[0].id);
      if (older.length) {
        messages = [...older, ...messages];
        hasMore = older.length === PAGE;
        onNewMessages?.(older);
      } else {
        hasMore = false;
      }
    } catch (e) {
      error = toMessage(e);
    } finally {
      loadingMore = false;
      // Keep the visible position: re-anchor after the prepend (scroll must not jump).
      void tick().then(() => {
        if (el) el.scrollTop = prevTop + (el.scrollHeight - prevHeight);
      });
    }
  }

  /** Polling: fetch the newest page, append only messages newer than ours. */
  function appendNew(incoming: Message[]) {
    const newest = lastId();
    const fresh = incoming.filter((m) => m.id > newest);
    if (!fresh.length) return;
    const stick = nearBottom();
    messages = [...messages, ...fresh];
    onNewMessages?.(fresh);
    if (stick) void tick().then(scrollBottom);
  }

  async function poll() {
    try {
      appendNew(await listMessages(room.id, PAGE));
    } catch {
      // Best effort: a hiccup must not spam the error banner every 5s.
    }
  }

  async function send() {
    const text = draft.trim();
    if (!text || sending) return;
    sending = true;
    error = null;
    try {
      const stored = await postMessage(room.id, text);
      if (!messages.some((m) => m.id === stored.id)) {
        messages = [...messages, stored];
      }
      draft = "";
      void tick().then(scrollBottom);
    } catch (e) {
      error = toMessage(e);
    } finally {
      sending = false;
    }
  }

  const KIND_LABEL: Record<Room["kind"], string> = {
    general: "общая",
    room: "комната",
    channel: "канал",
    direct: "личный",
  };

  function fmtTime(iso: string): string {
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) return iso;
    const hm = d.toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });
    const sameDay = d.toDateString() === new Date().toDateString();
    return sameDay
      ? hm
      : `${d.toLocaleDateString("ru-RU", { day: "2-digit", month: "2-digit" })} ${hm}`;
  }

  function plural(n: number, one: string, few: string, many: string): string {
    const n10 = n % 10;
    const n100 = n % 100;
    if (n10 === 1 && n100 !== 11) return one;
    if (n10 >= 2 && n10 <= 4 && (n100 < 12 || n100 > 14)) return few;
    return many;
  }

  onMount(() => {
    void loadInitial();
    pollTimer = setInterval(poll, POLL_MS);
  });

  onDestroy(() => {
    if (pollTimer !== undefined) clearInterval(pollTimer);
  });
</script>

<div class="roomchat">
  <header class="roomchat__header">
    <h2 class="roomchat__title">{room.title}</h2>
    <span class="roomchat__kind">
      {KIND_LABEL[room.kind]} · {room.member_count}
      {plural(room.member_count, "участник", "участника", "участников")}
    </span>
  </header>

  {#if error}
    <p class="roomchat__error" role="alert">{error}</p>
  {/if}

  <div class="roomchat__list" bind:this={listEl}>
    {#if hasMore}
      <button class="roomchat__more" type="button" disabled={loadingMore} onclick={() => void loadMore()}>
        {loadingMore ? "Загрузка…" : "Загрузить ещё"}
      </button>
    {/if}

    {#if loading}
      <p class="roomchat__hint">Загрузка сообщений…</p>
    {:else if messages.length === 0}
      <p class="roomchat__hint">Пока пусто — напишите первое сообщение.</p>
    {/if}

    {#each messages as m (m.id)}
      <div class="msg" class:mine={m.sender_id === meId}>
        <div class="msg__meta">
          <span class="msg__sender">{m.sender_id === meId ? "Вы" : userName(m.sender_id)}</span>
          <span class="msg__time">{fmtTime(m.created_at)}</span>
        </div>
        <div class="msg__text">{m.text}</div>
      </div>
    {/each}
  </div>

  <footer class="roomchat__composer">
    <input
      class="roomchat__input"
      type="text"
      bind:value={draft}
      placeholder="Сообщение…"
      maxlength={4096}
      onkeydown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          void send();
        }
      }}
    />
    <button class="roomchat__send" type="button" disabled={sending || !draft.trim()} onclick={() => void send()}>
      Отправить
    </button>
  </footer>
</div>

<style>
  .roomchat {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    background: var(--color-bg);
  }

  .roomchat__header {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-5);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-bg-panel);
  }

  .roomchat__title {
    margin: 0;
    font-size: var(--text-large);
    font-weight: 600;
  }

  .roomchat__kind {
    color: var(--color-text-muted);
    font-size: var(--text-small);
  }

  .roomchat__error {
    margin: 0;
    padding: var(--space-2) var(--space-5);
    font-size: var(--text-small);
    font-weight: 600;
    color: var(--color-danger);
    background: var(--color-bg-panel);
    border-bottom: 1px solid var(--color-border);
  }

  .roomchat__list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: var(--space-4) var(--space-5);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .roomchat__more {
    align-self: center;
    flex-shrink: 0;
    padding: var(--space-1) var(--space-4);
    font-size: var(--text-small);
    font-weight: 600;
    color: var(--color-accent);
    background: transparent;
    border: 1px solid var(--color-accent);
    border-radius: 999px;
    cursor: pointer;
  }

  .roomchat__more:hover {
    background: var(--color-accent-soft);
  }

  .roomchat__more:disabled {
    opacity: 0.6;
    cursor: default;
  }

  .roomchat__hint {
    margin: auto 0;
    text-align: center;
    color: var(--color-text-muted);
    font-size: var(--text-small);
  }

  .msg {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    max-width: 70%;
    background: var(--color-bg-panel);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    padding: var(--space-2) var(--space-3);
    box-shadow: var(--shadow-sm);
  }

  .msg.mine {
    align-self: flex-end;
    background: var(--color-accent-soft);
    border-color: transparent;
  }

  .msg__meta {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
  }

  .msg__sender {
    font-size: var(--text-small);
    font-weight: 600;
    color: var(--color-accent);
  }

  .msg__time {
    font-size: 11px;
    color: var(--color-text-muted);
  }

  .msg__text {
    white-space: pre-wrap;
    word-break: break-word;
  }

  .roomchat__composer {
    display: flex;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-5);
    border-top: 1px solid var(--color-border);
    background: var(--color-bg-panel);
  }

  .roomchat__input {
    flex: 1;
    min-width: 0;
    padding: var(--space-2) var(--space-3);
    font: inherit;
    color: var(--color-text);
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
  }

  .roomchat__input:focus {
    outline: 2px solid var(--color-accent);
    outline-offset: -1px;
  }

  .roomchat__send {
    flex-shrink: 0;
    padding: var(--space-2) var(--space-4);
    font-size: var(--text-small);
    font-weight: 600;
    color: var(--color-bg-panel);
    background: var(--color-accent);
    border: 1px solid var(--color-accent);
    border-radius: var(--radius-md);
    cursor: pointer;
  }

  .roomchat__send:disabled {
    opacity: 0.6;
    cursor: default;
  }
</style>
