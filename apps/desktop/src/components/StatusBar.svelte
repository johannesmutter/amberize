<script>
  /**
   * Status bar showing sync status, progress indicator, errors, archive stats, and sync button.
   */

  const BYTES_PER_KB = 1024;
  const BYTES_PER_MB = 1024 * 1024;
  const BYTES_PER_GB = 1024 * 1024 * 1024;

  let {
    syncing = false,
    syncing_account = null,
    last_sync = null,
    error = null,
    error_account_id = null,
    progress = null,
    total_messages = 0,
    db_size_bytes = 0,
    oldest_date = null,
    newest_date = null,
    on_sync,
    on_fix_error,
  } = $props();

  /** Toggle between showing mail count and storage size. */
  let show_storage = $state(false);

  /**
   * Format the last sync time as relative
   * @param {string | null} timestamp
   */
  function format_relative_time(timestamp) {
    if (!timestamp) return 'never';

    try {
      const date = new Date(timestamp);
      const now = new Date();
      const diff_ms = now.getTime() - date.getTime();
      const diff_minutes = Math.floor(diff_ms / (1000 * 60));
      const diff_hours = Math.floor(diff_ms / (1000 * 60 * 60));
      const diff_days = Math.floor(diff_ms / (1000 * 60 * 60 * 24));

      if (diff_minutes < 1) return 'just now';
      if (diff_minutes < 60) return `${diff_minutes}m ago`;
      if (diff_hours < 24) return `${diff_hours}h ago`;
      if (diff_days === 1) return 'yesterday';
      return `${diff_days}d ago`;
    } catch {
      return 'unknown';
    }
  }

  /**
   * Format a byte count into a human-readable string.
   * @param {number} bytes
   * @returns {string}
   */
  function format_bytes(bytes) {
    if (bytes < BYTES_PER_KB) return `${bytes} B`;
    if (bytes < BYTES_PER_MB) return `${(bytes / BYTES_PER_KB).toFixed(1)} KB`;
    if (bytes < BYTES_PER_GB) return `${(bytes / BYTES_PER_MB).toFixed(1)} MB`;
    return `${(bytes / BYTES_PER_GB).toFixed(2)} GB`;
  }

  /**
   * Format a number with locale-aware thousand separators.
   * @param {number} n
   * @returns {string}
   */
  function format_number(n) {
    return n.toLocaleString();
  }

  let idle_text = $derived(`Synced ${format_relative_time(last_sync)}`);

  /** @type {string} */
  let progress_text = $derived.by(() => {
    if (!syncing) return '';
    if (!progress) {
      return syncing_account
        ? `Syncing ${syncing_account}…`
        : 'Syncing…';
    }

    const { mailbox_name, mailbox_index, mailbox_count, messages_ingested } = progress;
    const mailbox_label = mailbox_count > 1
      ? `${mailbox_name} (${mailbox_index}/${mailbox_count})`
      : mailbox_name;

    if (messages_ingested > 0) {
      return `Syncing ${mailbox_label} — ${messages_ingested} messages archived`;
    }
    return `Syncing ${mailbox_label}…`;
  });

  /** Fraction 0..1 for the progress bar, based on mailbox index. */
  let progress_fraction = $derived.by(() => {
    if (!syncing || !progress || progress.mailbox_count === 0) return 0;
    return progress.mailbox_index / progress.mailbox_count;
  });

  let stats_label = $derived(
    show_storage
      ? format_bytes(db_size_bytes)
      : `${format_number(total_messages)} emails`
  );

  let retention_label = $derived.by(() => {
    if (!oldest_date || !newest_date) return 'Retention window unavailable';
    return `${format_short_date(oldest_date)} -> ${format_short_date(newest_date)}`;
  });

  function toggle_stats_view() {
    show_storage = !show_storage;
  }

  /**
   * @param {string} date_value
   * @returns {string}
   */
  function format_short_date(date_value) {
    try {
      const date = new Date(date_value);
      if (isNaN(date.getTime())) return date_value;
      return date.toLocaleDateString(undefined, {
        year: 'numeric',
        month: 'short',
        day: 'numeric',
      });
    } catch {
      return date_value;
    }
  }

  function handle_error_click() {
    if (error_account_id != null) {
      on_fix_error?.(error_account_id);
    }
  }
