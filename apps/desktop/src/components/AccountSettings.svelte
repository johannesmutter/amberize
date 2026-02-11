<script>
  import { tauri_invoke } from '../lib/tauri_bridge.js';
  import ConfirmDialog from './ConfirmDialog.svelte';

  let { db_path, account, on_removed } = $props();

  /** @type {any[]} */
  let mailboxes = $state([]);
  let error_message = $state('');
  let show_remove_dialog = $state(false);

  // Credentials repair (keychain)
  let password_value = $state('');
  let password_busy = $state(false);
  let password_error = $state('');
  let password_saved = $state(false);

  // Load mailboxes when account changes
  $effect(() => {
    if (account?.id) {
      void load_mailboxes(account.id);
    }
  });

  $effect(() => {
    // Clear any “saved” badge as soon as the user edits the input again.
    const _ = password_value;
    password_saved = false;
  });

  async function load_mailboxes(account_id) {
    if (!db_path?.trim()) return;
    try {
      mailboxes = await tauri_invoke('list_mailboxes', {
        dbPath: db_path,
        accountId: account_id,
      });
    } catch (err) {
      error_message = err instanceof Error ? err.message : String(err);
    }
  }

  /**
   * Store the password in the OS keychain for this account and optionally sync immediately.
   * @param {boolean} sync_after
   */
  async function save_password(sync_after) {
    password_error = '';
    if (!db_path?.trim()) return;
    if (!account?.id) return;

    if (!password_value) {
      password_error = 'Password is required.';
      return;
    }

    password_busy = true;
    try {
      await tauri_invoke('set_account_password', {
        dbPath: db_path,
        accountId: account.id,
        password: password_value,
      });

      password_value = '';
      password_saved = true;

      if (sync_after) {
        await tauri_invoke('sync_account_once_command', {
          dbPath: db_path,
          accountId: account.id,
        });
        await load_mailboxes(account.id);
      }
    } catch (err) {
      password_error = err instanceof Error ? err.message : String(err);
    } finally {
      password_busy = false;
    }
  }

  async function toggle_mailbox(mailbox) {
    error_message = '';
    const next_enabled = !mailbox.sync_enabled;
    mailbox.sync_enabled = next_enabled;
    mailboxes = [...mailboxes];

    try {
      await tauri_invoke('set_mailbox_sync_enabled', {
        dbPath: db_path,
        mailboxId: mailbox.id,
        syncEnabled: next_enabled,
      });
    } catch (err) {
      mailbox.sync_enabled = !next_enabled;
      mailboxes = [...mailboxes];
      error_message = err instanceof Error ? err.message : String(err);
    }
  }

  /**
   * @param {string | null | undefined} timestamp
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

  function handle_remove_click() {
    show_remove_dialog = true;
  }

  async function handle_remove_confirm() {
    try {
      await tauri_invoke('remove_account', {
        dbPath: db_path,
        accountId: account.id,
      });
      show_remove_dialog = false;
      on_removed?.();
    } catch (err) {
      error_message = err instanceof Error ? err.message : String(err);
      show_remove_dialog = false;
    }
  }

  function handle_remove_cancel() {
    show_remove_dialog = false;
  }
</script>

<div class="account-settings">
  <h1 class="title">{account.email_address}</h1>

  <!-- Connection Section -->
  <section class="section">
    <h2 class="section-title">CONNECTION</h2>
    <div class="info-grid">
      <div class="info-label">IMAP:</div>
      <div class="info-value">{account.imap_host}:{account.imap_port}</div>
      <div class="info-label">Username:</div>
      <div class="info-value">{account.imap_username}</div>
    </div>
  </section>

  <!-- Credentials Section -->
  <section class="section">
    <h2 class="section-title">CREDENTIALS</h2>

    {#if account.auth_kind === 'oauth2'}
      <p class="hint">
        This account uses {account.oauth_provider === 'google' ? 'Google' : 'OAuth'} sign-in.
        Tokens are stored in your system's credential store. To re-authenticate, remove and re-add the account.
      </p>
    {:else}
      <p class="hint">
      If sync shows “missing secret…”, re-enter your password here. It is stored in your system's
      credential store (not in the database).
    </p>

    <div class="password-row">
      <label class="password-label" for="account_password">Password</label>
      <div class="password-controls">
        <input
          id="account_password"
          class="password-input"
          type="password"
          autocomplete="current-password"
          bind:value={password_value}
          placeholder="Enter password…"
          disabled={password_busy}
        />
        <button
          type="button"
          class="password-button"
          onclick={() => save_password(true)}
          disabled={password_busy || !password_value}
        >
          {password_busy ? 'Saving…' : 'Save & sync'}
        </button>
        {#if password_saved}
          <span class="password-saved">Saved</span>
        {/if}
      </div>
    </div>

    {#if password_error}
      <p class="error">{password_error}</p>
    {/if}
    {/if}
  </section>

  <!-- Folders Section -->
  <section class="section">
    <h2 class="section-title">FOLDERS TO SYNC</h2>
    {#if mailboxes.length === 0}
      <p class="hint">No folders found.</p>
    {:else}
      <div class="folder-list">
        {#each mailboxes as mailbox (mailbox.id)}
          <label class="folder-item" class:excluded={mailbox.hard_excluded}>
            <input
              type="checkbox"
              checked={mailbox.sync_enabled}
              disabled={mailbox.hard_excluded}
              onchange={() => toggle_mailbox(mailbox)}
            />
            <span class="folder-name">{mailbox.imap_name}</span>
            {#if mailbox.gobd_recommended}
              <span class="folder-tag folder-gobd" title="Recommended for tax-compliant archiving (GoBD)">GoBD</span>
            {/if}
            {#if mailbox.hard_excluded}
              <span class="folder-tag">(excluded)</span>
            {:else if mailbox.last_error}
              <span class="folder-tag folder-error" title={mailbox.last_error}>(sync error)</span>
            {:else if mailbox.last_sync_at}
              <span class="folder-tag">(last sync {format_relative_time(mailbox.last_sync_at)})</span>
            {/if}
          </label>
        {/each}
      </div>
    {/if}
  </section>

  {#if error_message}
    <p class="error">{error_message}</p>
  {/if}

  <!-- Danger Zone -->
  <section class="section danger-zone">
    <h2 class="section-title danger">DANGER ZONE</h2>
    <button type="button" class="remove-button" onclick={handle_remove_click}>
      Remove this account
    </button>
  </section>
</div>

{#if show_remove_dialog}
  <ConfirmDialog
    title="Remove account"
    message="This will remove the account from your archive. Existing emails will remain in the database. Type the email address to confirm:"
    confirm_text={account.email_address}
    on_confirm={handle_remove_confirm}
    on_cancel={handle_remove_cancel}
  />
{/if}

<style>
  .account-settings {
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

  .info-grid {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--space-sm) var(--space-lg);
    font-size: var(--font-size-sm);
  }

  .info-label {
    color: var(--color-text-secondary);
  }

  .info-value {
    font-family: ui-monospace, 'SF Mono', Menlo, monospace;
    font-size: var(--font-size-sm);
  }

  .hint {
    color: var(--color-text-tertiary);
    margin: 0;
    font-size: var(--font-size-sm);
  }

  .folder-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .folder-item {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding: var(--space-sm) 0;
    cursor: pointer;
    font-size: var(--font-size-sm);
    border-radius: var(--radius-sm);
    transition: background var(--transition-fast);
  }

  .folder-item:hover {
    background: var(--color-bg-secondary);
  }

  .folder-item.excluded {
    opacity: 0.5;
  }

  .folder-item input {
    cursor: pointer;
  }

  .folder-item.excluded input {
    cursor: not-allowed;
  }

  .folder-name {
    font-family: ui-monospace, 'SF Mono', Menlo, monospace;
  }

  .folder-tag {
    color: var(--color-text-tertiary);
    font-size: var(--font-size-xs);
  }

  .folder-tag.folder-error {
    color: var(--color-error);
  }

  .folder-tag.folder-gobd {
    color: var(--color-accent);
    font-weight: var(--font-weight-medium);
    background: var(--color-accent-soft);
    padding: 1px 5px;
    border-radius: var(--radius-sm);
    font-size: 10px;
    letter-spacing: 0.03em;
  }

  .error {
    color: var(--color-error);
    margin: var(--space-md) 0;
    font-size: var(--font-size-sm);
  }

  .password-row {
    display: flex;
    flex-direction: column;
    gap: var(--space-xs);
    margin-top: var(--space-sm);
  }

  .password-label {
    font-weight: var(--font-weight-medium);
    font-size: var(--font-size-sm);
    color: var(--color-text);
  }

  .password-controls {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
  }

  .password-input {
    flex: 1;
    min-width: 180px;
    padding: var(--space-sm) var(--space-md);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-md);
    background: var(--color-bg);
    color: var(--color-text);
    font: inherit;
    font-size: var(--font-size-sm);
    transition: all var(--transition-fast);
  }

  .password-input:focus {
    outline: none;
    border-color: var(--color-accent);
    box-shadow: 0 0 0 3px var(--color-accent-soft);
  }

  .password-input:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .password-button {
    padding: var(--space-sm) var(--space-md);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-md);
    background: var(--color-bg);
    color: var(--color-text-secondary);
    font: inherit;
    font-size: var(--font-size-sm);
    cursor: pointer;
    transition: all var(--transition-fast);
    white-space: nowrap;
  }

  .password-button:hover:not(:disabled) {
    background: var(--color-bg-tertiary);
    color: var(--color-text);
  }

  .password-button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .password-saved {
    font-size: var(--font-size-xs);
    color: var(--color-accent);
    font-weight: var(--font-weight-medium);
  }

  /* Danger Zone - subtle but clear */
  .danger-zone {
    margin-top: var(--space-2xl);
    padding-top: var(--space-lg);
    border-top: 1px solid var(--color-border);
  }

  .section-title.danger {
    color: var(--color-error);
  }

  .remove-button {
    padding: var(--space-sm) var(--space-md);
    border: 1px solid var(--color-error);
    border-radius: var(--radius-md);
    background: transparent;
    color: var(--color-error);
    font: inherit;
    font-size: var(--font-size-sm);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .remove-button:hover {
    background: var(--color-error);
    color: white;
  }
</style>
