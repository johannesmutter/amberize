<script>
  import { translations, os_terms, get_os_vars, t } from '$lib/i18n.js';
  import { app } from '$lib/app.svelte.js';
  import { reveal } from '$lib/reveal.js';

  const GITHUB_URL = 'https://github.com/johannesmutter/amberize';
  const DOWNLOAD_URL = `${GITHUB_URL}/releases/latest`;
  const WEBSITE_URL = 'https://mutter.co';
  const TWITTER_URL = 'https://x.com/johannesmutter';

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

  /**
   * Compute the "Also available for X and Y" string for other platforms.
   * @returns {string}
   */
  let other_platforms_text = $derived(() => {
    const all_os = ['mac', 'windows', 'linux'];
    const locale_terms = os_terms[app.locale] ?? os_terms.en;
    const others = all_os
      .filter(o => o !== app.os)
      .map(o => locale_terms[o]?.os_name ?? o);
    const and_word = tr('platform_and');
    return `${tr('also_available')} ${others.join(` ${and_word} `)}`;
  });

  let features = $derived(
    [1, 2, 3, 4, 5, 6].map(i => ({
      title: tr(`feature_${i}_title`),
      description: tr(`feature_${i}_desc`)
    }))
  );

  let compliance_items = $derived([1, 2, 3, 4].map(i => ({
    label: tr(`compliance_${i}_label`),
    term: tr(`compliance_${i}_term`),
    detail: tr(`compliance_${i}_detail`)
  })));

  let limitation_items = $derived([1, 2].map(i => ({
    title: tr(`limit_${i}_title`),
    desc: tr(`limit_${i}_desc`)
  })));

  /**
   * Split title on \n to allow a line break in the heading.
   * @param {string} raw
   * @returns {string[]}
   */
  function title_lines(raw) {
    return raw.split('\n');
  }
</script>

