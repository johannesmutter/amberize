import { tauri_invoke } from './tauri_bridge.js';

const CONFIG_STORAGE_KEY = 'amberize_config_v1';

/**
 * @typedef {Object} AppConfigV1
 * @property {string} db_path
 */

/**
 * @returns {Promise<AppConfigV1 | null>}
 */
export async function load_config() {
  const tauri_config = await load_tauri_config();
  if (tauri_config) return tauri_config;
  return load_local_config();
}

/**
 * @param {AppConfigV1} config
 * @returns {Promise<void>}
 */
export async function save_config(config) {
  const normalized = normalize_config(config);
  if (!normalized) return;

  if (is_tauri_runtime()) {
    await tauri_invoke('save_app_config', { config: normalized });
    return;
  }

  save_local_config(normalized);
}

/**
 * @returns {Promise<void>}
 */
export async function clear_config() {
  if (is_tauri_runtime()) {
    await tauri_invoke('clear_app_config');
    return;
  }
  localStorage.removeItem(CONFIG_STORAGE_KEY);
}

/**
 * @returns {boolean}
 */
function is_tauri_runtime() {
  return typeof window !== 'undefined' && typeof window.__TAURI_INTERNALS__ !== 'undefined';
}

/**
 * @returns {Promise<AppConfigV1 | null>}
 */
async function load_tauri_config() {
  if (!is_tauri_runtime()) return null;
  try {
    const config = await tauri_invoke('get_app_config');
    return normalize_config(config);
  } catch {
    return null;
  }
}

/**
 * @returns {AppConfigV1 | null}
 */
function load_local_config() {
  try {
    const raw = localStorage.getItem(CONFIG_STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw);
    return normalize_config(parsed);
  } catch {
    return null;
  }
}

/**
 * @param {AppConfigV1} config
 */
function save_local_config(config) {
  localStorage.setItem(CONFIG_STORAGE_KEY, JSON.stringify(config));
}

/**
 * @param {unknown} value
 * @returns {AppConfigV1 | null}
 */
function normalize_config(value) {
  if (!value || typeof value !== 'object') return null;
  if (typeof value.db_path !== 'string') return null;
  const db_path = value.db_path.trim();
  if (!db_path) return null;
  return { db_path };
}

