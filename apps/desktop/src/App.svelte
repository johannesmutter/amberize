<script>
  import ArchiveLocationScreen from './components/ArchiveLocationScreen.svelte';
  import MainDashboard from './components/MainDashboard.svelte';
  import SettingsPage from './components/SettingsPage.svelte';
  import { load_config } from './lib/config.js';
  import { tauri_invoke, listen_scoped, tauri_open_dialog, tauri_save_dialog, tauri_check_update, tauri_restart_app } from './lib/tauri_bridge.js';

  let config = $state(null);
  let config_loading = $state(true);
  let startup_error = $state('');
  let startup_warning = $state('');
  let dismissed_startup_warning = $state('');
  let recovery_selecting = $state(false);
  let recovery_selection_error = $state('');
  let banner_height = $state(0);
  let integrity_generation = 0;
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
    integrity_generation++;
    config_loading = true; startup_error = '';
    try {
      config = await load_config();
      if (config?.db_path) await tauri_invoke('set_active_db_path', {dbPath:config.db_path});
      startup_warning = await tauri_invoke('get_startup_warning') ?? '';
      if (config?.db_path) await refresh_integrity();
      current_page = config?.db_path ? 'dashboard' : 'archive-location';
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
    const generation = ++integrity_generation;
    const path = config?.db_path;
    try {
      const [status, warning] = await Promise.all([
        tauri_invoke('get_integrity_status'), tauri_invoke('get_startup_warning'),
      ]);
      if (generation !== integrity_generation || disposed || path !== config?.db_path) return;
      integrity_status = status;
      startup_warning = warning ?? '';
    } catch (err) {
      if (generation === integrity_generation && !disposed && path === config?.db_path) startup_warning = String(err);
    }
  }

  async function dismiss_startup_warning() {
    const warning = startup_warning;
    dismissed_startup_warning = warning;
    // Locally dismiss immediately; matching protects a newer backend warning.
    await tauri_invoke('dismiss_startup_warning', {warning}).catch(() => {});
  }

  async function choose_recovery_archive() {
    if (recovery_selecting) return;
    recovery_selecting = true;
    recovery_selection_error = '';
    try {
      const path = await tauri_open_dialog({
        title: 'Choose your existing archive', multiple: false, directory: false,
        filters: [{name: 'SQLite archive', extensions: ['sqlite3', 'sqlite', 'db']}],
      });
      if (path) await handle_archive_selected(path, false);
    } catch (err) {
      recovery_selection_error = err instanceof Error ? err.message : String(err);
    } finally { recovery_selecting = false; }
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
    integrity_generation++;
    config = saved; startup_error = ''; startup_warning = ''; integrity_status = null;
    dismissed_startup_warning = ''; recovery_selection_error = '';
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

</script>

<svelte:window onkeydown={handle_global_keydown} />

<div class="app-titlebar" data-tauri-drag-region>
  <span class="app-titlebar-text" data-tauri-drag-region>Amberize — Seal your emails in time</span>
</div>

<div class="app-shell" style:--banner-height={`${banner_height > 0 ? banner_height + 1 : 0}px`}>
<div class="banner-stack" bind:clientHeight={banner_height}>
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

{#if startup_warning && startup_warning !== dismissed_startup_warning && !startup_error && !config_loading}
  <div class="update-banner update-banner-warning" role="status">
    <span class="update-banner-text">{startup_warning}</span>
    <button type="button" class="update-banner-dismiss" onclick={dismiss_startup_warning} aria-label="Dismiss archive warning">×</button>
  </div>
{/if}
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
</div>

{#if config_loading}
  <div class="boot-state" role="status"><span class="loading-spinner" aria-hidden="true"></span>Opening your archive…</div>
{:else if startup_error && current_page !== 'settings'}
  <div class="boot-state">
    <section class="archive-recovery" aria-labelledby="recovery-title">
      <h1 id="recovery-title">{config?.db_path ? 'Your archive is unavailable' : 'Your archive settings could not be loaded'}</h1>
      <p>{config?.db_path ? 'If you moved the archive, choose its existing file at the new location. If its drive is disconnected, reconnect it and retry.' : 'Retry loading your settings, or choose your existing archive.'}</p>
      {#if config?.db_path}
        <div class="saved-location"><span>Saved archive location</span><code>{config.db_path}</code></div>
      {/if}
      <p class="recovery-note">Your saved location will be kept until an archive opens successfully.</p>
      <div class="recovery-actions">
        <button class="primary" onclick={choose_recovery_archive} disabled={recovery_selecting}>{recovery_selecting ? 'Opening archive…' : 'Choose existing archive'}</button>
        <button onclick={restore_archive} disabled={recovery_selecting}>Retry saved archive</button>
      </div>
      {#if recovery_selection_error}<p class="recovery-error" role="alert">{recovery_selection_error}</p>{/if}
      <details><summary>Technical details</summary><p>{startup_error}</p></details>
    </section>
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
  {#key config?.db_path}
  <MainDashboard
    db_path={config?.db_path ?? ''}
    on_open_settings={handle_open_settings}
    dashboard_action_nonce={dashboard_action_nonce}
    dashboard_action_type={dashboard_action_type}
  />
  {/key}
{/if}
</div>

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

  .banner-stack {
    position: fixed;
    top: var(--titlebar-height);
    left: 0;
    right: 0;
    z-index: 99;
  }

  .update-banner {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-md);
    padding: var(--space-xs) var(--space-lg);
    background: var(--color-accent);
    color: var(--color-text-on-accent);
    font-size: var(--font-size-xs);
    flex-wrap: wrap;
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
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
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
    min-width: 40px;
    min-height: 40px;
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
    flex-direction: column;
    gap: var(--space-md);
    align-items: center;
    justify-content: center;
    min-height: 100vh;
    padding: calc(var(--titlebar-height) + var(--banner-height, 0px) + var(--space-xl)) var(--space-lg) var(--space-xl);
    color: var(--color-text-secondary);
    font-size: var(--font-size-sm);
  }

  .archive-recovery { width: 100%; max-width: 520px; line-height: 1.6; }
  .archive-recovery h1 { margin: 0 0 var(--space-md); color: var(--color-text); font-size: var(--font-size-xl); text-wrap: balance; }
  .archive-recovery p { margin: 0 0 var(--space-lg); }
  .saved-location { padding: var(--space-md); margin-bottom: var(--space-md); background: var(--color-bg-secondary); border: 1px solid var(--color-border); border-radius: var(--radius-md); }
  .saved-location span { display: block; margin-bottom: var(--space-xs); font-size: var(--font-size-xs); }
  .saved-location code { display: block; color: var(--color-text); overflow-wrap: anywhere; }
  .recovery-note { font-size: var(--font-size-xs); }
  .recovery-actions { display: flex; flex-wrap: wrap; gap: var(--space-sm); margin-bottom: var(--space-lg); }
  .recovery-actions button { min-height: 40px; }
  .archive-recovery details { overflow-wrap: anywhere; font-size: var(--font-size-xs); }
  .archive-recovery summary { cursor: pointer; }
  .archive-recovery details p { margin-top: var(--space-sm); }
  .recovery-error { color: var(--color-error); }
</style>
