import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';

const root = fileURLToPath(new URL('../', import.meta.url));
const temporary = await mkdtemp(join(root, '.live-frame-'));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022',
    '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/liveFrame.ts'], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout || String(result.error));
  const { liveFrame, liveVisibility, LIVE_DEVICE_SIZES } = await import(pathToFileURL(join(temporary, 'liveFrame.js')).href);

  await test('desktop live keeps the page width and the whole scroll height', () => {
    assert.deepEqual(liveFrame('desktop', { width: 1200, height: 900 }, 2400), { width: 1200, height: 2400 });
    // A page shorter than its viewport still frames the viewport.
    assert.deepEqual(liveFrame('desktop', { width: 1200, height: 900 }, 400), { width: 1200, height: 900 });
    assert.deepEqual(liveFrame('desktop', { width: 1024, height: 700 }, 2400.4), { width: 1024, height: 2401 });
    // A page that has not reported its height yet falls back to the viewport.
    assert.deepEqual(liveFrame('desktop', { width: 1200, height: 900 }, Number.NaN), { width: 1200, height: 900 });
    assert.deepEqual(liveFrame('desktop', { width: 1200, height: 900 }, 0), { width: 1200, height: 900 });
  });

  await test('a device frame is exactly the device viewport, never the page size', () => {
    assert.deepEqual(liveFrame('tablet', { width: 1200, height: 900 }, 2400), { width: 768, height: 1024 });
    assert.deepEqual(liveFrame('phone', { width: 1200, height: 900 }, 2400), { width: 390, height: 844 });
    assert.deepEqual(liveFrame('phone', { width: 390, height: 844 }, 2400), { width: 390, height: 844 });
    assert.equal(liveFrame('tablet', { width: 1200, height: 900 }, 2400).width, LIVE_DEVICE_SIZES.tablet.width);
    // The frame is a copy, so a caller cannot change the shared preset.
    const frame = liveFrame('phone', { width: 1200, height: 900 }, 900);
    frame.width = 1;
    assert.equal(liveFrame('phone', { width: 1200, height: 900 }, 900).width, 390);
  });

  await test('live hides the static editing page and its screen guide', () => {
    assert.deepEqual(liveVisibility(null, 1200), { staticPage: true, screenGuide: true, requests: true });
    for (const device of ['desktop', 'tablet', 'phone']) {
      const shown = liveVisibility(liveFrame(device, { width: 1200, height: 900 }, 2400), 1200);
      assert.equal(shown.staticPage, false, `${device} static page`);
      assert.equal(shown.screenGuide, false, `${device} screen guide`);
    }
  });

  await test('requests are drawn only when the live frame is the page width', () => {
    assert.equal(liveVisibility(liveFrame('desktop', { width: 1200, height: 900 }, 2400), 1200).requests, true);
    assert.equal(liveVisibility(liveFrame('tablet', { width: 1200, height: 900 }, 2400), 1200).requests, false);
    assert.equal(liveVisibility(liveFrame('phone', { width: 1200, height: 900 }, 2400), 1200).requests, false);
    // A page that is already the device width keeps its requests in device mode.
    assert.equal(liveVisibility(liveFrame('phone', { width: 390, height: 844 }, 2400), 390).requests, true);
    assert.equal(liveVisibility(liveFrame('tablet', { width: 768, height: 1024 }, 2400), 768).requests, true);
  });
} finally {
  await rm(temporary, { recursive: true, force: true });
}
