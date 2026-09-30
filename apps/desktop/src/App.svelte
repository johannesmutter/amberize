<script>
  import ArchiveLocationScreen from './components/ArchiveLocationScreen.svelte';
  import MainDashboard from './components/MainDashboard.svelte';
  import SettingsPage from './components/SettingsPage.svelte';
  import { load_config } from './lib/config.js';
  import { tauri_invoke, listen_scoped, tauri_save_dialog, tauri_check_update, tauri_restart_app } from './lib/tauri_bridge.js';

  let config = $state(null);
  let config_loading = $state(true);
  let startup_error = $state('');
  let startup_warning = $state('');
  let listeners_ready = $state(false);

  /** @type {'archive-location' | 'dashboard' | 'settings'} */
  let current_page = $state('archive-location');

  /** @type {string | null} */
  let settings_section = $state(null);
  /** @type {number | null} */
  let settings_account_id = $state(null);
  let settings_navigation_nonce = $state(0);

  // Update check state
  /** @type {{ version: string, body: string | null, download_and_install: () => Promise<void> } | null} */
  let available_update = $state(null);
  let update_checking = $state(false);
  /** @type {string | null} */
  let update_message = $state(null);
  let update_installing = $state(false);
  let update_restart_ready = $state(false);
  let dashboard_action_nonce = $state(0);
  let dashboard_action_type = $state('');
  /** @type {{ ok: boolean, issues: string[] } | null} */
  let integrity_status = $state(null);
  let export_operation_id = $state('');
  let export_choosing = $state(false);
  let export_cancel_requested = $state(false);
  let export_message = $state('');
  let disposed = false;
  $effect(() => () => {
    disposed = true;
    if (export_operation_id) void tauri_invoke('cancel_auditor_export', {operationId:export_operation_id}).catch(() => {});
  });

  async function restore_archive() {
    config_loading = true; startup_error = '';
    try {
      config = await load_config();
      current_page = config?.db_path ? 'dashboard' : 'archive-location';
      if (config?.db_path) await tauri_invoke('set_active_db_path', {dbPath:config.db_path});
      startup_warning = await tauri_invoke('get_startup_warning') ?? '';
      if (config?.db_path) await refresh_integrity();
    } catch (err) { startup_error = err instanceof Error ? err.message : String(err); }
    finally { config_loading = false; }
  }
  $effect(() => { void restore_archive(); });
  $effect(() => listen_scoped({
    menu_open_settings: () => handle_open_settings(),
    menu_check_updates: () => void handle_check_for_updates(),
    tray_export_auditor: () => void handle_tray_export_auditor(),
    tray_documentation: () => void handle_tray_documentation(),
    integrity_status_updated: () => void refresh_integrity(),
  }, () => { listeners_ready = true; }));
  $effect(() => { if (listeners_ready && !config_loading) void tauri_invoke('frontend_ready').catch(() => {}); });
  async function refresh_integrity() {
    try {
      integrity_status = await tauri_invoke('get_integrity_status');
      startup_warning = await tauri_invoke('get_startup_warning') ?? '';
    } catch (err) { startup_warning = String(err); }
  }

  // Auto-check for updates on launch (non-blocking, silent on no-update)
  $effect(() => {
    void (async () => {
      try {
        const update = await tauri_check_update();
        if (update) {
          available_update = update;
        }
      } catch {
        // silently ignore — user can manually check via menu
      }
    })();
  });

  async function handle_check_for_updates() {
    if (update_checking || update_installing) return;
    update_checking = true;
    update_message = null;

    try {
      const update = await tauri_check_update();
      if (update) {
        available_update = update;
        update_message = null;
        update_restart_ready = false;
      } else {
        available_update = null;
        update_message = 'You are running the latest version.';
        update_restart_ready = false;
      }
    } catch (err) {
      update_message = err instanceof Error ? err.message : String(err);
    } finally {
      update_checking = false;
    }
  }

  async function handle_install_update() {
    if (!available_update || update_installing) return;
    update_installing = true;
    update_message = 'Downloading update…';
    const installed_version = available_update.version;

    try {
      await available_update.download_and_install();
      // Keep UI state consistent: install can succeed even if this process
      // has not restarted yet.
      available_update = null;
      update_message = `Update installed (v${installed_version}). Please restart Amberize to finish.`;
      update_restart_ready = true;
    } catch (err) {
      update_message = err instanceof Error ? err.message : String(err);
      update_restart_ready = false;
      console.error('Failed to install update:', err);
    } finally {
      update_installing = false;
    }
  }

  async function handle_restart_after_update() {
    if (!update_restart_ready || update_installing) return;
    update_message = 'Restarting app…';
    try {
      await tauri_restart_app();
    } catch (err) {
      update_message = err instanceof Error ? err.message : String(err);
    }
  }

  function dismiss_update() {
    available_update = null;
    update_message = null;
    update_restart_ready = false;
  }

  /**
   * @returns {string}
   */
  function get_configured_db_path() {
    const db_path = config?.db_path?.trim() ?? '';
    return db_path;
  }

  async function handle_tray_documentation() {
    const db_path = get_configured_db_path();
    if (!db_path) {
      current_page = 'archive-location';
      return;
    }

    try {
      await tauri_invoke('open_documentation', { dbPath: db_path });
    } catch (err) {
      console.error('Failed to open documentation:', err);
    }
  }

  async function handle_tray_export_auditor() {
    if (export_operation_id || export_choosing) return;
    const db_path = get_configured_db_path();
    if (!db_path) {
      current_page = 'archive-location';
      return;
    }

    const date_stamp = new Date().toISOString().slice(0, 10);
    const default_filename = `amberize-auditor-package-${date_stamp}.zip`;

    /** @type {string | null} */
    let output_zip_path = null;
    export_choosing = true;
    try {
      output_zip_path = await tauri_save_dialog({
        title: 'Export auditor package',
        defaultPath: default_filename,
        filters: [{ name: 'ZIP archive', extensions: ['zip'] }],
      });
    } catch (err) {
      export_message = err instanceof Error ? err.message : String(err);
      return;
    } finally {
      export_choosing = false;
    }

    if (!output_zip_path || disposed) return;
    const operation_id = crypto.randomUUID();
    export_operation_id = operation_id;
    export_cancel_requested = false;
    export_message = 'Exporting auditor package…';

    try {
      await tauri_invoke('export_auditor_package', {
        dbPath: db_path,
        outputZipPath: output_zip_path,
        operationId: operation_id,
      });
      export_message = 'Auditor package exported.';
    } catch (err) {
      export_message = err instanceof Error ? err.message : String(err);
    } finally {
      export_operation_id = '';
      export_cancel_requested = false;
    }
  }

  async function cancel_package_export() {
    const operation_id = export_operation_id;
    if (!operation_id || export_cancel_requested) return;
    export_cancel_requested = true;
    try {
      const published = await tauri_invoke('cancel_auditor_export', {operationId:operation_id});
      if (export_operation_id === operation_id) export_message = published ? 'Export saved. Finishing…' : 'Canceling export…';
    } catch (err) {
      if (export_operation_id === operation_id) {
        export_cancel_requested = false;
        export_message = `Could not cancel export: ${err instanceof Error ? err.message : String(err)}`;
      }
    }
  }

  async function handle_tray_sync_now() {
    const db_path = get_configured_db_path();
    if (!db_path) {
      current_page = 'archive-location';
      return;
    }

    current_page = 'dashboard';

    try {
      await tauri_invoke('sync_all_accounts_command', { dbPath: db_path });
    } catch (err) {
      console.error('Failed to start sync:', err);
    }
  }

  function handle_global_keydown(event) {
    if (current_page !== 'dashboard') return;
    if (event.defaultPrevented) return;

    const has_cmd_or_ctrl = event.metaKey || event.ctrlKey;
    if (!has_cmd_or_ctrl) return;

    const normalized_key = event.key.toLowerCase();

    if (normalized_key === 'k') {
      event.preventDefault();
      dashboard_action_nonce += 1;
      dashboard_action_type = 'focus_search';
      return;
    }

    if (normalized_key === 'r') {
      event.preventDefault();
      dashboard_action_nonce += 1;
      dashboard_action_type = 'sync_now';
      return;
    }

    if (normalized_key === ',') {
      event.preventDefault();
      handle_open_settings();
    }
  }

  /**
   * @param {string} db_path
   */
  async function handle_archive_selected(db_path, create = false) {
    const saved = await tauri_invoke('select_archive', {dbPath:db_path.trim(),create});
    config = saved; startup_error = ''; startup_warning = ''; integrity_status = null;
    current_page = 'dashboard'; await refresh_integrity();
  }

  /**
   * @param {string} [section]
   * @param {number} [account_id]
   */
  function handle_open_settings(section = null, account_id = null) {
    settings_navigation_nonce++;
    settings_section = config?.db_path ? section : 'general';
    settings_account_id = account_id;
    current_page = 'settings';
  }

  function handle_close_settings() {
    current_page = config?.db_path && !startup_error ? 'dashboard' : 'archive-location';
    settings_section = null;
    settings_account_id = null;
  }

  function handle_change_db_path() {
    current_page = 'archive-location';
    settings_section = null;
    settings_account_id = null;
    integrity_status = null;
  }

  function handle_start_sync(account_id) {
    // Navigate back to dashboard and start sync
    current_page = 'dashboard';
    void start_sync_for_account(account_id);
  }

  async function start_sync_for_account(account_id) {
    const db_path = config?.db_path?.trim() ?? '';
    if (!db_path) return;
    if (typeof account_id !== 'number') return;

    try {
      await tauri_invoke('sync_account_once_command', {
        dbPath: db_path,
        accountId: account_id,
      });
    } catch {
      // ignore — the dashboard status bar will show errors on manual sync
    }
  }

  $effect(() => { if (config?.db_path && current_page === 'dashboard') void refresh_integrity(); });
