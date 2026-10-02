<script>
  let { title = 'Loading your archive…' } = $props();
</script>

<div class="archive-loading" role="status" aria-live="polite">
  <span class="loading-spinner" aria-hidden="true"></span>
  <p class="loading-title">{title}</p>
  <p class="loading-description">Reading your saved archive. Larger archives can take longer.</p>
</div>

<style>
  .archive-loading { flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: var(--space-md); padding: var(--space-xl); text-align: center; }
  .loading-title { margin: 0; color: var(--color-text); font-size: var(--font-size-lg); font-weight: var(--font-weight-medium); }
  .loading-description { margin: 0; max-width: 32ch; color: var(--color-text-secondary); line-height: 1.6; }
</style>
