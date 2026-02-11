<script>
  /**
   * Confirmation dialog that requires typing specific text to confirm.
   */

  let { title, message, confirm_text, on_confirm, on_cancel } = $props();

  let input_value = $state('');

  let can_confirm = $derived(input_value === confirm_text);

  function handle_submit(event) {
    event.preventDefault();
    if (can_confirm) {
      on_confirm?.();
    }
  }

  function handle_backdrop_click(event) {
    if (event.target === event.currentTarget) {
      on_cancel?.();
    }
  }

  function handle_keydown(event) {
    if (event.key === 'Escape') {
      on_cancel?.();
    }
  }
</script>

<svelte:window onkeydown={handle_keydown} />

<div class="dialog-backdrop" onclick={handle_backdrop_click} role="presentation">
  <div class="dialog" role="alertdialog" aria-labelledby="dialog-title" aria-describedby="dialog-message">
    <h2 id="dialog-title" class="dialog-title">{title}</h2>
    <p id="dialog-message" class="dialog-message">{message}</p>

    <form class="dialog-form" onsubmit={handle_submit}>
      <input
        type="text"
        class="dialog-input"
        bind:value={input_value}
        placeholder="Type to confirm..."
        autocomplete="off"
        autofocus
      />
      <p class="dialog-hint">Type: <code>{confirm_text}</code></p>

      <div class="dialog-actions">
        <button type="button" class="btn" onclick={on_cancel}>
          Cancel
        </button>
        <button type="submit" class="btn danger" disabled={!can_confirm}>
          Confirm
        </button>
      </div>
    </form>
  </div>
</div>

<style>
  .dialog-backdrop {
    position: fixed;
    inset: 0;
    background: oklch(0% 0 0 / 0.4);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
    backdrop-filter: blur(2px);
  }

  .dialog {
    background: var(--color-bg);
    border-radius: var(--radius-lg);
    padding: var(--space-xl);
    max-width: 380px;
    width: 90%;
    box-shadow: var(--shadow-lg), 0 0 0 1px var(--color-border);
  }

  .dialog-title {
    margin: 0 0 var(--space-sm);
    font-size: var(--font-size-md);
    font-weight: var(--font-weight-semibold);
    color: var(--color-text);
  }

  .dialog-message {
    margin: 0 0 var(--space-lg);
    color: var(--color-text-secondary);
    line-height: 1.5;
    font-size: var(--font-size-sm);
  }

  .dialog-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-md);
  }

  .dialog-input {
    padding: var(--space-sm) var(--space-md);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-md);
    background: var(--color-bg);
    color: var(--color-text);
    font: inherit;
    font-size: var(--font-size-sm);
    transition: all var(--transition-fast);
  }

  .dialog-input:focus {
    outline: none;
    border-color: var(--color-accent);
    box-shadow: 0 0 0 3px var(--color-accent-soft);
  }

  .dialog-hint {
    margin: 0;
    font-size: var(--font-size-xs);
    color: var(--color-text-tertiary);
  }

  .dialog-hint code {
    background: var(--color-bg-tertiary);
    padding: 2px 6px;
    border-radius: var(--radius-sm);
    font-family: ui-monospace, 'SF Mono', Menlo, monospace;
    font-size: var(--font-size-xs);
    color: var(--color-text);
  }

  .dialog-actions {
    display: flex;
    gap: var(--space-sm);
    justify-content: flex-end;
    margin-top: var(--space-sm);
  }

  .btn {
    padding: var(--space-sm) var(--space-md);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-md);
    background: var(--color-bg);
    color: var(--color-text);
    font: inherit;
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-medium);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .btn:hover:not(:disabled) {
    background: var(--color-bg-tertiary);
  }

  .btn:active:not(:disabled) {
    transform: scale(0.98);
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn.danger {
    background: var(--color-error);
    color: white;
    border-color: var(--color-error);
  }

  .btn.danger:hover:not(:disabled) {
    background: oklch(45% 0.2 25);
    border-color: oklch(45% 0.2 25);
  }

  .btn.danger:disabled {
    background: oklch(55% 0.15 25 / 0.5);
    border-color: transparent;
  }
</style>
