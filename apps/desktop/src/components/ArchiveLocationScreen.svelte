<script>
  import { tauri_invoke, tauri_open_dialog, tauri_save_dialog } from '../lib/tauri_bridge.js';

  let { on_continue } = $props();

  const DEFAULT_DB_FILENAME = 'email-archive.sqlite3';

  let db_path = $state('');
  let error_message = $state('');
  let sync_folder_warning = $state('');

  /**
   * Open an existing archive database file.
   */
  async function open_existing() {
    error_message = '';
    try {
      const selected = await tauri_open_dialog({
        title: 'Open existing archive',
        multiple: false,
        directory: false,
        filters: [{ name: 'SQLite database', extensions: ['sqlite3', 'db'] }],
      });
      if (!selected) return;
      db_path = selected;
      await update_sync_folder_warning(selected);
    } catch (err) {
      error_message = err instanceof Error ? err.message : String(err);
    }
  }

  /**
   * Create a new archive database file.
   */
  async function create_new() {
    error_message = '';
    try {
      const selected = await tauri_save_dialog({
        title: 'Create new archive',
        defaultPath: DEFAULT_DB_FILENAME,
        filters: [{ name: 'SQLite database', extensions: ['sqlite3', 'db'] }],
      });
      if (!selected) return;
      db_path = selected;
      await update_sync_folder_warning(selected);
    } catch (err) {
      error_message = err instanceof Error ? err.message : String(err);
    }
  }

  /**
   * Warn when the selected path appears to be cloud-synced.
   * @param {string} selected_path
   */
  async function update_sync_folder_warning(selected_path) {
    sync_folder_warning = '';
    try {
      const risky = await tauri_invoke('is_sync_folder_path', { path: selected_path });
      if (!risky) return;
      sync_folder_warning = 'This folder looks like iCloud/Dropbox/OneDrive storage. Cloud sync can corrupt a local archive database.';
    } catch {
      // ignore in browser dev mode
    }
  }

  function handle_continue() {
    const trimmed = db_path.trim();
    if (!trimmed) {
      error_message = 'Please choose a location for your email archive.';
      return;
    }
    on_continue(trimmed);
  }

  function handle_keydown(event) {
    if (event.key === 'Enter') {
      handle_continue();
    }
  }
</script>

<div class="archive-location-screen">
  <div class="content">
    <div class="branding">
      <h1 class="app-name">Amberize</h1>
      <p class="tagline">Seal emails in time</p>
    </div>

    <h2>Choose your archive</h2>
    <p class="subtitle">Open an existing archive or create a new one</p>

    <div class="button-row">
      <button type="button" class="action-button" onclick={open_existing}>
        Open Existing
      </button>
      <button type="button" class="action-button" onclick={create_new}>
        Create New
      </button>
    </div>

    {#if db_path}
      <div class="selected-path">
        <span class="path-label">Selected:</span>
        <code class="path-value">{db_path}</code>
      </div>
    {/if}

    {#if error_message}
      <p class="error">{error_message}</p>
    {/if}

    {#if sync_folder_warning}
      <p class="warning">{sync_folder_warning}</p>
    {/if}

    <div class="actions">
      <button
        type="button"
        class="continue-button"
        onclick={handle_continue}
        disabled={!db_path.trim()}
      >
        Continue
      </button>
    </div>
  </div>
</div>

<style>
  .archive-location-screen {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 100vh;
    padding: var(--space-2xl);
    padding-top: calc(var(--titlebar-height) + var(--space-2xl));
    background: var(--color-bg);
  }

  .content {
    max-width: 420px;
    width: 100%;
    text-align: center;
  }

  .branding {
    margin-bottom: var(--space-2xl);
  }

  .app-name {
    margin: 0;
    font-size: 32px;
    font-weight: var(--font-weight-semibold);
    color: var(--color-accent);
    letter-spacing: -0.02em;
  }

  .tagline {
    margin: var(--space-xs) 0 0;
    color: var(--color-text-tertiary);
    font-size: var(--font-size-sm);
    font-style: italic;
  }

  h2 {
    margin: 0 0 var(--space-sm);
    font-size: var(--font-size-lg);
    font-weight: var(--font-weight-semibold);
    color: var(--color-text);
  }

  .subtitle {
    margin: 0 0 var(--space-xl);
    color: var(--color-text-secondary);
    font-size: var(--font-size-sm);
  }

  .button-row {
    display: flex;
    gap: var(--space-md);
    justify-content: center;
    margin-bottom: var(--space-lg);
  }

  .action-button {
    flex: 1;
    padding: var(--space-md) var(--space-lg);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-md);
    background: var(--color-bg);
    color: var(--color-text);
    font: inherit;
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-medium);
    cursor: pointer;
    transition: all var(--transition-fast);
    white-space: nowrap;
  }

  .action-button:hover {
    background: var(--color-bg-tertiary);
    border-color: var(--color-accent-muted, var(--color-accent));
  }

  .action-button:active {
    transform: scale(0.98);
  }

  .selected-path {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-xs);
    margin-bottom: var(--space-md);
    padding: var(--space-md);
    background: var(--color-bg-secondary);
    border-radius: var(--radius-md);
    border: 1px solid var(--color-border);
  }

  .path-label {
    font-size: var(--font-size-xs);
    color: var(--color-text-tertiary);
    font-weight: var(--font-weight-medium);
  }

  .path-value {
    font-size: var(--font-size-xs);
    color: var(--color-text-secondary);
    word-break: break-all;
    line-height: 1.4;
    text-align: center;
  }

  .error {
    color: var(--color-error);
    margin: 0 0 var(--space-md);
    font-size: var(--font-size-sm);
  }

  .warning {
    color: var(--color-warning, #c97d00);
    margin: 0 0 var(--space-md);
    font-size: var(--font-size-sm);
    line-height: 1.4;
  }

  .actions {
    margin-top: var(--space-xl);
  }

  .continue-button {
    padding: var(--space-md) var(--space-2xl);
    border: none;
    border-radius: var(--radius-md);
    background: var(--color-accent);
    color: var(--color-text-on-accent);
    font: inherit;
    font-weight: var(--font-weight-medium);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .continue-button:hover:not(:disabled) {
    background: var(--color-accent-hover);
  }

  .continue-button:active:not(:disabled) {
    transform: scale(0.98);
  }

  .continue-button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
