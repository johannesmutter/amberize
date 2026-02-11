<script>
  /**
   * Virtual scrolling list with optional checkboxes for bulk selection.
   * Fixed 80px row height for consistent rendering.
   */

  let {
    items = [],
    bulk_mode = false,
    selected_ids = new Set(),
    selected_item_id = null,
    show_location = true,
    on_select,
    on_toggle_selection,
    on_click,
    on_reach_end,
  } = $props();

  const ROW_HEIGHT = 80;
  const BUFFER_COUNT = 5;

  let container_element = $state(null);
  let scroll_top = $state(0);
  let container_height = $state(400);

  $effect(() => {
    if (!container_element) return;

    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        container_height = entry.contentRect.height;
      }
    });

    observer.observe(container_element);
    container_height = container_element.clientHeight;

    return () => observer.disconnect();
  });

  $effect(() => {
    if (!container_element) return;
    if (selected_item_id == null) return;

    const selected_index = items.findIndex((item) => item.id === selected_item_id);
    if (selected_index < 0) return;

    const item_top = selected_index * ROW_HEIGHT;
    const item_bottom = item_top + ROW_HEIGHT;
    const viewport_top = container_element.scrollTop;
    const viewport_bottom = viewport_top + container_height;

    if (item_top < viewport_top) {
      container_element.scrollTop = item_top;
      return;
    }

    if (item_bottom > viewport_bottom) {
      container_element.scrollTop = item_bottom - container_height;
    }
  });

  function handle_scroll(event) {
    scroll_top = event.target.scrollTop;

    if (!on_reach_end) return;
    const remaining_px = event.target.scrollHeight - (event.target.scrollTop + event.target.clientHeight);
    const threshold_px = ROW_HEIGHT * BUFFER_COUNT * 2;
    if (remaining_px <= threshold_px) {
      on_reach_end?.();
    }
  }

  let visible_range = $derived.by(() => {
    const start_index = Math.max(0, Math.floor(scroll_top / ROW_HEIGHT) - BUFFER_COUNT);
    const visible_count = Math.ceil(container_height / ROW_HEIGHT) + BUFFER_COUNT * 2;
    const end_index = Math.min(items.length, start_index + visible_count);
    return { start_index, end_index };
  });

  let visible_items = $derived.by(() => {
    const { start_index, end_index } = visible_range;
    return items.slice(start_index, end_index).map((item, i) => ({
      item,
      index: start_index + i,
      top: (start_index + i) * ROW_HEIGHT,
    }));
  });

  let total_height = $derived(items.length * ROW_HEIGHT);

  function handle_row_click(item) {
    on_click?.(item);
  }

  function handle_checkbox_change(event, item) {
    event.stopPropagation();
    on_toggle_selection?.(item.id);
  }

  /**
   * Format participants (from/to) display
   * @param {any} item
   */
  function format_participants(item) {
    return item.from_address || 'Unknown sender';
  }

  /**
   * Format location display (folder + account)
   * @param {any} item
   */
  function format_location(item) {
    // TODO: backend needs to return mailbox_name and account_email
    const folder = item.mailbox_name || 'Inbox';
    const account = item.account_email || '';
    if (!show_location) return format_date(item);
    return account ? `${folder} ${account} ${format_date(item)}` : `${folder} ${format_date(item)}`;
  }

  /**
   * Format date for display
   * @param {any} item
   */
  function format_date(item) {
    if (!item.date_header) return '';
    // Parse the date and format it nicely
    try {
      const date = new Date(item.date_header);
      const now = new Date();
      const diff_days = Math.floor((now.getTime() - date.getTime()) / (1000 * 60 * 60 * 24));

      if (diff_days === 0) {
        return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
      } else if (diff_days < 7) {
        return date.toLocaleDateString([], { weekday: 'short' });
      } else if (date.getFullYear() === now.getFullYear()) {
        return date.toLocaleDateString([], { day: 'numeric', month: 'short' });
      }
      return date.toLocaleDateString([], { day: '2-digit', month: '2-digit', year: '2-digit' });
    } catch {
      return item.date_header;
    }
  }
</script>

<div
  class="virtual-list"
  bind:this={container_element}
  onscroll={handle_scroll}
>
  <div class="spacer" style="height: {total_height}px;">
    {#each visible_items as { item, index, top } (item.id)}
      <div
        class="row"
        class:selected={selected_item_id === item.id}
        class:checked={selected_ids.has(item.id)}
        style="top: {top}px;"
        onclick={() => handle_row_click(item)}
        role="button"
        tabindex="0"
        onkeydown={(e) => e.key === 'Enter' && handle_row_click(item)}
      >
        {#if bulk_mode}
          <div class="checkbox-col">
            <input
              type="checkbox"
              checked={selected_ids.has(item.id)}
              onchange={(e) => handle_checkbox_change(e, item)}
              onclick={(e) => e.stopPropagation()}
            />
          </div>
        {/if}
        <div class="content">
          <div class="row-1">
            <span class="participants">{format_participants(item)}</span>
            <span class="location">{format_location(item)}</span>
          </div>
          <div class="row-2">
            <span class="subject">{item.subject || '(no subject)'}</span>
          </div>
          <div class="row-3">
            <span class="snippet">{item.snippet || ''}</span>
          </div>
        </div>
      </div>
    {/each}
  </div>
</div>

<style>
  .virtual-list {
    flex: 1;
    overflow-y: auto;
    position: relative;
    background: var(--color-bg);
  }

  .spacer {
    position: relative;
  }

  .row {
    position: absolute;
    left: 0;
    right: 0;
    height: 80px;
    display: flex;
    align-items: flex-start;
    padding: var(--space-md) var(--space-lg);
    border-bottom: 1px solid var(--color-border);
    cursor: pointer;
    background: var(--color-bg);
    transition: background var(--transition-fast);
  }

  .row:hover {
    background: var(--color-bg-secondary);
  }

  .row.selected {
    background: var(--color-accent-soft);
  }

  .row.checked {
    background: var(--color-accent-soft);
  }

  .checkbox-col {
    flex-shrink: 0;
    width: 20px;
    margin-right: var(--space-sm);
    padding-top: 2px;
  }

  .checkbox-col input {
    margin: 0;
    cursor: pointer;
  }

  .content {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .row-1 {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: var(--space-md);
  }

  .participants {
    font-weight: var(--font-weight-semibold);
    color: var(--color-text);
    font-size: var(--font-size-sm);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .location {
    color: var(--color-text-tertiary);
    font-size: var(--font-size-xs);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .row-2 {
    overflow: hidden;
  }

  .subject {
    color: var(--color-text);
    font-size: var(--font-size-sm);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    display: block;
  }

  .row-3 {
    overflow: hidden;
  }

  .snippet {
    color: var(--color-text-secondary);
    font-size: var(--font-size-sm);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    display: block;
  }
</style>
