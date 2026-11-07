import assert from 'node:assert/strict';
import test from 'node:test';
import { compileModules } from './typescript.mjs';

const modules = await compileModules(['src/projectCanvasGeometry.ts']);
try {
  const geometry = await modules.load('projectCanvasGeometry.js');
  const {
    boundaryHeight, layoutBoundaries, drawnBoundaryRect, snapValue, canvasRectFromPage, pageRectFromCanvas,
    pushApartBoundaries, boundaryMoves, boundariesByPosition, dotGrid, unionBounds, sameBoundaries,
    fitCamera, textEditingCamera, zoomCameraAt, panCamera, drawnFrameRect, referenceNoteRect, movedReferenceRect, resizedReferenceRect,
    convertedReferenceRects, referenceFrameRect, pushApartReference, referenceMoves, insideRect, boundaryAt, boundaryNear,
    insideAnyBoundary, canvasTargetKey, selectedPointers, selectedReference, selectedElementIds,
    selectedElementTargets, hasCanvasTarget, toggleCanvasTarget, sizeTarget, nearestEdgeAnchors, pointerEndpointKey,
    samePointerEndpoint, pointerEndpointRect, pointerRoute, pointerAt, pointSegmentDistance, pointerVisible,
    viewportWorldRect, screenPoint, pointerBroken, pointerLabel, pointerEndpointName, automaticLinks,
    BOUNDARY_GAP, BOUNDARY_PUSH_GAP, GRID_SPACING, MIN_SIZE, MAX_BOUNDARY_HEIGHT, FIT_PADDING, CANVAS_DOT_MIN_SCREEN, REFERENCE_ATTACHMENT_WIDTH, REFERENCE_ATTACHMENT_HEIGHT, REFERENCE_ATTACHMENT_GAP,
    POINTER_HIT_PX,
  } = geometry;
  const { MIN_ZOOM, ZOOM_LEVELS } = await modules.load('projectCanvasGeometry.js');

  const size = (id, x, y, width, height, kind = 'page') => ({ id, label: id, x, y, width, height, kind });
  const box = (x, y, width = 400, height = 300) => ({ x, y, width, height });
  const boundary = (sizeId, rect, pageId = `p-${sizeId}`) => ({ pageId, sizeId, label: sizeId, kind: 'page', rect });

  await test('boundary heights clamp to a sane range', () => {
    assert.equal(boundaryHeight(900), 900);
    assert.equal(boundaryHeight(0), MIN_SIZE);
    assert.equal(boundaryHeight(-40), MIN_SIZE);
    assert.equal(boundaryHeight(Number.NaN), MIN_SIZE);
    assert.equal(boundaryHeight(12), 12);
    assert.equal(boundaryHeight(20000), MAX_BOUNDARY_HEIGHT);
  });

  await test('boundaries lay out home first, one per linked size, keeping stored spots', () => {
    const laid = layoutBoundaries([
      { id: 'second', sizes: [size('second', Number.NaN, Number.NaN, 800, 0)] },
      { id: 'home', sizes: [size('home', 200, -100, 1440, 900), size('home-phone', 1800, 40, 390, 844, 'screen')] },
    ], 'home');
    assert.deepEqual(laid.map(item => item.pageId), ['home', 'home', 'second']);
    assert.deepEqual(laid[0].rect, { x: 200, y: -100, width: 1440, height: 900 });
    assert.equal(laid[0].kind, 'page');
    assert.deepEqual(laid[1].rect, { x: 1800, y: 40, width: 390, height: 844 });
    assert.equal(laid[1].kind, 'screen');
    // The default-sized second boundary follows the rightmost placed boundary.
    const right = Math.max(200 + 1440, 1800 + 390) + BOUNDARY_GAP;
    assert.deepEqual(laid[2].rect, { x: right, y: 0, width: 800, height: MIN_SIZE });
    assert.equal(laid[0].sizeId, 'home');
    assert.equal(BOUNDARY_GAP, 160);
  });

  await test('a boundary without a stored position follows the others and screens keep their exact height', () => {
    const laid = layoutBoundaries([
      { id: 'home', sizes: [size('home', 0, 0, 1000, 1200)] },
      { id: 'phone', sizes: [size('phone', Number.NaN, Number.NaN, 390, 3000, 'screen')] },
    ], 'home');
    assert.deepEqual(laid[0].rect, { x: 0, y: 0, width: 1000, height: 1200 });
    assert.deepEqual(laid[1].rect, { x: 1160, y: 0, width: 390, height: 3000 });
  });

  await test('drawn boundaries snap to 8px with no minimum size', () => {
    assert.equal(GRID_SPACING, 8);
    assert.equal(MIN_SIZE, 1);
    assert.equal(MIN_SIZE, 1);
    assert.equal(snapValue(13), 16);
    assert.equal(snapValue(Number.NaN), 0);
    assert.deepEqual(drawnBoundaryRect({ x: 3, y: 5 }, { x: 100, y: 60 }), { x: 0, y: 8, width: 104, height: 56 });
    const large = drawnBoundaryRect({ x: 0, y: 0 }, { x: 1000, y: 700 });
    assert.deepEqual(large, { x: 0, y: 0, width: 1000, height: 704 });
    assert.deepEqual(drawnBoundaryRect({ x: 1000, y: 700 }, { x: 0, y: 0 }), large);
  });

  await test('a moved boundary pushes neighbours aside and the push chains', () => {
    const before = [
      boundary('a', box(0, 0)),
      boundary('b', box(400, 0)),
      boundary('c', box(800, 0)),
    ];
    const after = pushApartBoundaries(before, 'a');
    assert.deepEqual(after.map(item => item.rect), [box(0, 0), box(440, 0), box(880, 0)]);
    assert.equal(BOUNDARY_PUSH_GAP, 40);
    assert.deepEqual(before[1].rect, box(400, 0), 'the input is untouched');
    for (let i = 0; i < after.length; i++) for (let j = i + 1; j < after.length; j++) {
      const first = after[i].rect, second = after[j].rect;
      const overlapX = Math.min(first.x + first.width, second.x + second.width) - Math.max(first.x, second.x);
      const overlapY = Math.min(first.y + first.height, second.y + second.height) - Math.max(first.y, second.y);
      assert.ok(overlapX <= -BOUNDARY_PUSH_GAP || overlapY <= -BOUNDARY_PUSH_GAP, `${after[i].sizeId} still overlaps ${after[j].sizeId}`);
    }
  });

  await test('push-apart separates along the least-overlapping axis and leaves clear boundaries alone', () => {
    const after = pushApartBoundaries([
      boundary('a', box(0, 0)),
      boundary('b', box(380, 10)),
    ], 'a');
    assert.deepEqual(after[0].rect, box(0, 0));
    assert.deepEqual(after[1].rect, box(440, 10));

    const clear = [
      boundary('a', box(0, 0)),
      boundary('b', box(1000, 0)),
    ];
    assert.deepEqual(pushApartBoundaries(clear, 'a'), clear);
    assert.deepEqual(pushApartBoundaries(clear, 'missing'), clear);
  });

  await test('only boundaries whose stored placement changed need saving', () => {
    const previous = [boundary('a', box(0, 0)), boundary('b', box(400, 0))];
    assert.deepEqual(boundaryMoves(previous, previous), []);
    const moved = [{ ...previous[0] }, { ...previous[1], rect: box(440, 0), kind: 'screen' }];
    assert.deepEqual(boundaryMoves(moved, previous), [{ pageId: 'p-b', sizeId: 'b', x: 440, y: 0, kind: 'screen' }]);
    const added = [...previous, boundary('c', box(900, 0))];
    assert.deepEqual(boundaryMoves(added, previous).map(move => move.sizeId), ['c']);
  });

  await test('layers list boundaries top-left first', () => {
    const ordered = boundariesByPosition([
      boundary('right', box(800, 0)),
      boundary('bottom', box(0, 500)),
      boundary('left', box(0, 0)),
    ]);
    assert.deepEqual(ordered.map(item => item.sizeId), ['left', 'right', 'bottom']);
  });

  await test('the dot grid keeps at least six screen pixels between dots and fades out', () => {
    assert.equal(GRID_SPACING, 8);
    assert.equal(CANVAS_DOT_MIN_SCREEN, 6);
    assert.equal(dotGrid({ x: 0, y: 0, zoom: 1 }, false), null);
    assert.equal(dotGrid({ x: 0, y: 0, zoom: 0 }, true), null);
    assert.equal(dotGrid({ x: 0, y: 0, zoom: Number.NaN }, true), null);
    const one = dotGrid({ x: 0, y: 0, zoom: 1 }, true);
    assert.equal(one.spacing, 8);
    assert.equal(one.size, 8);
    assert.equal(one.fade, 1);
    const half = dotGrid({ x: 0, y: 0, zoom: 0.5 }, true);
    assert.equal(half.spacing, 16);
    assert.equal(half.size, 8);
    const tiny = dotGrid({ x: 3, y: -5, zoom: 0.05 }, true);
    assert.ok(tiny.spacing * 0.05 >= 6);
    assert.equal(tiny.fade, 0.5);
    assert.ok(tiny.x >= 0 && tiny.x < tiny.size);
    assert.ok(tiny.y >= 0 && tiny.y < tiny.size);
  });

  await test('union covers every boundary and comparison catches real changes', () => {
    assert.deepEqual(unionBounds([]), { x: 0, y: 0, width: 0, height: 0 });
    const one = boundary('a', box(0, 0));
    const two = boundary('b', box(200, 100, 100, 50));
    assert.deepEqual(unionBounds([one.rect, two.rect]), { x: 0, y: 0, width: 400, height: 300 });
    assert.equal(sameBoundaries([one, two], [one, two]), true);
    assert.equal(sameBoundaries([one, two], [one, { ...two, label: 'B' }]), false);
    assert.equal(sameBoundaries([one], [one, two]), false);
  });

  await test('fit centers boundaries inside the padding and never magnifies past 100%', () => {
    const camera = fitCamera({ x: 100, y: 50, width: 800, height: 400 }, { width: 1000, height: 800 });
    assert.equal(camera.zoom, 1);
    assert.equal(camera.x, 100 - 100 * 1);
    assert.equal(camera.y, 200 - 50 * 1);
    const small = fitCamera({ x: 0, y: 0, width: 100, height: 100 }, { width: 1000, height: 800 });
    assert.equal(small.zoom, 1);
    assert.equal(FIT_PADDING, 48);
    const tinyViewport = fitCamera({ x: 0, y: 0, width: 100_000, height: 100_000 }, { width: 100, height: 100 });
    assert.equal(tinyViewport.zoom, MIN_ZOOM);
  });

  await test('inline editing brings small text to 16 screen pixels and keeps it inside the canvas', () => {
    const viewport = { width: 900, height: 600 }; // Canvas width already excludes the dock.
    const before = { x: 20, y: -40, zoom: 0.32 };
    for (const bounds of [box(2500, 1700, 240, 60), box(-2000, -800, 0, 24)]) {
      const camera = textEditingCamera(before, bounds, 20, viewport);
      assert.equal(camera.zoom, 0.8);
      const topLeft = screenPoint(bounds, camera);
      assert.ok(topLeft.x >= 24 && topLeft.y >= 24);
      assert.ok(topLeft.x + bounds.width * camera.zoom <= viewport.width - 24);
      assert.ok(topLeft.y + bounds.height * camera.zoom <= viewport.height - 24);
    }
    assert.equal(textEditingCamera(before, box(0, 0, 200, 30), 14, viewport).zoom, 1);
    assert.equal(textEditingCamera(before, box(0, 0, 200, 30), 8, viewport).zoom, 2);
    assert.equal(textEditingCamera(before, box(0, 0, 20, 3), 1, viewport).zoom, ZOOM_LEVELS.at(-1));
    const large = box(0, 0, 2000, 1000);
    const fitted = textEditingCamera(before, large, 20, viewport);
    assert.ok(large.width * fitted.zoom <= viewport.width - 48);
    assert.ok(large.height * fitted.zoom <= viewport.height - 48);
  });

  await test('inline editing leaves readable text and invalid font sizes alone', () => {
    const bounds = box(1000, 2000, 200, 30), viewport = { width: 900, height: 600 };
    for (const zoom of [0.6, 0.8, 1, 2]) {
      const camera = { x: 5, y: 10, zoom };
      assert.equal(textEditingCamera(camera, bounds, 20, viewport), camera);
    }
    const camera = { x: 5, y: 10, zoom: 0.32 };
    for (const size of [0, -1, Number.NaN, Infinity]) {
      assert.equal(textEditingCamera(camera, bounds, size, viewport), camera);
    }
    assert.equal(textEditingCamera(camera, bounds, 20, { width: 0, height: 0 }), camera);
  });

  await test('zoom stays anchored under the cursor and clamps to the edit range', () => {
    const camera = zoomCameraAt({ x: 10, y: 20, zoom: 1 }, { x: 110, y: 220 }, 2);
    assert.equal(camera.zoom, 2);
    assert.equal((110 - 10) * 2 + camera.x, 110);
    assert.equal((220 - 20) * 2 + camera.y, 220);
    assert.equal(zoomCameraAt(camera, { x: 0, y: 0 }, 100).zoom, ZOOM_LEVELS.at(-1));
    assert.equal(zoomCameraAt(camera, { x: 0, y: 0 }, 0).zoom, MIN_ZOOM);
    assert.equal(zoomCameraAt(camera, { x: 0, y: 0 }, Number.NaN).zoom, 2);
    assert.deepEqual(panCamera({ x: 1, y: 2, zoom: 1 }, { x: 4, y: 5 }), { x: 5, y: 7, zoom: 1 });
    assert.deepEqual(panCamera({ x: 1, y: 2, zoom: 1 }, { x: Number.NaN, y: 0 }), { x: 1, y: 2, zoom: 1 });
  });

  await test('frames convert between canvas and page coordinates through the boundary origin', () => {
    assert.deepEqual(canvasRectFromPage(box(10, 20), { x: 100, y: 200 }), box(110, 220));
    assert.deepEqual(pageRectFromCanvas(box(110, 220), { x: 100, y: 200 }), box(10, 20));
  });

  await test('a frame drag snaps to 8px and clamps to its boundary', () => {
    const area = box(100, 100, 400, 300);
    assert.equal(GRID_SPACING, 8);
    assert.equal(MIN_SIZE, 1);
    assert.deepEqual(drawnFrameRect({ x: 120, y: 120 }, { x: 320, y: 280 }, area), box(120, 120, 200, 160));
    const reversed = drawnFrameRect({ x: 320, y: 280 }, { x: 120, y: 120 }, area);
    assert.deepEqual(reversed, box(120, 120, 200, 160));
    const clamped = drawnFrameRect({ x: 120, y: 120 }, { x: 900, y: 900 }, area);
    assert.deepEqual(clamped, box(120, 120, 380, 280));
    const minimal = drawnFrameRect({ x: 120, y: 120 }, { x: 121, y: 121 }, area);
    assert.deepEqual(minimal, box(120, 120, 1, 1));
    assert.equal(drawnFrameRect({ x: 10, y: 10 }, { x: 200, y: 200 }, area), null);
    assert.equal(drawnFrameRect({ x: Number.NaN, y: 120 }, { x: 200, y: 200 }, area), null);
  });

  await test('a Frame drag outside boundaries draws a snapped note of any size', () => {
    assert.equal(MIN_SIZE, 1);
    assert.equal(MIN_SIZE, 1);
    assert.equal(MIN_SIZE, 1);
    assert.equal(MIN_SIZE, 1);
    assert.deepEqual(referenceNoteRect({ x: 2, y: 3 }, { x: 100, y: 60 }), { x: 0, y: 0, width: 104, height: 64 });
    assert.deepEqual(referenceNoteRect({ x: 400, y: 400 }, { x: 200, y: 200 }), { x: 200, y: 200, width: 200, height: 200 });
  });

  await test('reference items move and resize freely', () => {
    assert.deepEqual(movedReferenceRect(box(13, 21), { x: 5, y: 6 }), box(16, 24));
    assert.equal(MIN_SIZE, 1);
    assert.deepEqual(resizedReferenceRect(box(100, 100, 200, 150), 'se', { x: 400, y: 400 }), box(100, 100, 300, 300));
    assert.deepEqual(resizedReferenceRect(box(100, 100, 200, 150), 'nw', { x: 50, y: 50 }), box(48, 48, 252, 202));
    assert.deepEqual(resizedReferenceRect(box(100, 100, 200, 150), 'se', { x: 101, y: 101 }), box(100, 100, 4, 4));
    assert.deepEqual(resizedReferenceRect(box(100, 100, 200, 150), 'nw', { x: 399, y: 399 }), box(299, 249, 1, 1));
  });

  await test('a frame dragged out places a note and its attachment cards one row beside it', () => {
    assert.equal(REFERENCE_ATTACHMENT_WIDTH, 200);
    assert.equal(REFERENCE_ATTACHMENT_HEIGHT, 140);
    assert.equal(REFERENCE_ATTACHMENT_GAP, 16);
    const converted = convertedReferenceRects(box(10, 20, 50, 40), 2);
    assert.deepEqual(converted.note, box(10, 20, 50, 40));
    assert.deepEqual(converted.attachments, [
      box(10 + 50 + 16, 20, 200, 140),
      box(10 + 50 + 16 + 216, 20, 200, 140),
    ]);
    assert.deepEqual(convertedReferenceRects(box(0, 0), 0).attachments, []);
  });

  await test('a reference item dropped into a boundary keeps its size where it fits and clamps inside', () => {
    assert.deepEqual(referenceFrameRect(box(110, 110, 100, 60), box(100, 100, 400, 300)), box(112, 112, 100, 60));
    assert.deepEqual(referenceFrameRect(box(10, 10, 10, 10), box(100, 100, 400, 300)), box(100, 100, 10, 10));
    assert.deepEqual(referenceFrameRect(box(480, 380, 200, 200), box(100, 100, 400, 300)), box(300, 200, 200, 200));
  });

  await test('boundaries push reference items aside and items do not push each other', () => {
    const pushed = pushApartReference([
      { itemId: 'a', rect: box(200, 0, 100, 100) },
      { itemId: 'b', rect: box(0, 400, 100, 100) },
    ], [{ rect: box(0, 0, 400, 300) }]);
    assert.deepEqual(pushed[0].rect, box(200, -140, 100, 100), 'the shortest axis moves the item out');
    assert.deepEqual(pushed[1].rect, box(0, 400, 100, 100), 'the second item was already clear');
    assert.deepEqual(referenceMoves(pushed, pushed), []);
    const moved = [...pushed];
    moved[0] = { ...moved[0], rect: box(460, 0, 100, 100) };
    assert.deepEqual(referenceMoves(moved, pushed), [{ itemId: 'a', x: 460, y: 0 }]);
  });

  await test('a press just outside a boundary corner still finds that boundary', () => {
    const boundaries = [boundary('left', box(0, 0, 100, 100)), boundary('right', box(300, 0, 100, 100))];
    assert.equal(boundaryNear(boundaries, { x: -6, y: -6 }, 16).sizeId, 'left');
    assert.equal(boundaryNear(boundaries, { x: 290, y: 50 }, 16).sizeId, 'right');
    assert.equal(boundaryNear(boundaries, { x: 200, y: 50 }, 16), null);
  });

  await test('inside and outside detection reads the topmost boundary', () => {
    assert.equal(insideRect(box(0, 0, 10, 10), { x: 0, y: 0 }), true);
    assert.equal(insideRect(box(0, 0, 10, 10), { x: 11, y: 5 }), false);
    assert.equal(insideRect(box(0, 0, 10, 10), { x: Number.NaN, y: 5 }), false);
    const boundaries = [boundary('below', box(0, 0, 100, 100)), boundary('above', box(50, 50, 100, 100))];
    assert.equal(boundaryAt(boundaries, { x: 60, y: 60 }).sizeId, 'above');
    assert.equal(boundaryAt(boundaries, { x: 10, y: 10 }).sizeId, 'below');
    assert.equal(boundaryAt(boundaries, { x: 400, y: 400 }), null);
    assert.equal(insideAnyBoundary(boundaries, { x: 60, y: 60 }), true);
    assert.equal(boundaryAt([{ itemId: 'note', rect: box(0, 0, 10, 10) }], { x: 5, y: 5 }).itemId, 'note');
  });

  await test('canvas selection toggles elements, boundaries and pointers with Shift or Ctrl', () => {
    const elementTarget = { kind: 'element', pageId: 'p', elementId: 'e', sizeId: 's' };
    const allSizes = { kind: 'element', pageId: 'p', elementId: 'e', sizeId: null };
    const referenceTarget = { kind: 'reference', itemId: 'r' };
    const pointerTarget = { kind: 'pointer', pointerId: 'x' };
    assert.equal(canvasTargetKey(elementTarget), 'element:p:e:s');
    assert.equal(canvasTargetKey(allSizes), 'element:p:e:all');
    assert.equal(canvasTargetKey(referenceTarget), 'reference:r');
    assert.equal(canvasTargetKey(pointerTarget), 'pointer:x');
    assert.deepEqual(toggleCanvasTarget([elementTarget], pointerTarget, false), [pointerTarget]);
    assert.deepEqual(toggleCanvasTarget([elementTarget], pointerTarget, true), [elementTarget, pointerTarget]);
    assert.deepEqual(toggleCanvasTarget([elementTarget, pointerTarget], elementTarget, true), [pointerTarget]);
    assert.equal(hasCanvasTarget([elementTarget], { ...elementTarget }), true);
    assert.equal(hasCanvasTarget([allSizes], elementTarget), false);
    assert.deepEqual(selectedPointers([pointerTarget, referenceTarget]), ['x']);
    assert.deepEqual(selectedReference([pointerTarget, referenceTarget]), ['r']);
    assert.deepEqual(selectedElementIds([elementTarget, allSizes]), ['e', 'e']);
    assert.deepEqual(selectedElementTargets([elementTarget, referenceTarget]), [elementTarget]);
    assert.deepEqual(sizeTarget(elementTarget), { size_id: 's' });
    assert.deepEqual(sizeTarget(allSizes), { all_sizes: true });
  });

  const pointer = (from, to, comment = 'Opens here') => ({ id: 'pointer', from, to, comment, wiring: null });
  const request = (elementId, rect, pageId = 'home', sizeId = 'home') => ({ pageId, elementId, sizeId, rect });

  await test('pointer lines leave the nearest edge midpoints and route at any distance', () => {
    const source = box(0, 0, 100, 100);
    assert.deepEqual(nearestEdgeAnchors(source, box(200, 0, 100, 100)), {
      from: { x: 100, y: 50 }, to: { x: 200, y: 50 }, fromEdge: 'right', toEdge: 'left',
    });
    assert.deepEqual(nearestEdgeAnchors(source, box(-200, 0, 100, 100)), {
      from: { x: 0, y: 50 }, to: { x: -100, y: 50 }, fromEdge: 'left', toEdge: 'right',
    });
    assert.deepEqual(nearestEdgeAnchors(source, box(0, 200, 100, 100)), {
      from: { x: 50, y: 100 }, to: { x: 50, y: 200 }, fromEdge: 'bottom', toEdge: 'top',
    });
    assert.deepEqual(nearestEdgeAnchors(source, box(0, -200, 100, 100)), {
      from: { x: 50, y: 0 }, to: { x: 50, y: -100 }, fromEdge: 'top', toEdge: 'bottom',
    });
  });

  await test('pointer endpoint keys compare the whole endpoint', () => {
    const element = { kind: 'element', pageId: 'p', elementId: 'e', sizeId: 's' };
    assert.equal(pointerEndpointKey(element), 'element:p:e:s');
    assert.equal(pointerEndpointKey({ kind: 'boundary', pageId: 'p', sizeId: null }), 'boundary:p:');
    assert.equal(samePointerEndpoint(element, { ...element }), true);
    assert.equal(samePointerEndpoint(element, { ...element, sizeId: 'other' }), false);
    assert.equal(samePointerEndpoint({ kind: 'url', url: 'https://a.test' }, { kind: 'url', url: 'https://a.test' }), true);
  });

  await test('pointer routes resolve frames, boundaries and reference cards', () => {
    const world = {
      requests: [request('e1', box(0, 0, 100, 100))],
      boundaries: [{ pageId: 'home', sizeId: 'home', rect: box(0, 0, 800, 600) }, { pageId: 'other', sizeId: 'other', rect: box(1000, 0, 800, 600) }],
      reference: [{ itemId: 'r1', rect: box(-300, 0, 200, 140), url: 'https://a.test' }],
    };
    assert.deepEqual(pointerEndpointRect({ kind: 'element', pageId: 'home', elementId: 'e1' }, world), box(0, 0, 100, 100));
    assert.deepEqual(pointerEndpointRect({ kind: 'boundary', pageId: 'other' }, world), box(1000, 0, 800, 600));
    assert.deepEqual(pointerEndpointRect({ kind: 'reference', itemId: 'r1' }, world), box(-300, 0, 200, 140));
    assert.deepEqual(pointerEndpointRect({ kind: 'url', url: 'https://a.test' }, world), box(-300, 0, 200, 140));

    const route = pointerRoute(pointer(
      { kind: 'element', pageId: 'home', elementId: 'e1' },
      { kind: 'boundary', pageId: 'other' },
    ), world);
    assert.equal(route.broken, false);
    assert.equal(route.brokenEnd, null);
    assert.deepEqual(route.from, { x: 100, y: 50 });
    assert.deepEqual(route.to, { x: 1000, y: 300 });
    assert.deepEqual(route.source, box(0, 0, 100, 100));
    assert.deepEqual(route.target, box(1000, 0, 800, 600));
  });

  await test('broken pointers survive deletion and fall back to a boundary or a stub', () => {
    const world = {
      requests: [request('e1', box(0, 0, 100, 100))],
      boundaries: [{ pageId: 'home', sizeId: 'home', rect: box(0, 0, 800, 600) }],
      reference: [],
    };
    const missingElement = pointerRoute(pointer(
      { kind: 'element', pageId: 'home', elementId: 'e1' },
      { kind: 'element', pageId: 'home', elementId: 'gone' },
    ), world);
    assert.equal(missingElement.broken, true);
    assert.equal(missingElement.brokenEnd, 'to');
    assert.deepEqual(missingElement.target, box(0, 0, 800, 600), 'the page boundary keeps the line visible');

    const stub = pointerRoute(pointer(
      { kind: 'element', pageId: 'home', elementId: 'e1' },
      { kind: 'reference', itemId: 'gone' },
    ), world);
    assert.equal(stub.broken, true);
    assert.equal(stub.brokenEnd, 'to');
    assert.equal(stub.target, null);
    assert.deepEqual(stub.to, { x: 100 + 120, y: 100 + 120 });

    assert.equal(pointerRoute(pointer(
      { kind: 'boundary', pageId: 'gone' },
      { kind: 'reference', itemId: 'gone' },
    ), { requests: [], boundaries: [], reference: [] }), null);
  });

  await test('pointer hit testing uses the line with a six screen pixel tolerance', () => {
    assert.equal(POINTER_HIT_PX, 6);
    assert.equal(pointSegmentDistance({ x: 5, y: 5 }, { x: 0, y: 0 }, { x: 10, y: 0 }), 5);
    assert.equal(pointSegmentDistance({ x: 5, y: 5 }, { x: 0, y: 0 }, { x: 0, y: 0 }), Math.hypot(5, 5));
    const items = [
      { id: 'far', route: { from: { x: 0, y: 0 }, to: { x: 100, y: 0 } } },
      { id: 'near', route: { from: { x: 0, y: 20 }, to: { x: 100, y: 20 } } },
    ];
    assert.equal(pointerAt(items, { x: 50, y: 21 }, 5).id, 'near');
    assert.equal(pointerAt(items, { x: 50, y: 30 }, 5), null);
    assert.equal(pointerAt([{ id: 'same', route: { from: { x: 0, y: 0 }, to: { x: 100, y: 0 } } },
      { id: 'later', route: { from: { x: 0, y: 0 }, to: { x: 100, y: 0 } } }], { x: 50, y: 0 }, 1).id, 'later');
  });

  await test('pointers whose two ends are outside the viewport are culled', () => {
    const viewport = box(0, 0, 100, 100);
    assert.equal(pointerVisible(box(0, 0, 10, 10), null, viewport), true);
    assert.equal(pointerVisible(box(200, 200, 10, 10), box(300, 300, 10, 10), viewport), false);
    assert.equal(pointerVisible(box(200, 200, 10, 10), box(50, 50, 10, 10), viewport), true);
    assert.deepEqual(screenPoint({ x: 10, y: 20 }, { x: 5, y: 6, zoom: 2 }), { x: 25, y: 46 });
    assert.deepEqual(viewportWorldRect({ x: -100, y: -50, zoom: 2 }, { width: 1000, height: 800 }), { x: 50, y: 25, width: 500, height: 400 });
  });

  const project = {
    pages: [
      {
        id: 'home', name: 'Home', path: '/', sizes: [{ id: 'home', label: 'Desktop', x: 0, y: 0, width: 800, height: 600, kind: 'page' }],
        elements: [{ id: 'e1', kind: 'text', text: 'Home link', name: null, url: '/about/', children: [], rect: box(0, 0, 100, 40), overrides: {}, visible: true }],
      },
      {
        id: 'about', name: 'About', path: '/about', sizes: [{ id: 'about', label: 'Desktop', x: 1000, y: 0, width: 800, height: 600, kind: 'page' }],
        elements: [{ id: 'e2', kind: 'text', text: 'External', name: null, url: 'https://example.test', children: [], rect: box(0, 0, 100, 40), overrides: {}, visible: true }],
      },
    ],
    reference: [
      { id: 'note', rect: box(-300, 0, 200, 140), kind: { kind: 'note', text: 'Read me' } },
      { id: 'card', rect: box(-300, 200, 200, 140), kind: { kind: 'link', url: 'https://example.com', title: 'Example' } },
    ],
    linked: {},
  };

  await test('pointer broken checks read the current project', () => {
    assert.equal(pointerBroken(pointer({ kind: 'boundary', pageId: 'home' }, { kind: 'boundary', pageId: 'about' }), project), false);
    assert.equal(pointerBroken(pointer({ kind: 'element', pageId: 'home', elementId: 'e1' }, { kind: 'boundary', pageId: 'about' }), project), false);
    assert.equal(pointerBroken(pointer({ kind: 'element', pageId: 'home', elementId: 'gone' }, { kind: 'boundary', pageId: 'about' }), project), true);
    assert.equal(pointerBroken(pointer({ kind: 'boundary', pageId: 'home', sizeId: 'gone' }, { kind: 'boundary', pageId: 'about' }), project), true);
    assert.equal(pointerBroken(pointer({ kind: 'boundary', pageId: 'home' }, { kind: 'reference', itemId: 'gone' }), project), true);
    assert.equal(pointerBroken(pointer({ kind: 'boundary', pageId: 'home' }, { kind: 'url', url: 'https://example.com' }), project), false);
    assert.equal(pointerBroken(pointer({ kind: 'boundary', pageId: 'home' }, { kind: 'url', url: 'https://missing.test' }), project), true);
    assert.equal(pointerBroken(pointer({ kind: 'boundary', pageId: 'gone' }, { kind: 'boundary', pageId: 'about' }), project), true);
  });

  await test('pointer names fall back to the page, layer and reference labels', () => {
    const boundaryPointer = pointer({ kind: 'boundary', pageId: 'home' }, { kind: 'reference', itemId: 'note' });
    assert.equal(pointerEndpointName(boundaryPointer.from, project, 'source'), 'Home');
    assert.equal(pointerEndpointName(boundaryPointer.to, project, 'target'), 'Read me');
    assert.equal(pointerLabel(boundaryPointer, project), 'Opens here');
    const unnamed = pointer({ kind: 'element', pageId: 'home', elementId: 'e1' }, { kind: 'element', pageId: 'home', elementId: 'gone' }, '');
    assert.equal(pointerEndpointName(unnamed.from, project, 'source'), 'Home link');
    assert.equal(pointerEndpointName(unnamed.to, project, 'target'), 'Missing target');
    assert.equal(pointerLabel(unnamed, project), 'Home link → Missing target');
    assert.equal(pointerEndpointName({ kind: 'reference', itemId: 'card' }, project, 'target'), 'Example');
    assert.equal(pointerEndpointName({ kind: 'url', url: 'https://a.test' }, project, 'target'), 'https://a.test');
  });

  await test('automatic link lines follow a layer link to another boundary once', () => {
    const boundaries = [
      boundary('home', box(0, 0, 800, 600), 'home'),
      boundary('about', box(1000, 0, 800, 600), 'about'),
    ];
    const boxes = [
      request('e1', box(0, 0, 100, 40), 'home', 'home'),
      request('e1', box(0, 0, 100, 40), 'home', 'home'),
      request('e2', box(1000, 0, 100, 40), 'about', 'about'),
    ];
    const links = automaticLinks(project, boundaries, boxes);
    assert.equal(links.length, 1, 'one line per page pair');
    assert.equal(links[0].fromPageId, 'home');
    assert.equal(links[0].toPageId, 'about');
    assert.deepEqual(links[0].source, box(0, 0, 100, 40));
    assert.deepEqual(links[0].target, box(1000, 0, 800, 600));
    assert.deepEqual(links[0].route, nearestEdgeAnchors(box(0, 0, 100, 40), box(1000, 0, 800, 600)));
    assert.deepEqual(automaticLinks(project, boundaries, []), []);
  });
} finally {
  await modules.cleanup();
}
