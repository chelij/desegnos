import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';

const root = fileURLToPath(new URL('../', import.meta.url));
const temporary = await mkdtemp(join(root, '.canvas-geometry-'));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--jsx', 'react-jsx', '--module', 'esnext',
    '--target', 'ES2022', '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/canvasGeometry.ts', 'src/shortcuts.ts'], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout || String(result.error));
  const { resizedElementSize, elementResizeRect, clientToCanvas, geometryForGesture, gridSteps, keyboardRectChange, rectDelta, requestAt, requestPlacement,
    initialWorkspaceBounds, expandWorkspace, percent, percentageTicks, alignmentLines, WORLD_MIN, WORLD_MAX, wheelZoom, zoomOrigin, steppedZoom } = await import(pathToFileURL(join(temporary, 'canvasGeometry.js')).href);
  const { editorShortcut } = await import(pathToFileURL(join(temporary, 'shortcuts.js')).href);
  await test('local resize resolves independent axes and scaled or rotated layouts', () => {
    assert.deepEqual(resizedElementSize({x:200,y:80},{x:1,y:1},[1,0,0,1],{x:30,y:-12}),{x:230,y:68});
    assert.deepEqual(resizedElementSize({x:200,y:80},{x:1,y:1},[0,2,2,0],{x:20,y:40}),{x:220,y:90});
    assert.equal(resizedElementSize({x:200,y:80},{x:1,y:1},[0,0,0,0],{x:20,y:40}),null);
  });
  await test('near45-degree resize stays proportional, finite and padding-aware instead of exploding', () => {
    for(const angle of [44.999,45,45.001]) {
      const rad=angle*Math.PI/180, a=Math.abs(Math.cos(rad)), b=Math.abs(Math.sin(rad));
      const result=resizedElementSize({x:200,y:80},{x:24,y:20},[a,b,b,a],{x:10,y:0});
      assert.ok(result.x>200&&result.x<220&&result.y>80&&result.y<90,JSON.stringify(result));
      assert.ok(Math.abs(result.x/result.y-2.5)<1e-9);
      const smallest=resizedElementSize({x:200,y:80},{x:24,y:20},[a,b,b,a],{x:-10000,y:-10000});
      assert.ok(smallest.x>=24&&smallest.y>=20);
    }
  });
  await test('element corners preserve their opposite anchor through signed movement and all four handles', () => {
    const initial={x:-35,y:27,width:240,height:84};
    for (const corner of ['nw','ne','sw','se']) for (const delta of [{x:0,y:0},{x:30,y:-12},{x:-18,y:24}]) {
      const result=elementResizeRect(initial,delta,corner,{x:10,y:10},false);
      assert.equal(corner.includes('w') ? result.x+result.width : result.x, corner.includes('w') ? initial.x+initial.width : initial.x);
      assert.equal(corner.includes('n') ? result.y+result.height : result.y, corner.includes('n') ? initial.y+initial.height : initial.y);
      assert.equal(corner.includes('w') ? result.x : result.x+result.width, (corner.includes('w') ? initial.x : initial.x+initial.width)+delta.x);
      assert.equal(corner.includes('n') ? result.y : result.y+result.height, (corner.includes('n') ? initial.y : initial.y+initial.height)+delta.y);
    }
  });
  await test('corner resizing snaps the dragged edges to the grid and bounds crossing without flipping', () => {
    const initial={x:13,y:-7,width:240,height:84};
    const snapped=elementResizeRect(initial,{x:19,y:22},'se',{x:20,y:10},true);
    assert.deepEqual(snapped,{x:13,y:-7,width:267,height:107});
    const crossed=elementResizeRect(initial,{x:-1000,y:-1000},'se',{x:1,y:1},false);
    assert.deepEqual(crossed,{x:13,y:-7,width:1,height:1});
    for(const corner of ['nw','ne','sw','se']) {
      const result=elementResizeRect(initial,{x:corner.includes('w')?-1e9:1e9,y:corner.includes('n')?-1e9:1e9},corner,{x:1,y:1},false);
      assert.ok(result.x>=WORLD_MIN && result.y>=WORLD_MIN && result.x+result.width<=WORLD_MAX && result.y+result.height<=WORLD_MAX);
    }
  });
  await test('cursor-anchored wheel zoom preserves the world point through coalesced zoom changes', () => {
    const pointer = { x: 317, y: 193 };
    let origin = { x: -555, y: 333 }; let zoom = 0.75;
    const anchor = { x: origin.x + pointer.x / zoom, y: origin.y + pointer.y / zoom };
    for (const delta of [-120, -120, 120, 180, -360]) {
      const next = wheelZoom(zoom, delta); origin = zoomOrigin(origin, pointer, zoom, next); zoom = next;
      assert.ok(Math.abs(origin.x + pointer.x / zoom - anchor.x) < 1e-9);
      assert.ok(Math.abs(origin.y + pointer.y / zoom - anchor.y) < 1e-9);
    }
    assert.equal(wheelZoom(4, -120), 4);
    assert.equal(wheelZoom(0.001, 120), 0.001);
    assert.ok(wheelZoom(0.005, 120) < 0.005);
    assert.ok(steppedZoom(0.05, -1) < 0.05);
    assert.equal(steppedZoom(0.001, -1), 0.001);
    assert.equal(wheelZoom(1, NaN), 1);
    assert.equal(steppedZoom(0.87, 1), 1);
    assert.equal(steppedZoom(0.87, -1), 0.75);
    assert.equal(steppedZoom(4, 1), 4);
  });
  await test('editor shortcuts preserve text undo, clipboard, composition and handled canvas keys', () => {
    const key = (name, patch = {}, editing = false) => editorShortcut({ key: name, ctrlKey: true, metaKey: false, shiftKey: false, altKey: false, ...patch }, editing);
    assert.equal(key('z'), 'undo'); assert.equal(key('z', { shiftKey: true }), 'redo'); assert.equal(key('y'), 'redo');
    for (const name of ['z', 'y', 'a', 'c', 'v', 'x', 'Delete', 'Backspace']) assert.equal(key(name, {}, true), null);
    assert.equal(key('z', { isComposing: true }), null); assert.equal(key('z', { defaultPrevented: true }), null);
    assert.equal(key('z', { altKey: true }), null); assert.equal(key('z', { ctrlKey: false, metaKey: true }), 'undo');
    for (const [name, action] of [['n','new'],['o','open'],['s','save'],['+','zoomIn'],['=','zoomIn'],['-','zoomOut'],['0','fit'],['1','actualSize'],['2','doubleSize'],['PageDown','nextPage'],['PageUp','previousPage'],['Tab','nextPage']]) assert.equal(key(name), action);
    assert.equal(key('Unidentified', { code: 'Tab', shiftKey: true }), 'previousPage');
    assert.equal(key('Unidentified', { code: 'KeyQ', shiftKey: true }), null);
    assert.equal(key('s', { shiftKey: true }), 'saveAs'); assert.equal(key('Tab', { shiftKey: true }), 'previousPage');
    assert.equal(key('?', { ctrlKey: false, shiftKey: true }), 'help'); assert.equal(key('F1', { ctrlKey: false }), 'help');
    assert.equal(key('Delete', { ctrlKey: false }), 'remove'); assert.equal(key('Delete', { ctrlKey: false }, true), null);
  });
  const guide = { width: 1440, height: 900 };
  const grid = { spacing: 8, unit: 'pixels', snap: true, rulers: true };
  const free = { ...grid, snap: false };
  const origin = { left: -120, top: -50 };
  for (const zoom of [0.25, 0.5, 0.75, 1, 1.5, 2]) {
    await test(`pointer coordinates follow signed world origin and panning at ${zoom * 100}% zoom`, () => {
      const world = { x: -1600, y: -900 };
      const target = { x: -200, y: 1800 };
      assert.deepEqual(clientToCanvas({ x: origin.left + (target.x - world.x) * zoom, y: origin.top + (target.y - world.y) * zoom }, origin, zoom, world), target);
      assert.deepEqual(clientToCanvas({ x: origin.left + 200 * zoom, y: origin.top + 300 * zoom }, origin, zoom), { x: 200, y: 300 });
    });
  }
  await test('drawing backwards crosses the screen guide origin into negative workspace coordinates', () => {
    const gesture = { kind: 'new', start: { x: 300, y: 200 }, initial: { x: 300, y: 200, width: 0, height: 0 } };
    assert.deepEqual(geometryForGesture(gesture, { x: -90, y: -50 }, guide, free), { x: -90, y: -50, width: 390, height: 250 });
  });
  await test('movement crosses every side of the screen guide and keeps its size', () => {
    const initial = { x: 97, y: 65, width: 195, height: 163 };
    const gesture = { kind: 'move', start: { x: 120, y: 80 }, initial };
    assert.deepEqual(geometryForGesture(gesture, { x: 1700, y: 1080 }, guide, grid), { x: 1680, y: 1064, width: 195, height: 163 });
    assert.deepEqual(geometryForGesture(gesture, { x: -200, y: -200 }, guide, grid), { x: -224, y: -216, width: 195, height: 163 });
  });
  await test('grid resizing snaps the far edges in world coordinates, preserving an off-grid origin', () => {
    const initial = { x: -25, y: 13, width: 190, height: 117 };
    const gesture = { kind: 'resize', start: { x: 165, y: 130 }, initial };
    const rect = geometryForGesture(gesture, { x: 204, y: 173 }, guide, grid);
    assert.deepEqual(rect, { x: -25, y: 13, width: 233, height: 163 });
    assert.equal((rect.x + rect.width) % 8, 0);
    assert.equal((rect.y + rect.height) % 8, 0);
  });
  await test('percentage snapping uses each screen dimension independently', () => {
    const settings = { ...grid, spacing: 5, unit: 'percent' };
    assert.deepEqual(gridSteps(guide, settings), { x: 72, y: 45 });
    const gesture = { kind: 'new', start: { x: -80, y: -50 }, initial: { x: -80, y: -50, width: 0, height: 0 } };
    assert.deepEqual(geometryForGesture(gesture, { x: 760, y: 460 }, guide, settings), { x: -72, y: -45, width: 864, height: 495 });
    assert.deepEqual(gridSteps({ width: 390, height: 844 }, { ...settings, spacing: 2.5 }), { x: 9.75, y: 21.1 });
  });
  await test('snap can be disabled independently of grid visibility and spacing', () => {
    const initial = { x: -11, y: 17, width: 195, height: 163 };
    const gesture = { kind: 'move', start: { x: 0, y: 0 }, initial };
    assert.deepEqual(geometryForGesture(gesture, { x: 2.75, y: -1.25 }, guide, { ...free, spacing: 23 }), { ...initial, x: -8.25, y: 15.75 });
  });
  await test('captured pointer deltas retain the grab point across the origin', () => {
    const initial = { x: 80, y: 60, width: 100, height: 100 };
    const gesture = { kind: 'move', start: { x: 100, y: 80 }, initial };
    assert.deepEqual(geometryForGesture(gesture, { x: -40, y: 80 }, guide, free), { ...initial, x: -60 });
    assert.deepEqual(geometryForGesture(gesture, { x: 25, y: 80 }, guide, free), { ...initial, x: 5 });
  });
  await test('the technical workspace limits clamp both rectangle endpoints', () => {
    const initial = { x: 99900, y: -99900, width: 100, height: 80 };
    assert.deepEqual(geometryForGesture({ kind: 'move', start: { x: 0, y: 0 }, initial }, { x: 500, y: -500 }, guide, grid), { ...initial, x: 99900, y: -100000 });
    const lowerRight = { x: 99980, y: 99980, width: 17, height: 17 };
    assert.deepEqual(geometryForGesture({ kind: 'resize', start: { x: 0, y: 0 }, initial: lowerRight }, { x: 500, y: 500 }, guide, grid), { ...lowerRight, width: 20, height: 20 });
    assert.deepEqual(geometryForGesture({ kind: 'new', start: { x: 0, y: 0 }, initial }, { x: -200000, y: 200000 }, guide, free), { x: -100000, y: 0, width: 100000, height: 100000 });
  });
  await test('queued keyboard changes accumulate against the accepted rectangle', async () => {
    let accepted = { x: -10, y: -20, width: 100, height: 80 };
    const changes = [rectDelta({ x: 1, y: 0 }, guide), rectDelta({ x: 1, y: 0 }, guide), rectDelta({ x: 8, y: 0 }, guide), rectDelta({ x: 0, y: 1 }, guide, true)];
    let queue = Promise.resolve();
    for (const change of changes) queue = queue.then(async () => { await Promise.resolve(); accepted = change(accepted); });
    await queue;
    assert.deepEqual(accepted, { x: 0, y: -20, width: 100, height: 81 });
  });
  await test('keyboard placement advances to the next grid line even from a negative off-grid origin', () => {
    const initial = { x: -3, y: -13, width: 100, height: 80 };
    const minimum = { x: 1, y: 1 };
    assert.deepEqual(keyboardRectChange({ x: 1, y: 1 }, guide, grid, false, false, minimum)(initial), { ...initial, x: 0, y: -8 });
    assert.deepEqual(keyboardRectChange({ x: -1, y: -1 }, guide, grid, false, false, minimum)(initial), { ...initial, x: -8, y: -16 });
    assert.deepEqual(keyboardRectChange({ x: 1, y: 1 }, guide, grid, false, true, minimum)(initial), { ...initial, x: -2, y: -12 });
    assert.deepEqual(keyboardRectChange({ x: 1, y: 1 }, guide, free, false, false, minimum)(initial), { ...initial, x: -2, y: -12 });
  });
  await test('percentage keyboard resizing follows far-edge spacing on the correct axis', () => {
    const initial = { x: -3, y: -13, width: 100, height: 80 };
    const settings = { ...grid, spacing: 5, unit: 'percent' };
    assert.deepEqual(keyboardRectChange({ x: 1, y: 1 }, guide, settings, true, false, { x: 1, y: 1 })(initial), { ...initial, width: 147, height: 103 });
  });
  await test('bounded keyboard changes retain object identity for a no-op', () => {
    const accepted = { x: 99900, y: 99920, width: 100, height: 80 };
    assert.equal(rectDelta({ x: 8, y: 8 }, guide)(accepted), accepted);
    assert.equal(rectDelta({ x: 8, y: 8 }, guide, true)(accepted), accepted);
  });
  await test('all gestures stay in the signed technical world for narrow and non-grid screen guides', () => {
    for (const width of [1, 7, 390, 1440]) {
      const screen = { width, height: 844 };
      const initial = { x: -99990, y: 99990, width: 10, height: 10 };
      for (const settings of [free, grid, { ...grid, spacing: 2.5, unit: 'percent' }]) {
        for (const kind of ['new', 'move', 'resize']) {
          for (const point of [{ x: -200000, y: -200000 }, { x: -3, y: 7 }, { x: 200000, y: 200000 }]) {
            const rect = geometryForGesture({ kind, start: { x: -99990, y: 99990 }, initial }, point, screen, settings);
            assert.ok(rect.x >= WORLD_MIN && rect.y >= WORLD_MIN);
            assert.ok(rect.width >= 0 && rect.height >= 0);
            assert.ok(rect.x + rect.width <= WORLD_MAX && rect.y + rect.height <= WORLD_MAX);
          }
        }
      }
    }
  });
  await test('request text boxes stay readable at every zoom without being constrained to mobile screen width', () => {
    for (const zoom of [0.25, 0.5, 0.75, 1, 1.5, 2]) {
      const box = requestAt({ x: -64, y: -64 }, zoom);
      assert.ok(Math.abs(box.width * zoom - 320) < 0.001);
      assert.ok(Math.abs(box.height * zoom - 160) < 0.001);
      assert.equal(box.x, -64);
      assert.equal(box.y, -64);
    }
  });
  await test('tiny requests gain usable dimensions and clamp gracefully at the technical world edge', () => {
    assert.deepEqual(requestPlacement({ x: 99999, y: 99999, width: 1, height: 1 }, 1), { x: 99840, y: 99920, width: 160, height: 80 });
    assert.deepEqual(requestAt({ x: -100001, y: -100001 }, 0.25), { x: -100000, y: -100000, width: 1280, height: 640 });
  });
  await test('pointer and keyboard resizing preserve the readable minimum', () => {
    const initial = { x: -16, y: -32, width: 320, height: 160 };
    const minimum = { x: 160, y: 80 };
    const gesture = { kind: 'resize', start: { x: 304, y: 128 }, initial };
    assert.deepEqual(geometryForGesture(gesture, { x: -500, y: -500 }, guide, free, minimum), { x: -16, y: -32, width: 160, height: 80 });
    const smallest = { ...initial, width: 160, height: 80 };
    assert.equal(rectDelta({ x: -8, y: -8 }, guide, true, minimum)(smallest), smallest);
  });
  await test('initial workspace surrounds the screen guide and every off-screen request', () => {
    const rects = [{ x: -3000, y: -2000, width: 100, height: 100 }, { x: 4000, y: 5000, width: 200, height: 300 }];
    assert.deepEqual(initialWorkspaceBounds(guide, rects, { x: 1200, y: 800 }), { left: -4200, top: -2800, right: 5400, bottom: 6100 });
  });
  await test('scrolling near each edge dynamically extends workspace without shifting world coordinates', () => {
    const bounds = { left: -600, top: -600, right: 2040, bottom: 1500 };
    const visible = { left: -580, top: -550, right: 2000, bottom: 1450 };
    const extended = expandWorkspace(bounds, visible, { x: 1200, y: 800 });
    assert.deepEqual(extended, { left: -1780, top: -1350, right: 3200, bottom: 2250 });
    for (const zoom of [0.25, 0.75, 1, 2]) {
      const oldScroll = (visible.left - bounds.left) * zoom;
      const newScroll = (visible.left - extended.left) * zoom;
      const target = -300;
      assert.equal((target - bounds.left) * zoom - oldScroll, (target - extended.left) * zoom - newScroll);
    }
  });
  await test('workspace expansion stops at technical limits and does not shrink existing working space', () => {
    const bounds = { left: -99900, top: -99900, right: 99900, bottom: 99900 };
    assert.deepEqual(expandWorkspace(bounds, { left: -99900, top: -99900, right: 99900, bottom: 99900 }, { x: 1200, y: 800 }), { left: -100000, top: -100000, right: 100000, bottom: 100000 });
    assert.deepEqual(expandWorkspace(bounds, { left: -500, top: -500, right: 500, bottom: 500 }, { x: 1200, y: 800 }), bounds);
  });
  await test('alignment guides use the screen and other requests independently of grid snapping', () => {
    const other = { x: -100, y: -200, width: 100, height: 200 };
    assert.deepEqual(alignmentLines({ x: -100, y: -200, width: 100, height: 200 }, guide, [other], 1), { x: [0, -100, -50], y: [0, -200, -100] });
    assert.deepEqual(alignmentLines({ x: 720, y: 450, width: 720, height: 450 }, guide, [], 1), { x: [720, 1440], y: [450, 900] });
    assert.deepEqual(alignmentLines({ x: 721, y: 451, width: 719, height: 449 }, guide, [], 1), { x: [1440], y: [900] });
    assert.deepEqual(alignmentLines({ x: 721, y: 451, width: 719, height: 449 }, guide, [], 0.5), { x: [720, 1440], y: [450, 900] });
    assert.deepEqual(alignmentLines(undefined, guide, [other], 1), { x: [], y: [] });
  });
  await test('ruler percentages stay relative to the screen guide beyond either edge', () => {
    assert.equal(percent(-720, 1440), '−50%');
    assert.equal(percent(2880, 1440), '200%');
    assert.equal(percent(112.5, 900), '12.5%');
    assert.equal(percent(-0.001, 1440), '0%');
    for (const zoom of [0.25, 0.5, 1, 2]) {
      const ticks = percentageTicks(-2000, 1200, 1440, zoom);
      assert.ok(ticks.length > 0 && ticks.length < 20);
      assert.ok(ticks.some(tick => tick.value < 0));
      for (const tick of ticks) {
        assert.ok(tick.position >= 0 && tick.position <= 1200);
        assert.equal(tick.position, (tick.value + 2000) * zoom);
      }
    }
  });
} finally {
  await rm(temporary, { recursive: true, force: true });
}
