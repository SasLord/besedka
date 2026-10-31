<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import {
    ApiError,
    deleteMessage,
    editMessage,
    listMessages,
    markRoomRead,
    messageReaders,
    postMessage,
  } from "../api";
  import type { Message, Room, UserRef } from "../api";

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
      markRead();
      void refreshReadReceipts();
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
      markRead();
      void refreshReadReceipts();
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
      markRead();
      void tick().then(scrollBottom);
    } catch (e) {
      error = toMessage(e);
    } finally {
      sending = false;
    }
  }

  // ----- Wave 2.3: edit + delete own messages -----
  let editingId = $state<number | null>(null);
  let editDraft = $state("");
  let saving = $state(false);
  let editInputEl: HTMLInputElement | undefined = $state();
  /** Two-click delete: the message whose «Удалить» turned into «Точно?». */
  let confirmDeleteId = $state<number | null>(null);
  let confirmDeleteTimer: ReturnType<typeof setTimeout> | undefined;

  function startEdit(m: Message) {
    if (editingId !== null) return; // one editor at a time
    // A pending «Точно?» from before must not survive the switch to edit mode.
    if (confirmDeleteTimer !== undefined) clearTimeout(confirmDeleteTimer);
    confirmDeleteId = null;
    editingId = m.id;
    editDraft = m.text;
    void tick().then(() => {
      editInputEl?.focus();
      editInputEl?.select();
    });
  }

  function cancelEdit() {
    editingId = null;
    editDraft = "";
  }

  async function saveEdit(m: Message) {
    const text = editDraft.trim();
    if (!text || saving) return;
    saving = true;
    error = null;
    try {
      const updated = await editMessage(room.id, m.id, text);
      messages = messages.map((x) => (x.id === updated.id ? updated : x));
      cancelEdit();
    } catch (e) {
      error = toMessage(e);
    } finally {
      saving = false;
    }
  }

  /** First click on «Удалить»: arm the confirmation («Точно?»), auto-resets in 3 s. */
  function armDelete(m: Message) {
    if (confirmDeleteId === m.id) return; // already armed — second click deletes
    confirmDeleteId = m.id;
    if (confirmDeleteTimer !== undefined) clearTimeout(confirmDeleteTimer);
    confirmDeleteTimer = setTimeout(() => (confirmDeleteId = null), 3000);
  }

  async function remove(m: Message) {
    if (confirmDeleteTimer !== undefined) clearTimeout(confirmDeleteTimer);
    confirmDeleteId = null;
    error = null;
    try {
      await deleteMessage(room.id, m.id);
      messages = messages.filter((x) => x.id !== m.id);
    } catch (e) {
      error = toMessage(e);
    }
  }

  // ----- Wave 2.3: read receipts (read marks) -----
  /** msg id -> who has read it (fetched lazily, refreshed while the room is open). */
  let readersById = $state(new Map<number, UserRef[]>());
  let expandedReadId = $state<number | null>(null);

  /** The room is open = everything up to now is read. Best effort. */
  function markRead() {
    void markRoomRead(room.id).catch(() => {
      /* not a member or a hiccup — the chat itself must not break */
    });
  }

  /** Fetch the reader lists of my messages (newest first, capped at 20). */
  async function refreshReadReceipts() {
    const mine = messages.filter((m) => m.sender_id === meId).slice(0, 20);
    if (!mine.length) return;
    const settled = await Promise.allSettled(mine.map((m) => messageReaders(room.id, m.id)));
    const next = new Map(readersById);
    settled.forEach((res, i) => {
      if (res.status === "fulfilled") next.set(mine[i].id, res.value);
    });
    readersById = next;
  }

  function toggleReaders(m: Message) {
    if (expandedReadId === m.id) {
      expandedReadId = null;
      return;
    }
    expandedReadId = m.id;
    if (!readersById.has(m.id)) void refreshReadReceipts();
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
    if (confirmDeleteTimer !== undefined) clearTimeout(confirmDeleteTimer);
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
          {#if m.edited_at}<span class="msg__edited">(изменено)</span>{/if}
          {#if m.sender_id === meId}
            <button
              class="msg__read"
              type="button"
              title="Кто прочитал"
              onclick={() => toggleReaders(m)}
            >
              ✓ {readersById.get(m.id)?.length ?? "…"}
            </button>
          {/if}
        </div>
        {#if editingId === m.id}
          <div class="msg__edit">
            <input
              class="msg__edit-input"
              type="text"
              bind:value={editDraft}
              bind:this={editInputEl}
              maxlength={4096}
              onkeydown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  void saveEdit(m);
                } else if (e.key === "Escape") {
                  cancelEdit();
                }
              }}
            />
            <div class="msg__edit-actions">
              <button
                class="msg__btn"
                type="button"
                disabled={saving || !editDraft.trim()}
                onclick={() => void saveEdit(m)}
              >
                {saving ? "Сохранение…" : "Сохранить"}
              </button>
              <button class="msg__btn" type="button" disabled={saving} onclick={cancelEdit}>
                Отмена
              </button>
            </div>
          </div>
        {:else}
          <div class="msg__text">{m.text}</div>
          {#if m.sender_id === meId && expandedReadId === m.id}
            <div class="msg__readers">
              {#if readersById.has(m.id)}
                Прочитали: {readersById.get(m.id)?.map((u) => u.display_name).join(", ") || "пока никто"}
              {:else}
                Загружаю…
              {/if}
            </div>
          {/if}
          {#if m.sender_id === meId}
            <div class="msg__actions">
              <button class="msg__btn" type="button" title="Изменить текст" onclick={() => startEdit(m)}>
                ✎ Изменить
              </button>
              <button
                class="msg__btn msg__btn--danger"
                type="button"
                title={confirmDeleteId === m.id ? "Нажмите ещё раз, чтобы удалить" : "Удалить сообщение"}
                onclick={() => (confirmDeleteId === m.id ? void remove(m) : armDelete(m))}
              >
                {confirmDeleteId === m.id ? "⚠ Точно?" : "🗑 Удалить"}
              </button>
            </div>
          {/if}
        {/if}
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

  .msg__edited {
    font-size: 11px;
    font-style: italic;
    color: var(--color-text-muted);
  }

  .msg__read {
    padding: 0;
    font-size: 11px;
    color: var(--color-text-muted);
    background: transparent;
    border: none;
    cursor: pointer;
  }

  .msg__read:hover {
    color: var(--color-accent);
  }

  .msg__readers {
    font-size: 11px;
    color: var(--color-text-muted);
    text-align: right;
  }

  .msg__actions {
    display: none;
    gap: var(--space-2);
  }

  .msg.mine:hover .msg__actions,
  .msg.mine:focus-within .msg__actions {
    display: flex;
  }

  .msg__btn {
    padding: 2px var(--space-2);
    font-size: 11px;
    color: var(--color-text-muted);
    background: transparent;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    cursor: pointer;
  }

  .msg__btn:hover {
    color: var(--color-text);
    background: var(--color-accent-soft);
  }

  .msg__btn:disabled {
    opacity: 0.6;
    cursor: default;
  }

  .msg__btn--danger:hover {
    color: var(--color-danger);
    border-color: var(--color-danger);
    background: transparent;
  }

  .msg__edit {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .msg__edit-input {
    width: 100%;
    padding: var(--space-1) var(--space-2);
    font: inherit;
    color: var(--color-text);
    background: var(--color-bg);
    border: 1px solid var(--color-accent);
    border-radius: var(--radius-md);
  }

  .msg__edit-input:focus {
    outline: 2px solid var(--color-accent);
    outline-offset: -1px;
  }

  .msg__edit-actions {
    display: flex;
    gap: var(--space-2);
    justify-content: flex-end;
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