</script>

<div class="status-bar">
  <div class="status-content">
    {#if error}
      <button type="button" class="error-message" onclick={handle_error_click}>
        <span class="error-icon">⚠</span>
        <span class="error-text">{error}</span>
        {#if error_account_id != null}
          <span class="error-action">Click to fix.</span>
        {/if}
      </button>
    {:else if syncing}
      <div class="sync-progress">
        <div class="sync-progress-top">
          <span class="sync-icon spinning">↻</span>
          <span class="sync-progress-text">{progress_text}</span>
        </div>
        <div class="progress-bar-track">
          <div
            class="progress-bar-fill"
            class:indeterminate={!progress}
            style:width={progress ? `${Math.round(progress_fraction * 100)}%` : '100%'}
          ></div>
        </div>
      </div>
    {:else}
      <span class="status-text">
        <span class="sync-icon">✓</span>
        {idle_text}
      </span>
    {/if}
  </div>

  <button
    type="button"
    class="stats-toggle"
    onclick={toggle_stats_view}
    title={`${show_storage ? 'Show email count' : 'Show storage used'} • ${retention_label}`}
  >
    {stats_label}
  </button>

  <span class="retention-text" title="Informational retention window from archived message dates">
    {retention_label}
  </span>

  <button
    type="button"
    class="sync-button"
    onclick={on_sync}
    disabled={syncing}
  >
    Sync Now
  </button>
</div>

<style>
  .status-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-sm) var(--space-lg);
    border-top: 1px solid var(--color-border);
    background: var(--color-bg-secondary);
    font-size: var(--font-size-xs);
    gap: var(--space-md);
  }

  .status-content {
    flex: 1;
    min-width: 0;
  }

  .status-text {
    color: var(--color-text-tertiary);
    display: flex;
    align-items: center;
    gap: var(--space-sm);
  }

  .sync-icon {
    font-size: 1em;
    color: var(--color-accent);
  }

  .sync-icon.spinning {
    display: inline-block;
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  /* Sync progress layout */
  .sync-progress {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .sync-progress-top {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    color: var(--color-text-secondary);
  }

  .sync-progress-text {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Progress bar */
  .progress-bar-track {
    height: 3px;
    background: var(--color-border);
    border-radius: 2px;
    overflow: hidden;
  }

  .progress-bar-fill {
    height: 100%;
    background: var(--color-accent);
    border-radius: 2px;
    transition: width 0.3s ease;
  }

  .progress-bar-fill.indeterminate {
    width: 30% !important;
    animation: indeterminate 1.5s ease-in-out infinite;
  }

  @keyframes indeterminate {
    0% { transform: translateX(-100%); }
    100% { transform: translateX(400%); }
  }

  /* Error */
  .error-message {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    background: none;
    border: none;
    padding: 0;
    margin: 0;
    font: inherit;
    color: inherit;
    cursor: pointer;
    text-align: left;
  }

  .error-message:hover .error-text,
  .error-message:hover .error-action {
    text-decoration: underline;
  }

  .error-icon {
    color: var(--color-error);
    font-size: 1em;
  }

  .error-text {
    color: var(--color-error);
  }

  .error-action {
    color: var(--color-text-secondary);
  }

  /* Stats toggle (centered) */
  .stats-toggle {
    background: none;
    border: none;
    padding: var(--space-xs) var(--space-sm);
    font: inherit;
    font-size: var(--font-size-xs);
    color: var(--color-text-tertiary);
    cursor: pointer;
    white-space: nowrap;
    flex-shrink: 0;
    transition: color var(--transition-fast);
  }

  .stats-toggle:hover {
    color: var(--color-text-secondary);
  }

  .retention-text {
    color: var(--color-text-tertiary);
    font-size: var(--font-size-xs);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: min(36vw, 360px);
  }

  /* Sync button */
  .sync-button {
    padding: var(--space-xs) var(--space-sm);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: var(--color-text-secondary);
    cursor: pointer;
    font: inherit;
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-medium);
    flex-shrink: 0;
    transition: all var(--transition-fast);
  }

  .sync-button:hover:not(:disabled) {
    background: var(--color-bg-tertiary);
    color: var(--color-text);
  }

  .sync-button:active:not(:disabled) {
    transform: scale(0.98);
  }

  .sync-button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
