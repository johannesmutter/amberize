import { beforeEach, describe, expect, test, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';
import App from './App.svelte';

const tauri_invoke = vi.fn();
const tauri_save_dialog = vi.fn();
const tauri_open_dialog = vi.fn();
const tauri_listen = vi.fn();
const tauri_check_update = vi.fn();

vi.mock('./lib/tauri_bridge.js', () => ({
  tauri_invoke: (...args) => tauri_invoke(...args),
  tauri_listen: (...args) => tauri_listen(...args),
  tauri_save_dialog: (...args) => tauri_save_dialog(...args),
  tauri_open_dialog: (...args) => tauri_open_dialog(...args),
  tauri_check_update: (...args) => tauri_check_update(...args),
}));

describe('App', () => {
  beforeEach(() => {
    localStorage.clear();
    tauri_invoke.mockReset();
    tauri_save_dialog.mockReset();
    tauri_open_dialog.mockReset();
    tauri_listen.mockReset();
    tauri_check_update.mockReset();
    tauri_check_update.mockResolvedValue(null);
  });

  test('shows ArchiveLocationScreen when not configured', async () => {
    const { findByText } = render(App);
    expect(await findByText('Choose your archive')).toBeInTheDocument();
  });

  test('ignores backend config sync failures', async () => {
    tauri_invoke.mockImplementation(async (cmd) => {
      if (cmd === 'clear_active_db_path') throw new Error('not tauri');
      return null;
    });

    const { findByText } = render(App);
    expect(await findByText('Choose your archive')).toBeInTheDocument();
    await Promise.resolve();
    expect(await findByText('Choose your archive')).toBeInTheDocument();
  });

  test('shows MainDashboard when configured', async () => {
    localStorage.setItem('amberize_config_v1', JSON.stringify({ db_path: '/tmp/a.sqlite3' }));
    tauri_listen.mockResolvedValue(() => {});
    tauri_invoke.mockImplementation(async (cmd) => {
      if (cmd === 'set_active_db_path') return null;
      if (cmd === 'clear_active_db_path') return null;
      if (cmd === 'list_accounts') return [];
      if (cmd === 'list_messages') return [];
      if (cmd === 'get_sync_status') return null;
      if (cmd === 'get_sync_interval') return null;
      if (cmd === 'get_archive_stats') return { total_messages: 0, db_size_bytes: 0 };
      return null;
    });
    const { findByPlaceholderText } = render(App);
    expect(await findByPlaceholderText('Search emails...')).toBeInTheDocument();
  });

  test('archive selection persists config and navigates to dashboard', async () => {
    tauri_listen.mockResolvedValue(() => {});
    tauri_save_dialog.mockResolvedValue('/tmp/a.sqlite3');
    tauri_invoke.mockImplementation(async (cmd) => {
      if (cmd === 'set_active_db_path') return null;
      if (cmd === 'clear_active_db_path') return null;
      if (cmd === 'list_accounts') return [];
      if (cmd === 'list_messages') return [];
      if (cmd === 'get_sync_status') return null;
      if (cmd === 'get_sync_interval') return null;
      if (cmd === 'get_archive_stats') return { total_messages: 0, db_size_bytes: 0 };
      return null;
    });

    const { findByText, findByPlaceholderText } = render(App);

    // Start on ArchiveLocationScreen
    expect(await findByText('Choose your archive')).toBeInTheDocument();

    // Click "Create New" to invoke save dialog
    await fireEvent.click(await findByText('Create New'));

    // Click "Continue" to transition to MainDashboard
    await fireEvent.click(await findByText('Continue'));

    // Verify config was persisted
    expect(localStorage.getItem('amberize_config_v1')).toContain('/tmp/a.sqlite3');

    // Verify dashboard is now shown
    expect(await findByPlaceholderText('Search emails...')).toBeInTheDocument();
  });
});
