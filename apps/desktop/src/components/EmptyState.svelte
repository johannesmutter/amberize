<script>
  let {
    title = '',
    description = '',
    hint = '',
    variant = 'inbox',
    compact = false,
  } = $props();

  const icon_paths = {
    inbox: 'M4 8h16v8a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V8zm0 0l3-4h10l3 4m-11 5h6',
    search: 'M11 19a8 8 0 1 1 5.29-14.01A8 8 0 0 1 11 19zm10 2-4.3-4.3',
    archive: 'M3 7h18v4H3V7zm2 4h14v8a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2v-8zm5 3h4',
    message: 'M4 6h16v12H4V6zm0 0 8 6 8-6',
    accounts: 'M8 12a3 3 0 1 1 0-6 3 3 0 0 1 0 6zm8 1a3 3 0 1 1 0-6 3 3 0 0 1 0 6zM3 20a5 5 0 0 1 10 0m-1 0h9',
  };

  let icon_path = $derived(icon_paths[variant] ?? icon_paths.archive);
</script>

<div class="empty-state" class:compact>
  <div class="empty-state-icon" aria-hidden="true">
    <svg viewBox="0 0 24 24" fill="none" role="presentation">
      <path d={icon_path} />
    </svg>
  </div>
  <p class="empty-state-title">{title}</p>
  {#if description}
    <p class="empty-state-description">{description}</p>
  {/if}
  {#if hint}
    <p class="empty-state-hint">{hint}</p>
  {/if}
</div>

<style>
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-xs);
    text-align: center;
    color: var(--color-text-tertiary);
    padding: var(--space-2xl);
    width: 100%;
    height: 100%;
  }

  .empty-state.compact {
    padding: var(--space-lg);
    gap: 4px;
  }

  .empty-state-icon {
    width: 48px;
    height: 48px;
    border-radius: 12px;
    display: flex;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--color-border);
    background: var(--color-bg-secondary);
    margin-bottom: var(--space-xs);
  }

  .empty-state.compact .empty-state-icon {
    width: 38px;
    height: 38px;
    border-radius: 10px;
    margin-bottom: 2px;
  }

  .empty-state-icon svg {
    width: 22px;
    height: 22px;
  }

  .empty-state-icon path {
    stroke: var(--color-text-tertiary);
    stroke-width: 1.6;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .empty-state-title {
    margin: 0;
    color: var(--color-text-secondary);
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-medium);
  }

  .empty-state-description,
  .empty-state-hint {
    margin: 0;
    font-size: var(--font-size-xs);
    line-height: 1.45;
    max-width: 48ch;
  }
</style>
