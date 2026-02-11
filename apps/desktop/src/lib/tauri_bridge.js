/**
 * Thin wrapper around Tauri JS APIs.
 * Uses dynamic imports so the app can still load in a plain browser during UI dev.
 */

/**
 * @template T
 * @param {string} command
 * @param {Record<string, any>} [args]
 * @returns {Promise<T>}
 */
export async function tauri_invoke(command, args = {}) {
  try {
    const mod = await import('@tauri-apps/api/core');
    return await mod.invoke(command, args);
  } catch (err) {
    throw new Error(`Tauri invoke unavailable (${command}): ${stringify_error(err)}`);
  }
}

/**
 * @param {string} event_name
 * @param {(event: any) => void} handler
 * @returns {Promise<() => void>} unlisten fn
 */
export async function tauri_listen(event_name, handler) {
  try {
    const mod = await import('@tauri-apps/api/event');
    return await mod.listen(event_name, handler);
  } catch (err) {
    throw new Error(`Tauri listen unavailable (${event_name}): ${stringify_error(err)}`);
  }
}

/**
 * @param {import('@tauri-apps/api/dialog').SaveDialogOptions} options
 * @returns {Promise<string | null>}
 */
export async function tauri_save_dialog(options) {
  try {
    const mod = await import('@tauri-apps/plugin-dialog');
    const selected = await mod.save(options);
    if (typeof selected !== 'string') return null;
    return selected;
  } catch (err) {
    throw new Error(`Tauri save dialog unavailable: ${stringify_error(err)}`);
  }
}

/**
 * @param {import('@tauri-apps/api/dialog').OpenDialogOptions} options
 * @returns {Promise<string | null>}
 */
export async function tauri_open_dialog(options) {
  try {
    const mod = await import('@tauri-apps/plugin-dialog');
    const selected = await mod.open(options);
    if (typeof selected !== 'string') return null;
    return selected;
  } catch (err) {
    throw new Error(`Tauri open dialog unavailable: ${stringify_error(err)}`);
  }
}

/**
 * Check for application updates using the Tauri updater plugin.
 * Returns an update object if available, or null if already up-to-date.
 * @returns {Promise<{version: string, body: string | null, download_and_install: () => Promise<void>} | null>}
 */
export async function tauri_check_update() {
  try {
    const mod = await import('@tauri-apps/plugin-updater');
    const update = await mod.check();
    if (!update) return null;
    return {
      version: update.version,
      body: update.body ?? null,
      download_and_install: () => update.downloadAndInstall(),
    };
  } catch (err) {
    throw new Error(`Update check unavailable: ${stringify_error(err)}`);
  }
}

/**
 * @param {unknown} err
 * @returns {string}
 */
function stringify_error(err) {
  if (err instanceof Error) return err.message;
  try {
    return JSON.stringify(err);
  } catch {
    return String(err);
  }
}

