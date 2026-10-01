import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { chmod, lstat, mkdtemp, readFile, readdir, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { prepareLauncher } from './prepare_appimage_tools.mjs';

const bytes = Buffer.from('synthetic pinned launcher');
const digest = createHash('sha256').update(bytes).digest('hex');

async function fixture(run) {
  const root = await mkdtemp(join(tmpdir(), 'amberize-apprun-test-'));
  try { await run(join(root, 'AppRun-x86_64'), root); }
  finally { await rm(root, { recursive: true, force: true }); }
}

test('repairs a cached 0770 launcher without changing its bytes or downloading again', async () => {
  await fixture(async (path) => {
    await writeFile(path, bytes);
    await chmod(path, 0o770);
    await prepareLauncher(path, () => { throw new Error('unexpected download'); }, digest);
    assert.deepEqual(await readFile(path), bytes);
    assert.equal((await lstat(path)).mode & 0o777, 0o755);
  });
});

test('fresh launcher remains readable/executable for others under a restrictive umask', async () => {
  await fixture(async (path) => {
    const previous = process.umask(0o077);
    try { await prepareLauncher(path, async () => bytes, digest); }
    finally { process.umask(previous); }
    assert.equal((await lstat(path)).mode & 0o777, 0o755);
  });
});

test('rejects altered downloads without publishing a partial cache file', async () => {
  await fixture(async (path, root) => {
    await assert.rejects(prepareLauncher(path, async () => Buffer.from('altered'), digest), /checksum/);
    assert.deepEqual(await readdir(root), []);
  });
});

test('refuses to chmod a symlink or an unrecognized cached launcher', async () => {
  await fixture(async (path, root) => {
    const target = join(root, 'unrelated');
    await writeFile(target, bytes, { mode: 0o600 });
    await symlink(target, path);
    await assert.rejects(prepareLauncher(path, async () => bytes, digest), /regular file/);
    assert.equal((await lstat(target)).mode & 0o777, 0o600);
    await rm(path);
    await writeFile(path, Buffer.from('different'), { mode: 0o600 });
    await assert.rejects(prepareLauncher(path, async () => bytes, digest), /checksum/);
    assert.equal((await lstat(path)).mode & 0o777, 0o600);
  });
});
