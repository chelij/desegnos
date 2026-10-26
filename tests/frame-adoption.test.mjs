import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';

const root = fileURLToPath(new URL('../', import.meta.url));
const temporary = await mkdtemp(join(root, '.frame-adoption-'));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022',
    '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/frameAdoption.ts'], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout || String(result.error));
  const { adoptsFrameTarget } = await import(pathToFileURL(join(temporary, 'frameAdoption.js')).href);

  const request = { id: 'one', rect: { x: 0, y: 0, width: 10, height: 10 }, zIndex: 1, prompt: '' };
  const frame = { pageId: 'page', iframeId: 'iframepage', document: { kind: 'page', pageId: 'inner' }, rect: { x: 0, y: 0, width: 5, height: 5 } };

  await test('a pending host-page request may gain its frame document', () => {
    assert.equal(adoptsFrameTarget(request), true);
    assert.equal(adoptsFrameTarget({ ...request, frameTarget: frame }), true);
  });

  await test('an explicit whole-page request is never pulled back into a frame', () => {
    assert.equal(adoptsFrameTarget({ ...request, wholePage: true }), false);
    // An inconsistent saved state still honours the whole-page choice.
    assert.equal(adoptsFrameTarget({ ...request, wholePage: true, frameTarget: frame }), false);
  });

  await test('turning whole page off keeps the request adoptable and finished requests are skipped', () => {
    assert.equal(adoptsFrameTarget({ ...request, wholePageDisabled: true }), true);
    assert.equal(adoptsFrameTarget({ ...request, completed: true }), false);
    assert.equal(adoptsFrameTarget({ ...request, completed: true, wholePage: true }), false);
  });
} finally {
  await rm(temporary, { recursive: true, force: true });
}