</script>

<svelte:window onkeydown={handle_global_keydown} />

<div class="app-titlebar" data-tauri-drag-region>
  <span class="app-titlebar-text" data-tauri-drag-region>Amberize — Seal your emails in time</span>
</div>

{#if available_update}
  <div class="update-banner">
    <span class="update-banner-text">Update available: v{available_update.version}</span>
    {#if update_message}
      <span class="update-banner-text">- {update_message}</span>
    {/if}
    <button type="button" class="update-banner-action" onclick={handle_install_update} disabled={update_installing}>
      {update_installing ? 'Installing…' : 'Install & Restart'}
    </button>
    <button type="button" class="update-banner-dismiss" onclick={dismiss_update} aria-label="Dismiss">
      ×
    </button>
  </div>
{:else if update_message}
  <div class="update-banner update-banner-info">
    <span class="update-banner-text">{update_message}</span>
    {#if update_restart_ready}
      <button type="button" class="update-banner-action" onclick={handle_restart_after_update}>
        Restart now
      </button>
    {/if}
    <button type="button" class="update-banner-dismiss" onclick={dismiss_update} aria-label="Dismiss">
      ×
    </button>
  </div>
{/if}

{#if startup_warning}<div class="update-banner update-banner-warning" role="status">{startup_warning}</div>{/if}
{#if export_message}
  <div class="update-banner update-banner-info" role="status">
    <span class="update-banner-text">{export_message}</span>
    {#if export_operation_id}<button type="button" class="update-banner-action" disabled={export_cancel_requested} onclick={cancel_package_export}>Cancel export</button>
    {:else}<button type="button" class="update-banner-dismiss" aria-label="Dismiss export status" onclick={() => {export_message = '';}}>×</button>{/if}
  </div>
{/if}

{#if integrity_status && !integrity_status.ok && integrity_status.issues?.length}
  <div class="update-banner update-banner-warning">
    <span class="update-banner-text">
      Archive integrity warning detected.
      {integrity_status.issues?.[0] ?? 'Please inspect diagnostics and restore from backup if needed.'}
    </span>
    <button type="button" class="update-banner-action" onclick={() => handle_open_settings('diagnostics')}>
      Open Diagnostics
    </button>
    <button type="button" class="update-banner-dismiss" onclick={() => { integrity_status = null; }} aria-label="Dismiss">
      ×
    </button>
  </div>
{/if}

{#if config_loading}
  <div class="boot-state">Loading…</div>
{:else if startup_error && current_page !== 'settings'}
  <div class="boot-state" role="alert">
    <p>Amberize could not restore your archive.</p><p>{startup_error}</p>
    {#if config?.db_path}<code>{config.db_path}</code>{/if}
    <p>Reconnect the archive's drive, then retry, or choose its existing file. Your saved location has been kept.</p>
    <button onclick={restore_archive}>Retry saved archive</button>
    <button onclick={() => { startup_error = ''; handle_change_db_path(); }}>Choose archive</button>
  </div>
{:else if current_page === 'archive-location'}
  <ArchiveLocationScreen on_continue={handle_archive_selected} />
{:else if current_page === 'settings'}
  <SettingsPage
    db_path={config?.db_path ?? ''}
    initial_section={settings_section}
    initial_account_id={settings_account_id}
    navigation_nonce={settings_navigation_nonce}
    on_back={handle_close_settings}
    on_start_sync={handle_start_sync}
    on_change_db_path={handle_change_db_path}
  />
{:else}
  <MainDashboard
    db_path={config?.db_path ?? ''}
    on_open_settings={handle_open_settings}
    dashboard_action_nonce={dashboard_action_nonce}
    dashboard_action_type={dashboard_action_type}
  />
{/if}

<style>
  .app-titlebar {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    height: var(--titlebar-height);
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--color-bg);
    border-bottom: 1px solid var(--color-border);
    z-index: 100;
    user-select: none;
    -webkit-user-select: none;
    -webkit-app-region: drag;
  }

  .app-titlebar-text {
    font-size: var(--font-size-xs);
    color: var(--color-text-tertiary);
    letter-spacing: 0.01em;
    -webkit-app-region: drag;
  }

  .update-banner {
    position: fixed;
    top: var(--titlebar-height);
    left: 0;
    right: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-md);
    padding: var(--space-xs) var(--space-lg);
    background: var(--color-accent);
    color: var(--color-text-on-accent);
    font-size: var(--font-size-xs);
    z-index: 99;
  }

  .update-banner-info {
    background: var(--color-bg-secondary);
    color: var(--color-text-secondary);
    border-bottom: 1px solid var(--color-border);
  }

  .update-banner-warning {
    background: color-mix(in srgb, var(--color-warning, #d97706) 20%, var(--color-bg));
    color: var(--color-text);
    border-bottom: 1px solid var(--color-border);
  }

  .update-banner-text {
    font-weight: var(--font-weight-medium);
  }

  .update-banner-action {
    padding: 2px var(--space-sm);
    border: 1px solid currentColor;
    border-radius: var(--radius-sm);
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-medium);
    cursor: pointer;
    transition: opacity var(--transition-fast);
  }

  .update-banner-action:hover:not(:disabled) {
    opacity: 0.8;
  }

  .update-banner-action:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .update-banner-dismiss {
    background: none;
    border: none;
    color: inherit;
    font-size: 16px;
    cursor: pointer;
    padding: 0 var(--space-xs);
    line-height: 1;
    opacity: 0.7;
    transition: opacity var(--transition-fast);
  }

  .update-banner-dismiss:hover {
    opacity: 1;
  }

  .boot-state {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: calc(100vh - var(--titlebar-height));
    padding-top: var(--titlebar-height);
    color: var(--color-text-tertiary);
    font-size: var(--font-size-sm);
  }
</style>
