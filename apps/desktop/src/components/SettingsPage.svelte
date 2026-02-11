<script>
  import { tauri_invoke } from '../lib/tauri_bridge.js';
  import AccountSettings from './AccountSettings.svelte';
  import AddAccountForm from './AddAccountForm.svelte';
  import GeneralSettings from './GeneralSettings.svelte';
  import EmptyState from './EmptyState.svelte';

  let { db_path, initial_section = 'accounts', initial_account_id = null, on_back, on_start_sync, on_change_db_path } = $props();

  /** @type {'accounts' | 'general' | 'diagnostics' | 'activity-log'} */
  let active_section = $state(
    initial_section === 'general' ? 'general'
    : initial_section === 'diagnostics' ? 'diagnostics'
    : initial_section === 'activity-log' ? 'activity-log'
    : 'accounts'
  );

  /** @type {any[]} */
  let accounts = $state([]);

  /** @type {number | 'new' | null} */
  let selected_account_id = $state(initial_account_id);

  let adding_account = $state(false);

  // Load accounts on mount
  $effect(() => {
    void load_accounts();
  });

  async function load_accounts() {
    if (!db_path?.trim()) return;
    try {
      accounts = await tauri_invoke('list_accounts', { dbPath: db_path });
      // Select first active account if none selected
      const active = accounts.filter(a => !a.disabled);
      if (selected_account_id === null && active.length > 0 && active_section === 'accounts') {
        selected_account_id = active[0].id;
      }
    } catch {
      accounts = [];
    }
  }

  function handle_add_account_click() {
    adding_account = true;
    selected_account_id = 'new';
  }

  function handle_account_added(account) {
    adding_account = false;
    void load_accounts();
    selected_account_id = account.id;
    // Start sync for the new account
    on_start_sync?.(account.id);
  }

  function handle_cancel_add() {
    adding_account = false;
    if (accounts.length > 0) {
      selected_account_id = accounts[0].id;
    } else {
      selected_account_id = null;
    }
  }

  async function handle_account_removed(account_id) {
    await load_accounts();
    // Filter out the removed account and select the first remaining
    const remaining = accounts.filter(a => a.id !== account_id && !a.disabled);
    selected_account_id = remaining[0]?.id ?? null;
  }

  function handle_select_account(account_id) {
    adding_account = false;
    selected_account_id = account_id;
  }

  function handle_select_general() {
    active_section = 'general';
    adding_account = false;
    selected_account_id = null;
  }

  function handle_select_diagnostics() {
    active_section = 'diagnostics';
    adding_account = false;
    selected_account_id = null;
    void run_diagnostics();
  }

  function handle_select_activity_log() {
    active_section = 'activity-log';
    adding_account = false;
    selected_account_id = null;
    if (log_events.length === 0) {
      void load_events();
    }
  }

  /** @type {any | null} */
  let diagnostic_data = $state(null);
  let diagnostic_loading = $state(false);
  let diagnostic_error = $state('');
  let app_version = $state('');

  // Load app version once on mount.
  $effect(() => {
    void (async () => {
      try {
        const { getVersion } = await import('@tauri-apps/api/app');
        app_version = await getVersion();
      } catch {
        app_version = 'unknown';
      }
    })();
  });

  async function run_diagnostics() {
    if (!db_path?.trim()) return;
    diagnostic_loading = true;
    diagnostic_error = '';
    diagnostic_data = null;
    try {
      diagnostic_data = await tauri_invoke('diagnose_database', { dbPath: db_path });
    } catch (err) {
      diagnostic_error = err instanceof Error ? err.message : String(err);
    } finally {
      diagnostic_loading = false;
    }
  }

  async function handle_reset_cursors(account_id) {
    if (!db_path?.trim()) return;
    try {
      const updated = await tauri_invoke('reset_mailbox_cursors', { dbPath: db_path, accountId: account_id });
      diagnostic_error = '';
      // Refresh diagnostics to show the reset state
      await run_diagnostics();
    } catch (err) {
      diagnostic_error = err instanceof Error ? err.message : String(err);
    }
  }

  function copy_diagnostic_json() {
    if (!diagnostic_data) return;
    const json = JSON.stringify(diagnostic_data, null, 2);
    navigator.clipboard.writeText(json).catch(() => {});
  }

  // ── Activity Log state ──────────────────────────────────────────
  const EVENTS_PAGE_SIZE = 100;
  const LOG_ROW_HEIGHT = 44;
  const LOG_BUFFER_COUNT = 10;

  /** @type {any[]} */
  let log_events = $state([]);
  let log_loading = $state(false);
  let log_error = $state('');
  let log_has_more = $state(false);
  let log_offset = $state(0);
  let log_total_count = $state(0);
  /** @type {string} */
  let log_kind_filter = $state('');
  let log_exporting = $state(false);

  // Virtual scroll state
  let log_scroll_top = $state(0);
  let log_container_height = $state(400);
  let log_container_el = $state(null);

  $effect(() => {
    if (!log_container_el) return;
    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        log_container_height = entry.contentRect.height;
      }
    });
    observer.observe(log_container_el);
    log_container_height = log_container_el.clientHeight;
    return () => observer.disconnect();
  });

  let log_visible_range = $derived.by(() => {
    const start = Math.max(0, Math.floor(log_scroll_top / LOG_ROW_HEIGHT) - LOG_BUFFER_COUNT);
    const count = Math.ceil(log_container_height / LOG_ROW_HEIGHT) + LOG_BUFFER_COUNT * 2;
    const end = Math.min(log_events.length, start + count);
    return { start, end };
  });

  let log_visible_items = $derived.by(() => {
    const { start, end } = log_visible_range;
    return log_events.slice(start, end).map((event, i) => ({
      event,
      top: (start + i) * LOG_ROW_HEIGHT,
    }));
  });

  let log_total_height = $derived(log_events.length * LOG_ROW_HEIGHT);

  function handle_log_scroll(e) {
    log_scroll_top = e.target.scrollTop;
    // Trigger load-more when near the bottom
    if (log_has_more && !log_loading) {
      const remaining = e.target.scrollHeight - (e.target.scrollTop + e.target.clientHeight);
      if (remaining < LOG_ROW_HEIGHT * LOG_BUFFER_COUNT * 2) {
        void load_events(true);
      }
    }
  }

  /** Human-readable labels for event kinds */
  const EVENT_KIND_LABELS = {
    'app_started': 'App Started',
    'coverage_gap': 'Coverage Gap',
    'sync_finished': 'Sync Completed',
    'account_created': 'Account Created',
    'account_removed': 'Account Removed',
    'mailbox_sync_changed': 'Folder Sync Changed',
    'message_eml_exported': 'Message Exported',
    'documentation_generated': 'Docs Generated',
    'auditor_export': 'Auditor Export',
  };

  /** Event kinds that represent warnings or issues */
  const WARNING_KINDS = new Set(['coverage_gap']);

  /**
   * Load events from the backend.
   * @param {boolean} [append=false] - If true, appends to existing list (pagination).
   */
  async function load_events(append = false) {
    if (!db_path?.trim()) return;
    log_loading = true;
    log_error = '';
    try {
      const offset = append ? log_offset : 0;
      const kind_filter = log_kind_filter || null;
      const result = await tauri_invoke('list_events', {
        dbPath: db_path,
        kindFilter: kind_filter,
        limit: EVENTS_PAGE_SIZE,
        offset,
      });
      const events = result.events ?? [];
      log_total_count = result.total_count ?? 0;
      if (append) {
        log_events = [...log_events, ...events];
      } else {
        log_events = events;
      }
      log_offset = (append ? log_offset : 0) + events.length;
      log_has_more = events.length >= EVENTS_PAGE_SIZE;
    } catch (err) {
      log_error = err instanceof Error ? err.message : String(err);
    } finally {
      log_loading = false;
    }
  }

  function handle_log_filter_change() {
    log_events = [];
    log_offset = 0;
    log_has_more = false;
    void load_events();
  }

  /**
   * Export all events as CSV via the backend command.
   */
  async function handle_export_csv() {
    if (!db_path?.trim()) return;
    log_exporting = true;
    log_error = '';
    try {
      // Use Tauri's save dialog to pick output path
      const { save } = await import('@tauri-apps/plugin-dialog');
      const path = await save({
        defaultPath: 'activity-log.csv',
        filters: [{ name: 'CSV', extensions: ['csv'] }],
      });
      if (!path) {
        log_exporting = false;
        return;
      }
      await tauri_invoke('export_events_csv', { dbPath: db_path, outputPath: path });
    } catch (err) {
      log_error = err instanceof Error ? err.message : String(err);
    } finally {
      log_exporting = false;
    }
  }

  /**
   * Format an RFC-3339 timestamp for display.
   * @param {string} iso_str
   * @returns {string}
   */
  function format_event_time(iso_str) {
    try {
      const date = new Date(iso_str);
      return date.toLocaleString(undefined, {
        year: 'numeric',
        month: 'short',
        day: 'numeric',
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit',
      });
    } catch {
      return iso_str;
    }
  }

  /**
   * Return a human-readable label for an event kind.
   * @param {string} kind
   * @returns {string}
   */
  function event_kind_label(kind) {
    return EVENT_KIND_LABELS[kind] ?? kind.replace(/_/g, ' ');
  }

  /**
   * Format an ISO 8601 / RFC 3339 date string into a short human-readable form.
   * @param {string} iso
   * @returns {string}
   */
  function format_short_date(iso) {
    try {
      const d = new Date(iso);
      if (isNaN(d.getTime())) return iso;
      return d.toLocaleString(undefined, {
        month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit',
      });
    } catch {
      return iso;
    }
  }

  /**
   * Format a duration in seconds to a human-friendly string (e.g. "35 min").
   * @param {number} secs
   * @returns {string}
   */
  function format_duration(secs) {
    if (secs < 60) return `${secs}s`;
    const mins = Math.round(secs / 60);
    if (mins < 60) return `${mins} min`;
    const hours = Math.floor(mins / 60);
    const remaining_mins = mins % 60;
    return remaining_mins > 0 ? `${hours}h ${remaining_mins}m` : `${hours}h`;
  }

  /** Keys whose values are RFC 3339 timestamps. */
  const DATE_KEYS = new Set([
    'gap_start', 'gap_end_approx', 'last_heartbeat', 'system_boot_time',
  ]);

  /** Keys whose values are durations in seconds. */
  const DURATION_KEYS = new Set(['gap_seconds']);

  /** Keys to hide from display (internal/redundant). */
  const HIDDEN_KEYS = new Set(['v']);

  /**
   * Parse the JSON detail field and return a compact summary string.
   * Formats dates, durations, and filters internal fields.
   * @param {string | null | undefined} detail
   * @returns {string}
   */
  function format_event_detail(detail) {
    if (!detail) return '';
    try {
      const parsed = JSON.parse(detail);
      return Object.entries(parsed)
        .filter(([key]) => !HIDDEN_KEYS.has(key))
        .map(([key, value]) => {
          const label = key.replace(/_/g, ' ');
          if (DURATION_KEYS.has(key) && typeof value === 'number') {
            return `${label}: ${format_duration(value)}`;
          }
          if (DATE_KEYS.has(key) && typeof value === 'string') {
            return `${label}: ${format_short_date(value)}`;
          }
          return `${label}: ${value}`;
        })
        .join(', ');
    } catch {
      return detail;
    }
  }

  /** @type {string[]} */
  let available_kinds = $derived((() => {
    const kinds = new Set(log_events.map(e => e.kind));
    Object.keys(EVENT_KIND_LABELS).forEach(k => kinds.add(k));
    return [...kinds].sort();
  })());

  function handle_select_accounts() {
    active_section = 'accounts';
    if (accounts.length > 0 && !adding_account) {
      selected_account_id = accounts[0].id;
    }
  }

  let active_accounts = $derived(accounts.filter(a => !a.disabled));

  let selected_account = $derived(
    typeof selected_account_id === 'number'
      ? active_accounts.find(a => a.id === selected_account_id)
      : null
  );
