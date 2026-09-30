import { beforeEach, describe, expect, test, vi } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';
import MainDashboard from './MainDashboard.svelte';
import MessagePreview from './MessagePreview.svelte';
import GeneralSettings from './GeneralSettings.svelte';
import AddAccountForm from './AddAccountForm.svelte';
import ConfirmDialog from './ConfirmDialog.svelte';
import SettingsPage from './SettingsPage.svelte';
import VirtualList from './VirtualList.svelte';
const { invoke, save, handlers } = vi.hoisted(() => ({ invoke: vi.fn(), save: vi.fn(), handlers: new Map() }));
vi.mock('../lib/tauri_bridge.js', () => ({
  tauri_invoke: invoke, tauri_save_dialog: save,
  listen_scoped: entries => { for (const [name,fn] of Object.entries(entries)) handlers.set(name,fn); return () => handlers.clear(); },
}));
const rows = (count = 3) => Array.from({length:count}, (_,i) => ({id:i+1,message_blob_id:1001+i,subject:`Subject ${i+1}`,from_address:'sender',account_id:1,sort_timestamp:1000-i,date_header:'2026-01-01',snippet:'body',has_attachments:false}));
const deferred = () => { let resolve, reject; const promise = new Promise((r,j) => {resolve = r;reject = j;}); return {promise,resolve,reject}; };
beforeEach(() => {
  handlers.clear(); invoke.mockReset(); save.mockReset(); save.mockResolvedValue('/tmp/selected.zip');
  invoke.mockImplementation(async (cmd) => {
    if (cmd === 'list_accounts') return [{id:1,email_address:'test@example.com'}];
    if (cmd === 'list_messages') return rows();
    if (cmd === 'get_sync_status') return {sync_in_progress:false,last_sync_status:'OK — yesterday',last_sync_at:'2026-01-01'};
    if (cmd === 'get_archive_stats') return {};
    if (cmd === 'get_google_oauth_configured') return true;
    return null;
  });
});
describe('archive reliability', () => {
  test('export checkboxes are separate from preview buttons and toggle independently', async () => {
    const preview = vi.fn(), toggle = vi.fn();
    const ui = render(VirtualList, {items: rows(1), bulk_mode: true, on_click: preview, on_toggle_selection: toggle});
    const checkbox = ui.getByRole('checkbox', {name: 'Select Subject 1 for export'});
    expect(checkbox.closest('button,[role="button"]')).toBeNull();
    await fireEvent.click(ui.getByRole('button', {name: /Subject 1/}));
    expect(preview).toHaveBeenCalledWith(expect.objectContaining({id: 1}));
    expect(toggle).not.toHaveBeenCalled();
    await fireEvent.click(checkbox);
    expect(toggle).toHaveBeenCalledExactlyOnceWith(1);
    expect(preview).toHaveBeenCalledTimes(1);
  });
  test('browsing beyond 500 rows can return to the beginning and retains export selection', async () => {
    const all = rows(650);
    const base = invoke.getMockImplementation();
    invoke.mockImplementation((cmd, args) => {
      if (cmd !== 'list_messages') return base(cmd, args);
      const start = args.after ? all.findIndex(r => r.id === args.after.id) + 1 : 0;
      const end = args.before ? all.findIndex(r => r.id === args.before.id) : null;
      return Promise.resolve(end == null ? all.slice(start,start + args.limit) : all.slice(Math.max(0,end-args.limit),end));
    });
    const ui=render(MainDashboard,{db_path:'/tmp/archive.db'});
    await ui.findByText('Subject 1');
    await fireEvent.click(ui.getByText('Select for export'));
    await fireEvent.click(ui.getByLabelText('Select Subject 1 for export'));
    const list=ui.container.querySelector('.virtual-list');
    Object.defineProperty(list,'scrollHeight',{get:()=>40000,configurable:true});
    Object.defineProperty(list,'clientHeight',{get:()=>400,configurable:true});
    for(let page=1;page<=5;page++) {
      list.scrollTop=39600;
      await fireEvent.scroll(list);
      await waitFor(()=>expect(invoke.mock.calls.filter(([c])=>c==='list_messages')).toHaveLength(page+1));
      await waitFor(()=>expect(ui.queryByText('Loading more...')).not.toBeInTheDocument());
    }
    expect(invoke.mock.calls.filter(([c])=>c==='list_messages').at(-1)[1].after.id).toBe(500);
    await fireEvent.click(ui.getByRole('button',{name:'Load earlier page'}));
    list.scrollTop=0;await fireEvent.scroll(list);
    await ui.findByText('Subject 1');
    expect(ui.getByLabelText('Select Subject 1 for export')).toBeChecked();
    await fireEvent.click(ui.getByRole('button',{name:'Export',exact:true}));
    await waitFor(()=>expect(invoke).toHaveBeenCalledWith('export_auditor_package',expect.objectContaining({selectedBlobIds:[1001]})));
  });
  test('date and attachment filters are sent to the backend before pagination', async () => {
    const ui = render(MainDashboard,{db_path:'/tmp/archive.db'});
    await ui.findByText('Subject 1');
    await fireEvent.change(ui.getByLabelText('Filter by date'),{target:{value:'last_year'}});
    await fireEvent.change(ui.getByLabelText('Filter by attachments'),{target:{value:'has'}});
    await waitFor(() => expect(invoke.mock.calls.filter(([c])=>c==='list_messages').at(-1)[1]).toMatchObject({hasAttachments:true,dateFrom:expect.any(Number),dateTo:expect.any(Number),offset:0}));
  });
  test('selected export passes blob IDs, independently of location IDs', async () => {
    const ui = render(MainDashboard,{db_path:'/tmp/archive.db'});await ui.findByText('Subject 1');
    await fireEvent.click(ui.getByText('Select for export'));await fireEvent.click(ui.getByLabelText('Select Subject 1 for export'));
    await fireEvent.click(ui.getByRole('button',{name:'Export',exact:true}));
    await waitFor(()=>expect(invoke).toHaveBeenCalledWith('export_auditor_package',expect.objectContaining({selectedBlobIds:[1001]})));
  });
  test('selected export cancellation keeps the UI busy until the backend stops', async () => {
    const pending = deferred(); const base = invoke.getMockImplementation();
    invoke.mockImplementation((cmd,args) => cmd === 'export_auditor_package' ? pending.promise : base(cmd,args));
    const ui = render(MainDashboard,{db_path:'/tmp/archive.db'}); await ui.findByText('Subject 1');
    await fireEvent.click(ui.getByText('Select for export')); await fireEvent.click(ui.getByLabelText('Select Subject 1 for export'));
    await fireEvent.click(ui.getByRole('button',{name:'Export',exact:true}));
    const cancel = await ui.findByRole('button',{name:'Cancel export'});
    const operation = invoke.mock.calls.find(([c])=>c === 'export_auditor_package')[1].operationId;
    expect(ui.getByRole('button',{name:'Export',exact:true})).toBeDisabled();
    await fireEvent.click(cancel);
    await waitFor(()=>expect(invoke).toHaveBeenCalledWith('cancel_auditor_export',{operationId:operation}));
    expect(await ui.findByText('Canceling export…')).toBeInTheDocument();
    expect(cancel).toBeDisabled();
    pending.reject(new Error('Export canceled. Your previous file was kept.'));
    expect(await ui.findByText('Export canceled. Your previous file was kept.')).toBeInTheDocument();
    expect(ui.queryByRole('button',{name:'Cancel export'})).not.toBeInTheDocument();
    expect(ui.getByRole('button',{name:'Export',exact:true})).toBeEnabled();
  });
  test('late export cancellation reports a saved file instead of claiming cancellation', async () => {
    const pending = deferred(); const base = invoke.getMockImplementation();
    invoke.mockImplementation((cmd,args) => cmd === 'export_auditor_package' ? pending.promise : cmd === 'cancel_auditor_export' ? Promise.resolve(true) : base(cmd,args));
    const ui = render(MainDashboard,{db_path:'/tmp/archive.db'}); await ui.findByText('Subject 1');
    await fireEvent.click(ui.getByText('Select for export')); await fireEvent.click(ui.getByLabelText('Select Subject 1 for export'));
    await fireEvent.click(ui.getByRole('button',{name:'Export',exact:true}));
    await fireEvent.click(await ui.findByRole('button',{name:'Cancel export'}));
    expect(await ui.findByText('Export saved. Finishing…')).toBeInTheDocument();
    pending.resolve('/tmp/selected.zip');
    expect(await ui.findByText('Selected emails exported.')).toBeInTheDocument();
  });
  test('slow preview cannot replace the newly selected message', async () => {
    const one=deferred(),two=deferred();const base=invoke.getMockImplementation();invoke.mockImplementation((cmd,args)=>cmd==='get_message_detail' ? (args.messageBlobId===1001?one.promise:two.promise) : base(cmd,args));
    const ui=render(MainDashboard,{db_path:'/tmp/archive.db'});await ui.findByText('Subject 1');await fireEvent.click(ui.getByText('Subject 1'));await fireEvent.click(ui.getByText('Subject 2'));
    two.resolve({id:1002,body_text:'current preview'});await ui.findByText('current preview');one.resolve({id:1001,body_text:'stale preview'});
    await waitFor(()=>expect(ui.queryByText('stale preview')).not.toBeInTheDocument());expect(ui.getByText('current preview')).toBeInTheDocument();
  });
  test('hidden sync events make no list/stat requests and late details stay discarded', async () => {
    const pending=deferred();const base=invoke.getMockImplementation();invoke.mockImplementation((cmd,args)=>cmd==='get_message_detail'?pending.promise:base(cmd,args));
    const ui=render(MainDashboard,{db_path:'/tmp/archive.db'});await ui.findByText('Subject 1');await fireEvent.click(ui.getByText('Subject 1'));handlers.get('main_window_hidden')({});invoke.mockClear();
    handlers.get('sync_status_updated')({});handlers.get('sync_progress')({payload:{messages_ingested:99}});pending.resolve({id:1001,body_text:'late hidden detail'});
    await waitFor(()=>expect(ui.queryByText('late hidden detail')).not.toBeInTheDocument());expect(invoke.mock.calls.filter(([c])=>['list_messages','get_archive_stats','get_sync_status'].includes(c))).toHaveLength(0);
    handlers.get('main_window_shown')({});await ui.findByText('Subject 1');
  });
  test('a background failure is displayed as a failure, not successful sync',async()=>{
    const base=invoke.getMockImplementation();invoke.mockImplementation((cmd,args)=>cmd==='get_sync_status'?Promise.resolve({sync_in_progress:false,last_sync_status:'Error — today',last_sync_at:'2026-01-01',error:'Archive drive unavailable'}):base(cmd,args));
    const ui=render(MainDashboard,{db_path:'/tmp/archive.db'});expect(await ui.findByText('Archive drive unavailable')).toBeInTheDocument();expect(ui.queryByText(/^Synced /)).not.toBeInTheDocument();
  });
  test('query failures show a recovery error instead of a normal empty list',async()=>{
    const base=invoke.getMockImplementation();invoke.mockImplementation((cmd,args)=>cmd==='list_messages'?Promise.reject(new Error('Database locked')):base(cmd,args));const ui=render(MainDashboard,{db_path:'/tmp/archive.db'});expect(await ui.findByRole('alert')).toHaveTextContent('Database locked');
  });
  test('Settings reads the restored interval without pushing browser defaults',async()=>{
    localStorage.setItem('sync_interval_secs','60');const base=invoke.getMockImplementation();invoke.mockImplementation((cmd,args)=>cmd==='get_sync_interval'?Promise.resolve(3600):base(cmd,args));const ui=render(GeneralSettings);await waitFor(()=>expect(ui.getByLabelText('Background sync interval')).toHaveValue('3600'));expect(invoke.mock.calls.some(([c])=>c==='set_sync_interval')).toBe(false);
  });
  test('Add account opens the form from General Settings', async () => {
    const ui = render(SettingsPage,{db_path:'/tmp/archive.db',initial_section:'general'});
    await ui.findByLabelText('Background sync interval');
    await fireEvent.click(ui.getByRole('button',{name:'+ Add account'}));
    expect(await ui.findByRole('heading',{name:'Add account'})).toBeInTheDocument();
    expect(ui.getByLabelText('Email address')).toBeInTheDocument();
  });
  test('OAuth cancellation reaches the backend with its operation ID',async()=>{
    const pending=deferred();const base=invoke.getMockImplementation();invoke.mockImplementation((cmd,args)=>cmd==='add_google_oauth_account'?pending.promise:base(cmd,args));const ui=render(AddAccountForm,{db_path:'/tmp/archive.db'});await fireEvent.click(ui.getByRole('tab',{name:'Google / Gmail'}));await fireEvent.input(ui.getByLabelText('Google email address'),{target:{value:'a@example.com'}});await fireEvent.click(ui.getByRole('button',{name:'Sign in with Google'}));const operation=invoke.mock.calls.find(([c])=>c==='add_google_oauth_account')[1].operationId;await fireEvent.click(ui.getByRole('button',{name:'Cancel',exact:true}));await waitFor(()=>expect(invoke).toHaveBeenCalledWith('cancel_google_oauth',{operationId:operation}));pending.resolve({account:{id:1},mailboxes:[]});
  });
  test('OAuth browser failures retain a copyable authorization link for the current operation',async()=>{
    const pending=deferred();const base=invoke.getMockImplementation();invoke.mockImplementation((cmd,args)=>cmd==='add_google_oauth_account'?pending.promise:base(cmd,args));
    const ui=render(AddAccountForm,{db_path:'/tmp/archive.db'});await fireEvent.click(ui.getByRole('tab',{name:'Google / Gmail'}));await fireEvent.input(ui.getByLabelText('Google email address'),{target:{value:'a@example.com'}});await fireEvent.click(ui.getByRole('button',{name:'Sign in with Google'}));
    const operation=invoke.mock.calls.find(([c])=>c==='add_google_oauth_account')[1].operationId;
    handlers.get('google_oauth_browser')({payload:{operation_id:operation,url:'https://accounts.google.com/authorize',launch_error:'No default browser'}});
    expect(await ui.findByLabelText('Google authorization link')).toHaveValue('https://accounts.google.com/authorize');
    await fireEvent.click(ui.getByRole('button',{name:'Cancel',exact:true}));expect(ui.queryByLabelText('Google authorization link')).not.toBeInTheDocument();
    pending.resolve({account:{id:1},mailboxes:[]});
  });
});

