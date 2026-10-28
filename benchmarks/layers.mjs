// Run: node benchmarks/layers.mjs. Compilation is excluded; SSR excludes browser layout/paint.
import assert from 'node:assert/strict';
import { performance } from 'node:perf_hooks';
import { createElement as h } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { compileModules } from '../tests/typescript.mjs';

const modules = await compileModules(['src/projectCanvas/elementLayers.tsx', 'src/projectCanvas/sharedElementValues.ts']);
try {
  const { createElement, commonValue, elementPadding } = await modules.load('projectCanvas/elementGeometry.js');
  const { ElementLayers, flattenLayers, matchingLayerIds } = await modules.load('projectCanvas/elementLayers.js');
  const { sharedElementValues } = await modules.load('projectCanvas/sharedElementValues.js');
  const { normalizeAngle } = await modules.load('numberExpression.js');
  const elements = Array.from({ length: 640 }, (_, i) => createElement('box', { x: i * 32, y: 0, width: 24, height: 24 }));
  const project = { id: 'benchmark', revision: 1, pages: [{ id: 'page', elements }], linked: {} };
  const noop = () => {};
  const props = { project, pageId: 'page', sizeId: 'size', elements, selection: elements.slice(0, 320).map(element => element.id), select: noop, update: noop, remove: noop, move: noop };
  function time(label, run) {
    run();
    const samples = [];
    let result;
    for (let i = 0; i < 30; i++) {
      const start = performance.now(); result = run(); samples.push(performance.now() - start);
    }
    samples.sort((a, b) => a - b);
    console.log(`${label}: ${samples[15].toFixed(3)} ms median`);
    return result;
  }
  const markup = time('Layers / 640 elements, 320 selected', () => renderToStaticMarkup(h(ElementLayers, props)));
  const rows = (markup.match(/data-element-row=/g) ?? []).length;
  assert.ok(rows > 0 && rows < 30, 'only a viewport and overscan are rendered');
  console.log(`${rows} rendered rows; ${markup.length} markup bytes (SSR viewport: 280px)`);
  const flat = time('Flatten / 640', () => flattenLayers(elements, 'size', new Set(), null));
  assert.equal(flat.length, 640);
  assert.equal(flat[0].element.id, elements[639].id);
  assert.equal(flat[0].index, 639);
  const parent = createElement('box', { x: 0, y: 0, width: 100, height: 100 }, { name: 'Parent', children: elements.slice(0, 2) });
  const nested = [parent, elements[2]];
  const collapsed = new Set([`size:${parent.id}`]);
  assert.equal(flattenLayers(nested, 'size', collapsed, null).length, 2);
  const keep = matchingLayerIds(nested, 'Parent');
  assert.deepEqual(flattenLayers(nested, 'size', collapsed, keep).map(row => [row.element.id, row.depth]),
    [[parent.id, 0], [elements[1].id, 1], [elements[0].id, 1]]);
  elements[0].name = 'Needle';
  assert.deepEqual([...matchingLayerIds(nested, 'Needle')].sort(), [parent.id, elements[0].id].sort());

  // The former Properties implementation: a separate map/commonValue scan for each field.
  const selectors = {
    fill: e => e.style.fill, strokeColour: e => e.style.stroke.color, strokeWidth: e => e.style.stroke.width,
    radius: e => e.style.radius, opacity: e => e.style.opacity, shadow: e => e.style.shadow,
    shadowColour: e => e.style.shadow.color, textStyle: e => e.style.textStyle, textColour: e => e.style.textStyle.color,
    text: e => e.text, url: e => e.url, layout: e => e.layout,
    x: e => e.rect.x, y: e => e.rect.y, width: e => e.rect.width, height: e => e.rect.height,
    rotation: e => normalizeAngle(e.rotation), top: e => elementPadding(e).top, right: e => elementPadding(e).right,
    bottom: e => elementPadding(e).bottom, left: e => elementPadding(e).left,
    fillReference: e => e.style.references?.fill, strokeReference: e => e.style.references?.stroke,
    textColourReference: e => e.style.references?.textColour, textReference: e => e.style.references?.text,
    kind: e => e.kind, link: e => e.link,
  };
  const selection = elements.slice(0, 320);
  const before = values => Object.fromEntries(Object.entries(selectors).map(([key, read]) => [key, commonValue(values.map(read))]));
  const oldValues = time('Properties shared values / 320 / before', () => before(selection));
  const newValues = time('Properties shared values / 320 / after', () => sharedElementValues(selection));
  assert.deepEqual(newValues, oldValues);
  selection[1].style = { ...selection[1].style, fill: '#abcdef', textStyle: { ...selection[1].style.textStyle, size: 42 } };
  assert.deepEqual(sharedElementValues(selection), before(selection));
  assert.deepEqual(sharedElementValues([selection[0]]), before([selection[0]]));
  console.log('Ordering, nesting, collapse, search and shared/Mixed checks passed.');

} finally { await modules.cleanup(); }
