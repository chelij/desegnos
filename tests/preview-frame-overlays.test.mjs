import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';

const root = fileURLToPath(new URL('../', import.meta.url));
const temporary = await mkdtemp(join(root, '.preview-frames-live-'));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022',
    '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/previewFrames.ts'], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout || String(result.error));
  const { collectFrame, frameKey, frameVisible, intersectsFrame, liveFrameSource, overlayPlacement, overlaySandbox,
    placeOverlays, resolveFrameSource } = await import(pathToFileURL(join(temporary, 'previewFrames.js')).href);

  const pageUrl = 'desegni-preview://static-session.localhost/play/solopong/index.html';
  const candidate = (change = {}) => ({
    rect: { x: 40, y: 80, width: 320, height: 200 }, display: 'block', visibility: 'visible',
    src: '../llama-wrap-preview.html', sandbox: null, allow: null, allowFullscreen: false, ...change,
  });

  await test('a framed site file keeps its page rect and resolves against the page session', () => {
    const frame = collectFrame(candidate(), pageUrl, 0);
    assert.deepEqual(frame.rect, { x: 40, y: 80, width: 320, height: 200 });
    assert.equal(frame.src, 'desegni-preview://static-session.localhost/play/llama-wrap-preview.html');
    assert.equal(frame.key, frameKey(0, frame.src));
    assert.equal(frame.sandbox, 'allow-scripts');
    assert.equal(frame.allow, null);
    assert.equal(frame.allowFullscreen, false);
  });

  await test('query and hash on a relative frame survive resolution', () => {
    const frame = collectFrame(candidate({ src: './game.html?level=2#start' }), pageUrl, 1);
    assert.equal(frame.src, 'desegni-preview://static-session.localhost/play/solopong/game.html?level=2#start');
    assert.equal(frame.key, frameKey(1, frame.src));
  });

  await test('an external https embed stays absolute and keeps its attributes', () => {
    const frame = collectFrame(candidate({ src: 'https://games.example/play?id=3', allow: 'fullscreen; autoplay', allowFullscreen: true }), pageUrl, 2);
    assert.equal(frame.src, 'https://games.example/play?id=3');
    assert.equal(frame.allow, 'fullscreen; autoplay');
    assert.equal(frame.allowFullscreen, true);
  });

  await test('hidden, collapsed and empty frames produce no overlay', () => {
    assert.equal(frameVisible(candidate()), true);
    assert.equal(frameVisible(candidate({ display: 'none' })), false);
    assert.equal(frameVisible(candidate({ visibility: 'hidden' })), false);
    assert.equal(frameVisible(candidate({ visibility: 'collapse' })), false);
    assert.equal(frameVisible(candidate({ rect: { x: 0, y: 0, width: 0, height: 0 } })), false);
    assert.equal(collectFrame(candidate({ rect: { x: 0, y: 0, width: 0, height: 40 } }), pageUrl, 0), null);
    assert.equal(collectFrame(candidate({ src: '' }), pageUrl, 0), null);
    assert.equal(collectFrame(candidate({ src: 'javascript:alert(1)' }), pageUrl, 0), null);
    assert.equal(collectFrame(candidate({ src: 'data:text/html,hi' }), pageUrl, 0), null);
  });

  await test('a relative frame without a page session cannot be served', () => {
    assert.equal(resolveFrameSource('./game.html', null), null);
    assert.equal(resolveFrameSource('https://games.example/play', null), 'https://games.example/play');
  });

  await test('the live overlay keeps author narrowing, adds scripts and drops escapes', () => {
    assert.equal(overlaySandbox(null), 'allow-scripts');
    assert.equal(overlaySandbox(''), 'allow-scripts');
    assert.equal(overlaySandbox('allow-scripts allow-forms'), 'allow-scripts allow-forms');
    assert.equal(overlaySandbox('allow-forms allow-scripts allow-forms'), 'allow-scripts allow-forms');
    assert.equal(overlaySandbox('allow-scripts allow-same-origin'), 'allow-scripts');
    assert.equal(overlaySandbox('allow-scripts allow-top-navigation allow-top-navigation-by-user-activation allow-popups'),
      'allow-scripts allow-popups');
    assert.equal(overlaySandbox('allow-scripts allow-popups-to-escape-sandbox'), 'allow-scripts');
    // The static document stores its hardening, so the author's allow-scripts arrives as same-origin.
    assert.equal(overlaySandbox('allow-same-origin allow-forms'), 'allow-scripts allow-forms');
  });

  await test('own-site frames move to the live session and external frames do not', () => {
    const livePage = 'desegni-preview://live-session.localhost/play/solopong/index.html';
    const own = 'desegni-preview://static-session.localhost/play/llama-wrap-preview.html?level=2#start';
    assert.equal(liveFrameSource(own, pageUrl, livePage), 'desegni-preview://live-session.localhost/play/llama-wrap-preview.html?level=2#start');
    assert.equal(liveFrameSource('https://games.example/play', pageUrl, livePage), 'https://games.example/play');
    assert.equal(liveFrameSource(own, null, livePage), own);
    const windowsPage = 'http://desegni-preview.localhost/static/page/index.html';
    const windowsLive = 'http://desegni-preview.localhost/live/page/index.html';
    assert.equal(liveFrameSource('http://desegni-preview.localhost/static/game.html', windowsPage, windowsLive),
      'http://desegni-preview.localhost/live/game.html');
  });

  await test('the overlay box is the page rect offset and scaled with the canvas', () => {
    assert.deepEqual(overlayPlacement({ x: 140, y: 90, width: 320, height: 200 }, { x: 100, y: 50 }, 0.5),
      { left: 20, top: 20, width: 160, height: 100 });
    assert.deepEqual(overlayPlacement({ x: 0, y: 0, width: 1440, height: 900 }, { x: 0, y: -64 }, 1),
      { left: 0, top: 64, width: 1440, height: 900 });
  });

  await test('overlays are limited to visible frames in document order', () => {
    const frame = (x, y) => ({ key: `${x}:${y}`, rect: { x, y, width: 100, height: 100 } });
    const viewport = { x: 0, y: 0, width: 500, height: 500 };
    assert.equal(intersectsFrame(frame(0, 0).rect, viewport), true);
    assert.equal(intersectsFrame(frame(450, 450).rect, viewport), true);
    assert.equal(intersectsFrame(frame(500, 0).rect, viewport), false);
    assert.equal(intersectsFrame(frame(-100, 0).rect, viewport), false);
    const frames = [frame(0, 0), frame(600, 0), frame(100, 100), frame(200, 200)];
    assert.deepEqual(placeOverlays(frames, viewport).map(item => item.key), ['0:0', '100:100', '200:200']);
    assert.deepEqual(placeOverlays([...Array(10)].map((_, index) => frame(index * 10, 0)), viewport, 8).map(item => item.key),
      [...Array(8)].map((_, index) => `${index * 10}:0`));
    assert.deepEqual(placeOverlays(frames, viewport, 0), []);
  });
} finally {
  await rm(temporary, { recursive: true, force: true });
}
