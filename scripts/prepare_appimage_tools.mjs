import { createHash, randomUUID } from 'node:crypto';
import { chmod, lstat, mkdir, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

// This is the same launcher Tauri downloads; pin its bytes and change only its mode.
export const LAUNCHER_URL = 'https://github.com/tauri-apps/binary-releases/releases/download/apprun-old/AppRun-x86_64';
export const LAUNCHER_SHA256 = 'f30140a43a0a59e46db21bdefdf749b9e9f2c6946e92afabbacf98b8ae73fb4f';

export async function prepareLauncher(path, download, expectedSha256 = LAUNCHER_SHA256) {
  let existing;
  try {
    existing = await lstat(path);
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
  }
  if (existing && !existing.isFile()) throw new Error('AppRun cache must be a regular file');
  const bytes = existing ? await readFile(path) : await download();
  if (createHash('sha256').update(bytes).digest('hex') !== expectedSha256) {
    throw new Error('AppRun checksum does not match the pinned upstream launcher');
  }
  if (existing) {
    await chmod(path, 0o755);
  } else {
    const temporary = `${path}.${randomUUID()}.tmp`;
    try {
      await writeFile(temporary, bytes, { flag: 'wx', mode: 0o755 });
      await chmod(temporary, 0o755);
      await rename(temporary, path);
    } finally {
      await rm(temporary, { force: true });
    }
  }
}

export async function main() {
  if (process.platform !== 'linux') return;
  if (process.arch !== 'x64') throw new Error('The configured Linux release target requires x86_64');
  const cache = join(process.env.XDG_CACHE_HOME || join(homedir(), '.cache'), 'tauri');
  await mkdir(cache, { recursive: true });
  await prepareLauncher(join(cache, 'AppRun-x86_64'), async () => {
    const response = await fetch(LAUNCHER_URL, { signal: AbortSignal.timeout(30000) });
    if (!response.ok) throw new Error(`AppRun download failed: HTTP ${response.status}`);
    return Buffer.from(await response.arrayBuffer());
  });
  console.log('Prepared checksum-verified AppRun with mode 0755 before AppImage bundling');
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
