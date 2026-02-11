<script>
  import { tauri_invoke } from '../lib/tauri_bridge.js';

  let { db_path = '', on_change_db_path } = $props();

  const SYNC_INTERVAL_OPTIONS = [
    { label: '1 minute', value: 60 },
    { label: '5 minutes', value: 5 * 60 },
    { label: '15 minutes', value: 15 * 60 },
    { label: '30 minutes', value: 30 * 60 },
    { label: '1 hour', value: 60 * 60 },
    { label: '2 hours', value: 2 * 60 * 60 },
    { label: '6 hours', value: 6 * 60 * 60 },
  ];
  const DEFAULT_SYNC_INTERVAL_SECS = 5 * 60;
  const LOCAL_STORAGE_KEY_SYNC_INTERVAL = 'sync_interval_secs';

  let autostart_enabled = $state(false);
  let autostart_loading = $state(true);
  let autostart_busy = $state(false);
  let error_message = $state('');

  let sync_interval_secs = $state(DEFAULT_SYNC_INTERVAL_SECS);

  // Load settings on mount
  $effect(() => {
    void load_autostart_state();
    void load_sync_interval();
  });

  async function load_autostart_state() {
    autostart_loading = true;
    try {
      autostart_enabled = await tauri_invoke('autostart_is_enabled');
    } catch {
      autostart_enabled = false;
    } finally {
      autostart_loading = false;
    }
  }

  async function load_sync_interval() {
    // Read from localStorage first, then push to backend.
    try {
      const stored = localStorage.getItem(LOCAL_STORAGE_KEY_SYNC_INTERVAL);
      if (stored) {
        const parsed = parseInt(stored, 10);
        if (!isNaN(parsed) && parsed >= 60) {
          sync_interval_secs = parsed;
        }
      }
    } catch {
      // localStorage unavailable
    }

    try {
      await tauri_invoke('set_sync_interval', { intervalSecs: sync_interval_secs });
    } catch {
      // ignore
    }
  }

  async function toggle_autostart() {
    error_message = '';
    autostart_busy = true;

    const new_enabled = !autostart_enabled;

    try {
      await tauri_invoke('autostart_set_enabled', { enabled: new_enabled });
      autostart_enabled = new_enabled;
    } catch (err) {
      error_message = err instanceof Error ? err.message : String(err);
    } finally {
      autostart_busy = false;
    }
  }

  /**
   * Handle sync interval change from the select dropdown.
   * @param {Event} event
   */
  async function handle_sync_interval_change(event) {
    const new_value = parseInt(/** @type {HTMLSelectElement} */ (event.target).value, 10);
    if (isNaN(new_value)) return;

    sync_interval_secs = new_value;

    try {
      localStorage.setItem(LOCAL_STORAGE_KEY_SYNC_INTERVAL, String(new_value));
    } catch {
      // localStorage unavailable
    }

    try {
      await tauri_invoke('set_sync_interval', { intervalSecs: new_value });
    } catch (err) {
      error_message = err instanceof Error ? err.message : String(err);
    }
  }
</script>

