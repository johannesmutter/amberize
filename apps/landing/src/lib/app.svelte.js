/**
 * Shared reactive app state for locale and OS detection.
 * Import from any Svelte component to read/write locale and OS.
 * @module
 */
import { detect_locale, detect_os } from './i18n.js';

let _locale = $state('de');
let _os = $state('mac');

export const app = {
  get locale() { return _locale; },
  set locale(v) { _locale = v; },

  get os() { return _os; },
  set os(v) { _os = v; },

  /** Detect locale and OS from the browser. Call once on mount. */
  init() {
    _locale = detect_locale();
    _os = detect_os();
  },

  /** Toggle between 'en' and 'de'. */
  toggle_locale() {
    _locale = _locale === 'en' ? 'de' : 'en';
  }
};
