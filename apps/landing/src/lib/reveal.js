/**
 * Svelte action that fades in an element when it scrolls into view.
 * The `.reveal-ready` class (which sets opacity: 0) is added client-side
 * to avoid invisible content on SSR / pre-rendered pages.
 *
 * Usage: <section use:reveal>
 *
 * @param {HTMLElement} node
 * @param {{ threshold?: number, rootMargin?: string }} opts
 * @returns {{ destroy: () => void }}
 */
export function reveal(node, opts = {}) {
  const { threshold = 0.12, rootMargin = '0px 0px -60px 0px' } = opts;

  /* Respect reduced-motion preference */
  const prefers_reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  if (prefers_reduced) return { destroy() {} };

  /* Add the hidden state client-side only (not in SSR HTML) */
  node.classList.add('reveal-ready');

  const observer = new IntersectionObserver(
    ([entry]) => {
      if (entry.isIntersecting) {
        node.classList.add('revealed');
        observer.unobserve(node);
      }
    },
    { threshold, rootMargin }
  );

  observer.observe(node);

  return {
    destroy() {
      observer.unobserve(node);
    }
  };
}
