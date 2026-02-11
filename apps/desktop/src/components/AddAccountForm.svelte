<script>
  import { tauri_invoke } from '../lib/tauri_bridge.js';

  let { db_path, on_success, on_cancel } = $props();

  const DEFAULT_IMAP_PORT = 993;

  // Auth method: 'imap' (password) or 'google' (OAuth2)
  let auth_method = $state('imap');

  // IMAP password form state
  let email_address = $state('');
  let imap_host = $state('');
  let imap_port = $state(DEFAULT_IMAP_PORT);
  let imap_username = $state('');
  let password = $state('');
  let mailbox_selection_mode = $state('manual');

  // Google OAuth state
  let google_email = $state('');
  /**
   * Generation counter for the Google OAuth flow.  Incremented on each
   * attempt so that stale backend responses (from a cancelled/retried flow)
   * are silently discarded.
   */
  let google_request_generation = 0;

  // After connection
  /** @type {any | null} */
  let created_account = $state(null);
  /** @type {any[]} */
  let mailboxes = $state([]);

  let busy = $state(false);
  let error_message = $state('');

  /** @type {'connect' | 'folders'} */
  let step = $state('connect');


  /**
   * Auto-fill IMAP username from email on blur, but only when the
   * username field is still empty (never touched by user or auto-fill).
   */
  function sync_username_from_email() {
    if (!imap_username.trim() && email_address.trim()) {
      imap_username = email_address.trim();
    }
  }


  /**
   * Translate raw backend/IMAP error strings into user-friendly messages.
   * @param {unknown} err
   * @returns {string}
   */
  function humanize_connection_error(err) {
    const raw = err instanceof Error ? err.message : String(err);

    if (raw.includes('login failed') || raw.includes('AUTHENTICATIONFAILED')) {
      return 'Authentication failed. Please check your username and password.';
    }
    if (raw.includes('tcp connect failed')) {
      return 'Could not reach the mail server. Please check the IMAP host and port.';
    }
    if (raw.includes('tls handshake failed')) {
      return 'Secure connection failed. The server may not support TLS on this port.';
    }
    if (raw.includes('unsupported security mode')) {
      return 'This app requires a TLS connection. Please use a port that supports TLS (usually 993).';
    }
    if (raw.includes('imap protocol error')) {
      return 'Unexpected response from the mail server. Please verify your settings.';
    }
    if (raw.includes('not configured')) {
      return 'Google sign-in is not available. The app was not built with Google OAuth credentials.';
    }
    if (raw.includes('CallbackTimeout')) {
      return 'Authorization timed out. Please try again.';
    }
    if (raw.includes('AuthorizationDenied')) {
      return 'Authorization was denied. Please try again and grant access.';
    }

    return raw;
  }

  /** Basic email format validation (user@domain). */
  const EMAIL_PATTERN = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

  function validate_imap() {
    if (!email_address.trim()) {
      error_message = 'Email address is required.';
      return false;
    }
    if (!EMAIL_PATTERN.test(email_address.trim())) {
      error_message = 'Please enter a valid email address.';
      return false;
    }
    if (!imap_host.trim()) {
      error_message = 'IMAP host is required.';
      return false;
    }
    if (!imap_username.trim()) {
      error_message = 'IMAP username is required.';
      return false;
    }
    if (!password) {
      error_message = 'Password is required.';
      return false;
    }
    if (!Number.isFinite(Number(imap_port)) || Number(imap_port) <= 0) {
      error_message = 'IMAP port must be a positive number.';
      return false;
    }
    return true;
  }

  function validate_google() {
    if (!google_email.trim()) {
      error_message = 'Email address is required.';
      return false;
    }
    if (!EMAIL_PATTERN.test(google_email.trim())) {
      error_message = 'Please enter a valid email address.';
      return false;
    }
    return true;
  }

  async function handle_connect() {
    error_message = '';
    if (!validate_imap()) return;

    busy = true;
    try {
      const result = await tauri_invoke('create_account_and_discover_mailboxes', {
        dbPath: db_path,
        input: {
          label: email_address.trim(),
          email_address: email_address.trim(),
          imap_host: imap_host.trim(),
          imap_port: Number(imap_port),
          imap_username: imap_username.trim(),
          password,
          mailbox_selection_mode,
        },
      });
      created_account = result.account;
      mailboxes = result.mailboxes ?? [];
      step = 'folders';
    } catch (err) {
      error_message = humanize_connection_error(err);
    } finally {
      busy = false;
    }
  }

  async function handle_google_connect() {
    error_message = '';
    if (!validate_google()) return;

    const this_generation = ++google_request_generation;
    busy = true;
    try {
      const result = await tauri_invoke('add_google_oauth_account', {
        dbPath: db_path,
        input: {
          email: google_email.trim(),
          mailbox_selection_mode,
        },
      });
      // If the user cancelled while we were waiting, discard this result.
      if (this_generation !== google_request_generation) return;
      created_account = result.account;
      mailboxes = result.mailboxes ?? [];
      step = 'folders';
    } catch (err) {
      if (this_generation !== google_request_generation) return;
      error_message = humanize_connection_error(err);
    } finally {
      if (this_generation === google_request_generation) {
        busy = false;
      }
    }
  }

  /**
   * Cancel a pending Google OAuth flow.  Bumps the generation counter so
   * the in-flight backend response is silently ignored when it arrives.
   */
  function cancel_google_auth() {
    google_request_generation++;
    busy = false;
    error_message = '';
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

  function handle_save() {
    if (created_account) {
      on_success?.(created_account);
    }
  }
</script>

<div class="add-account-form">
  <h1 class="title">Add account</h1>

  {#if step === 'connect'}
    <section class="section">
      <!-- Auth method selector -->
      <div class="auth-method-selector" role="tablist" aria-label="Authentication method">
        <button
          class="auth-method-tab"
          class:active={auth_method === 'imap'}
          role="tab"
          aria-selected={auth_method === 'imap'}
          onclick={() => { auth_method = 'imap'; error_message = ''; }}
          disabled={busy}
        >
          IMAP (Password)
        </button>
        <button
          class="auth-method-tab"
          class:active={auth_method === 'google'}
          role="tab"
          aria-selected={auth_method === 'google'}
          onclick={() => { auth_method = 'google'; error_message = ''; }}
          disabled={busy}
        >
          Google / Gmail
        </button>
      </div>

      {#if auth_method === 'imap'}
        <!-- Existing IMAP password form -->
        <p class="description">Enter your IMAP credentials. TLS is always enabled.</p>

        <form class="form" onsubmit={(e) => { e.preventDefault(); handle_connect(); }}>
          <div class="form-group">
            <label class="label" for="email">Email address</label>
            <input
              id="email"
              class="input"
              type="email"
              bind:value={email_address}
              onblur={sync_username_from_email}
              placeholder="you@example.com"
              disabled={busy}
            />
          </div>

          <div class="form-group">
            <label class="label" for="password">Password</label>
            <input
              id="password"
              class="input"
              type="password"
              bind:value={password}
              disabled={busy}
            />
          </div>

          <div class="form-row">
            <div class="form-group flex-1">
              <label class="label" for="imap_host">IMAP server</label>
              <input
                id="imap_host"
                class="input"
                type="text"
                bind:value={imap_host}
                placeholder="imap.example.com"
                disabled={busy}
              />
            </div>
            <div class="form-group width-100">
              <label class="label" for="imap_port">Port</label>
              <input
                id="imap_port"
                class="input"
                type="number"
                bind:value={imap_port}
                disabled={busy}
              />
            </div>
          </div>

          <div class="form-group">
            <label class="label" for="imap_username">IMAP username</label>
            <input
              id="imap_username"
              class="input"
              type="text"
              bind:value={imap_username}

              disabled={busy}
            />
            <p class="hint">Usually the same as your email address.</p>
          </div>

          {#if error_message}
            <p class="error">{error_message}</p>
          {/if}

          <div class="actions">
            <button type="button" class="btn" onclick={on_cancel} disabled={busy}>
              Cancel
            </button>
            <button type="submit" class="btn primary" disabled={busy}>
              {busy ? 'Connecting...' : 'Connect & discover folders'}
            </button>
          </div>
        </form>
      {:else}
        <!-- Google OAuth — simple sign-in flow -->
        <p class="description">
          Sign in with your Google account to archive Gmail or Google Workspace emails.
        </p>

        <form class="form" onsubmit={(e) => { e.preventDefault(); handle_google_connect(); }}>
          <div class="form-group">
            <label class="label" for="google_email">Google email address</label>
            <input
              id="google_email"
              class="input"
              type="email"
              bind:value={google_email}
              placeholder="you@gmail.com"
              disabled={busy}
            />
          </div>

          {#if error_message}
            <p class="error">{error_message}</p>
          {/if}

          {#if busy}
            <p class="hint oauth-hint">
              A browser window should have opened. Complete the sign-in there, then return here.
            </p>
            <div class="actions">
              <button type="button" class="btn" onclick={cancel_google_auth}>
                Cancel
              </button>
              <button type="button" class="btn" onclick={() => { cancel_google_auth(); handle_google_connect(); }}>
                Retry
              </button>
              <span class="waiting-indicator">Waiting for authorization...</span>
            </div>
          {:else}
            <div class="actions">
              <button type="button" class="btn" onclick={on_cancel}>
                Cancel
              </button>
              <button type="submit" class="btn primary google-btn">
                Sign in with Google
              </button>
            </div>
          {/if}
        </form>
      {/if}
    </section>
  {:else}
    <section class="section">
      <p class="description">Select which folders to sync for {created_account?.email_address}.</p>

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
              {/if}
            </label>
          {/each}
        </div>
      {/if}

      {#if error_message}
        <p class="error">{error_message}</p>
      {/if}

      <div class="actions">
        <button type="button" class="btn primary" onclick={handle_save}>
          Save
        </button>
      </div>
    </section>
  {/if}
</div>

<style>
  .add-account-form {
    max-width: 420px;
  }

  .title {
    margin: 0 0 var(--space-lg);
    font-size: var(--font-size-lg);
    font-weight: var(--font-weight-semibold);
    color: var(--color-text);
  }

  .section {
    margin-bottom: var(--space-xl);
  }

  .description {
    color: var(--color-text-secondary);
    margin: 0 0 var(--space-xl);
    font-size: var(--font-size-sm);
  }

  /* Auth method selector */
  .auth-method-selector {
    display: flex;
    gap: 0;
    margin-bottom: var(--space-xl);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-md);
    overflow: hidden;
  }

  .auth-method-tab {
    flex: 1;
    padding: var(--space-sm) var(--space-md);
    border: none;
    background: var(--color-bg);
    color: var(--color-text-secondary);
    font: inherit;
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-medium);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .auth-method-tab:not(:last-child) {
    border-right: 1px solid var(--color-border-strong);
  }

  .auth-method-tab.active {
    background: var(--color-accent);
    color: var(--color-text-on-accent);
  }

  .auth-method-tab:hover:not(.active):not(:disabled) {
    background: var(--color-bg-secondary);
  }

  .auth-method-tab:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .oauth-hint {
    text-align: center;
    margin-top: 0;
    margin-bottom: 0;
  }

  .waiting-indicator {
    font-size: var(--font-size-sm);
    color: var(--color-text-tertiary);
    margin-left: auto;
  }

  .google-btn {
    min-width: 200px;
  }

  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-lg);
  }

  .form-row {
    display: flex;
    gap: var(--space-md);
  }

  .form-group {
    display: flex;
    flex-direction: column;
    gap: var(--space-xs);
  }

  .form-group.flex-1 {
    flex: 1;
  }

  .form-group.width-100 {
    width: 80px;
  }

  .label {
    font-weight: var(--font-weight-medium);
    font-size: var(--font-size-sm);
    color: var(--color-text);
  }

  .input {
    padding: var(--space-sm) var(--space-md);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-md);
    background: var(--color-bg);
    color: var(--color-text);
    font: inherit;
    font-size: var(--font-size-sm);
    transition: all var(--transition-fast);
  }

  .input:hover:not(:disabled) {
    border-color: var(--color-accent-muted);
  }

  .input:focus {
    outline: none;
    border-color: var(--color-accent);
    box-shadow: 0 0 0 3px var(--color-accent-soft);
  }

  .input:disabled {
    opacity: 0.5;
    background: var(--color-bg-secondary);
  }

  .hint {
    color: var(--color-text-tertiary);
    font-size: var(--font-size-xs);
    margin: var(--space-xs) 0 0;
  }

  .error {
    color: var(--color-error);
    margin: 0;
    font-size: var(--font-size-sm);
  }

  .actions {
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

  .btn.primary {
    background: var(--color-accent);
    color: var(--color-text-on-accent);
    border-color: var(--color-accent);
  }

  .btn.primary:hover:not(:disabled) {
    background: var(--color-accent-hover);
    border-color: var(--color-accent-hover);
  }

  /* Folder list */
  .folder-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-bottom: var(--space-lg);
  }

  .folder-item {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding: var(--space-sm) 0;
    cursor: pointer;
    font-size: var(--font-size-sm);
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

  .folder-tag.folder-gobd {
    color: var(--color-accent);
    font-weight: var(--font-weight-medium);
    background: var(--color-accent-soft);
    padding: 1px 5px;
    border-radius: var(--radius-sm);
    font-size: 10px;
    letter-spacing: 0.03em;
  }
</style>