</script>

<div class="settings-page">
  <!-- Sidebar -->
  <nav class="sidebar">
    <div class="sidebar-section sidebar-top">
      <button type="button" class="back-button" onclick={on_back}>
        ← Back to archive
      </button>
    </div>

    <div class="sidebar-divider"></div>

    <div class="sidebar-section">
      <div class="sidebar-title">ACCOUNTS</div>
      <ul class="sidebar-list">
        {#each active_accounts as account (account.id)}
          <li>
            <button
              type="button"
              class="sidebar-item"
              class:active={selected_account_id === account.id && active_section === 'accounts'}
              onclick={() => { handle_select_accounts(); handle_select_account(account.id); }}
            >
              {account.email_address}
            </button>
          </li>
        {/each}
        {#if adding_account}
          <li>
            <button
              type="button"
              class="sidebar-item"
              class:active={selected_account_id === 'new'}
              onclick={() => { selected_account_id = 'new'; }}
            >
              (new account)
            </button>
          </li>
        {/if}
      </ul>
      {#if !adding_account}
        <button type="button" class="add-button" onclick={handle_add_account_click}>
          + Add account
        </button>
      {/if}
    </div>

    <div class="sidebar-divider"></div>

    <div class="sidebar-section">
      <ul class="sidebar-list">
        <li>
          <button
            type="button"
            class="sidebar-item"
            class:active={active_section === 'general'}
            onclick={handle_select_general}
          >
            General
          </button>
        </li>
        <li>
          <button
            type="button"
            class="sidebar-item"
            class:active={active_section === 'diagnostics'}
            onclick={handle_select_diagnostics}
          >
            Diagnostics
          </button>
        </li>
        <li>
          <button
            type="button"
            class="sidebar-item"
            class:active={active_section === 'activity-log'}
            onclick={handle_select_activity_log}
          >
            Activity Log
          </button>
        </li>
      </ul>
    </div>
  </nav>

  <!-- Content -->
  <main class="content">
    {#if active_section === 'activity-log'}
      <div class="log-section">
        <div class="log-header">
          <div>
            <h2>Activity Log</h2>
            <p class="log-subtitle">
              Tamper-evident audit trail. Each event is cryptographically
              chained to the previous one.
              {#if log_total_count > 0}
                <span class="log-count">{log_total_count.toLocaleString()} events total</span>
              {/if}
            </p>
          </div>
        </div>

        <div class="log-controls">
          <div class="log-filter">
            <label for="log-kind-filter">Filter:</label>
            <select
              id="log-kind-filter"
              bind:value={log_kind_filter}
              onchange={handle_log_filter_change}
            >
              <option value="">All events</option>
              {#each available_kinds as kind}
                <option value={kind}>{event_kind_label(kind)}</option>
              {/each}
            </select>
          </div>

          <div class="log-actions">
            <button
              type="button"
              class="log-button"
              onclick={() => load_events()}
              disabled={log_loading}
            >
              {log_loading ? 'Loading…' : 'Refresh'}
            </button>
            <button
              type="button"
              class="log-button"
              onclick={handle_export_csv}
              disabled={log_exporting || log_total_count === 0}
              title="Export all events as CSV for auditors"
            >
              {log_exporting ? 'Exporting…' : 'Export CSV'}
            </button>
          </div>
        </div>

        {#if log_error}
          <div class="log-error">{log_error}</div>
        {/if}

        {#if log_events.length > 0}
          <!-- Sticky header -->
          <div class="log-table-header">
            <span class="log-col-time">Time</span>
            <span class="log-col-kind">Type</span>
            <span class="log-col-detail">Details</span>
            <span class="log-col-hash">Chain Hash</span>
          </div>

          <!-- Virtual-scrolled rows -->
          <div
            class="log-virtual-list"
            bind:this={log_container_el}
            onscroll={handle_log_scroll}
          >
            <div class="log-spacer" style="height: {log_total_height}px;">
              {#each log_visible_items as { event, top } (event.id)}
                <div
                  class="log-row"
                  class:log-row-warning={WARNING_KINDS.has(event.kind)}
                  style="top: {top}px;"
                >
                  <span class="log-col-time log-cell-time">{format_event_time(event.occurred_at)}</span>
                  <span class="log-col-kind log-cell-kind">
                    <span class="log-kind-badge" class:log-kind-warning={WARNING_KINDS.has(event.kind)}>
                      {event_kind_label(event.kind)}
                    </span>
                  </span>
                  <span class="log-col-detail log-cell-detail">
                    {#if event.account_id}<span class="log-meta">Acct #{event.account_id}</span>{/if}
                    {#if event.detail}
                      <span class="log-detail-text" title={format_event_detail(event.detail)}>{format_event_detail(event.detail)}</span>
                    {/if}
                  </span>
                  <span class="log-col-hash log-cell-hash" title={event.hash}>
                    {event.hash.substring(0, 12)}…
                  </span>
                </div>
              {/each}
            </div>
          </div>

          {#if log_loading}
            <div class="log-loading-indicator">Loading more…</div>
          {/if}
        {:else if !log_loading}
          <div class="log-empty">
            <EmptyState
              compact={true}
              variant="archive"
              title="No events recorded yet"
              description="Events will appear here as the app archives emails and monitors coverage."
            />
          </div>
        {/if}

        {#if log_loading && log_events.length === 0}
          <div class="log-loading">Loading events…</div>
        {/if}
      </div>
    {:else if active_section === 'diagnostics'}
      <div class="diagnostics-section">
        <h2>Database Diagnostics</h2>
        <p class="diag-subtitle">Inspect the current state of the archive database to troubleshoot sync or display issues.</p>
        <p class="diag-db-path">
          DB path: <code>{db_path}</code>
          {#if app_version}
            <span class="diag-version">App version: {app_version}</span>
          {/if}
        </p>

        <div class="diag-actions">
          <button type="button" class="diag-button primary" onclick={run_diagnostics} disabled={diagnostic_loading}>
            {diagnostic_loading ? 'Running…' : 'Run Diagnostics'}
          </button>
          {#if diagnostic_data}
            <button type="button" class="diag-button" onclick={copy_diagnostic_json}>
              Copy JSON
            </button>
          {/if}
        </div>

        {#if diagnostic_error}
          <div class="diag-error">{diagnostic_error}</div>
        {/if}

        {#if diagnostic_data}
          <div class="diag-results">
            <h3>Row Counts</h3>
            <table class="diag-table">
              <tbody>
                <tr><td>Accounts</td><td>{diagnostic_data.accounts_count}</td></tr>
                <tr><td>Mailboxes</td><td>{diagnostic_data.mailboxes_count}</td></tr>
                <tr><td>Message Blobs</td><td>{diagnostic_data.message_blobs_count}</td></tr>
                <tr><td>Message Locations</td><td>{diagnostic_data.message_locations_count}</td></tr>
                <tr><td>Events</td><td>{diagnostic_data.events_count}</td></tr>
              </tbody>
            </table>

            <h3>Listing Query Results</h3>
            <table class="diag-table">
              <tbody>
                <tr><td>All folders (what UI shows)</td><td><strong>{diagnostic_data.listing_result_count}</strong></td></tr>
                <tr><td>INBOX only</td><td>{diagnostic_data.inbox_listing_count}</td></tr>
              </tbody>
            </table>

            {#if diagnostic_data.accounts?.length > 0}
              <h3>Accounts</h3>
              <table class="diag-table full">
                <thead>
                  <tr><th>ID</th><th>Email</th><th>Host</th><th>Disabled</th><th>Actions</th></tr>
                </thead>
                <tbody>
                  {#each diagnostic_data.accounts as acct (acct.id)}
                    <tr>
                      <td>{acct.id}</td>
                      <td>{acct.email_address}</td>
                      <td>{acct.imap_host}</td>
                      <td class:diag-warn={acct.disabled}>{acct.disabled ? 'YES' : 'no'}</td>
                      <td>
                        <button type="button" class="diag-button small" onclick={() => handle_reset_cursors(acct.id)}>
                          Reset cursors & resync
                        </button>
                      </td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            {/if}

            {#if diagnostic_data.mailboxes?.length > 0}
              <h3>Mailboxes</h3>
              <table class="diag-table full">
                <thead>
                  <tr><th>ID</th><th>Name</th><th>Sync</th><th>Excluded</th><th>UIDV</th><th>Last UID</th><th>Last Sync</th><th>Error</th></tr>
                </thead>
                <tbody>
                  {#each diagnostic_data.mailboxes as mbox (mbox.id)}
                    <tr>
                      <td>{mbox.id}</td>
                      <td>{mbox.imap_name}</td>
                      <td>{mbox.sync_enabled ? 'yes' : 'no'}</td>
                      <td>{mbox.hard_excluded ? 'YES' : 'no'}</td>
                      <td>{mbox.uidvalidity ?? '—'}</td>
                      <td>{mbox.last_seen_uid}</td>
                      <td class="diag-date">{mbox.last_sync_at ?? '—'}</td>
                      <td class:diag-warn={mbox.last_error}>{mbox.last_error || '—'}</td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            {/if}

            {#if diagnostic_data.recent_locations?.length > 0}
              <h3>Recent Message Locations (newest 20)</h3>
              <table class="diag-table full">
                <thead>
                  <tr><th>ID</th><th>Blob</th><th>Acct</th><th>Mailbox</th><th>UID</th><th>Gone?</th><th>Acct Disabled?</th><th>Subject</th></tr>
                </thead>
                <tbody>
                  {#each diagnostic_data.recent_locations as loc (loc.id)}
                    <tr>
                      <td>{loc.id}</td>
                      <td>{loc.message_blob_id}</td>
                      <td>{loc.account_id}</td>
                      <td>{loc.mailbox_name ?? loc.mailbox_id}</td>
                      <td>{loc.uid}</td>
                      <td class:diag-warn={loc.gone_from_server_at}>{loc.gone_from_server_at ?? '—'}</td>
                      <td class:diag-warn={loc.account_disabled}>{loc.account_disabled ? 'YES' : 'no'}</td>
                      <td class="diag-subject">{loc.subject ?? '(no subject)'}</td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            {/if}
          </div>
        {/if}
      </div>
    {:else if active_section === 'general'}
      <GeneralSettings db_path={db_path} on_change_db_path={on_change_db_path} />
    {:else if adding_account || selected_account_id === 'new'}
      <AddAccountForm
        db_path={db_path}
        on_success={handle_account_added}
        on_cancel={handle_cancel_add}
      />
    {:else if selected_account}
      <AccountSettings
        db_path={db_path}
        account={selected_account}
        on_removed={() => handle_account_removed(selected_account.id)}
      />
    {:else}
      <EmptyState
        variant="accounts"
        title="No accounts configured"
        description='Click "Add account" to connect an email account.'
      />
    {/if}
  </main>
</div>

<style>
  .settings-page {
    display: flex;
    height: 100vh;
    padding-top: var(--titlebar-height);
    background: var(--color-bg);
  }

  /* Sidebar - Clean and minimal like Things/Craft */
  .sidebar {
    width: 200px;
    border-right: 1px solid var(--color-border);
    display: flex;
    flex-direction: column;
    background: var(--color-bg-secondary);
    flex-shrink: 0;
  }

  .sidebar-section {
    padding: var(--space-md);
  }

  .sidebar-title {
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-semibold);
    color: var(--color-text-tertiary);
    letter-spacing: 0.04em;
    text-transform: uppercase;
    margin-bottom: var(--space-sm);
    padding: 0 var(--space-sm);
  }

  .sidebar-list {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .sidebar-item {
    display: block;
    width: 100%;
    padding: var(--space-sm) var(--space-md);
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--color-text);
    font: inherit;
    font-size: var(--font-size-sm);
    text-align: left;
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    transition: background var(--transition-fast);
  }

  .sidebar-item:hover {
    background: var(--color-bg-tertiary);
  }

  .sidebar-item.active {
    background: var(--color-accent-soft);
    color: var(--color-accent);
    font-weight: var(--font-weight-medium);
  }

  .add-button {
    display: block;
    width: 100%;
    padding: var(--space-sm) var(--space-md);
    border: none;
    background: none;
    color: var(--color-accent);
    font: inherit;
    font-size: var(--font-size-sm);
    text-align: left;
    cursor: pointer;
    transition: opacity var(--transition-fast);
  }

  .add-button:hover {
    opacity: 0.8;
  }

  .sidebar-divider {
    height: 1px;
    background: var(--color-border);
    margin: var(--space-sm) var(--space-md);
  }

  .sidebar-top {
    padding-top: var(--space-sm);
  }

  .back-button {
    display: block;
    width: 100%;
    padding: var(--space-sm) var(--space-md);
    border: none;
    background: none;
    color: var(--color-accent);
    font: inherit;
    font-size: var(--font-size-sm);
    text-align: left;
    cursor: pointer;
    transition: opacity var(--transition-fast);
  }

  .back-button:hover {
    opacity: 0.8;
  }

  /* Content */
  .content {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-xl);
    background: var(--color-bg);
  }

  /* Diagnostics */
  .diagnostics-section h2 {
    margin: 0 0 var(--space-sm);
    font-size: var(--font-size-lg);
    font-weight: var(--font-weight-semibold);
    color: var(--color-text);
  }

  .diagnostics-section h3 {
    margin: var(--space-lg) 0 var(--space-sm);
    font-size: var(--font-size-md);
    font-weight: var(--font-weight-medium);
    color: var(--color-text);
  }

  .diag-subtitle {
    color: var(--color-text-secondary);
    font-size: var(--font-size-sm);
    margin: 0 0 var(--space-sm);
  }

  .diag-db-path {
    font-size: var(--font-size-xs);
    color: var(--color-text-tertiary);
    margin: 0 0 var(--space-md);
  }

  .diag-db-path code {
    background: var(--color-bg-secondary);
    padding: 2px 6px;
    border-radius: var(--radius-sm);
    font-family: var(--font-mono);
    font-size: var(--font-size-xs);
    word-break: break-all;
  }

  .diag-version {
    display: inline-block;
    margin-left: var(--space-md);
    color: var(--color-text-tertiary);
  }

  .diag-actions {
    display: flex;
    gap: var(--space-sm);
    margin-bottom: var(--space-md);
  }

  .diag-button {
    padding: var(--space-sm) var(--space-md);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-md);
    background: var(--color-bg);
    color: var(--color-text);
    font: inherit;
    font-size: var(--font-size-sm);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .diag-button:hover:not(:disabled) {
    background: var(--color-bg-tertiary);
  }

  .diag-button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .diag-button.primary {
    background: var(--color-accent);
    border-color: var(--color-accent);
    color: var(--color-text-on-accent);
  }

  .diag-button.primary:hover:not(:disabled) {
    background: var(--color-accent-hover);
  }

  .diag-button.small {
    padding: var(--space-xs) var(--space-sm);
    font-size: var(--font-size-xs);
  }

  .diag-error {
    padding: var(--space-sm) var(--space-md);
    border-radius: var(--radius-md);
    background: var(--color-error-bg, #3a1c1c);
    color: var(--color-error);
    font-size: var(--font-size-sm);
    margin-bottom: var(--space-md);
  }

  .diag-results {
    margin-top: var(--space-md);
  }

  .diag-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--font-size-xs);
    margin-bottom: var(--space-sm);
  }

  .diag-table:not(.full) {
    max-width: 400px;
  }

  .diag-table th,
  .diag-table td {
    padding: var(--space-xs) var(--space-sm);
    border-bottom: 1px solid var(--color-border);
    text-align: left;
    vertical-align: top;
  }

  .diag-table th {
    font-weight: var(--font-weight-medium);
    color: var(--color-text-secondary);
    background: var(--color-bg-secondary);
    position: sticky;
    top: 0;
  }

  .diag-table td:first-child {
    color: var(--color-text-secondary);
  }

  .diag-table td:last-child {
    font-variant-numeric: tabular-nums;
  }

  .diag-warn {
    color: var(--color-warning, #e5a100);
    font-weight: var(--font-weight-medium);
  }

  .diag-date {
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    max-width: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .diag-subject {
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* ── Activity Log ──────────────────────────────────────────── */
  .log-section {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  .log-section h2 {
    margin: 0 0 var(--space-xs);
    font-size: var(--font-size-lg);
    font-weight: var(--font-weight-semibold);
    color: var(--color-text);
  }

  .log-header {
    flex-shrink: 0;
  }

  .log-subtitle {
    color: var(--color-text-secondary);
    font-size: var(--font-size-sm);
    margin: 0 0 var(--space-md);
    line-height: 1.5;
  }

  .log-count {
    color: var(--color-text-tertiary);
    font-variant-numeric: tabular-nums;
  }

  .log-controls {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-md);
    margin-bottom: var(--space-md);
    flex-shrink: 0;
  }

  .log-filter {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
  }

  .log-filter label {
    font-size: var(--font-size-sm);
    color: var(--color-text-secondary);
    white-space: nowrap;
  }

  .log-filter select {
    padding: var(--space-xs) var(--space-sm);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-md);
    background: var(--color-bg);
    color: var(--color-text);
    font: inherit;
    font-size: var(--font-size-sm);
    cursor: pointer;
  }

  .log-actions {
    display: flex;
    gap: var(--space-sm);
  }

  .log-button {
    padding: var(--space-xs) var(--space-md);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-md);
    background: var(--color-bg);
    color: var(--color-text);
    font: inherit;
    font-size: var(--font-size-xs);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .log-button:hover:not(:disabled) {
    background: var(--color-bg-tertiary);
  }

  .log-button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .log-error {
    padding: var(--space-sm) var(--space-md);
    border-radius: var(--radius-md);
    background: var(--color-error-bg, #3a1c1c);
    color: var(--color-error);
    font-size: var(--font-size-sm);
    margin-bottom: var(--space-md);
    flex-shrink: 0;
  }

  /* Sticky header row */
  .log-table-header {
    display: flex;
    padding: var(--space-xs) var(--space-sm);
    background: var(--color-bg-secondary);
    border: 1px solid var(--color-border);
    border-bottom: none;
    border-radius: var(--radius-md) var(--radius-md) 0 0;
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-medium);
    color: var(--color-text-secondary);
    flex-shrink: 0;
  }

  /* Virtual scroll container */
  .log-virtual-list {
    flex: 1;
    overflow-y: auto;
    border: 1px solid var(--color-border);
    border-top: none;
    border-radius: 0 0 var(--radius-md) var(--radius-md);
    position: relative;
    min-height: 200px;
  }

  .log-spacer {
    position: relative;
  }

  /* Individual row */
  .log-row {
    position: absolute;
    left: 0;
    right: 0;
    height: 44px;
    display: flex;
    align-items: center;
    padding: 0 var(--space-sm);
    border-bottom: 1px solid var(--color-border);
    font-size: var(--font-size-xs);
    background: var(--color-bg);
  }

  .log-row-warning {
    background: rgba(229, 161, 0, 0.04);
  }

  /* Column widths – shared between header and rows */
  .log-col-time {
    width: 170px;
    flex-shrink: 0;
  }

  .log-col-kind {
    width: 145px;
    flex-shrink: 0;
  }

  .log-col-detail {
    flex: 1;
    min-width: 100px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .log-col-hash {
    width: 110px;
    flex-shrink: 0;
    text-align: right;
  }

  .log-cell-time {
    font-variant-numeric: tabular-nums;
    color: var(--color-text-secondary);
  }

  .log-cell-kind {
    white-space: nowrap;
  }

  .log-kind-badge {
    display: inline-block;
    padding: 1px 6px;
    border-radius: var(--radius-sm);
    background: var(--color-bg-tertiary);
    font-size: 10px;
    font-weight: var(--font-weight-medium);
  }

  .log-kind-warning {
    background: rgba(229, 161, 0, 0.15);
    color: var(--color-warning, #e5a100);
  }

  .log-cell-detail {
    display: flex;
    align-items: center;
    gap: var(--space-xs);
    overflow: hidden;
  }

  .log-meta {
    padding: 0 4px;
    border-radius: var(--radius-sm);
    background: var(--color-bg-secondary);
    font-size: 10px;
    color: var(--color-text-tertiary);
    flex-shrink: 0;
  }

  .log-detail-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-text-secondary);
  }

  .log-cell-hash {
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--color-text-tertiary);
    white-space: nowrap;
  }

  .log-loading-indicator {
    text-align: center;
    padding: var(--space-sm);
    color: var(--color-text-tertiary);
    font-size: var(--font-size-xs);
    flex-shrink: 0;
  }

  .log-empty {
    text-align: center;
    padding: var(--space-xl);
    color: var(--color-text-tertiary);
    flex: 1;
    display: flex;
    flex-direction: column;
    justify-content: center;
  }

  .log-loading {
    text-align: center;
    padding: var(--space-xl);
    color: var(--color-text-tertiary);
    font-size: var(--font-size-sm);
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
  }
</style>