<!-- Hero -->
<section class="hero" use:reveal>
  <p class="hero-eyebrow">{tr('hero_eyebrow')}</p>
  <h1>
    {#each title_lines(tr('hero_title')) as line, i}
      {#if i > 0}<br />{/if}{line}
    {/each}
  </h1>
  <p class="hero-sub">{tr('hero_sub')}</p>
  <div class="hero-actions">
    <a href={DOWNLOAD_URL} class="btn btn-primary" target="_blank" rel="noopener">
      {tr('hero_cta_download')}
    </a>
    <a href="#how-it-works" class="btn btn-secondary">
      {tr('hero_cta_how')}
    </a>
  </div>
  <p class="platform-note">
    <a href={DOWNLOAD_URL} target="_blank" rel="noopener">{other_platforms_text()}</a>
  </p>
</section>

<!-- Trust -->
<section class="trust" use:reveal>
  <p>{tr('trust')}</p>
</section>

<!-- Demo video -->
<section class="demo" use:reveal>
  <div class="demo-video-wrapper">
    <!-- svelte-ignore a11y_media_has_caption -->
    <video
      class="demo-video"
      autoplay
      muted
      loop
      playsinline
      preload="none"
      poster="/images/cover.webp"
    >
      <source src="/images/amberize.webm" type="video/webm" />
    </video>
  </div>
</section>

<!-- How it works -->
<section id="how-it-works" class="how-it-works" use:reveal>
  <h2>{tr('how_title')}</h2>
  <ol class="steps">
    <li class="step">
      <span class="step-number">1</span>
      <h3>{tr('step_1_title')}</h3>
      <p>{tr('step_1_desc')}</p>
    </li>
    <li class="step">
      <span class="step-number">2</span>
      <h3>{tr('step_2_title')}</h3>
      <p>{tr('step_2_desc')}</p>
    </li>
    <li class="step">
      <span class="step-number">3</span>
      <h3>{tr('step_3_title')}</h3>
      <p>{tr('step_3_desc')}</p>
    </li>
  </ol>
</section>

<!-- Features -->
<section id="features" class="features" use:reveal>
  <div class="features-grid">
    {#each features as feature, i}
      <div class="feature-card">
        <span class="feature-index">{String(i + 1).padStart(2, '0')}</span>
        <h3>{feature.title}</h3>
        <p>{feature.description}</p>
      </div>
    {/each}
  </div>
</section>

<!-- GoBD Compliance -->
<section id="compliance" class="compliance" use:reveal>
  <h2>{tr('compliance_title')}</h2>
  <p class="section-intro">{tr('compliance_intro')}</p>
  <div class="compliance-list">
    {#each compliance_items as item}
      <div class="compliance-item">
        <span class="compliance-term">{item.term}</span>
        <h3>{item.label}</h3>
        <p>{item.detail}</p>
      </div>
    {/each}
  </div>
</section>

<!-- Limitations -->
<section class="limitations" use:reveal>
  <h2>{tr('limits_title')}</h2>
  <p class="section-intro">{tr('limits_intro')}</p>
  <div class="limitations-grid">
    {#each limitation_items as item}
      <div class="limitation-card">
        <h3>{item.title}</h3>
        <p>{item.desc}</p>
      </div>
    {/each}
  </div>
</section>

<!-- About -->
<section class="about" use:reveal>
  <div class="about-inner">
    <img
      class="about-photo"
      src="/images/johannes.jpg"
      alt="Johannes Mutter"
      width="96"
      height="96"
      loading="lazy"
    />
    <div class="about-content">
      <h2>{tr('about_title')}</h2>
      <p>{tr('about_text')}</p>
      <div class="about-links">
        <a href={GITHUB_URL} target="_blank" rel="noopener">{tr('about_github')}</a>
        <span class="about-sep">·</span>
        <a href={WEBSITE_URL} target="_blank" rel="noopener">{tr('about_website')}</a>
        <span class="about-sep">·</span>
        <a href={TWITTER_URL} target="_blank" rel="noopener">{tr('about_twitter')}</a>
      </div>
    </div>
  </div>
</section>

<!-- CTA -->
<section class="cta" use:reveal>
  <h2>{tr('cta_title')}</h2>
  <p>{tr('cta_sub')}</p>
  <div class="hero-actions">
    <a href={DOWNLOAD_URL} class="btn btn-primary" target="_blank" rel="noopener">
      {tr('cta_button')}
    </a>
  </div>
  <p class="platform-note">
    <a href={DOWNLOAD_URL} target="_blank" rel="noopener">{other_platforms_text()}</a>
  </p>
  <p class="cta-note">{tr('cta_note')}</p>
</section>

<style>
  /* ───── Hero ───── */
  .hero {
    text-align: center;
    padding: 5rem 1.5rem 3rem;
    max-width: 720px;
    margin: 0 auto;
  }
  .hero-eyebrow {
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.15em;
    color: var(--color-amber);
    margin-bottom: 1.25rem;
    font-variant-numeric: lining-nums;
  }
  .hero-sub {
    margin-top: 1.5rem;
    font-size: var(--text-base);
    color: var(--color-text-secondary);
    line-height: 1.7;
    max-width: 540px;
    margin-left: auto;
    margin-right: auto;
  }
  .hero-actions {
    margin-top: 2rem;
    display: flex;
    gap: 0.75rem;
    justify-content: center;
    flex-wrap: wrap;
  }

  /* ───── Platform note ───── */
  .platform-note {
    margin-top: 0.75rem;
    font-size: var(--text-xs);
    color: var(--color-text-faint);
    letter-spacing: 0.01em;
  }
  .platform-note a {
    color: var(--color-text-muted);
    text-decoration: none;
    border-bottom: 1px solid var(--color-border);
    padding-bottom: 1px;
    transition: color 0.2s ease, border-color 0.2s ease;
  }
  .platform-note a:hover {
    color: var(--color-amber);
    border-color: var(--color-amber);
  }

  /* ───── Trust ───── */
  .trust {
    text-align: center;
    padding: 0 1.5rem 3rem;
  }
  .trust p {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    max-width: 520px;
    margin: 0 auto;
    line-height: 1.6;
  }

  /* ───── Demo video ───── */
  .demo {
    max-width: 820px;
    margin: 0 auto;
    padding: 1rem 1.5rem 3rem;
  }
  .demo-video-wrapper {
    position: relative;
    border-radius: 18px;
    overflow: hidden;
    border: 1px solid var(--color-border-light);
    box-shadow: 0 8px 40px rgba(0, 0, 0, 0.06), 0 4px 8px rgba(0, 0, 0, 0.04);
  }
  .demo-video {
    display: block;
    width: 100%;
    height: auto;
  }

  /* ───── Shared section intro ───── */
  .section-intro {
    text-align: center;
    color: var(--color-text-secondary);
    font-size: var(--text-base);
    max-width: 560px;
    margin: 0 auto 2.5rem;
    line-height: 1.7;
    text-wrap: balance;
  }

  /* ───── Features ───── */
  .features {
    max-width: 780px;
    margin: 0 auto;
    padding: 2rem 1.5rem 4rem;
  }
  .features-grid {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 0;
  }
  .feature-card {
    padding: 2rem 2rem 2rem 2.25rem;
    border-bottom: 1px solid var(--color-border-light);
    transition: background 0.3s ease;
  }
  .feature-card:nth-child(even) {
    border-left: 1px solid var(--color-border-light);
  }
  .feature-card:nth-last-child(-n+2) {
    border-bottom: none;
  }
  .feature-card:hover {
    background: rgba(180, 83, 9, 0.015);
  }
  .feature-index {
    display: block;
    font-family: var(--font-serif);
    font-size: var(--text-xs);
    color: var(--color-amber);
    letter-spacing: 0.05em;
    margin-bottom: 0.75rem;
    opacity: 0.55;
    font-variant-numeric: lining-nums tabular-nums;
  }
  .feature-card h3 {
    margin-bottom: 0.5rem;
  }
  .feature-card p {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    line-height: 1.6;
  }

  /* ───── How it works ───── */
  .how-it-works {
    background: var(--color-surface);
    padding: 5rem 1.5rem;
    border-top: 1px solid var(--color-border-light);
    border-bottom: 1px solid var(--color-border-light);
  }
  .how-it-works h2 {
    text-align: center;
    margin-bottom: 3.5rem;
  }
  .steps {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 1.5rem;
    max-width: 920px;
    margin: 0 auto;
    list-style: none;
    padding: 0;
  }
  .step {
    text-align: center;
    position: relative;
  }
  /* dashed connector between steps */
  .step:not(:last-child)::after {
    content: '';
    position: absolute;
    top: 1.25rem;
    left: calc(50% + 1.6rem);
    right: calc(-50% - 1.5rem + 1.6rem);
    border-top: 1.5px dashed var(--color-border);
  }
  .step-number {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 2.5rem;
    height: 2.5rem;
    border-radius: 50%;
    background: var(--color-surface);
    border: 1.5px solid var(--color-amber);
    color: var(--color-amber);
    font-family: var(--font-serif);
    font-weight: 400;
    font-size: var(--text-base);
    margin-bottom: 1rem;
    font-variant-numeric: lining-nums tabular-nums;
    position: relative;
    z-index: 1;
  }
  .step h3 {
    margin-bottom: 0.5rem;
  }
  .step p {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    max-width: 260px;
    margin: 0 auto;
    line-height: 1.6;
  }

  /* ───── Compliance ───── */
  .compliance {
    max-width: 960px;
    margin: 0 auto;
    padding: 4rem 1.5rem;
    border-top: 1px solid var(--color-border-light);
  }
  .compliance h2 {
    text-align: center;
    margin-bottom: 0.75rem;
  }
  .compliance-list {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 1.5rem;
  }
  .compliance-item {
    position: relative;
    background: var(--color-surface);
    border: 1px solid var(--color-border-light);
    border-radius: 10px;
    padding: 1.75rem 2rem 1.5rem;
    transition: border-color 0.3s ease, box-shadow 0.3s ease;
    display: grid;
    grid-template-rows: subgrid;
    grid-row: span 2;
    gap: 0.5rem;
  }
  .compliance-item:hover {
    border-color: var(--color-border);
    box-shadow: 0 4px 20px rgba(0, 0, 0, 0.04);
  }
  .compliance-term {
    position: absolute;
    top: -0.625rem;
    left: 1.5rem;
    font-family: var(--font-sans);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    background: var(--color-bg);
    border: 1px solid var(--color-border-light);
    border-radius: 4px;
    padding: 0.1rem 0.625rem;
    letter-spacing: 0.02em;
    line-height: 1.3;
  }
  .compliance-item h3 {
    font-size: var(--text-lg);
  }
  .compliance-item p {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    line-height: 1.6;
    margin: 0;
  }

  /* ───── Limitations ───── */
  .limitations {
    max-width: 720px;
    margin: 0 auto;
    padding: 3rem 1.5rem 4rem;
    border-top: 1px solid var(--color-border-light);
  }
  .limitations h2 {
    text-align: center;
    margin-bottom: 0.75rem;
  }
  .limitations-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
    gap: 1rem;
  }
  .limitation-card {
    background: var(--color-amber-soft);
    border: 1px solid #f0e5c8;
    border-radius: 10px;
    padding: 1.25rem 1.5rem;
    transition: border-color 0.25s ease, box-shadow 0.25s ease, transform 0.25s ease;
  }
  .limitation-card:hover {
    border-color: #e5d5a0;
    box-shadow: 0 2px 12px rgba(180, 83, 9, 0.06);
    transform: translateY(-1px);
  }
  .limitation-card h3 {
    color: #7c5e10;
    margin-bottom: 0.35rem;
  }
  .limitation-card p {
    font-size: var(--text-sm);
    color: #6b5a1e;
    line-height: 1.7;
    margin: 0;
  }

  /* ───── About ───── */
  .about {
    max-width: 720px;
    margin: 0 auto;
    padding: 4rem 1.5rem;
    border-top: 1px solid var(--color-border-light);
  }
  .about-inner {
    display: flex;
    gap: 2rem;
    align-items: flex-start;
  }
  .about-photo {
    width: 96px;
    height: 96px;
    border-radius: 50%;
    object-fit: cover;
    flex-shrink: 0;
    border: 2px solid var(--color-border-light);
    box-shadow: 0 2px 12px rgba(0, 0, 0, 0.06);
    transition: border-color 0.3s ease, box-shadow 0.3s ease;
  }
  .about-photo:hover {
    border-color: var(--color-amber);
    box-shadow: 0 4px 16px rgba(180, 83, 9, 0.1);
  }
  .about-content h2 {
    margin-bottom: 0.5rem;
  }
  .about-content p {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    line-height: 1.6;
  }
  .about-links {
    margin-top: 0.75rem;
    font-size: var(--text-sm);
  }
  .about-links a {
    color: var(--color-amber);
    text-decoration: none;
    border-bottom: 1px solid transparent;
    padding-bottom: 1px;
    transition: color 0.2s ease, border-color 0.2s ease;
  }
  .about-links a:hover {
    color: var(--color-amber-hover);
    border-bottom-color: var(--color-amber-hover);
  }
  .about-sep {
    margin: 0 0.5rem;
    color: var(--color-border);
  }

  /* ───── CTA ───── */
  .cta {
    text-align: center;
    padding: 5rem 1.5rem;
    background: var(--color-surface);
    border-top: 1px solid var(--color-border-light);
  }
  .cta h2 {
    margin-bottom: 0.75rem;
  }
  .cta > p {
    color: var(--color-text-secondary);
    font-size: var(--text-base);
    margin-bottom: 2rem;
    max-width: 480px;
    margin-left: auto;
    margin-right: auto;
    line-height: 1.6;
  }
  .cta-note {
    margin-top: 0.75rem;
    font-size: var(--text-xs);
    color: var(--color-text-faint);
    letter-spacing: 0.01em;
  }

  /* ───── Responsive ───── */
  @media (max-width: 640px) {
    .hero {
      padding: 3.5rem 1.5rem 2.5rem;
    }
    .features-grid {
      grid-template-columns: 1fr;
    }
    .feature-card:nth-child(even) {
      border-left: none;
    }
    .feature-card:nth-last-child(1) {
      border-bottom: none;
    }
    .feature-card:nth-last-child(2) {
      border-bottom: 1px solid var(--color-border-light);
    }
    .steps {
      grid-template-columns: 1fr;
      gap: 2.5rem;
      max-width: 320px;
    }
    .step:not(:last-child)::after {
      display: none;
    }
    .about-inner {
      flex-direction: column;
      align-items: center;
      text-align: center;
    }
  }
</style>