test('confirmation dialog contains keyboard focus and restores it on close',async()=>{
  const opener=document.createElement('button');document.body.append(opener);opener.focus();
  const cancel=vi.fn();const ui=render(ConfirmDialog,{title:'Remove account',message:'Confirm removal',confirm_text:'DELETE',on_cancel:cancel});
  const input=ui.getByLabelText('Confirmation text');await waitFor(()=>expect(input).toHaveFocus());
  await fireEvent.keyDown(input,{key:'Tab',shiftKey:true});expect(ui.getByRole('button',{name:'Cancel'})).toHaveFocus();
  await fireEvent.keyDown(document.activeElement,{key:'Tab'});expect(input).toHaveFocus();
  await fireEvent.keyDown(input,{key:'Escape'});expect(cancel).toHaveBeenCalledOnce();
  ui.unmount();expect(opener).toHaveFocus();opener.remove();
});

describe('external-content policy',()=>{
  test.each(['<img src=//example.org/track>', '<img srcset="https://example.org/track 1x">', '<img src=https://example.org/track>'])('blocks before consent and provides an explicit load action: %s',async html=>{
    const ui=render(MessagePreview,{message:{id:1,body_html:html,attachments:[]}});const frame=ui.container.querySelector('iframe');expect(frame.getAttribute('srcdoc')).toContain('img-src data:;');await fireEvent.click(ui.getByRole('button',{name:/Load external images/i}));expect(frame.getAttribute('srcdoc')).toContain('img-src data: https: http:;');
  });
  test('CID images explicitly embedded in the body do not render twice',()=>{
    const ui=render(MessagePreview,{message:{id:1,body_html:'<img src="data:image/png;base64,AA==">',attachments:[{data_uri:'data:image/png;base64,AA==',embedded_in_body:true,content_type:'image/png',size:1}]}});expect(ui.container.querySelectorAll('img')).toHaveLength(0);
  });
});
