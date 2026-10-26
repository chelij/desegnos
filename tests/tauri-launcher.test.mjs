import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFile, mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

// A fake CLI records the actual child environment; --help must never run Cargo.
test('Tauri launcher normalizes CI=1 and preserves other CI values', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'desegnos tauri launcher '));
  try {
    await mkdir(join(directory, 'scripts'));
    await mkdir(join(directory, 'node_modules/@tauri-apps/cli'), { recursive: true });
    for (const name of ['tauri.mjs']) {
      await copyFile(new URL(`../scripts/${name}`, import.meta.url), join(directory, 'scripts', name));
    }
    await writeFile(join(directory, 'scripts/toolchain.mjs'), 'export const toolchainEnvironment = () => ({ ...process.env });\n');
    await writeFile(join(directory, 'node_modules/@tauri-apps/cli/tauri.js'), 'console.log(JSON.stringify({ ci: process.env.CI, args: process.argv.slice(2) }));\n');
    for (const [input, expected] of [['1', 'true'], ['true', 'true'], ['false', 'false'], ['0', '0']]) {
      const result = spawnSync(process.execPath, ['scripts/tauri.mjs', 'build', '--help'], {
        cwd: directory, env: { ...process.env, CI: input }, encoding: 'utf8',
      });
      assert.ifError(result.error);
      assert.equal(result.status, 0, result.stderr);
      assert.deepEqual(JSON.parse(result.stdout), { ci: expected, args: ['build', '--help'] });
    }
  } finally { await rm(directory, { recursive: true, force: true }); }
});
