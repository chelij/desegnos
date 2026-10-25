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
    '--target', 'ES2022', '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/canvasGeometry.ts', 'src/shortcuts.ts', 'src/layerOrder.ts', 'src/wholePageScope.ts'], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout || String(result.error));
  const { pointInEllipse, pointInPolygon, pointInRegion, absolutePoints, relativePoints, simplifyFreehand, freehandArea, resizedElementSize, elementResizeRect, clientToCanvas, geometryForGesture, gridSteps, keyboardRectChange, rectDelta, requestAt, requestPlacement,
    workspaceWindow, expandWorkspace, percent, percentageTicks, pixelTicks, selectionLabelPosition, elementToolbarPosition, attachedRequestRect, alignmentLines, intersectsWorkspace, clipToWorkspace, validWorkspace, wheelZoom, zoomOrigin, steppedZoom, zoomLabel } = await import(pathToFileURL(join(temporary, 'canvasGeometry.js')).href);
  const { editorShortcut } = await import(pathToFileURL(join(temporary, 'shortcuts.js')).href);
  const { reorderedLayers } = await import(pathToFileURL(join(temporary, 'layerOrder.js')).href);
  const { isWholePage } = await import(pathToFileURL(join(temporary, 'wholePageScope.js')).href);
  await test('whole-page badges follow base layers, tie order, layout intent and opt-out', () => {
    const base = { id: 'base', zIndex: 0, prompt: 'make it greyscale, not green', rect: { x: 0, y: -8, width: 1440, height: 912 } };
    const tail = { id: 'tail', zIndex: 1, prompt: 'too long tail, I want one page one screen only', rect: { x: 0, y: 964, width: 1440, height: 140 } };
    const image = { id: 'image', zIndex: 2, prompt: 'replace with transparent background version', elements: [{ id: 'image001' }], rect: { x: 60, y: 100, width: 200, height: 200 } };
    const page = { viewport: { width: 1440, height: 900 }, regions: [base, tail, image] };
    assert.equal(isWholePage(base, page), true);
    assert.equal(isWholePage(tail, page), true);
    assert.equal(isWholePage(image, page), false);
    base.zIndex = 3;
    assert.equal(isWholePage(base, page), false);
    assert.equal(isWholePage({ ...base, wholePage: true }, page), true);
    assert.equal(isWholePage({ ...tail, wholePageDisabled: true }, page), false);
    assert.equal(isWholePage({ ...tail, rect: { ...tail.rect, width: 300 } }, page), false);
    assert.equal(isWholePage({ ...tail, prompt: 'Add a footer note' }, page), false);
    base.zIndex = 0; tail.zIndex = 0;
    assert.equal(isWholePage(base, page), true);
    page.regions = [tail, base, image];
    assert.equal(isWholePage(base, page), false);
    tail.completed = true;
    assert.equal(isWholePage(base, page), true);
  });
  await test('canvas tool shortcuts never interrupt text input and AI stays explicit', () => {
    const event = key => ({ key, ctrlKey: false, metaKey: false, altKey: false, shiftKey: false });
    for (const [key, action] of [['v','selectTool'], ['r','requestTool'], ['h','panTool'], ['c','circleTool'], ['l','freehandTool']]) {
      assert.equal(editorShortcut(event(key), false), action);
      assert.equal(editorShortcut(event(key), true), null);
      assert.equal(editorShortcut({ ...event(key), isComposing: true }, false), null);
    }
    assert.equal(editorShortcut({ ...event('k'), ctrlKey: true }, false), 'askAI');
    assert.equal(editorShortcut({ ...event('k'), metaKey: true }, false), 'askAI');
    assert.equal(editorShortcut({ ...event('k'), ctrlKey: true }, true), null);
  });
  await test('shape shortcuts respect modifiers, composition and request-tool memory', () => {
    const event = { key: 'R', ctrlKey: false, metaKey: false, shiftKey: true, altKey: false };
    assert.equal(editorShortcut(event, false), 'rectangleTool');
    assert.equal(editorShortcut(event, true), null);
    for (const key of ['c', 'l', 'r']) {
      for (const extra of [{ altKey: true }, { isComposing: true }, { defaultPrevented: true }]) assert.equal(editorShortcut({ ...event, key, shiftKey: false, ...extra }, false), null);
    }
    assert.equal(editorShortcut({ ...event, key:'c', shiftKey:false, ctrlKey:true }, false), 'copyLayer');
    assert.equal(editorShortcut({ ...event, key:'l', shiftKey:false, ctrlKey:true }, false), null);
  });
  await test('ellipse hit-testing uses radii, includes edges and excludes bounding corners', () => {
    const rect = { x: -70, y: 20, width: 200, height: 80 };
    for (const point of [{x:30,y:60},{x:-70,y:60},{x:130,y:60},{x:30,y:20}]) assert.equal(pointInEllipse(point,rect),true,JSON.stringify(point));
    for (const point of [{x:-70,y:20},{x:130,y:100},{x:30,y:19}]) assert.equal(pointInEllipse(point,rect),false,JSON.stringify(point));
    assert.equal(pointInEllipse({x:0,y:0},{x:0,y:0,width:0,height:0}),false);
  });
  await test('polygon hit-testing handles concavity, closure, edges and even-odd crossings', () => {
    const polygon=[{x:0,y:0},{x:100,y:0},{x:100,y:30},{x:30,y:30},{x:30,y:100},{x:0,y:100}];
    for(const point of [{x:10,y:10},{x:0,y:50},{x:30,y:30},{x:10,y:90}]) assert.equal(pointInPolygon(point,polygon),true);
    for(const point of [{x:90,y:90},{x:-1,y:50}]) assert.equal(pointInPolygon(point,polygon),false);
    assert.equal(pointInPolygon({x:10,y:10},[...polygon].reverse()),true);
    const bow=[{x:0,y:0},{x:100,y:100},{x:0,y:100},{x:100,y:0}];
    assert.equal(pointInPolygon({x:50,y:10},bow),true);
    assert.equal(pointInPolygon({x:10,y:50},bow),false);
  });
  await test('shape hit-testing defaults old regions to rectangles', () => {
    const rect={x:0,y:0,width:100,height:100};
    assert.equal(pointInRegion({x:0,y:0},rect),true);
    assert.equal(pointInRegion({x:0,y:0},rect,{kind:'ellipse'}),false);
    assert.equal(pointInRegion({x:90,y:90},rect,{kind:'freehand',points:[[0,0],[1,0],[0,1]]}),false);
    assert.equal(pointInRegion({x:10,y:10},rect,{kind:'freehand',points:[[0,0],[1,0],[0,1]]}),true);
  });
  await test('closed-loop simplification removes duplicate samples and straight-line noise', () => {
    const stroke=[...Array.from({length:100},(_,i)=>({x:i,y:0})),{x:100,y:0},{x:100,y:100},{x:0,y:100},{x:0,y:0},{x:0,y:0}];
    const before=structuredClone(stroke), result=simplifyFreehand(stroke,1);
    assert.deepEqual(result,[{x:0,y:0},{x:100,y:0},{x:100,y:100},{x:0,y:100}]);
    assert.deepEqual(stroke,before);
  });
  await test('lasso simplification caps noisy strokes at 200 points and retains small loops', () => {
    const stroke=Array.from({length:8000},(_,i)=>{const t=i/8000*Math.PI*2,r=100+(i%2?4:-4);return {x:r*Math.cos(t),y:r*Math.sin(t)};});
    const result=simplifyFreehand(stroke,.01);
    assert.ok(result.length>=3 && result.length<=200,result.length);
    assert.equal(pointInPolygon({x:0,y:0},result),true);
    const triangle=freehandArea([{x:0,y:0},{x:2,y:0},{x:1,y:2},{x:0,y:0}],100);
    assert.ok(triangle && triangle.shape.points.length===3);
  });
  await test('freehand areas reject clicks, lines and nonfinite samples, and close on release', () => {
    for (const points of [[],[{x:1,y:1}],[{x:0,y:0},{x:20,y:20},{x:40,y:40}],[{x:0,y:0},{x:NaN,y:10},{x:20,y:20}]]) assert.equal(freehandArea(points),null);
    const area=freehandArea([{x:-50,y:20},{x:100,y:20},{x:25,y:120}],0);
    assert.deepEqual(area,{rect:{x:-50,y:20,width:150,height:100},shape:{kind:'freehand',points:[[0,0],[1,0],[.5,1]]}});
    assert.equal(pointInRegion({x:25,y:50},area.rect,area.shape),true);
  });
  await test('relative lasso points survive movement and nonuniform resizing', () => {
    const guide={width:1200,height:800}, grid={spacing:8,unit:'pixels',snap:false,rulers:true};
    const points=[[0,0],[1,0],[.5,1]], original={x:-50,y:20,width:150,height:100};
    const absolute=absolutePoints(points,original);
    assert.deepEqual(relativePoints(absolute,original),points);
    const moved=geometryForGesture({kind:'move',start:{x:0,y:0},initial:original},{x:20,y:-40},guide,{...grid,snap:false});
    assert.deepEqual(absolutePoints(points,moved),absolute.map(p=>({x:p.x+20,y:p.y-40})));
    const resized=geometryForGesture({kind:'resize',start:{x:0,y:0},initial:original},{x:150,y:-50},guide,{...grid,snap:false});
    assert.deepEqual(absolutePoints(points,resized),[{x:-50,y:20},{x:250,y:20},{x:100,y:70}]);
    assert.deepEqual(points,[[0,0],[1,0],[.5,1]]);
  });
  await test('Shift produces a perfect circle in every drag direction, including percent-grid snapping', () => {
    const guide={width:1200,height:800}, grid={spacing:8,unit:'pixels',snap:false,rulers:true};
    const start={x:40,y:40};
    for (const point of [{x:140,y:100},{x:-60,y:100},{x:140,y:-20},{x:-60,y:-20}]) {
      const circle=geometryForGesture({kind:'new',start,initial:{...start,width:0,height:0},proportional:true},point,guide,{...grid,snap:false});
      assert.equal(circle.width,100); assert.equal(circle.height,100);
      assert.equal(point.x<start.x ? circle.x+circle.width : circle.x,start.x);
      assert.equal(point.y<start.y ? circle.y+circle.height : circle.y,start.y);
    }
    const circle=geometryForGesture({kind:'new',start,initial:{...start,width:0,height:0},proportional:true},{x:147,y:98},guide,{spacing:5,unit:'percent',snap:true,rulers:true});
    assert.equal(circle.width,circle.height);
    const ellipse=geometryForGesture({kind:'new',start,initial:{...start,width:0,height:0}},{x:140,y:100},guide,{...grid,snap:false});
    assert.equal(ellipse.width,100); assert.equal(ellipse.height,60);
  });
  await test('layer moves change one step or an end without mutating input, and ends are true no-ops', () => {
    const input=['a','b','c'];
    assert.deepEqual(reorderedLayers(input,'a','forward'),['b','a','c']);
    assert.deepEqual(reorderedLayers(input,'c','backward'),['a','c','b']);
    assert.deepEqual(reorderedLayers(input,'a','front'),['b','c','a']);
    assert.deepEqual(reorderedLayers(input,'c','back'),['c','a','b']);
    for(const direction of ['front','forward'])assert.equal(reorderedLayers(input,'c',direction),null);
    for(const direction of ['back','backward'])assert.equal(reorderedLayers(input,'a',direction),null);
    assert.deepEqual(input,['a','b','c']);
  });
  await test('layer clipboard and stacking shortcuts preserve all text editing and composition', () => {
    const input={key:'c',ctrlKey:true,metaKey:false,shiftKey:false,altKey:false};
    for(const [key,action] of [['c','copyLayer'],['x','cutLayer'],['v','pasteLayer'],[']','layerForward'],['[','layerBackward']]) {
      assert.equal(editorShortcut({...input,key},false),action); assert.equal(editorShortcut({...input,key},true),null);
      assert.equal(editorShortcut({...input,key,isComposing:true},false),null);
      assert.equal(editorShortcut({...input,key,defaultPrevented:true},false),null);
    }
    assert.equal(editorShortcut({...input,key:'}',code:'BracketRight',shiftKey:true},false),'layerFront');
    assert.equal(editorShortcut({...input,key:'{',code:'BracketLeft',shiftKey:true},false),'layerBack');
    assert.equal(editorShortcut({...input,key:'c',ctrlKey:false,metaKey:true},false),'copyLayer');
  });
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
      assert.ok(Object.values(result).every(Number.isFinite));
      assert.ok(result.width>1e8 && result.height>1e8);
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
  await test('the zoom readout uses whole percentages like other editors', () => {
    assert.equal(zoomLabel(0.892), '89%');
    assert.equal(zoomLabel(0.644), '64%');
    assert.equal(zoomLabel(1), '100%');
    assert.equal(zoomLabel(0.005), '1%');
  });
  await test('editor shortcuts preserve text undo, clipboard, composition and handled canvas keys', () => {
    const key = (name, patch = {}, editing = false) => editorShortcut({ key: name, ctrlKey: true, metaKey: false, shiftKey: false, altKey: false, ...patch }, editing);
    assert.equal(key('z'), 'undo'); assert.equal(key('z', { shiftKey: true }), 'redo'); assert.equal(key('y'), 'redo');
    for (const name of ['z', 'y', 'a', 'c', 'v', 'x', 'Delete', 'Backspace']) assert.equal(key(name, {}, true), null);
    assert.equal(key('z', { isComposing: true }), null); assert.equal(key('z', { defaultPrevented: true }), null);
    assert.equal(key('z', { altKey: true }), null); assert.equal(key('z', { ctrlKey: false, metaKey: true }), 'undo');
    assert.equal(key('z', { ctrlKey: false, altKey: true }), 'undoElement'); assert.equal(key('z', { ctrlKey: false, altKey: true }, true), null); assert.equal(key('Enter', { ctrlKey: false, altKey: true }), null);
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
  await test('drawing, moving and resizing cross the former world edge without clipping', () => {
    const initial = { x: 99900, y: -99900, width: 100, height: 80 };
    assert.deepEqual(geometryForGesture({ kind: 'move', start: { x: 0, y: 0 }, initial }, { x: 500, y: -500 }, guide, free), { ...initial, x: 100400, y: -100400 });
    const lowerRight = { x: 99980, y: 99980, width: 17, height: 17 };
    assert.deepEqual(geometryForGesture({ kind: 'resize', start: { x: 0, y: 0 }, initial: lowerRight }, { x: 500, y: 500 }, guide, free), { ...lowerRight, width: 517, height: 517 });
    assert.deepEqual(geometryForGesture({ kind: 'new', start: { x: 0, y: 0 }, initial }, { x: -2000000, y: 3000000 }, guide, free), { x: -2000000, y: 0, width: 2000000, height: 3000000 });
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
  await test('keyboard changes continue past the old edge and retain identity only for a real no-op', () => {
    const accepted = { x: 99900, y: 99920, width: 100, height: 80 };
    assert.deepEqual(rectDelta({ x: 8, y: 8 }, guide)(accepted), { ...accepted, x: 99908, y: 99928 });
    assert.deepEqual(rectDelta({ x: 8, y: 8 }, guide, true)(accepted), { ...accepted, width: 108, height: 88 });
    assert.equal(rectDelta({ x: 0, y: 0 }, guide)(accepted), accepted);
  });
  await test('selection labels stay above the area, flip below and clamp to the visible viewport', () => {
    const viewport = { x: 28, y: 28, width: 800, height: 600 }, size = { x: 245, y: 28 };
    const area = { x: 200, y: 180, width: 336, height: 440 };
    assert.deepEqual(selectionLabelPosition(area, size, viewport), { x: 200, y: 124 });
    assert.deepEqual(selectionLabelPosition({ ...area, y: 40, height: 100 }, size, viewport), { x: 200, y: 168 });
    assert.deepEqual(selectionLabelPosition({ ...area, x: -100 }, size, viewport), { x: 36, y: 124 });
    assert.deepEqual(selectionLabelPosition({ ...area, x: 790 }, size, viewport), { x: 575, y: 124 });
    // Vertical clamping is safe only when the label remains outside the area.
    assert.deepEqual(selectionLabelPosition({ ...area, y: 900 }, size, viewport), { x: 200, y: 592 });
    assert.equal(selectionLabelPosition({ ...area, y: 0, height: 900 }, size, viewport), null);
    assert.equal(selectionLabelPosition(area, size, { ...viewport, width: 200 }), null);
  });
  await test('selection labels never overlap detached request text, menus or handles', () => {
    const viewport = { x: 0, y: 0, width: 800, height: 600 }, size = { x: 280, y: 30 };
    const area = { x: 200, y: 180, width: 100, height: 100 };
    const menu = { x: 200, y: 130, width: 300, height: 42 };
    const prompt = { x: 200, y: 290, width: 320, height: 200 };
    assert.deepEqual(selectionLabelPosition(area, size, viewport, [menu]), { x: 200, y: 308 });
    assert.equal(selectionLabelPosition(area, size, viewport, [menu, prompt]), null);
    const handles = [{ x: 300, y: 156, width: 24, height: 24 }, { x: 300, y: 280, width: 24, height: 24 }];
    assert.deepEqual(selectionLabelPosition(area, size, viewport, handles), { x: 200, y: 122 });
    // A short area must not allow a clamped label back inside its text box.
    assert.equal(selectionLabelPosition({ x: 0, y: 0, width: 336, height: 600 }, size, viewport), null);
  });
  await test('attached request cards open below short elements and stay inside the visible canvas for tall ones', () => {
    const viewport = { x: 0, y: 0, width: 1000, height: 700 };
    assert.deepEqual(attachedRequestRect({ x: 200, y: 100, width: 300, height: 100 }, 1, viewport), { x: 200, y: 210, width: 300, height: 112 });
    // A tall element pushes the below-element spot past the bottom edge; the card opens beside it, level with its visible top.
    assert.deepEqual(attachedRequestRect({ x: 200, y: 100, width: 300, height: 2000 }, 1, viewport), { x: 510, y: 100, width: 300, height: 112 });
    assert.deepEqual(attachedRequestRect({ x: 200, y: -400, width: 300, height: 2000 }, 1, viewport, 300), { x: 510, y: 0, width: 300, height: 300 });
    assert.deepEqual(attachedRequestRect({ x: 600, y: 100, width: 390, height: 2000 }, 1, viewport), { x: 200, y: 100, width: 390, height: 112 });
    // No room on either side: the card sits on the element, inside the view.
    assert.deepEqual(attachedRequestRect({ x: 0, y: 100, width: 1000, height: 2000 }, 1, viewport), { x: 0, y: 110, width: 420, height: 112 });
    // Off-screen elements and narrow elements clamp both axes into view without changing the area.
    assert.deepEqual(attachedRequestRect({ x: 900, y: -500, width: 50, height: 50 }, 1, viewport), { x: 740, y: 0, width: 260, height: 112 });
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
  await test('tiny requests gain usable dimensions without moving their requested origin', () => {
    assert.deepEqual(requestPlacement({ x: 99999, y: 99999, width: 1, height: 1 }, 1), { x: 99999, y: 99999, width: 160, height: 80 });
    assert.deepEqual(requestAt({ x: -100001, y: -100001 }, 0.25), { x: -100001, y: -100001, width: 1280, height: 640 });
  });
  await test('request editor minima remain readable when an existing area shrinks at small zoom', () => {
    const area = { x: -20, y: 40, width: 320, height: 160 };
    for (const zoom of [0.001, 0.1, 0.25, 0.5, 1, 2, 4]) {
      const editor = requestPlacement(area, zoom);
      assert.ok(editor.width * zoom >= 160);
      assert.ok(editor.height * zoom >= 80);
      assert.equal(editor.x, area.x);
      assert.equal(editor.y, area.y);
      assert.ok(editor.width >= area.width && editor.height >= area.height);
    }
    assert.deepEqual(area, { x: -20, y: 40, width: 320, height: 160 });
  });
  await test('pointer and keyboard resizing preserve the readable minimum', () => {
    const initial = { x: -16, y: -32, width: 320, height: 160 };
    const minimum = { x: 160, y: 80 };
    const gesture = { kind: 'resize', start: { x: 304, y: 128 }, initial };
    assert.deepEqual(geometryForGesture(gesture, { x: -500, y: -500 }, guide, free, minimum), { x: -16, y: -32, width: 160, height: 80 });
    const smallest = { ...initial, width: 160, height: 80 };
    assert.equal(rectDelta({ x: -8, y: -8 }, guide, true, minimum)(smallest), smallest);
  });
  await test('the browser window stays small at distant signed canvas positions and every zoom', () => {
    for (const zoom of [.001,.25,.75,1,2,4]) for (const origin of [{x:0,y:0},{x:-1e9,y:2e9}]) {
      const size={x:1200/zoom,y:800/zoom}, bounds=workspaceWindow(origin,size);
      assert.ok((bounds.right-bounds.left)*zoom <= Math.max(3600,1200+1200*zoom)+.001);
      assert.ok((bounds.bottom-bounds.top)*zoom <= Math.max(2400,800+1200*zoom)+.001);
      assert.ok(bounds.left<origin.x && bounds.top<origin.y && bounds.right>origin.x+size.x && bounds.bottom>origin.y+size.y);
    }
  });
  await test('repeated window rebasing keeps a captured world point at its screen position', () => {
    for (const zoom of [.001,.25,.75,1,2,4]) {
      const size={x:1200/zoom,y:800/zoom}; let bounds=workspaceWindow({x:0,y:0},size);
      for(let index=0;index<30;index++) {
        const origin={x:bounds.left+12/zoom,y:bounds.bottom-size.y-12/zoom},visible={left:origin.x,top:origin.y,right:origin.x+size.x,bottom:origin.y+size.y};
        const next=expandWorkspace(bounds,visible,{x:Math.max(600,size.x),y:Math.max(600,size.y)}),target={x:origin.x+100/zoom,y:origin.y+200/zoom};
        const projected=b=>({x:(target.x-b.left)*zoom-(origin.x-b.left)*zoom,y:(target.y-b.top)*zoom-(origin.y-b.top)*zoom});
        const before=projected(bounds),after=projected(next);
        assert.ok(Math.abs(before.x-after.x)<1e-6 && Math.abs(before.y-after.y)<1e-6);
        assert.ok((next.right-next.left)*zoom<7000 && (next.bottom-next.top)*zoom<7000);
        bounds=next;
      }
    }
  });
  await test('off-window content is hidden without dropping huge intersecting requests', () => {
    const bounds=workspaceWindow({x:0,y:0},{x:1200,y:800});
    assert.equal(intersectsWorkspace({x:1e9,y:1e9,width:100,height:100},bounds),false);
    assert.equal(intersectsWorkspace({x:-1e6,y:-1e6,width:2e6,height:2e6},bounds),true);
    assert.equal(expandWorkspace(bounds,{left:0,top:0,right:1200,bottom:800},{x:1200,y:800}),bounds);
  });
  await test('enormous intersecting rectangles paint only the bounded window without changing logical geometry', () => {
    const bounds=workspaceWindow({x:0,y:0},{x:1200,y:800}),rect={x:-1e9,y:-1e9,width:2e9,height:2e9};
    assert.deepEqual(clipToWorkspace(rect,bounds),{x:bounds.left,y:bounds.top,width:bounds.right-bounds.left,height:bounds.bottom-bounds.top});
    assert.equal(rect.width,2e9);
    assert.equal(clipToWorkspace({x:1e10,y:1e10,width:10,height:10},bounds),null);
    assert.equal(validWorkspace(bounds,{x:1200,y:800}),true);
    assert.equal(validWorkspace(workspaceWindow({x:1e308,y:1e308},{x:1200,y:800}),{x:1200,y:800}),false);
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
  await test('pixel rulers stay sparse, signed and aligned at every supported zoom', () => {
    for (const zoom of [0.001, 0.1, 0.25, 0.75, 1, 2, 4]) {
      const ticks = pixelTicks(-2000, 1200, zoom);
      assert.ok(ticks.length > 0 && ticks.length < 20);
      for (const tick of ticks) {
        assert.ok(tick.position >= 0 && tick.position <= 1200);
        assert.equal(tick.position, (tick.value + 2000) * zoom);
      }
      if (zoom >= 0.25) assert.ok(ticks.some(tick => tick.value < 0));
    }
  });
  await test('element toolbar flips below nearby content and stays inside the visible canvas', () => {
    const selection = { x: 200, y: 180, width: 300, height: 100 }, size = { x: 240, y: 42 }, viewport = { x: 28, y: 28, width: 800, height: 600 };
    assert.deepEqual(elementToolbarPosition(selection, size, viewport), { x: 200, y: 130 });
    assert.deepEqual(elementToolbarPosition(selection, size, viewport, [{ x: 180, y: 125, width: 340, height: 45 }]), { x: 200, y: 288 });
    // Busy surroundings never send the menu to a far corner while a spot beside the selection is free of it.
    const crowded = elementToolbarPosition({ x: 300, y: 500, width: 130, height: 37 }, size, viewport, [{ x: 0, y: 0, width: 2000, height: 2000 }]);
    assert.ok(Math.abs(crowded.y - 500) < 120 && Math.abs(crowded.x - 300) < 400, JSON.stringify(crowded));
    const nearTop = elementToolbarPosition({ ...selection, y: 35 }, size, viewport);
    assert.deepEqual(nearTop, { x: 200, y: 143 });
    for (const rect of [{ x: -50, y: -30, width: 10, height: 10 }, { x: 820, y: 620, width: 40, height: 40 }]) {
      const position = elementToolbarPosition(rect, size, viewport);
      assert.ok(position.x >= viewport.x + 8 && position.x + size.x <= viewport.x + viewport.width - 8);
      assert.ok(position.y >= viewport.y + 8 && position.y + size.y <= viewport.y + viewport.height - 8);
    }
  });
} finally {
  await rm(temporary, { recursive: true, force: true });
}
