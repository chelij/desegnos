// Run: node benchmarks/canvas.mjs (compilation is excluded from timings).
import assert from 'node:assert/strict';
import { performance } from 'node:perf_hooks';
import { compileModules } from '../tests/typescript.mjs';

const modules = await compileModules(['src/projectCanvas/selectionGeometry.ts', 'src/projectCanvas/canvasView.ts', 'src/projectCanvas/elements.tsx', 'src/projectCanvas/moving.ts']);
try {
  const { createElement } = await modules.load('projectCanvas/elementGeometry.js');
  const { canvasView } = await modules.load('projectCanvas/canvasView.js');
  const { measureElementSelection, projectMeasuredSelection } = await modules.load('projectCanvas/elements.js');
  const { selectionEntries } = await modules.load('projectCanvas/selectionGeometry.js');
  const elements = Array.from({ length: 640 }, (_, i) => createElement('box', {
    x: i % 32 * 32, y: Math.floor(i / 32) * 32, width: 24, height: 24,
  }));
  const page = { id: 'page', name: 'Image', path: '/', included: true, output: 'image', background: '#ffffff',
    sizes: [{ id: 'size', label: 'Image', kind: 'screen', width: 1080, height: 1080, x: 0, y: 0 }], elements };
  const project = { id: 'benchmark', revision: 1, homePageId: page.id, pages: [page], linked: {}, reference: [], pointers: [] };
  const boundary = () => canvasView(project).boundaries[0].rect;
  const targets = elements.map(element => ({ kind: 'element', pageId: page.id, elementId: element.id, sizeId: 'size' }));
  function time(label, run, iterations = 20) {
    run();
    const samples = [];
    for (let i = 0; i < iterations; i++) { const start = performance.now(); run(); samples.push(performance.now() - start); }
    samples.sort((a, b) => a - b);
    console.log(`${label}: ${samples[Math.floor(samples.length / 2)].toFixed(3)} ms median`);
  }
  time('cold canvas view / 640', () => { project.revision++; assert.equal(canvasView(project).boxes.length, 640); });
  time('cached canvas view / 640', () => assert.equal(canvasView(project).boxes.length, 640));
  for (const count of [80, 160, 320, 640]) {
    time(`selection entries / ${count}`, () => assert.equal(selectionEntries(project, targets.slice(0, count), boundary).length, count));
  }
  // Count DOM queries separately: this fixture checks traversal, not browser paint/layout time.
  let queries = 0;
  const nodes = elements.map(element => ({ dataset: { element: element.id },
    closest: () => ({ dataset: { size: 'size' } }),
    getBoundingClientRect: () => ({ left: element.rect.x, top: element.rect.y, width: 24, height: 24, right: element.rect.x + 24, bottom: element.rect.y + 24 }) }));
  const surface = { getBoundingClientRect: () => ({ left: 0, top: 0 }), querySelectorAll: selector => {
    queries++;
    return selector === '[data-element]' || selector === '[data-element-kind="box"]' ? nodes : nodes.filter(node => selector === `[data-element="${node.dataset.element}"]`);
  } };
  assert.equal(measureElementSelection(surface, targets).length, 640);
  console.log(`selection measurement / 640: ${queries} DOM queries`);
  assert.equal(queries, 1);
  const measured = measureElementSelection(surface, targets);
  const from = { x: 0, y: 0, zoom: 1 }, to = { x: 12, y: -8, zoom: 2 };
  queries = 0;
  time('pan/zoom selection projection / 640', () => {
    const projected = projectMeasuredSelection(measured, from, to);
    assert.deepEqual(projected[0].rect, { left: 12, top: -8, width: 48, height: 48 });
  });
  assert.deepEqual(projectMeasuredSelection(projectMeasuredSelection(measured, from, to), to, from), measured);
  assert.equal(queries, 0, 'camera changes never re-query the DOM');
  const inherited = measureElementSelection(surface, [{ ...targets[0], sizeId: 'other' }, { ...targets[0], sizeId: null }]);
  assert.deepEqual(inherited.map(row => row.inherited), [true, false]);
  const { createMovingActions } = await modules.load('projectCanvas/moving.js');
  const view = canvasView(project), dropSurface = { current: surface };
  const { commitElementMove } = createMovingActions(() => ({
    project, boundaries: view.boundaries, surface: dropSurface, camera: { x: 0, y: 0, zoom: 1 },
    boundaryUnder: () => view.boundaries[0], boundaryRect: boundary,
    duplicateRepeat: { current: null }, selectTargets: () => {},
    moveElements: moves => { assert.equal(moves.length, 640); assert.equal(moves[0].rect.x, 8); },
  }));
  queries = 0;
  await commitElementMove({ entries: selectionEntries(project, targets, boundary), start: { x: 0, y: 0 }, axisLocked: false }, { x: 8, y: 8 }, false);
  console.log(`drop 640 boxes: ${queries} DOM queries`);
  assert.equal(queries, 1);
  // Selected ancestors suppress children.
  const parent = createElement('box', { x: 0, y: 0, width: 100, height: 100 }, { children: [elements[0]] });
  page.elements = [parent, ...elements.slice(1)]; project.revision++;
  const nested = selectionEntries(project, [{ ...targets[0], elementId: parent.id }, targets[0], targets[1]], boundary);
  assert.deepEqual(nested.map(entry => entry.id), [parent.id, elements[1].id]);
} finally { await modules.cleanup(); }
