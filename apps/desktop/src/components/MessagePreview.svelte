<script>
  /**
   * Message preview panel — renders a parsed message detail with
   * headers, body (HTML or plain-text), attachments list, and export button.
   *
   * Expected `message` shape (from get_message_detail):
   *   { id, sha256, subject, from_address, to_addresses, cc_addresses,
   *     date_header, body_text, body_html, attachments }
   */

  import DOMPurify from 'dompurify';
  import EmptyState from './EmptyState.svelte';

  let {
    message = null,
    loading = false,
    on_export,
  } = $props();

  /**
   * Format an ISO date string for display.
   * @param {string | null | undefined} date_str
   * @returns {string}
   */
  function format_date(date_str) {
    if (!date_str) return '';
    try {
      const date = new Date(date_str);
      return date.toLocaleString([], {
        weekday: 'short',
        year: 'numeric',
        month: 'short',
        day: 'numeric',
        hour: '2-digit',
        minute: '2-digit',
      });
    } catch {
      return date_str;
    }
  }

  /**
   * Format byte size for display.
   * @param {number} bytes
   * @returns {string}
   */
  function format_size(bytes) {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  /**
   * Sanitize untrusted email HTML using DOMPurify.
   * Strips scripts, event handlers, javascript: URIs, dangerous elements
   * (iframe, object, embed, form, link, base, meta), and SVG script injection.
   * The HTML is also rendered inside a sandboxed iframe with a strict CSP.
   * @param {string} html
   * @returns {string}
   */
  function sanitize_html(html) {
    return DOMPurify.sanitize(html, {
      FORBID_TAGS: ['style', 'form', 'input', 'textarea', 'select', 'button', 'base', 'link', 'meta'],
      FORBID_ATTR: ['style'],
      ALLOW_DATA_ATTR: false,
    });
  }

  /** Pattern matching external URLs in src attributes (http:// or https://). */
  const EXTERNAL_IMAGE_PATTERN = /(?:src|background)\s*=\s*["']https?:\/\//i;

  /**
   * Check whether the email HTML references any external (remote) images.
   * @param {string} html
   * @returns {boolean}
   */
  function has_external_images(html) {
    return EXTERNAL_IMAGE_PATTERN.test(html);
  }

  /**
   * Detect whether HTML likely contains rich layout/styling that should be
   * rendered as HTML instead of falling back to plain text.
   * @param {string} html
   * @returns {boolean}
   */
  function has_rich_html_markup(html) {
    return /<(table|img|style|head|body|font|button|svg|video|picture)\b/i.test(html)
      || /(class|id|bgcolor|background|align)=["'][^"']+["']/i.test(html);
  }

  /**
   * Build a self-contained HTML document for the email iframe.
   * Includes a small base style reset so the email body renders cleanly
   * inside the preview pane. No scripts — the parent measures height
   * directly via contentDocument (possible because of allow-same-origin).
   *
   * When `block_remote` is true a CSP meta tag restricts images to
   * data: URIs only, preventing any network requests for remote images.
   * @param {string} body_html
   * @param {boolean} block_remote
   * @returns {string}
   */
  function build_iframe_srcdoc(body_html, block_remote) {
    const sanitized = sanitize_html(body_html);
    const img_policy = block_remote ? 'img-src data:;' : 'img-src data: https: http:;';
    const csp_tag = `<meta http-equiv="Content-Security-Policy" content="default-src 'none'; ${img_policy} style-src 'unsafe-inline'; font-src data:; base-uri 'none'; form-action 'none';">`;
    return `<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
${csp_tag}
<style>
  html, body {
    margin: 0;
    padding: 0;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    font-size: 14px;
    line-height: 1.5;
    /* Keep email rendering in a stable light mode to avoid dark-mode clashes
       with sender-provided HTML/CSS that can make text unreadable. */
    color-scheme: light;
    color: #1d1d1f;
    word-break: break-word;
    overflow-wrap: break-word;
    background: #ffffff;
  }
  img { max-width: 100%; height: auto; }
  a { color: #0066cc; }
  blockquote {
    margin: 0.5em 0;
    padding-left: 1em;
    border-left: 3px solid #d1d1d6;
    color: #6e6e73;
  }
  pre, code {
    background: #f5f5f7;
    padding: 2px 4px;
    border-radius: 3px;
    font-size: 13px;
  }
  table { border-collapse: collapse; max-width: 100%; }
  td, th { padding: 4px 8px; }
</style>
</head>
<body>${sanitized}</body>
</html>`;
  }

  let has_html = $derived(!!message?.body_html);
  let has_text = $derived(!!message?.body_text);
  let has_rich_html = $derived(has_html && has_rich_html_markup(message.body_html));
  let render_html = $derived(has_html && (!has_text || has_rich_html));
  let contains_external_images = $derived(render_html && has_external_images(message.body_html));

  /** User opt-in to load remote images for the current message. */
  let allow_external_images = $state(false);

  /** Reset the opt-in whenever a different message is selected. */
  $effect(() => {
    // Read message id to create the reactive dependency.
    message?.id;
    allow_external_images = false;
  });

  let block_remote = $derived(contains_external_images && !allow_external_images);
  let srcdoc = $derived(render_html ? build_iframe_srcdoc(message.body_html, block_remote) : '');

  /** @type {HTMLIFrameElement | undefined} */
  let iframe_el = $state();

  /**
   * After the iframe loads, measure its content height and resize the
   * iframe element so the parent .body div handles all scrolling.
   * A ResizeObserver on the iframe body catches late-loading content
   * (e.g. images) that may change the height after initial load.
   */
  function resize_iframe() {
    if (!iframe_el?.contentDocument) return;
    const height = iframe_el.contentDocument.documentElement.scrollHeight;
    iframe_el.style.height = `${height}px`;
  }

  $effect(() => {
    if (!iframe_el) return;

    /** @type {ResizeObserver | null} */
    let observer = null;

    function on_load() {
      resize_iframe();
      // Watch for late content changes (image loads, font swaps, etc.)
      if (iframe_el?.contentDocument?.body) {
        observer = new ResizeObserver(resize_iframe);
        observer.observe(iframe_el.contentDocument.body);
      }
    }

    iframe_el.addEventListener('load', on_load);
    return () => {
      iframe_el?.removeEventListener('load', on_load);
      observer?.disconnect();
    };
  });

  /** Maximum data URI size to render (2 MB). Larger images are treated as file attachments. */
  const MAX_DATA_URI_RENDER_SIZE = 2 * 1024 * 1024;

  /** Inline images with data URIs that aren't CID-referenced in the HTML body. */
  let standalone_images = $derived(
    (message?.attachments ?? []).filter(a => {
      if (!a.data_uri) return false;
      // Reject data URIs that exceed the size limit to prevent memory issues.
      if (a.data_uri.length > MAX_DATA_URI_RENDER_SIZE) return false;
      // If CID is referenced in HTML, it's already embedded — skip.
      if (a.content_id && render_html && message.body_html.includes(`cid:${a.content_id}`)) return false;
      return true;
    })
  );

  /** Non-image attachments (or images too large for data URIs). */
  let file_attachments = $derived(
    (message?.attachments ?? []).filter(a => !a.data_uri)
  );
</script>

<div class="message-preview">
  {#if loading}
    <EmptyState compact={true} variant="message" title="Loading..." />
  {:else if !message}
    <EmptyState compact={true} variant="message" title="Select an email to view" />
  {:else}
    <div class="headers">
      <button type="button" class="export-button" onclick={on_export}>
        Export .eml
      </button>
      <div class="header-row">
        <span class="header-label">From:</span>
        <span class="header-value">{message.from_address || 'Unknown'}</span>
      </div>
      {#if message.to_addresses}
        <div class="header-row">
          <span class="header-label">To:</span>
          <span class="header-value">{message.to_addresses}</span>
        </div>
      {/if}
      {#if message.cc_addresses}
        <div class="header-row">
          <span class="header-label">Cc:</span>
          <span class="header-value">{message.cc_addresses}</span>
        </div>
      {/if}
      <div class="header-row">
        <span class="header-label">Subject:</span>
        <span class="header-value subject">{message.subject || '(no subject)'}</span>
      </div>
      <div class="header-row">
        <span class="header-label">Date:</span>
        <span class="header-value">{format_date(message.date_header)}</span>
      </div>
    </div>

    {#if block_remote}
      <div class="external-images-bar">
        <span class="external-images-text">External images are hidden to protect your privacy.</span>
        <button
          type="button"
          class="external-images-btn"
          onclick={() => { allow_external_images = true; }}
        >Load external images</button>
      </div>
    {/if}

    <div class="body">
      {#if render_html}
        <div class="body-html">
          <iframe
            bind:this={iframe_el}
            title="Email content"
            {srcdoc}
            sandbox="allow-same-origin"
            class="email-iframe"
          ></iframe>
        </div>
      {:else if has_text}
        <pre class="body-text">{message.body_text}</pre>
      {:else if standalone_images.length === 0}
        <div class="body-empty">No message body.</div>
      {/if}

      {#if standalone_images.length > 0}
        <div class="inline-images">
          {#each standalone_images as img}
            <figure class="inline-image">
              <img src={img.data_uri} alt={img.filename || 'Attached image'} />
              {#if img.filename}
                <figcaption>{img.filename}</figcaption>
              {/if}
            </figure>
          {/each}
        </div>
      {/if}
    </div>

    {#if file_attachments.length > 0}
      <div class="attachments">
        <span class="attachments-label">
          {file_attachments.length} attachment{file_attachments.length === 1 ? '' : 's'}
        </span>
        <div class="attachment-list">
          {#each file_attachments as att}
            <div class="attachment-chip">
              <span class="attachment-icon">📎</span>
              <span class="attachment-name">{att.filename || 'unnamed'}</span>
              <span class="attachment-size">{format_size(att.size)}</span>
            </div>
          {/each}
        </div>
      </div>
    {/if}

  {/if}
</div>

<style>
  .message-preview {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--color-bg-secondary);
  }

  /* ─── Headers ─── */
  .headers {
    position: relative;
    padding: var(--space-lg);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    background: var(--color-bg);
  }

  .header-row {
    display: flex;
    gap: var(--space-sm);
    margin-bottom: var(--space-xs);
    line-height: 1.4;
    font-size: var(--font-size-sm);
  }

  .header-label {
    color: var(--color-text-tertiary);
    min-width: 55px;
    flex-shrink: 0;
  }

  .header-value {
    color: var(--color-text);
    word-break: break-word;
  }

  .header-value.subject {
    font-weight: var(--font-weight-semibold);
    font-size: var(--font-size-md);
    color: var(--color-text);
  }

  /* ─── External images bar ─── */
  .external-images-bar {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding: var(--space-sm) var(--space-lg);
    background: var(--color-bg-tertiary, #f5f5f7);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .external-images-text {
    font-size: var(--font-size-xs);
    color: var(--color-text-secondary);
    flex: 1;
  }

  .external-images-btn {
    padding: var(--space-xs) var(--space-sm);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: var(--color-accent);
    cursor: pointer;
    font: inherit;
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-medium);
    white-space: nowrap;
    transition: all var(--transition-fast);
  }

  .external-images-btn:hover {
    background: var(--color-bg-tertiary);
  }

  .external-images-btn:active {
    transform: scale(0.98);
  }

  /* ─── Body ─── */
  .body {
    flex: 1;
    overflow: auto;
    background: var(--color-bg);
    min-height: 0;
  }

  .body-html {
    padding: var(--space-lg);
  }

  .email-iframe {
    width: 100%;
    min-height: 80px;
    border: none;
    display: block;
    background: transparent;
  }

  .body-text {
    margin: 0;
    padding: var(--space-lg);
    font-family: var(--font-family);
    font-size: var(--font-size-sm);
    line-height: 1.6;
    white-space: pre-wrap;
    word-break: break-word;
    color: var(--color-text);
  }

  .body-empty {
    padding: var(--space-lg);
    color: var(--color-text-tertiary);
    font-size: var(--font-size-sm);
  }

  .inline-images {
    padding: var(--space-md) var(--space-lg);
  }

  .inline-image {
    margin: 0 0 var(--space-md);
  }

  .inline-image img {
    max-width: 100%;
    height: auto;
    border-radius: 4px;
    display: block;
  }

  .inline-image figcaption {
    margin-top: var(--space-xs);
    font-size: var(--font-size-xs);
    color: var(--color-text-tertiary);
  }

  /* ─── Attachments ─── */
  .attachments {
    padding: var(--space-sm) var(--space-lg);
    border-top: 1px solid var(--color-border);
    flex-shrink: 0;
    background: var(--color-bg);
  }

  .attachments-label {
    display: block;
    font-size: var(--font-size-xs);
    color: var(--color-text-tertiary);
    margin-bottom: var(--space-xs);
    font-weight: var(--font-weight-medium);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .attachment-list {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-xs);
  }

  .attachment-chip {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 3px 8px;
    border-radius: var(--radius-sm);
    background: var(--color-bg-secondary);
    border: 1px solid var(--color-border);
    font-size: var(--font-size-xs);
    color: var(--color-text-secondary);
  }

  .attachment-icon {
    font-size: 0.85em;
  }

  .attachment-name {
    color: var(--color-text);
    max-width: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .attachment-size {
    color: var(--color-text-tertiary);
    flex-shrink: 0;
  }

  /* ─── Export button (top-right of headers) ─── */
  .export-button {
    position: absolute;
    top: var(--space-lg);
    right: var(--space-lg);
    padding: var(--space-xs) var(--space-sm);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: var(--color-text-secondary);
    cursor: pointer;
    font: inherit;
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-medium);
    transition: all var(--transition-fast);
    white-space: nowrap;
  }

  .export-button:hover {
    background: var(--color-bg-tertiary);
    color: var(--color-text);
  }

  .export-button:active {
    transform: scale(0.98);
  }
</style>
