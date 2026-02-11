import { describe, expect, test, beforeEach } from 'vitest';
import { clear_config, load_config, save_config } from './config.js';

describe('config', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  test('load_config returns null when empty', async () => {
    expect(await load_config()).toBeNull();
  });

  test('save_config + load_config roundtrip', async () => {
    await save_config({ db_path: '/tmp/archive.sqlite3' });
    expect(await load_config()).toEqual({ db_path: '/tmp/archive.sqlite3' });
  });

  test('load_config rejects invalid json', async () => {
    localStorage.setItem('amberize_config_v1', '{');
    expect(await load_config()).toBeNull();
  });

  test('load_config rejects missing db_path', async () => {
    localStorage.setItem('amberize_config_v1', JSON.stringify({}));
    expect(await load_config()).toBeNull();
  });

  test('load_config rejects non-object values', async () => {
    localStorage.setItem('amberize_config_v1', JSON.stringify('x'));
    expect(await load_config()).toBeNull();
  });

  test('load_config rejects empty db_path', async () => {
    localStorage.setItem('amberize_config_v1', JSON.stringify({ db_path: '   ' }));
    expect(await load_config()).toBeNull();
  });

  test('clear_config removes stored config', async () => {
    await save_config({ db_path: '/tmp/archive.sqlite3' });
    await clear_config();
    expect(await load_config()).toBeNull();
  });
});

