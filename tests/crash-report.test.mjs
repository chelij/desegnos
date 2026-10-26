import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';

const root = fileURLToPath(new URL('../', import.meta.url));
const temporary = await mkdtemp(join(root, '.crash-report-'));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022',
    '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/crashReportText.ts'], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout || String(result.error));
  const { CRASH_NOTICE, crashOpenFailure } = await import(pathToFileURL(join(temporary, 'crashReportText.js')).href);

  await test('the notice uses one short plain sentence', () => {
    assert.equal(CRASH_NOTICE, 'Desegnos closed unexpectedly last time.');
  });

  await test('a failed open keeps the issue address for copying', () => {
    const url = 'https://github.com/chelij/desegnos/issues/new?title=Crash&body=Text';
    const failure = crashOpenFailure({ message: 'Couldn\'t open your browser.', url });
    assert.equal(failure.url, url);
  });

  await test('a plain failure becomes readable copy and has no fallback address', () => {
    const failure = crashOpenFailure('boom');
    assert.equal(failure.url, null);
    assert.equal(failure.message, 'Something went wrong. Try again.');
  });

  await test('an ordinary rejection is not a crash address', () => {
    const failure = crashOpenFailure(new Error('ordinary'));
    assert.equal(failure.url, null);
    assert.match(failure.message, /went wrong|Try again/);
  });
} finally {
  await rm(temporary, { recursive: true, force: true });
}
