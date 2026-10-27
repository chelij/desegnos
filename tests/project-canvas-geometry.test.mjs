import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';

const root = fileURLToPath(new URL('../', import.meta.url));
const temporary = await mkdtemp(join(root, '.project-canvas-geometry-'));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022',
    '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/projectCanvasGeometry.ts'], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout || String(result.error));
  // Node needs the extension the browser build leaves out.
  for (const name of await readdir(temporary)) {
    if (!name.endsWith('.js')) continue;
    const path = join(temporary, name);
    await writeFile(path, (await readFile(path, 'utf8')).replace(/from '(\.\/[^']+)'/g, (_, value) => `from '${value.endsWith('.js') ? value : value + '.js'}'`));
  }
  const { boundaryHeight, layoutBoundaries, unionBounds, sameBoundaries, fitCamera, zoomCameraAt, panCamera,
    snapshotCacheKey, snapshotContentKey, planSnapshots, rememberSnapshot, updateSnapshotHeight,
    BOUNDARY_GAP, MIN_BOUNDARY_HEIGHT, MAX_BOUNDARY_HEIGHT, FIT_PADDING } = await import(pathToFileURL(join(temporary, 'projectCanvasGeometry.js')).href);
  const { MIN_ZOOM, ZOOM_LEVELS } = await import(pathToFileURL(join(temporary, 'canvasGeometry.js')).href);

  await test('boundary heights clamp to a sane range', () => {
    assert.equal(boundaryHeight(900), 900);
    assert.equal(boundaryHeight(0), MIN_BOUNDARY_HEIGHT);
    assert.equal(boundaryHeight(-40), MIN_BOUNDARY_HEIGHT);
    assert.equal(boundaryHeight(Number.NaN), MIN_BOUNDARY_HEIGHT);
    assert.equal(boundaryHeight(12), MIN_BOUNDARY_HEIGHT);
    assert.equal(boundaryHeight(20000), MAX_BOUNDARY_HEIGHT);
  });

  await test('pages lay out left to right, home first, tops aligned', () => {
    const laid = layoutBoundaries([
      { id: 'second', width: 800, height: 0 },
      { id: 'third', width: 390, height: 844 },
      { id: 'home', width: 1440, height: 900 },
    ], 'home');
    assert.deepEqual(laid.map(item => item.pageId), ['home', 'second', 'third']);
    assert.deepEqual(laid.map(item => item.rect), [
      { x: 0, y: 0, width: 1440, height: 900 },
      { x: 1440 + BOUNDARY_GAP, y: 0, width: 800, height: MIN_BOUNDARY_HEIGHT },
      { x: 1440 + BOUNDARY_GAP + 800 + BOUNDARY_GAP, y: 0, width: 390, height: 844 },
    ]);
    assert.equal(BOUNDARY_GAP, 160);
    assert.deepEqual(layoutBoundaries([{ id: 'only', width: 0, height: 300 }], 'only')[0].rect, { x: 0, y: 0, width: 0, height: 300 });
  });

  await test('union covers every boundary and comparison catches real changes', () => {
    const first = layoutBoundaries([{ id: 'a', width: 400, height: 300 }, { id: 'b', width: 200, height: 500 }], 'a');
    assert.deepEqual(unionBounds(first.map(item => item.rect)), { x: 0, y: 0, width: 400 + BOUNDARY_GAP + 200, height: 500 });
    assert.deepEqual(unionBounds([]), { x: 0, y: 0, width: 0, height: 0 });
    assert.equal(sameBoundaries(first, layoutBoundaries([{ id: 'a', width: 400, height: 300 }, { id: 'b', width: 200, height: 500 }], 'a')), true);
    assert.equal(sameBoundaries(first, layoutBoundaries([{ id: 'a', width: 400, height: 300 }, { id: 'b', width: 200, height: 520 }], 'a')), false);
    assert.equal(sameBoundaries(first, layoutBoundaries([{ id: 'b', width: 400, height: 300 }, { id: 'a', width: 200, height: 500 }], 'a')), false);
  });

  await test('fit centers the boundaries inside the padding and never magnifies past 100%', () => {
    const fitted = fitCamera({ x: 0, y: 0, width: 1600, height: 1000 }, { width: 1200, height: 800 });
    assert.ok(Math.abs(fitted.zoom - 0.69) < 1e-9);
    assert.ok(Math.abs(fitted.x - FIT_PADDING) < 1e-9);
    assert.ok(Math.abs(fitted.y - (800 - 1000 * fitted.zoom) / 2) < 1e-9);
    const small = fitCamera({ x: 40, y: 20, width: 300, height: 200 }, { width: 1200, height: 800 });
    assert.equal(small.zoom, 1);
    assert.equal(small.x, (1200 - 300) / 2 - 40);
    assert.equal(small.y, (800 - 200) / 2 - 20);
    const huge = fitCamera({ x: 0, y: 0, width: 10_000_000, height: 1000 }, { width: 1200, height: 800 });
    assert.equal(huge.zoom, MIN_ZOOM);
  });

  await test('zoom stays anchored under the cursor and clamps to the edit range', () => {
    const camera = { x: 40, y: -20, zoom: 0.5 }, point = { x: 120, y: 80 };
    const world = { x: (point.x - camera.x) / camera.zoom, y: (point.y - camera.y) / camera.zoom };
    const zoomed = zoomCameraAt(camera, point, 1);
    assert.deepEqual(zoomed, { zoom: 1, x: -40, y: -120 });
    assert.deepEqual({ x: (point.x - zoomed.x) / zoomed.zoom, y: (point.y - zoomed.y) / zoomed.zoom }, world);
    assert.equal(zoomCameraAt(camera, point, 100).zoom, ZOOM_LEVELS.at(-1));
    assert.equal(zoomCameraAt(camera, point, 0).zoom, MIN_ZOOM);
    assert.equal(zoomCameraAt(camera, point, Number.NaN).zoom, 0.5);
  });

  await test('pan moves the camera by the drag delta', () => {
    const camera = { x: 1, y: 2, zoom: 0.5 };
    assert.deepEqual(panCamera(camera, { x: 10, y: -4 }), { x: 11, y: -2, zoom: 0.5 });
    assert.deepEqual(camera, { x: 1, y: 2, zoom: 0.5 });
    assert.equal(panCamera(camera, { x: Number.NaN, y: 0 }), camera);
  });

  await test('snapshot keys follow page content and viewport, not project revision', () => {
    const content = { html: '<h1>A</h1>', css: 'h1{color:red}', javascript: '' }, viewport = { width: 1440, height: 900 };
    const key = snapshotContentKey(content, viewport);
    assert.equal(snapshotContentKey({ ...content }, { ...viewport }), key);
    assert.notEqual(snapshotContentKey({ ...content, html: '<h1>B</h1>' }, viewport), key);
    assert.notEqual(snapshotContentKey({ ...content, css: 'h1{color:blue}' }, viewport), key);
    assert.notEqual(snapshotContentKey({ ...content, javascript: 'void 0' }, viewport), key);
    assert.notEqual(snapshotContentKey(content, { width: 390, height: 900 }), key);
    assert.notEqual(snapshotContentKey(content, { width: 1440, height: 844 }), key);
    assert.equal(snapshotContentKey(null, viewport), snapshotContentKey(null, { ...viewport }));
    assert.notEqual(snapshotContentKey(null, viewport), key);
  });

  await test('cached snapshots show immediately and only changed pages are captured', () => {
    const cache = new Map(), viewport = { width: 1440, height: 900 };
    const home = { id: 'home', viewport, content: { html: 'home', css: '', javascript: '' } };
    const about = { id: 'about', viewport, content: { html: 'about', css: '', javascript: '' } };
    const blank = { id: 'blank', viewport, content: null };
    const homeKey = snapshotContentKey(home.content, viewport), aboutKey = snapshotContentKey(about.content, viewport);
    rememberSnapshot(cache, 'p1', 'home', { key: homeKey, html: 'home:snapshot', height: 1200 });
    rememberSnapshot(cache, 'p1', 'about', { key: 'stale', html: 'about:old', height: 640 });
    let plan = planSnapshots(cache, 'p1', [home, about, blank]);
    assert.deepEqual(plan.snapshots, { home: { html: 'home:snapshot' }, about: { html: 'about:old' } });
    assert.deepEqual(plan.heights, { home: 1200, about: 640 });
    assert.deepEqual(plan.pending.map(item => item.page.id), ['about']);
    assert.equal(plan.pending[0].key, aboutKey);
    rememberSnapshot(cache, 'p1', 'about', { key: aboutKey, html: 'about:snapshot', height: 640 });
    plan = planSnapshots(cache, 'p1', [home, about, blank]);
    assert.deepEqual(plan.pending, []);
    assert.deepEqual(plan.snapshots.about, { html: 'about:snapshot' });
    plan = planSnapshots(cache, 'p1', [{ ...home, content: { ...home.content, css: 'body{}' } }, about, blank]);
    assert.deepEqual(plan.pending.map(item => item.page.id), ['home']);
    assert.deepEqual(plan.snapshots.home, { html: 'home:snapshot' });
    assert.notEqual(plan.pending[0].key, homeKey);
  });

  await test('cache drops deleted pages and leaves other projects alone', () => {
    const cache = new Map(), viewport = { width: 800, height: 600 }, content = { html: 'x', css: '', javascript: '' };
    rememberSnapshot(cache, 'p1', 'home', { key: snapshotContentKey(content, viewport), html: 'p1:home', height: 600 });
    rememberSnapshot(cache, 'p1', 'gone', { key: snapshotContentKey(content, viewport), html: 'p1:gone', height: 600 });
    rememberSnapshot(cache, 'p2', 'home', { key: snapshotContentKey(content, viewport), html: 'p2:home', height: 600 });
    const plan = planSnapshots(cache, 'p1', [{ id: 'home', viewport, content }]);
    assert.deepEqual(plan.snapshots, { home: { html: 'p1:home' } });
    assert.equal(cache.has(snapshotCacheKey('p1', 'gone')), false);
    assert.equal(cache.has(snapshotCacheKey('p1', 'home')), true);
    assert.equal(cache.has(snapshotCacheKey('p2', 'home')), true);
  });

  await test('measured heights update cache entries in place', () => {
    const cache = new Map(), key = snapshotCacheKey('p1', 'home');
    rememberSnapshot(cache, 'p1', 'home', { key: 'content', html: 'snapshot', height: 0 });
    updateSnapshotHeight(cache, 'p1', 'home', 913);
    assert.deepEqual(cache.get(key), { key: 'content', html: 'snapshot', height: 913 });
    updateSnapshotHeight(cache, 'p1', 'missing', 500);
    assert.equal(cache.size, 1);
    updateSnapshotHeight(cache, 'p1', 'home', 0);
    assert.equal(cache.get(key).height, 913);
  });
} finally {
  await rm(temporary, { recursive: true, force: true });
}
