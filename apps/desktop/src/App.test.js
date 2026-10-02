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
    const ui=render(App);expect(await ui.findByRole('heading',{name:'Your archive settings could not be loaded'})).toBeInTheDocument();expect(ui.getByText('Saved settings are damaged')).toBeInTheDocument();expect(ui.queryByPlaceholderText('Search emails...')).not.toBeInTheDocument();delete window.__TAURI_INTERNALS__;
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

  function missing_archive() {
    window.__TAURI_INTERNALS__ = {};
    const config = {db_path:'/disconnected/saved.sqlite3',sync_interval_secs:300};
    let warning = 'The saved archive is unavailable at /disconnected/saved.sqlite3';
    tauri_invoke.mockImplementation(async (cmd,args) => {
      if (cmd === 'get_app_config') return config;
      if (cmd === 'set_active_db_path') throw new Error(warning);
      if (cmd === 'get_startup_warning') return warning;
      if (cmd === 'select_archive') { config.db_path=args.dbPath; warning=''; return {...config}; }
      if (cmd === 'list_accounts') return [{id:1,email_address:'saved@example.com'}];
      if (cmd === 'list_messages') return [{id:1,subject:'Saved email',sort_timestamp:1,account_id:1}];
      if (cmd === 'get_archive_stats') return {total_messages:29562,db_size_bytes:123};
      return null;
    });
    return config;
  }

  test('choosing a moved archive restores its accounts/messages and removes the old warning',async()=>{
    missing_archive();tauri_open_dialog.mockResolvedValue('/moved/saved.sqlite3');
    const ui=render(App);await ui.findByRole('heading',{name:'Your archive is unavailable'});
    expect(ui.container.querySelector('.update-banner-warning')).toBeNull();
    await fireEvent.click(ui.getByRole('button',{name:'Choose existing archive'}));
    await ui.findByText('Saved email');
    expect(ui.getByRole('option',{name:'saved@example.com'})).toBeInTheDocument();
    expect(ui.queryByText(/The saved archive is unavailable/)).not.toBeInTheDocument();
    expect(tauri_invoke).toHaveBeenCalledWith('select_archive',{dbPath:'/moved/saved.sqlite3',create:false});
    expect(tauri_invoke).toHaveBeenCalledWith('list_accounts',{dbPath:'/moved/saved.sqlite3'});
  });

  test.each([null,'/wrong/unrelated.db'])('canceling or failing recovery selection keeps the saved location: %s',async selected=>{
    missing_archive();const base=tauri_invoke.getMockImplementation();
    tauri_open_dialog.mockResolvedValue(selected);
    tauri_invoke.mockImplementation((cmd,args)=>cmd==='select_archive'?Promise.reject(new Error('This is not an Amberize archive')):base(cmd,args));
    const ui=render(App);await ui.findByRole('heading',{name:'Your archive is unavailable'});
    await fireEvent.click(ui.getByRole('button',{name:'Choose existing archive'}));
    await waitFor(()=>expect(ui.getByRole('button',{name:'Choose existing archive'})).toBeEnabled());
    expect(ui.getByText('/disconnected/saved.sqlite3',{selector:'code'})).toBeInTheDocument();
    expect(ui.queryByPlaceholderText('Search emails...')).not.toBeInTheDocument();
    if(selected)expect(ui.getByRole('alert')).toHaveTextContent('This is not an Amberize archive');
    else expect(tauri_invoke.mock.calls.some(([cmd])=>cmd==='select_archive')).toBe(false);
  });

  test('activation waits before requesting integrity or mounting an empty dashboard',async()=>{
    window.__TAURI_INTERNALS__={};let complete;
    const activation=new Promise(resolve=>{complete=resolve;});
    tauri_invoke.mockImplementation(async cmd=>{
      if(cmd==='get_app_config')return {db_path:'/saved/archive.db'};
      if(cmd==='set_active_db_path')return activation;
      if(cmd==='list_accounts'||cmd==='list_messages')return [];
      return null;
    });
    const ui=render(App);await ui.findByText('Opening your archive…');
    expect(tauri_invoke.mock.calls.some(([cmd])=>['get_integrity_status','list_accounts','list_messages'].includes(cmd))).toBe(false);
    complete();await ui.findByPlaceholderText('Search emails...');
  });

  test('retry opens the remembered archive once its drive is available',async()=>{
    const config=missing_archive();const base=tauri_invoke.getMockImplementation();let available=false;
    tauri_invoke.mockImplementation((cmd,args)=>cmd==='set_active_db_path'&&available?Promise.resolve():cmd==='get_startup_warning'&&available?Promise.resolve(null):base(cmd,args));
    const ui=render(App);await ui.findByRole('heading',{name:'Your archive is unavailable'});available=true;
    await fireEvent.click(ui.getByRole('button',{name:'Retry saved archive'}));await ui.findByText('Saved email');
    expect(config.db_path).toBe('/disconnected/saved.sqlite3');
    expect(tauri_invoke.mock.calls.some(([cmd])=>cmd==='select_archive')).toBe(false);
  });

  test('dismissed startup warnings stay dismissed on integrity refresh without hiding a new warning',async()=>{
    localStorage.setItem('amberize_config_v1',JSON.stringify({db_path:'/saved/archive.db'}));
    const handlers=new Map();tauri_listen.mockImplementation(async(name,handler)=>{handlers.set(name,handler);return()=>{};});
    let warning='Settings recovered from the previous copy';
    tauri_invoke.mockImplementation(async cmd=>cmd==='get_startup_warning'?warning:['list_accounts','list_messages'].includes(cmd)?[]:null);
    const ui=render(App);await ui.findByText(warning);await fireEvent.click(ui.getByRole('button',{name:'Dismiss archive warning'}));
    expect(tauri_invoke).toHaveBeenCalledWith('dismiss_startup_warning',{warning});
    await waitFor(()=>expect(handlers.has('integrity_status_updated')).toBe(true));
    handlers.get('integrity_status_updated')({});await waitFor(()=>expect(ui.queryByText(warning)).not.toBeInTheDocument());
    warning='A different verification problem';handlers.get('integrity_status_updated')({});expect(await ui.findByText(warning)).toBeInTheDocument();
  });
});