<div class="general-settings">
  <h1 class="title">General</h1>

  <section class="section">
    <h2 class="section-title">DATABASE</h2>

    <div class="db-path-row">
      <div class="db-path-info">
        <span class="setting-label">Archive location</span>
        <code class="db-path-value">{db_path || '(not set)'}</code>
      </div>
      <button class="btn btn-secondary btn-sm" onclick={on_change_db_path}>
        Change
      </button>
    </div>
  </section>

  <section class="section">
    <h2 class="section-title">SYNC</h2>

    <div class="setting-item-row">
      <div class="setting-content">
        <span class="setting-label">Sync interval</span>
        <span class="setting-description">
          How often to check for new emails in the background.
        </span>
      </div>
      <select
        class="interval-select"
        value={sync_interval_secs}
        onchange={handle_sync_interval_change}
      >
        {#each SYNC_INTERVAL_OPTIONS as option (option.value)}
          <option value={option.value}>{option.label}</option>
        {/each}
      </select>
    </div>
    <p class="sync-hint">
      Set this to a shorter interval than your email client's check frequency.
      For example, if Apple Mail checks every 5 minutes, set the archive
      interval to 1 minute. This ensures emails are archived before they can
      be read and deleted.
    </p>
  </section>

  <section class="section">
    <h2 class="section-title">STARTUP</h2>

    <label class="setting-item">
      <input
        type="checkbox"
        checked={autostart_enabled}
        disabled={autostart_loading || autostart_busy}
        onchange={toggle_autostart}
      />
      <div class="setting-content">
        <span class="setting-label">Launch at login</span>
        <span class="setting-description">
          Automatically start Amberize when you log in to your computer.
        </span>
      </div>
    </label>
  </section>

  {#if error_message}
    <p class="error">{error_message}</p>
  {/if}
</div>

<style>
  .general-settings {
    max-width: 500px;
  }

  .title {
    margin: 0 0 var(--space-xl);
    font-size: var(--font-size-lg);
    font-weight: var(--font-weight-semibold);
    color: var(--color-text);
  }

  .section {
    margin-bottom: var(--space-xl);
  }

  .section-title {
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-semibold);
    color: var(--color-text-tertiary);
    letter-spacing: 0.04em;
    text-transform: uppercase;
    margin: 0 0 var(--space-md);
    padding-bottom: var(--space-sm);
    border-bottom: 1px solid var(--color-border);
  }

  .db-path-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-md);
    padding: var(--space-sm) 0;
  }

  .db-path-info {
    display: flex;
    flex-direction: column;
    gap: var(--space-xs);
    min-width: 0;
  }

  .db-path-value {
    font-size: var(--font-size-xs);
    color: var(--color-text-secondary);
    word-break: break-all;
    line-height: 1.4;
    background: var(--color-bg-secondary);
    padding: 2px 6px;
    border-radius: var(--radius-sm);
  }

  .setting-item {
    display: flex;
    align-items: flex-start;
    gap: var(--space-md);
    cursor: pointer;
    padding: var(--space-sm) 0;
    border-radius: var(--radius-sm);
  }

  .setting-item input {
    margin-top: 2px;
    cursor: pointer;
  }

  .setting-item input:disabled {
    cursor: not-allowed;
  }

  .setting-item-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-md);
    padding: var(--space-sm) 0;
  }

  .setting-content {
    display: flex;
    flex-direction: column;
    gap: var(--space-xs);
  }

  .setting-label {
    font-weight: var(--font-weight-medium);
    font-size: var(--font-size-sm);
    color: var(--color-text);
  }

  .setting-description {
    color: var(--color-text-secondary);
    font-size: var(--font-size-xs);
    line-height: 1.4;
  }

  .sync-hint {
    margin: var(--space-sm) 0 0;
    color: var(--color-text-tertiary);
    font-size: var(--font-size-xs);
    line-height: 1.5;
  }

  .interval-select {
    padding: 5px 8px;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: var(--color-text);
    font: inherit;
    font-size: var(--font-size-xs);
    cursor: pointer;
    flex-shrink: 0;
  }

  .interval-select:hover {
    border-color: var(--color-accent);
  }

  .interval-select:focus {
    outline: none;
    border-color: var(--color-accent);
    box-shadow: 0 0 0 2px var(--color-accent-soft, rgba(0, 122, 255, 0.15));
  }

  .btn {
    cursor: pointer;
    border: none;
    border-radius: var(--radius-sm);
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-medium);
    padding: 6px 12px;
    transition: background 0.15s;
    white-space: nowrap;
    flex-shrink: 0;
  }

  .btn-secondary {
    background: var(--color-bg-secondary);
    color: var(--color-text);
    border: 1px solid var(--color-border);
  }

  .btn-secondary:hover {
    background: var(--color-bg-tertiary, var(--color-border));
  }

  .error {
    color: var(--color-error);
    margin: var(--space-md) 0;
    font-size: var(--font-size-sm);
  }
</style>
