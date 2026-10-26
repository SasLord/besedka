<script lang="ts">
  let {
    title,
    text = "",
    icon = "💬",
  }: {
    title: string;
    text?: string;
    icon?: string;
  } = $props();
</script>

<div class="empty" role="status">
  <div class="empty__icon" aria-hidden="true">{icon}</div>
  <h1 class="empty__title">{title}</h1>
  {#if text}
    <p class="empty__text">{text}</p>
  {/if}
</div>

<style>
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    gap: var(--space-3);
    padding: var(--space-6);
    text-align: center;
  }

  .empty__icon {
    font-size: 44px;
    line-height: 1;
  }

  .empty__title {
    margin: 0;
    font-size: var(--text-large);
    font-weight: 600;
  }

  .empty__text {
    margin: 0;
    max-width: 42ch;
    color: var(--color-text-muted);
    font-size: var(--text-small);
  }
</style>
