<script>
  import { browser } from '$app/environment';
  import { translations, get_os_vars, t } from '$lib/i18n.js';
  import { app } from '$lib/app.svelte.js';

  let { children } = $props();

  const GITHUB_URL = 'https://github.com/johannesmutter/amberize';
  const DOWNLOAD_URL = `${GITHUB_URL}/releases/latest`;

  $effect(() => {
    if (browser) app.init();
  });

  let s = $derived(translations[app.locale]);
  let os_vars = $derived(get_os_vars(app.locale, app.os));

  /**
   * Translate a key with OS-aware placeholder replacement.
   * @param {string} key
   * @returns {string}
   */
  function tr(key) {
    return t(s, key, os_vars);
  }
</script>

<svelte:head>
  <meta name="description" content="Kostenlose Open-Source-Desktop-App für GoBD-konforme E-Mail-Archivierung. IMAP-E-Mails lokal archivieren mit manipulationssicherem Prüfpfad. Für macOS, Windows und Linux." />
  <meta name="author" content="Johannes Mutter" />
  <meta name="robots" content="index, follow" />
  <link rel="canonical" href="https://amberize.fly.dev/" />

  <!-- Open Graph -->
  <meta property="og:type" content="website" />
  <meta property="og:site_name" content="Amberize" />
  <meta property="og:title" content="Amberize — Ihre E-Mails, sicher archiviert für die nächste Betriebsprüfung" />
  <meta property="og:description" content="Kostenlose Open-Source-App für GoBD-konforme E-Mail-Archivierung. Durchsuchbar, manipulationssicher, prüfungsbereit. Keine Cloud. Kein Abo. Für macOS, Windows und Linux." />
  <meta property="og:url" content="https://amberize.fly.dev/" />
  <meta property="og:image" content="https://amberize.fly.dev/images/cover.png" />
  <meta property="og:image:width" content="1200" />
  <meta property="og:image:height" content="630" />
  <meta property="og:image:alt" content="Amberize — Lokale E-Mail-Archivierung für GoBD" />
  <meta property="og:locale" content="de_DE" />
  <meta property="og:locale:alternate" content="en_US" />

  <!-- Twitter Card -->
  <meta name="twitter:card" content="summary_large_image" />
  <meta name="twitter:site" content="@johannesmutter" />
  <meta name="twitter:creator" content="@johannesmutter" />
  <meta name="twitter:title" content="Amberize — Ihre E-Mails, sicher archiviert für die nächste Betriebsprüfung" />
  <meta name="twitter:description" content="Kostenlose Open-Source-App für GoBD-konforme E-Mail-Archivierung. Durchsuchbar, manipulationssicher, prüfungsbereit. Keine Cloud. Kein Abo." />
  <meta name="twitter:image" content="https://amberize.fly.dev/images/cover.png" />
  <meta name="twitter:image:alt" content="Amberize — Lokale E-Mail-Archivierung für GoBD" />
</svelte:head>

<div class="site" lang={app.locale}>
  <!-- Nav -->
  <nav class="nav">
    <a href="/" class="nav-brand">Amberize</a>
    <div class="nav-links">
      <a href="/#features">{tr('nav_features')}</a>
      <a href="/#compliance">{tr('nav_gobd')}</a>
      <a href={GITHUB_URL} target="_blank" rel="noopener">GitHub</a>
      <button class="lang-toggle" onclick={() => app.toggle_locale()} aria-label="Switch language">
        {app.locale === 'en' ? 'DE' : 'EN'}
      </button>
      <a href={DOWNLOAD_URL} class="nav-cta" target="_blank" rel="noopener">{tr('nav_download')}</a>
    </div>
  </nav>

  <!-- Page content -->
  {@render children()}

  <!-- Footer -->
  <footer class="footer">
    <div class="footer-links">
      <a href={GITHUB_URL} target="_blank" rel="noopener">{tr('footer_github')}</a>
      <span class="footer-sep">·</span>
      <a href="/impressum">{tr('footer_impressum')}</a>
      <span class="footer-sep">·</span>
      <a href="/datenschutz">{tr('footer_datenschutz')}</a>
    </div>
    <p class="footer-note">{tr('footer_text')}</p>
    <p class="footer-disclaimer">{tr('footer_disclaimer')}</p>
  </footer>
