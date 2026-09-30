import { beforeEach, describe, expect, test, vi } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';
import App from './App.svelte';

const tauri_invoke = vi.fn();
const tauri_save_dialog = vi.fn();
const tauri_open_dialog = vi.fn();
const tauri_listen = vi.fn();
const tauri_check_update = vi.fn();

vi.mock('./lib/tauri_bridge.js', () => ({
  tauri_invoke: (...args) => tauri_invoke(...args),
  tauri_listen: (...args) => tauri_listen(...args),
  listen_scoped: (handlers,ready) => {let disposed=false;const cleanups=[];void Promise.all(Object.entries(handlers).map(async([name,fn])=>{const off=await tauri_listen(name,fn);if(off){if(disposed)off();else cleanups.push(off);}})).then(()=>{if(!disposed)ready?.();});return()=>{disposed=true;for(const off of cleanups)off();};},
  tauri_save_dialog: (...args) => tauri_save_dialog(...args),
  tauri_open_dialog: (...args) => tauri_open_dialog(...args),
  tauri_check_update: (...args) => tauri_check_update(...args),
}));

describe('App', () => {
  beforeEach(() => {
    localStorage.clear();
    tauri_invoke.mockReset();tauri_invoke.mockResolvedValue(null);delete window.__TAURI_INTERNALS__;
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
  test('native Settings before archive setup opens General without querying an empty archive path',async()=>{
    const handlers=new Map();tauri_listen.mockImplementation(async(name,handler)=>{handlers.set(name,handler);return()=>{};});
    tauri_invoke.mockImplementation(async cmd=>cmd==='get_sync_interval'?300:null);
    const ui=render(App);await ui.findByText('Choose your archive');
    await vi.waitFor(()=>expect(handlers.has('menu_open_settings')).toBe(true));
    handlers.get('menu_open_settings')({});
    expect(await ui.findByLabelText('Background sync interval')).toHaveValue('300');
    expect(tauri_invoke.mock.calls.some(([cmd,args])=>cmd==='list_accounts' && !args?.dbPath)).toBe(false);
  });

  test('supports browser preview when no desktop archive is configured', async () => {
    tauri_invoke.mockImplementation(async (cmd) => {
      if (cmd === 'clear_active_db_path') throw new Error('not tauri');
      return null;
    });

    const { findByText } = render(App);
    expect(await findByText('Choose your archive')).toBeInTheDocument();
    await Promise.resolve();
    expect(await findByText('Choose your archive')).toBeInTheDocument();
  });

  test('native settings-read failure retains recovery messaging and does not use stale browser settings',async()=>{
    window.__TAURI_INTERNALS__={};localStorage.setItem('amberize_config_v1',JSON.stringify({db_path:'/stale-browser.db'}));
    tauri_invoke.mockImplementation(async cmd=>{if(cmd==='get_app_config')throw new Error('Saved settings are damaged');return null;});
    const ui=render(App);expect(await ui.findByText('Amberize could not restore your archive.')).toBeInTheDocument();expect(ui.getByText('Saved settings are damaged')).toBeInTheDocument();expect(ui.queryByPlaceholderText('Search emails...')).not.toBeInTheDocument();delete window.__TAURI_INTERNALS__;
  });
  test('failed archive selection stays in setup and shows the actual save failure',async()=>{
    tauri_save_dialog.mockResolvedValue('/tmp/new.db');tauri_invoke.mockImplementation(async cmd=>{if(cmd==='select_archive')throw new Error('Could not save archive settings: disk full');return null;});
    const ui=render(App);await fireEvent.click(await ui.findByText('Create New'));await fireEvent.click(ui.getByText('Continue'));expect(await ui.findByText('Could not save archive settings: disk full')).toBeInTheDocument();expect(ui.queryByPlaceholderText('Search emails...')).not.toBeInTheDocument();
  });
  test('shows MainDashboard when configured', async () => {
    localStorage.setItem('amberize_config_v1', JSON.stringify({ db_path: '/tmp/a.sqlite3' }));
    tauri_listen.mockResolvedValue(() => {});
    tauri_invoke.mockImplementation(async (cmd) => {
      if (cmd === 'select_archive') return {db_path:'/tmp/a.sqlite3',sync_interval_secs:300};
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
  test('tray package exports expose cancellation and show backend failures', async () => {
    localStorage.setItem('amberize_config_v1', JSON.stringify({db_path:'/tmp/archive.db'}));
    const handlers = new Map();
    tauri_listen.mockImplementation(async (name, handler) => {handlers.set(name,handler);return()=>{};});
    let reject_export;
    const pending = new Promise((resolve,reject) => {reject_export = reject;});
    tauri_invoke.mockImplementation(async cmd => {
      if (cmd === 'export_auditor_package') return pending;
      if (cmd === 'list_accounts' || cmd === 'list_messages') return [];
      return null;
    });
    tauri_save_dialog.mockResolvedValue('/tmp/package.zip');
    const ui = render(App); await ui.findByPlaceholderText('Search emails...');
    await waitFor(()=>expect(handlers.has('tray_export_auditor')).toBe(true));
    handlers.get('tray_export_auditor')({});
    const cancel = await ui.findByRole('button',{name:'Cancel export'});
    const operation = tauri_invoke.mock.calls.find(([c])=>c === 'export_auditor_package')[1].operationId;
    await fireEvent.click(cancel);
    await waitFor(()=>expect(tauri_invoke).toHaveBeenCalledWith('cancel_auditor_export',{operationId:operation}));
    reject_export(new Error('Export canceled. Your previous file was kept.'));
    expect(await ui.findByText('Export canceled. Your previous file was kept.')).toBeInTheDocument();
    expect(ui.queryByRole('button',{name:'Cancel export'})).not.toBeInTheDocument();
  });
  test('Open Diagnostics navigates an already open Settings view', async () => {
    localStorage.setItem('amberize_config_v1', JSON.stringify({db_path:'/tmp/archive.db'}));
    const handlers = new Map();
    tauri_listen.mockImplementation(async (name, handler) => {handlers.set(name,handler);return()=>{};});
    tauri_invoke.mockImplementation(async cmd => {
      if (cmd === 'get_integrity_status') return {ok:false,issues:['Test integrity warning']};
      if (cmd === 'list_accounts' || cmd === 'list_messages') return [];
      if (cmd === 'get_sync_interval') return 300;
      return null;
    });
    const ui = render(App); await ui.findByPlaceholderText('Search emails...');
    await waitFor(()=>expect(handlers.has('menu_open_settings')).toBe(true));
    handlers.get('menu_open_settings')({});
    await fireEvent.click(await ui.findByRole('button',{name:'General'}));
    await ui.findByLabelText('Background sync interval');
    await fireEvent.click(ui.getByRole('button',{name:'Open Diagnostics'}));
    expect(await ui.findByRole('heading',{name:'Database Diagnostics',exact:true})).toBeInTheDocument();
    await fireEvent.click(ui.getByRole('button',{name:'General'}));
    await ui.findByLabelText('Background sync interval');
    await fireEvent.click(ui.getByRole('button',{name:'Open Diagnostics'}));
    expect(await ui.findByRole('heading',{name:'Database Diagnostics',exact:true})).toBeInTheDocument();
    expect(tauri_invoke.mock.calls.filter(([c])=>c === 'diagnose_database')).toHaveLength(2);
  });

  test('archive selection persists config and navigates to dashboard', async () => {
    tauri_listen.mockResolvedValue(() => {});
    tauri_save_dialog.mockResolvedValue('/tmp/a.sqlite3');
    tauri_invoke.mockImplementation(async (cmd) => {
      if (cmd === 'select_archive') return {db_path:'/tmp/a.sqlite3',sync_interval_secs:300};
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
    await Promise.resolve();
    expect(tauri_invoke).toHaveBeenCalledWith('select_archive',{dbPath:'/tmp/a.sqlite3',create:true});

    // Verify dashboard is now shown
    expect(await findByPlaceholderText('Search emails...')).toBeInTheDocument();
  });
});
