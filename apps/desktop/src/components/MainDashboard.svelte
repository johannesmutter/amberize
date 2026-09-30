<script>
  import { untrack } from 'svelte';
  import { tauri_invoke, listen_scoped, tauri_save_dialog } from '../lib/tauri_bridge.js';
  import VirtualList from './VirtualList.svelte';
  import MessagePreview from './MessagePreview.svelte';
  import StatusBar from './StatusBar.svelte';
  import EmptyState from './EmptyState.svelte';

  let { db_path, on_open_settings, dashboard_action_nonce = 0, dashboard_action_type = '' } = $props();

  const DEFAULT_EXPORT_FILENAME = 'email-export.zip';
  const CURRENT_YEAR = new Date().getFullYear();
  const LAST_YEAR = CURRENT_YEAR - 1;
  const MS_PER_DAY = 24 * 60 * 60 * 1000;
  const MESSAGE_PAGE_SIZE = 100;
  const SEARCH_PAGE_SIZE = 50;
  const MIN_SEARCH_QUERY_LEN = 2;
  const SEARCH_DEBOUNCE_MS = 400;
  const MAX_IN_MEMORY_MESSAGES = 500;
  const MESSAGE_SORT_NEWEST = 'newest';
  const MESSAGE_SORT_OLDEST = 'oldest';

  // Accounts
  /** @type {any[]} */
  let accounts = $state([]);

  // Search and filters
  let search_query = $state('');
  let filter_account = $state(null);
  let filter_date = $state(null);
  /** @type {string} */
  let custom_range_start = $state('');
  /** @type {string} */
  let custom_range_end = $state('');
  let filter_attachments = $state(null);
  let sort_order = $state(MESSAGE_SORT_NEWEST);

  // Messages
  /** @type {any[]} */
  let messages = $state([]);
  let selected_message_id = $state(null);
  /** @type {any | null} */
  let selected_message = $state(null);
  let message_loading = $state(false);
  let preview_generation = 0;
  let window_visible = $state(true);
  let load_error = $state('');
  let window_origin = $state(0);
  let previous_available = $state(false);
  let selected_rows = $state(new Map());
  let export_operation_id = $state('');
  let export_choosing = $state(false);
  let export_cancel_requested = $state(false);
  let export_message = $state('');
  let disposed = false;
  $effect(() => () => {
    disposed = true;
    if (export_operation_id) void tauri_invoke('cancel_auditor_export', {operationId:export_operation_id}).catch(() => {});
  });

  // Message list paging (inbox view)
  let message_list_offset = $state(0);
  let message_list_has_more = $state(true);
  let message_list_loading = $state(false);
  let message_list_loading_more = $state(false);
  let message_list_pending_reset = $state(false);
  /** Generation counter to discard stale responses from superseded requests. */
  let message_list_generation = $state(0);

  // Bulk selection
  let bulk_mode = $state(false);
  let viewing_selection = $state(false);
  /** @type {Set<number>} */
  let selected_for_export = $state(new Set());

  // Sync status
  let sync_status = $state({
    syncing: false,
    syncing_account: null,
    last_sync: null,
    error: null,
    error_account_id: null,
  });

  // Sync progress (granular, updated per-message during sync)
  /** @type {{ mailbox_name: string, mailbox_index: number, mailbox_count: number, messages_fetched: number, messages_ingested: number } | null} */
  let sync_progress = $state(null);

  // Archive stats (mail count + storage)
  let archive_total_messages = $state(0);
  let archive_db_size_bytes = $state(0);
  let archive_oldest_date = $state(null);
  let archive_newest_date = $state(null);

  // Resizable split
  const SPLIT_RATIO_MIN = 0.2;
  const SPLIT_RATIO_MAX = 0.8;
  const SPLIT_RATIO_STEP = 0.05;

  let split_ratio = $state(0.4);
  let is_resizing = $state(false);
  let container_element = $state(null);
  let search_input_element = $state(null);
  let search_debounce_timeout = null;
  let hidden_state_needs_reload = $state(false);

  function clear_preview_state() {
    preview_generation++;
    selected_message_id = null;
    selected_message = null;
    message_loading = false;
  }

  function clear_hidden_window_state() {
    window_visible = false; sync_progress = null;
    clear_preview_state();

    // Reset list-heavy state so hidden-to-tray mode does not retain large arrays.
    messages = [];
    selected_for_export = new Set(); selected_rows = new Map();
    viewing_selection = false;
    bulk_mode = false;
    message_list_offset = 0;
    message_list_has_more = true;
    message_list_pending_reset = false;
    message_list_loading = false;
    message_list_loading_more = false;
    message_list_generation += 1;
    hidden_state_needs_reload = true;
  }

  // Load accounts, sync status, and archive stats on mount.
  $effect(() => {
    untrack(() => {
      void load_accounts();
      void load_sync_status();
      void load_archive_stats();
    });
  });

  // Reload archive stats when account filter changes.
  $effect(() => {
    // Read filter_account to create the reactive dependency.
    filter_account;
    untrack(() => {
      void load_archive_stats();
    });
  });

  // Enable/disable the native "Export Selected Email…" menu item.
  $effect(() => {
    const has_selection = selected_message_id != null;
    tauri_invoke('set_export_eml_menu_enabled', { enabled: has_selection }).catch(() => {});
  });

  // Native visibility is authoritative; document.hidden varies across webviews.
  $effect(() => listen_scoped({
    sync_status_updated: () => { if (!window_visible) {hidden_state_needs_reload = true; return;} void refresh_after_sync(); },
    menu_export_eml: () => {if (window_visible) void export_single();},
    sync_progress: event => {if (window_visible) sync_progress = event.payload;},
    main_window_hidden: () => clear_hidden_window_state(),
    main_window_shown: () => {
      window_visible = true;
      if (hidden_state_needs_reload) {hidden_state_needs_reload = false; void load_accounts(); void load_sync_status(); void load_archive_stats(); void load_messages(true);}
    },
  }));
  async function refresh_after_sync() {
    const was_syncing = sync_status.syncing;
    await load_sync_status();
    if (!window_visible) return;
    if (!sync_status.syncing) { sync_progress = null; if (was_syncing) { void load_messages(true, false, true); void load_archive_stats(); } }
  }

  // Reload messages when filters change.
  // IMPORTANT: load_messages reads internal loading state ($state) which must NOT
  // become a dependency of this effect, otherwise toggling message_list_loading
  // triggers an infinite re-run loop. We use untrack() to isolate those reads.
  $effect(() => {
    // Track only non-search filter dependencies for immediate refresh.
    const _ = [
      filter_account,
      filter_date,
      custom_range_start,
      custom_range_end,
      filter_attachments,
      sort_order,
    ];
    const should_load = !viewing_selection;
    if (should_load) {
      untrack(() => void load_messages(true));
    }
  });

  // Debounce search-triggered reloads so we don't query on every keystroke.
  $effect(() => {
    search_query;
    if (viewing_selection) return;

    if (search_debounce_timeout) {
      clearTimeout(search_debounce_timeout);
      search_debounce_timeout = null;
    }

    search_debounce_timeout = setTimeout(() => {
      void load_messages(true);
      search_debounce_timeout = null;
    }, SEARCH_DEBOUNCE_MS);

    return () => {
      if (!search_debounce_timeout) return;
      clearTimeout(search_debounce_timeout);
      search_debounce_timeout = null;
    };
  });

  async function load_accounts() {
    if (!window_visible || !db_path?.trim()) return;
    try {
      const all_accounts = await tauri_invoke('list_accounts', { dbPath: db_path });
      if (window_visible) accounts = all_accounts;
    } catch (err) { load_error = String(err); }
  }

  async function load_archive_stats() {
    if (!window_visible || !db_path?.trim()) return;
    try {
      const account_id = filter_account ? Number(filter_account) : null;
      const stats = await tauri_invoke('get_archive_stats', {
        dbPath: db_path,
        accountId: (account_id && !isNaN(account_id)) ? account_id : null,
      });
      if (!window_visible) return;
      if (stats && typeof stats === 'object') {
        archive_total_messages = stats.account_messages ?? stats.total_messages ?? 0;
        archive_db_size_bytes = stats.db_size_bytes ?? 0;
      }
      const date_range = await tauri_invoke('get_archive_date_range', { dbPath: db_path });
      if (!window_visible) return;
      archive_oldest_date = date_range?.oldest_date ?? null;
      archive_newest_date = date_range?.newest_date ?? null;
    } catch {
      // ignore — stats are informational only
      archive_oldest_date = null;
      archive_newest_date = null;
    }
  }

  $effect(() => {
    dashboard_action_nonce;
    const action_type = dashboard_action_type;
    if (action_type === 'focus_search') {
      search_input_element?.focus();
      search_input_element?.select();
      return;
    }
    if (action_type === 'sync_now') {
      void handle_sync();
    }
  });

  async function load_sync_status() {
    if (!window_visible || !db_path?.trim()) return;
    try {
      const status = await tauri_invoke('get_sync_status');
      if (status && typeof status === 'object') {
        if (!window_visible) return;
        const outcome = status.last_sync_status?.split(' — ')[0]?.toLowerCase() ?? 'never';
        const prev_error = outcome === 'error' || outcome === 'partial' ? (status.error || status.last_sync_status) : null;
        const prev_error_account_id = sync_status.error_account_id;
        sync_status = {
          syncing: status.sync_in_progress || false,
          syncing_account: null,
          last_sync: outcome === 'ok' ? (status.last_success_at || status.last_sync_at || null) : (status.last_sync_at || null),
          outcome,
          error: prev_error,
          error_account_id: prev_error_account_id,
        };
      }
    } catch {
      // ignore
    }
  }

  /**
   * Normalize account filter value (select binds can be stringy).
   * @param {any} value
   * @returns {number | null}
   */
  function normalize_account_id(value) {
    if (typeof value === 'number' && Number.isFinite(value)) return value;
    if (typeof value === 'string') {
      const parsed = Number(value);
      if (Number.isFinite(parsed)) return parsed;
    }
    return null;
  }

  async function load_messages(reset = false, previous = false, preserve_preview = false) {
    if (!window_visible || !db_path?.trim()) return;

    if (message_list_loading || message_list_loading_more) {
      if (reset) message_list_pending_reset = true;
      return;
    }

    if (reset) {
      message_list_offset = 0;
      message_list_has_more = true;
      messages = [];
      if (!preserve_preview) clear_preview_state();
      window_origin = 0; previous_available = false;
      message_list_generation += 1;
    }

    const query = search_query.trim();
    const is_searching = query.length > 0;
    const query_too_short = is_searching && query.length < MIN_SEARCH_QUERY_LEN;

    if (query_too_short) {
      messages = [];
      selected_message_id = null;
      selected_message = null;
      message_list_offset = 0;
      message_list_has_more = false;
      return;
    }

    if (!previous && !message_list_has_more) return;

    const account_id = normalize_account_id(filter_account);
    const offset = message_list_offset;
    const anchor = previous ? messages[0] : messages.at(-1);
    const cursor = !reset && anchor ? {sort_timestamp:anchor.sort_timestamp, id:anchor.id} : null;
    const {date_from,date_to} = date_bounds();
    const current_generation = message_list_generation;
    const page_size = is_searching ? SEARCH_PAGE_SIZE : MESSAGE_PAGE_SIZE;

    try {
      if (reset) {
        message_list_loading = true;
      } else {
        message_list_loading_more = true;
      }

      let results = await tauri_invoke('list_messages', {
        dbPath: db_path,
        accountId: account_id,
        mailboxName: null,
        query: query,
        limit: page_size,
        offset: 0,
        dateFrom: date_from, dateTo: date_to,
        hasAttachments: filter_attachments === 'has' ? true : filter_attachments === 'none' ? false : null,
        after: previous ? null : cursor, before: previous ? cursor : null,
        sortOrder: sort_order,
      });
      const fetched_count = Array.isArray(results) ? results.length : 0;

      // Discard response if a newer request has been issued (race condition guard).
      if (current_generation !== message_list_generation) return;

      if (!Array.isArray(results)) {
        results = [];
      }

      load_error = '';
      const existing_ids = new Set(messages.map(m => m.id));
      const unique_results = results.filter(m => !existing_ids.has(m.id));
      const next_messages = previous ? [...unique_results, ...messages] : [...messages, ...unique_results];
      if (previous) {
        window_origin = Math.max(0, window_origin - unique_results.length);
        messages = next_messages.slice(0, MAX_IN_MEMORY_MESSAGES);
        previous_available = fetched_count === page_size && window_origin > 0;
        message_list_has_more = true;
      } else {
        const removed = Math.max(0, next_messages.length - MAX_IN_MEMORY_MESSAGES);
        messages = next_messages.slice(removed);
        window_origin += removed;
        previous_available = window_origin > 0;
        message_list_offset = offset + fetched_count;
        message_list_has_more = fetched_count === page_size;
      }
    } catch (err) {
      if (current_generation !== message_list_generation || !window_visible) return;
      load_error = err instanceof Error ? err.message : String(err);
      if (reset) {
        messages = [];
      }
      message_list_has_more = false;
    } finally {
      message_list_loading = false;
      message_list_loading_more = false;

      if (message_list_pending_reset) {
        message_list_pending_reset = false;
        void load_messages(true);
      }
    }
  }

  function load_more_messages() {
    if (viewing_selection) return;
    if (message_list_loading || message_list_loading_more) return;
    if (!message_list_has_more) return;
    void load_messages(false);
  }

  /**
   * @param {string} value
   * @returns {Date | null}
   */
  function parse_date_input_yyyy_mm_dd(value) {
    const normalized = value.trim();
    if (!normalized) return null;

    const match = normalized.match(/^(\d{4})-(\d{2})-(\d{2})$/);
    if (!match) return null;

    const year = Number(match[1]);
    const month = Number(match[2]) - 1;
    const day = Number(match[3]);
    const date = new Date(year, month, day);
    if (Number.isNaN(date.getTime())) return null;

    // Validate (Date constructor auto-rolls invalid dates).
    if (
      date.getFullYear() !== year ||
      date.getMonth() !== month ||
      date.getDate() !== day
    ) {
      return null;
    }

    return date;
  }

  function date_bounds() {
    const now = new Date(); const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    let start = null, end = null;
    if (filter_date === 'today') start = today;
    if (filter_date === 'last7' || filter_date === 'last30') start = new Date(today.getTime() - (filter_date === 'last7' ? 7 : 30) * MS_PER_DAY);
    if (filter_date === 'this_year') {start = new Date(CURRENT_YEAR,0,1);end = new Date(CURRENT_YEAR+1,0,1);}
    if (filter_date === 'last_year') {start = new Date(LAST_YEAR,0,1);end = new Date(CURRENT_YEAR,0,1);}
    if (/^q[1-4]$/.test(filter_date ?? '')) {const month=(Number(filter_date[1])-1)*3;start=new Date(LAST_YEAR,month,1);end=new Date(LAST_YEAR,month+3,1);}
    if (filter_date === 'custom') {
      start = parse_date_input_yyyy_mm_dd(custom_range_start); end = parse_date_input_yyyy_mm_dd(custom_range_end);
      if (start && end && end < start) [start,end] = [end,start];
      if (end) end = new Date(end.getFullYear(),end.getMonth(),end.getDate()+1);
    }
    return {date_from:start ? Math.floor(start.getTime()/1000) : null,date_to:end ? Math.floor(end.getTime()/1000) : null};
  }

  function clear_custom_range() {
    custom_range_start = '';
    custom_range_end = '';
    filter_date = null;
  }

  async function handle_message_click(message) {
    if (!window_visible) return;
    const generation = ++preview_generation;
    selected_message_id = message.id; selected_message = null;
    message_loading = true;

    try {
      const detail = await tauri_invoke('get_message_detail', {
        dbPath: db_path,
        messageBlobId: message.message_blob_id,
      });
      if (generation === preview_generation && window_visible) selected_message = detail;
    } catch (err) {
      if (generation === preview_generation && window_visible) { selected_message = null; load_error = String(err); }
    } finally {
      if (generation === preview_generation) message_loading = false;
    }
  }

  function handle_toggle_selection(id) {
    const new_set = new Set(selected_for_export);
    if (new_set.has(id)) {
      new_set.delete(id); selected_rows.delete(id);
    } else {
      if (new_set.size >= 10000) {load_error = 'Select at most 10000 messages per export.'; return;}
      new_set.add(id); const row = messages.find(m => m.id === id); if (row) selected_rows.set(id, row);
    }
    selected_for_export = new_set; selected_rows = new Map(selected_rows);

    // If bulk mode enabled when selection made
    if (new_set.size > 0 && !bulk_mode) {
      bulk_mode = true;
    }

    // If viewing selection and all items deselected, exit view
    if (viewing_selection && new_set.size === 0) {
      viewing_selection = false;
    }
  }

  function toggle_bulk_mode() {
    if (bulk_mode) {
      // Exiting bulk mode
      bulk_mode = false;
      viewing_selection = false;
      selected_for_export = new Set(); selected_rows = new Map();
    } else {
      // Entering bulk mode
      bulk_mode = true;
    }
  }

  function view_selection() {
    viewing_selection = true;
  }

  function exit_selection_view() {
    viewing_selection = false;
  }

  function clear_selection() {
    selected_for_export = new Set(); selected_rows = new Map();
    bulk_mode = false;
    viewing_selection = false;
  }

  async function export_single() {
    if (!selected_message) return;

    let output_path = null;
    try {
      output_path = await tauri_save_dialog({
        title: 'Export message',
        defaultPath: `${selected_message.sha256}.eml`,
        filters: [{ name: 'Email message', extensions: ['eml'] }],
      });
    } catch {
      return;
    }

    if (!output_path) return;

    try {
      await tauri_invoke('export_message_blob_eml', {
        dbPath: db_path,
        messageBlobId: selected_message.id,
        outputPath: output_path,
      });
    } catch (err) {
      load_error = String(err);
    }
  }

  async function export_bulk() {
    if (selected_for_export.size === 0 || export_operation_id || export_choosing) return;
    const selected_blob_ids = [...new Set([...selected_rows.values()].map(m => m.message_blob_id))];
    export_choosing = true;

    let output_path = null;
    try {
      output_path = await tauri_save_dialog({
        title: 'Export selected emails',
        defaultPath: DEFAULT_EXPORT_FILENAME,
        filters: [{ name: 'ZIP archive', extensions: ['zip'] }],
      });
    } catch (err) {
      export_message = err instanceof Error ? err.message : String(err);
      return;
    } finally {
      export_choosing = false;
    }

    if (!output_path || disposed) return;
    const operation_id = crypto.randomUUID();
    export_operation_id = operation_id;
    export_cancel_requested = false;
    export_message = 'Exporting selected emails…';

    try {
      await tauri_invoke('export_auditor_package', {
        dbPath: db_path,
        outputZipPath: output_path,
        selectedBlobIds: selected_blob_ids,
        operationId: operation_id,
      });
      export_message = 'Selected emails exported.';
    } catch (err) {
      export_message = err instanceof Error ? err.message : String(err);
    } finally {
      export_operation_id = '';
      export_cancel_requested = false;
    }
  }

  async function cancel_bulk_export() {
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

  async function handle_sync() {
    if (!window_visible || !db_path?.trim()) return;
    // Prevent rapid double-clicks from triggering multiple syncs.
    if (sync_status.syncing) return;

    sync_status = { ...sync_status, syncing: true, error: null, error_account_id: null };
    sync_progress = null;

    try {
      const result = await tauri_invoke('sync_all_accounts_command', { dbPath: db_path });

      const first_error = Array.isArray(result?.errors) ? result.errors[0] : null;
      if (first_error?.message) {
        sync_status = {
          ...sync_status,
          syncing: false,
          error: first_error.message,
          error_account_id: first_error.account_id ?? null,
        };
      } else if (result?.accounts_with_errors > 0) {
        sync_status = {
          ...sync_status,
          syncing: false,
          error: 'Sync completed with mailbox errors. Open Settings → Accounts to see which folder failed.',
          error_account_id: null,
        };
      } else {
        // Sync succeeded — clear any previous error (0 new messages is normal for incremental syncs).
        sync_status = { ...sync_status, syncing: false, error: null, error_account_id: null };
      }

      void load_sync_status();
      void load_messages(true, false, true);
    } catch (err) {
      sync_status = {
        ...sync_status,
        syncing: false,
        error: err instanceof Error ? err.message : String(err),
      };
    }
  }

  function handle_fix_error(account_id) {
    on_open_settings?.('accounts', account_id);
  }

  function handle_manage_accounts() {
    on_open_settings?.('accounts');
  }

  /**
   * @param {EventTarget | null} target
   * @returns {boolean}
   */
  function is_text_input_target(target) {
    if (!(target instanceof HTMLElement)) return false;
    if (target.isContentEditable) return true;
    const tag_name = target.tagName.toLowerCase();
    return tag_name === 'input' || tag_name === 'textarea' || tag_name === 'select';
  }

  /**
   * @param {number} direction
   */
  async function navigate_selected_message(direction) {
    if (message_loading) return;
    if (!selected_message_id) return;
    if (displayed_messages.length === 0) return;

    const current_index = displayed_messages.findIndex((item) => item.id === selected_message_id);
    if (current_index < 0) return;

    const next_index = Math.max(
      0,
      Math.min(displayed_messages.length - 1, current_index + direction)
    );
    if (next_index === current_index) return;

    await handle_message_click(displayed_messages[next_index]);
  }

  function handle_dashboard_keydown(event) {
    if (event.defaultPrevented) return;
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    if (is_text_input_target(event.target)) return;

    if (event.key === 'ArrowDown') {
      event.preventDefault();
      void navigate_selected_message(1);
      return;
    }

    if (event.key === 'ArrowUp') {
      event.preventDefault();
      void navigate_selected_message(-1);
    }
  }

  // Resizable split handlers
  function start_resize(event) {
    is_resizing = true;
    event.preventDefault();
  }

  function do_resize(event) {
    if (!is_resizing || !container_element) return;
    const rect = container_element.getBoundingClientRect();
    const new_ratio = (event.clientX - rect.left) / rect.width;
    split_ratio = Math.max(SPLIT_RATIO_MIN, Math.min(SPLIT_RATIO_MAX, new_ratio));
  }

  function stop_resize() {
    is_resizing = false;
  }

  function handle_resize_keydown(event) {
    if (event.key === 'ArrowLeft') {
      split_ratio = Math.max(SPLIT_RATIO_MIN, split_ratio - SPLIT_RATIO_STEP);
      event.preventDefault();
      return;
    }
    if (event.key === 'ArrowRight') {
      split_ratio = Math.min(SPLIT_RATIO_MAX, split_ratio + SPLIT_RATIO_STEP);
      event.preventDefault();
    }
  }

  // Determine what to show in the list
  let displayed_messages = $derived.by(() => {
    if (viewing_selection) {
      return [...selected_rows.values()];
    }
    return messages;
  });

  // Show location only when not filtering by account
  let show_location = $derived(!filter_account);
</script>

<svelte:window onmousemove={do_resize} onmouseup={stop_resize} onkeydown={handle_dashboard_keydown} />

<div class="main-dashboard">
  {#if load_error}<p role="alert">{load_error} <button onclick={() => {void load_accounts();void load_messages(true);}}>Retry</button></p>{/if}
  {#if export_message}<p role="status">{export_message} {#if export_operation_id}<button disabled={export_cancel_requested} onclick={cancel_bulk_export}>Cancel export</button>{/if}</p>{/if}
  <!-- Top Row: Search + Filters -->
  <div class="top-row">
    <input
      type="text"
      class="search-input"
      placeholder="Search emails..."
      bind:this={search_input_element}
      bind:value={search_query}
      onkeydown={(e) => e.key === 'Enter' && load_messages(true)}
    />

    <div class="filters">
      <select
        class="filter-select"
        aria-label="Filter by account"
        bind:value={filter_account}
        onchange={(e) => {
          if (e.target.value === '__manage__') {
            filter_account = null;
            handle_manage_accounts();
          }
        }}
      >
        <option value={null}>All accounts</option>
        {#each accounts as account (account.id)}
          <option value={account.id}>{account.email_address}{account.disabled ? ' (archive only)' : ''}</option>
        {/each}
        <option disabled>───────────</option>
        <option value="__manage__">Manage accounts...</option>
      </select>

      <select class="filter-select" aria-label="Filter by date" bind:value={filter_date}>
        <option value={null}>Any date</option>
        <option value="today">Today</option>
        <option value="last7">Last 7 days</option>
        <option value="last30">Last 30 days</option>
        <option disabled>───────────</option>
        <option value="this_year">This year ({CURRENT_YEAR})</option>
        <option value="last_year">{LAST_YEAR}</option>
        <option value="q1">Q1 {LAST_YEAR}</option>
        <option value="q2">Q2 {LAST_YEAR}</option>
        <option value="q3">Q3 {LAST_YEAR}</option>
        <option value="q4">Q4 {LAST_YEAR}</option>
        <option disabled>───────────</option>
        <option value="custom">Custom range…</option>
      </select>

      <select class="filter-select filter-small" aria-label="Filter by attachments" bind:value={filter_attachments}>
        <option value={null}>All</option>
        <option value="has">Has attachments</option>
        <option value="none">No attachments</option>
      </select>
    </div>
  </div>

  {#if filter_date === 'custom'}
    <div class="custom-range-row" role="group" aria-label="Custom date range">
      <label class="custom-range-field">
        <span class="custom-range-label">From</span>
        <input class="custom-range-input" type="date" bind:value={custom_range_start} />
      </label>
      <label class="custom-range-field">
        <span class="custom-range-label">To</span>
        <input class="custom-range-input" type="date" bind:value={custom_range_end} />
      </label>
      <button type="button" class="custom-range-clear" onclick={clear_custom_range}>
        Clear
      </button>
    </div>
  {/if}

  <!-- Main Content: Split View -->
  <div class="split-view" bind:this={container_element}>
    <!-- Left Panel: Message List -->
    <div class="left-panel" style="width: {split_ratio * 100}%;">
      <!-- Bulk Export Header -->
      <div class="bulk-header" class:bulk-active={bulk_mode}>
        {#if bulk_mode}
          <div class="bulk-mode-active">
            <span class="bulk-mode-label">Export mode</span>
            {#if viewing_selection}
              <span class="bulk-mode-hint">Viewing {selected_for_export.size} selected</span>
            {:else if selected_for_export.size > 0}
              <button type="button" class="selection-count" onclick={view_selection}>
                {selected_for_export.size} selected
              </button>
            {:else}
              <span class="bulk-mode-hint">Use checkboxes to select</span>
            {/if}
          </div>
          <div class="bulk-header-right">
            <div class="bulk-actions">
              {#if viewing_selection}
                <button type="button" class="bulk-button primary" onclick={export_bulk} disabled={!!export_operation_id || export_choosing}>Export</button>
                <button type="button" class="bulk-button" onclick={exit_selection_view}>Back</button>
              {:else if selected_for_export.size > 0}
                <button type="button" class="bulk-button primary" onclick={export_bulk} disabled={!!export_operation_id || export_choosing}>Export</button>
                <button type="button" class="bulk-button" onclick={clear_selection}>Clear</button>
                <button type="button" class="bulk-button" onclick={toggle_bulk_mode}>Done</button>
              {:else}
                <button type="button" class="bulk-button" onclick={toggle_bulk_mode}>Done</button>
              {/if}
            </div>
            <select class="sort-select" bind:value={sort_order} aria-label="Sort emails">
              <option value={MESSAGE_SORT_NEWEST}>Newest first</option>
              <option value={MESSAGE_SORT_OLDEST}>Oldest first</option>
            </select>
          </div>
        {:else}
          <button type="button" class="mode-toggle" onclick={toggle_bulk_mode}>
            Select for export
          </button>
          <div class="bulk-header-right">
            <select class="sort-select" bind:value={sort_order} aria-label="Sort emails">
              <option value={MESSAGE_SORT_NEWEST}>Newest first</option>
              <option value={MESSAGE_SORT_OLDEST}>Oldest first</option>
            </select>
          </div>
        {/if}
      </div>

      {#if previous_available && !viewing_selection}<button onclick={() => load_messages(false,true)} disabled={message_list_loading_more}>Load earlier page</button>{/if}
      <!-- Message List -->
      {#if accounts.length === 0}
        <EmptyState
          variant="accounts"
          title="No email accounts configured"
          description='Select "Manage accounts" from the account filter above to add one.'
        />
      {:else if message_list_loading}
        <EmptyState compact={true} variant="inbox" title="Loading emails..." />
      {:else if displayed_messages.length === 0}
        {#if search_query.trim()}
          {#if search_query.trim().length < MIN_SEARCH_QUERY_LEN}
            <EmptyState
              variant="search"
              title={`Type at least ${MIN_SEARCH_QUERY_LEN} characters`}
              description="Short searches are skipped to keep the app responsive."
            />
          {:else}
            <EmptyState
              variant="search"
              title="No emails match your search"
              description="Try a shorter query or remove filters."
            />
          {/if}
        {:else}
          <EmptyState
            variant="archive"
            title="No archived emails yet"
            description="Click Sync Now below and ensure at least one folder is enabled in Settings."
          />
        {/if}
      {:else}
        <VirtualList
          origin={viewing_selection ? 0 : window_origin}
          on_reach_start={() => {if (previous_available && !viewing_selection) void load_messages(false,true);}}
          items={displayed_messages}
          bulk_mode={bulk_mode}
          selected_ids={selected_for_export}
          selected_item_id={selected_message_id}
          show_location={show_location}
          on_click={handle_message_click}
          on_toggle_selection={handle_toggle_selection}
          on_reach_end={load_more_messages}
        />
      {/if}
    </div>

    <!-- Resize Handle -->
    <div
      class="resize-handle"
      class:active={is_resizing}
      onmousedown={start_resize}
      role="slider"
      tabindex="0"
      aria-label="Resize panels"
      aria-orientation="vertical"
      aria-valuemin={Math.round(SPLIT_RATIO_MIN * 100)}
      aria-valuemax={Math.round(SPLIT_RATIO_MAX * 100)}
      aria-valuenow={Math.round(split_ratio * 100)}
      onkeydown={handle_resize_keydown}
    ></div>

    <!-- Right Panel: Message Preview -->
    <div class="right-panel" style="width: {(1 - split_ratio) * 100}%;">
      <MessagePreview
        message={selected_message}
        loading={message_loading}
        on_export={export_single}
      />
    </div>
  </div>

  <!-- Status Bar -->
  <StatusBar
    syncing={sync_status.syncing}
    syncing_account={sync_status.syncing_account}
    last_sync={sync_status.last_sync}
    error={sync_status.error}
    error_account_id={sync_status.error_account_id}
    progress={sync_progress}
    total_messages={archive_total_messages}
    db_size_bytes={archive_db_size_bytes}
    oldest_date={archive_oldest_date}
    newest_date={archive_newest_date}
    on_sync={handle_sync}
    on_fix_error={handle_fix_error}
  />
</div>

<style>
  .main-dashboard {
    display: flex;
    flex-direction: column;
    height: 100vh;
    padding-top: var(--titlebar-height);
    background: var(--color-bg);
  }

  /* Top Row - Search and Filters */
  .top-row {
    display: flex;
    align-items: center;
    gap: var(--space-md);
    padding: var(--space-md) var(--space-lg);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    background: var(--color-bg);
  }

  .search-input {
    flex: 1;
    min-width: 200px;
    padding: var(--space-sm) var(--space-md);
    padding-left: 32px;
    background: var(--color-bg-secondary);
    border: 1px solid transparent;
    border-radius: var(--radius-md);
    color: var(--color-text);
    font: inherit;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='16' height='16' viewBox='0 0 24 24' fill='none' stroke='%23999' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'%3E%3Ccircle cx='11' cy='11' r='8'/%3E%3Cpath d='M21 21l-4.35-4.35'/%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: 10px center;
    transition: all var(--transition-fast);
  }

  .search-input:hover {
    background-color: var(--color-bg-tertiary);
  }

  .search-input:focus {
    outline: none;
    background-color: var(--color-bg);
    border-color: var(--color-accent);
    box-shadow: 0 0 0 3px var(--color-accent-soft);
  }

  .search-input::placeholder {
    color: var(--color-text-tertiary);
  }

  .filters {
    display: flex;
    gap: var(--space-sm);
    flex-shrink: 0;
  }

  .custom-range-row {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding: var(--space-sm) var(--space-lg);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-bg-secondary);
    flex-shrink: 0;
    flex-wrap: wrap;
  }

  .custom-range-field {
    display: flex;
    align-items: center;
    gap: var(--space-xs);
    font-size: var(--font-size-xs);
    color: var(--color-text-tertiary);
  }

  .custom-range-label {
    min-width: 36px;
  }

  .custom-range-input {
    padding: var(--space-xs) var(--space-sm);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: var(--color-text);
    font: inherit;
    font-size: var(--font-size-xs);
  }

  .custom-range-input:focus {
    outline: none;
    border-color: var(--color-accent);
    box-shadow: 0 0 0 3px var(--color-accent-soft);
  }

  .custom-range-clear {
    padding: var(--space-xs) var(--space-sm);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: var(--color-text-secondary);
    cursor: pointer;
    font: inherit;
    font-size: var(--font-size-xs);
    transition: all var(--transition-fast);
  }

  .custom-range-clear:hover {
    background: var(--color-bg-tertiary);
    color: var(--color-text);
  }

  .filter-select {
    padding: var(--space-sm) var(--space-md);
    padding-right: 28px;
    background: var(--color-bg);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-md);
    color: var(--color-text);
    font: inherit;
    font-size: var(--font-size-sm);
    cursor: pointer;
    appearance: none;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'%3E%3Cpath fill='%23666' d='M3 4.5L6 7.5L9 4.5'/%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: right 8px center;
    transition: border-color var(--transition-fast);
  }

  .filter-select:hover {
    border-color: var(--color-accent-muted);
  }

  .filter-select:focus {
    outline: none;
    border-color: var(--color-accent);
    box-shadow: 0 0 0 3px var(--color-accent-soft);
  }

  .filter-small {
    max-width: 130px;
  }

  /* Split View */
  .split-view {
    display: flex;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  .left-panel {
    display: flex;
    flex-direction: column;
    min-width: 280px;
    background: var(--color-bg);
  }

  .right-panel {
    display: flex;
    flex-direction: column;
    min-width: 320px;
    background: var(--color-bg-secondary);
  }

  .resize-handle {
    width: 5px;
    cursor: col-resize;
    background: var(--color-border);
    flex-shrink: 0;
    position: relative;
    transition: background var(--transition-fast);
    /* Use a 1px line centered in the handle area */
    background: linear-gradient(
      to right,
      transparent 2px,
      var(--color-border) 2px,
      var(--color-border) 3px,
      transparent 3px
    );
  }

  .resize-handle:hover,
  .resize-handle.active {
    background: linear-gradient(
      to right,
      transparent 2px,
      var(--color-accent) 2px,
      var(--color-accent) 3px,
      transparent 3px
    );
  }

  /* Bulk Header - Compact and subtle */
  .bulk-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-sm) var(--space-md);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-bg-secondary);
    flex-shrink: 0;
    min-height: 36px;
    transition: background var(--transition-fast);
  }

  .bulk-header.bulk-active {
    background: var(--color-accent-soft);
    border-bottom-color: var(--color-accent-muted);
  }

  .mode-toggle {
    padding: var(--space-xs) var(--space-sm);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    font: inherit;
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-medium);
    color: var(--color-text-secondary);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .mode-toggle:hover {
    background: var(--color-bg-tertiary);
    color: var(--color-text);
  }

  .bulk-mode-active {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
  }

  .bulk-mode-label {
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-medium);
    color: var(--color-accent);
  }

  .bulk-mode-hint {
    font-size: var(--font-size-xs);
    color: var(--color-text-tertiary);
  }

  .selection-count {
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    font-size: var(--font-size-sm);
    color: var(--color-text-secondary);
    cursor: pointer;
  }

  .selection-count:hover {
    text-decoration: underline;
    color: var(--color-text);
  }

  .bulk-actions {
    display: flex;
    gap: var(--space-sm);
  }

  .bulk-header-right {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
  }

  .sort-select {
    padding: var(--space-xs) var(--space-sm);
    padding-right: 24px;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: var(--color-text-secondary);
    font: inherit;
    font-size: var(--font-size-xs);
    cursor: pointer;
    appearance: none;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'%3E%3Cpath fill='%23666' d='M3 4.5L6 7.5L9 4.5'/%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: right 6px center;
  }

  .sort-select:focus {
    outline: none;
    border-color: var(--color-accent);
    box-shadow: 0 0 0 3px var(--color-accent-soft);
  }

  .bulk-button {
    padding: var(--space-xs) var(--space-sm);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: var(--color-text-secondary);
    font: inherit;
    font-size: var(--font-size-xs);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .bulk-button:hover {
    background: var(--color-bg-tertiary);
    color: var(--color-text);
  }

  .bulk-button.primary {
    background: var(--color-accent);
    border-color: var(--color-accent);
    color: var(--color-text-on-accent);
  }

  .bulk-button.primary:hover {
    background: var(--color-accent-hover);
    border-color: var(--color-accent-hover);
  }

</style>