</div>

<style>
  /* ───── Design tokens ───── */
  :root {
    --color-amber: #b45309;
    --color-amber-hover: #92400e;
    --color-amber-light: #fef3c7;
    --color-amber-soft: #fffbeb;
    --color-dark: #1a1a2e;
    --color-text: #1a1a2e;
    --color-text-secondary: #4a4a5a;
    --color-text-muted: #777;
    --color-text-faint: #999;
    --color-border: #e8e5de;
    --color-border-light: #f0ede6;
    --color-surface: #fff;
    --color-bg: #fdfcf9;
    --font-serif: 'Fanwood Text', 'Georgia', 'Times New Roman', serif;
    --font-sans: -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif;

    /* ── Fluid modular type scale (400 → 1200 px) ── */
    --text-xs:   clamp(0.75rem, 0.6875rem + 0.25vw, 0.875rem);   /* 12 → 14 */
    --text-sm:   clamp(0.875rem, 0.8125rem + 0.25vw, 1rem);       /* 14 → 16 */
    --text-base: clamp(1rem, 0.9375rem + 0.25vw, 1.125rem);       /* 16 → 18 */
    --text-lg:   clamp(1.25rem, 1rem + 1vw, 1.75rem);             /* 20 → 28 */
    --text-xl:   clamp(1.75rem, 1.5rem + 1vw, 2.25rem);           /* 28 → 36 */
    --text-2xl:  clamp(2.25rem, 1.625rem + 2.5vw, 3.5rem);        /* 36 → 56 */
  }

  /* ───── Reset & Base ───── */
  :global(*) {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
    text-wrap: pretty;
  }
  :global(html) {
    scroll-behavior: smooth;
    hanging-punctuation: first last;
  }
  :global(body) {
    font-family: var(--font-sans);
    font-size: var(--text-base);
    color: var(--color-text);
    background: var(--color-bg);
    line-height: 1.6;
    font-variant-numeric: oldstyle-nums;
    -webkit-font-smoothing: antialiased;
  }
  :global(::selection) {
    background: rgba(180, 83, 9, 0.12);
    color: var(--color-dark);
  }
  :global(:focus-visible) {
    outline: 2px solid var(--color-amber);
    outline-offset: 2px;
    border-radius: 2px;
  }

  /* ───── Global heading styles ───── */
  :global(h1, h2, h3) {
    font-family: var(--font-serif);
    font-weight: 400;
    line-height: 1.25;
    letter-spacing: -0.015em;
    color: var(--color-dark);
    text-wrap: balance;
    font-variant-numeric: lining-nums;
  }
  :global(h1) {
    font-size: var(--text-2xl);
    line-height: 1.12;
    letter-spacing: -0.02em;
  }
  :global(h2) {
    font-size: var(--text-xl);
  }
  :global(h3) {
    font-size: var(--text-lg);
  }

  /* ───── Global link base ───── */
  :global(a) {
    color: inherit;
    text-decoration-skip-ink: auto;
  }

  /* ───── Buttons ───── */
  :global(.btn) {
    display: inline-block;
    padding: 0.75rem 1.75rem;
    border-radius: 8px;
    font-size: var(--text-sm);
    font-weight: 600;
    text-decoration: none;
    transition: background 0.2s ease, transform 0.2s ease, box-shadow 0.2s ease, border-color 0.2s ease;
  }
  :global(.btn-primary) {
    background: var(--color-amber);
    color: #fff;
    box-shadow: 0 1px 3px rgba(180, 83, 9, 0.15), 0 1px 2px rgba(0, 0, 0, 0.06);
  }
  :global(.btn-primary:hover) {
    background: var(--color-amber-hover);
    transform: translateY(-1px);
    box-shadow: 0 4px 12px rgba(180, 83, 9, 0.2), 0 2px 4px rgba(0, 0, 0, 0.06);
  }
  :global(.btn-primary:active) {
    transform: translateY(0);
    box-shadow: 0 1px 2px rgba(180, 83, 9, 0.2);
  }
  :global(.btn-secondary) {
    background: var(--color-surface);
    color: var(--color-dark);
    border: 1px solid var(--color-border);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.04);
  }
  :global(.btn-secondary:hover) {
    border-color: #ccc;
    transform: translateY(-1px);
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.06);
  }
  :global(.btn-secondary:active) {
    transform: translateY(0);
    box-shadow: none;
  }

  /* ───── Reveal animation ───── */
  :global(.reveal-ready) {
    opacity: 0;
    transform: translateY(16px);
    transition: opacity 0.6s cubic-bezier(0.25, 0.1, 0.25, 1),
                transform 0.6s cubic-bezier(0.25, 0.1, 0.25, 1);
  }
  :global(.reveal-ready.revealed) {
    opacity: 1;
    transform: translateY(0);
  }

  /* ───── Site wrapper ───── */
  .site {
    max-width: 100%;
    overflow-x: hidden;
  }

  /* ───── Nav ───── */
  .nav {
    display: flex;
    justify-content: space-between;
    align-items: center;
    max-width: 960px;
    margin: 0 auto;
    padding: 1.25rem 1.5rem;
  }
  .nav-brand {
    font-family: var(--font-serif);
    font-size: var(--text-lg);
    font-weight: 400;
    color: var(--color-dark);
    text-decoration: none;
    letter-spacing: -0.01em;
  }
  .nav-links {
    display: flex;
    gap: 1.5rem;
    align-items: center;
  }
  .nav-links a {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    text-decoration: none;
    transition: color 0.2s ease;
  }
  .nav-links a:hover {
    color: var(--color-dark);
  }
  .nav-cta {
    background: var(--color-amber);
    color: #fff !important;
    padding: 0.4rem 1rem;
    border-radius: 6px;
    font-weight: 600;
    transition: background 0.2s ease, transform 0.2s ease;
  }
  .nav-cta:hover {
    background: var(--color-amber-hover) !important;
    transform: translateY(-1px);
  }
  .lang-toggle {
    background: none;
    border: 1px solid var(--color-border);
    border-radius: 4px;
    padding: 0.2rem 0.5rem;
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    cursor: pointer;
    transition: border-color 0.2s ease, color 0.2s ease, background 0.2s ease;
    letter-spacing: 0.03em;
  }
  .lang-toggle:hover {
    border-color: var(--color-text-muted);
    color: var(--color-dark);
    background: rgba(0, 0, 0, 0.02);
  }

  /* ───── Footer ───── */
  .footer {
    text-align: center;
    padding: 2.5rem 1.5rem;
    font-size: var(--text-sm);
    color: var(--color-text-faint);
    border-top: 1px solid var(--color-border-light);
  }
  .footer-links {
    margin-bottom: 0.75rem;
  }
  .footer-links a {
    color: var(--color-text-muted);
    text-decoration: none;
    transition: color 0.2s ease;
    border-bottom: 1px solid transparent;
    padding-bottom: 1px;
  }
  .footer-links a:hover {
    color: var(--color-amber);
    border-bottom-color: var(--color-amber);
  }
  .footer-sep {
    margin: 0 0.5rem;
    color: var(--color-border);
  }
  .footer-note {
    font-size: var(--text-sm);
    color: var(--color-text-faint);
    margin-bottom: 0.5rem;
  }
  .footer-disclaimer {
    font-size: var(--text-xs);
    color: #bbb;
    max-width: 560px;
    margin: 0 auto;
    line-height: 1.5;
  }

  /* ───── Responsive ───── */
  @media (max-width: 600px) {
    .nav-links a:not(.nav-cta) {
      display: none;
    }
    .lang-toggle {
      display: inline-block;
    }
  }
</style>
