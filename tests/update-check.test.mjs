import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';

const root = fileURLToPath(new URL('../', import.meta.url));
const temporary = await mkdtemp(join(root, '.update-check-'));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022',
    '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/updateCheck.ts'], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout || String(result.error));
  const { updateSummary } = await import(pathToFileURL(join(temporary, 'updateCheck.js')).href);

  const release = { version: '0.2.0', name: 'Desegnos 0.2.0', url: 'https://github.com/chelij/desegnos/releases/tag/v0.2.0', publishedAt: '2026-11-01T10:00:00Z' };

  await test('a newer release offers one download link', () => {
    assert.deepEqual(updateSummary({ current: '0.1.0', latest: release, newer: true }),
      { variant: 'success', message: 'Desegnos 0.2.0 is available.', download: release.url });
  });
  await test('the newest release being current says so with the version', () => {
    assert.deepEqual(updateSummary({ current: '0.1.0', latest: { ...release, version: '0.1.0' }, newer: false }),
      { variant: 'info', message: 'Desegnos is up to date (0.1.0).', download: null });
  });
  await test('a repository without releases says so', () => {
    assert.deepEqual(updateSummary({ current: '0.1.0', latest: null, newer: false }),
      { variant: 'info', message: 'No releases yet.', download: null });
  });
} finally {
  await rm(temporary, { recursive: true, force: true });
}
