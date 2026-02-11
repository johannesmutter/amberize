import { describe, expect, test, vi, beforeEach } from 'vitest';

describe('tauri_bridge', () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
  });

  test('tauri_invoke calls core.invoke', async () => {
    vi.doMock('@tauri-apps/api/core', () => ({
      invoke: vi.fn(async (cmd, args) => ({ cmd, args })),
    }));

    const { tauri_invoke } = await import('./tauri_bridge.js');
    const result = await tauri_invoke('cmd', { a: 1 });
    expect(result).toEqual({ cmd: 'cmd', args: { a: 1 } });
  });

  test('tauri_invoke formats non-Error failures', async () => {
    vi.doMock('@tauri-apps/api/core', () => ({
      invoke: vi.fn(async () => {
        throw { code: 1 };
      }),
    }));

    const { tauri_invoke } = await import('./tauri_bridge.js');
    await expect(tauri_invoke('cmd')).rejects.toThrow('{"code":1}');
  });

  test('tauri_invoke falls back when error is not json-stringifiable', async () => {
    const circular = {};
    // @ts-ignore
    circular.self = circular;
    vi.doMock('@tauri-apps/api/core', () => ({
      invoke: vi.fn(async () => {
        throw circular;
      }),
    }));

    const { tauri_invoke } = await import('./tauri_bridge.js');
    await expect(tauri_invoke('cmd')).rejects.toThrow('[object Object]');
  });

  test('tauri_listen calls event.listen', async () => {
    const unlisten_fn = vi.fn();
    vi.doMock('@tauri-apps/api/event', () => ({
      listen: vi.fn(async (name, handler) => {
        handler({ event: name, payload: 1 });
        return unlisten_fn;
      }),
    }));

    const { tauri_listen } = await import('./tauri_bridge.js');
    const handler = vi.fn();
    const returned = await tauri_listen('e', handler);
    expect(returned).toBe(unlisten_fn);
    expect(handler).toHaveBeenCalledWith({ event: 'e', payload: 1 });
  });

  test('tauri_listen throws a readable error when event module fails', async () => {
    vi.doMock('@tauri-apps/api/event', () => ({
      listen: vi.fn(() => {
        throw new Error('boom');
      }),
    }));

    const { tauri_listen } = await import('./tauri_bridge.js');
    await expect(tauri_listen('e', () => {})).rejects.toThrow('boom');
  });

  test('tauri_save_dialog returns null for non-string selection', async () => {
    vi.doMock('@tauri-apps/plugin-dialog', () => ({
      save: vi.fn(async () => null),
    }));

    const { tauri_save_dialog } = await import('./tauri_bridge.js');
    const result = await tauri_save_dialog({ title: 'x' });
    expect(result).toBeNull();
  });

  test('tauri_save_dialog returns selected path', async () => {
    vi.doMock('@tauri-apps/plugin-dialog', () => ({
      save: vi.fn(async () => '/tmp/out.zip'),
    }));

    const { tauri_save_dialog } = await import('./tauri_bridge.js');
    const result = await tauri_save_dialog({ title: 'x' });
    expect(result).toBe('/tmp/out.zip');
  });

  test('tauri_save_dialog formats errors', async () => {
    vi.doMock('@tauri-apps/plugin-dialog', () => ({
      save: vi.fn(async () => {
        throw new Error('nope');
      }),
    }));

    const { tauri_save_dialog } = await import('./tauri_bridge.js');
    await expect(tauri_save_dialog({ title: 'x' })).rejects.toThrow('nope');
  });
});

