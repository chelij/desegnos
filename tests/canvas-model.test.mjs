import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { join } from 'node:path';
import { compileModules, root } from './typescript.mjs';

const modules = await compileModules([
  'src/projectCanvas/canvasView.ts',
  'src/projectCanvas/elements.tsx',
  'src/projectCanvas/arrange.ts',
  'src/projectCanvas/sizes.ts',
  'src/projectCanvas/links.ts',
  'src/projectCanvas/elementGeometry.ts',
  'src/projectCanvas/minimap.ts',
  'src/projectCanvas/pointerNames.ts',
  'src/projectCanvas/autoLinks.ts',
  'src/projectCanvas/pointerGeometry.ts',
  'src/projectCanvas/referenceGeometry.ts',
  'src/projectCanvas/canvasTargets.ts',
  'src/projectCanvas/boundaryLayout.ts',
  'src/projectCanvas/camera.ts',
  'src/shortcuts.ts',
]);
try {
  const { DEFAULT_GRID_SPACING: GRID_SPACING } = await modules.load('generated/limits.js');
  const { arrangeSiblingOrder } = await modules.load('projectCanvas/arrange.js');
  await test('live layer labels and default-name detection agree with Rust', async () => {
    const facts = await modules.load('projectCanvas/elementFacts.js');
    const cases = JSON.parse(await readFile(join(root, 'src/generated/elementFacts.json'), 'utf8'));
    for (const item of cases) {
      assert.equal(facts.elementLabel(item.element), item.label, JSON.stringify(item.element));
      assert.equal(facts.isDefaultLayerName(item.element), item.isDefault, JSON.stringify(item.element));
    }
  });
  await test('a page added to a slide or image project keeps page defaults', async () => {
    const { boundaryDefaults, layoutKind } = await modules.load('projectCanvas/boundaryDefaults.js');
    for (const template of ['slideDeck', 'socialImage']) {
      const project = { template, pages: [] };
      assert.deepEqual(boundaryDefaults(project, 'page'), { x: 0, y: 0, width: 1440, height: 900 });
      assert.equal(layoutKind('page', project), 'page');
    }
    assert.deepEqual(boundaryDefaults({ template: 'appScreen', pages: [] }, 'page'), { x: 0, y: 0, width: 390, height: 844 });
  });
  await test('arranging siblings distinguishes steps from edges and preserves selected blocks', () => {
    const ids = ['a', 'b', 'c', 'd', 'e'];
    assert.deepEqual(arrangeSiblingOrder(ids, ['b', 'c'], 'forward'), ['a', 'd', 'b', 'c', 'e']);
    assert.deepEqual(arrangeSiblingOrder(ids, ['b', 'c'], 'front'), ['a', 'd', 'e', 'b', 'c']);
    assert.deepEqual(arrangeSiblingOrder(ids, ['c', 'd'], 'backward'), ['a', 'c', 'd', 'b', 'e']);
    assert.deepEqual(arrangeSiblingOrder(ids, ['c', 'd'], 'back'), ['c', 'd', 'a', 'b', 'e']);
    assert.deepEqual(arrangeSiblingOrder(ids, ['d', 'e'], 'forward'), ids);
    assert.deepEqual(arrangeSiblingOrder(ids, ['a', 'b'], 'backward'), ids);
    assert.deepEqual(arrangeSiblingOrder(ids, ['a', 'c'], 'forward'), ['b', 'a', 'd', 'c', 'e']);
    assert.deepEqual(arrangeSiblingOrder(ids, [], 'front'), ids);
    assert.deepEqual(arrangeSiblingOrder(ids, ids, 'back'), ids);
    assert.deepEqual(ids, ['a', 'b', 'c', 'd', 'e']);
  });
  const canvas = await modules.load('projectCanvas/canvasView.js');
  const sizes = await modules.load('projectCanvas/sizes.js');
  const links = await modules.load('projectCanvas/links.js');
  const elementGeometry = await modules.load('projectCanvas/elementGeometry.js');
  const geometry = Object.assign({}, ...await Promise.all(['camera', 'boundaryLayout', 'referenceGeometry', 'canvasTargets', 'pointerGeometry', 'pointerNames', 'autoLinks'].map(name => modules.load(`projectCanvas/${name}.js`))));
  const elementFacts = await modules.load('projectCanvas/elementFacts.js');
  const minimap = await modules.load('projectCanvas/minimap.js');
  const shortcuts = await modules.load('shortcuts.js');

  const baseStyle = () => ({
    fill: '#ffffff',
    stroke: { color: null, width: 0 },
    radius: 0,
    opacity: 1,
    shadow: { x: 0, y: 0, blur: 0, color: null },
    textStyle: { fontFamily: 'system-ui', size: 16, weight: 400, italic: false, lineHeight: 1.4, letterSpacing: 0, color: '#111111', align: 'left' },
  });
  const makeElement = (id, patch = {}) => ({
    id, kind: 'box', name: null, link: null, rect: { x: 0, y: 0, width: 100, height: 100 }, rotation: 0,
    visible: true, locked: false, style: baseStyle(), text: '', image: null, url: null,
    layout: { kind: 'free' }, children: [], meta: { requests: [], context: [] }, ...patch,
  });
  const makeSize = (id, label, width, height, kind = 'page') => ({ id, label, width, height, kind, x: 0, y: 0 });
  const makePage = (id, name, path, sizes, elements) => ({ id, name, path, included: true, output: 'webpage', sizes, background: '#ffffff', elements });
  let projectSequence = 0;
  const makeProject = (pages = [], patch = {}) => ({
    schemaVersion: 14, id: `project-${++projectSequence}`, name: 'Test', revision: 1,
    viewport: { width: 800, height: 600 }, appearance: 'light', showGrid: true,
    gridSettings: { spacing: 8, unit: 'pixels', snap: true, rulers: true }, generationMode: 'fuzzy',
    homePageId: pages[0]?.id ?? 'home', pages, reference: [], pointers: [], linked: {},
    brief: { purpose: '', audience: '', brandName: '', tone: '' }, latestGeneration: null,
    decisions: [], exceptions: [], appliedJobIds: [], extensions: {}, ...patch,
  });

  const { wrapperStyle } = await modules.load('projectCanvas/elements.js');
  await test('Hug text and controls cap their width at the containing edge', () => {
    for (const kind of ['text', 'button', 'link', 'input']) {
      const element = makeElement('heading', { kind, rect: { x: 104, y: 0, width: 800, height: 60 } });
      element.style.sizing = { width: 'fit', height: 'fit' };
      assert.equal(wrapperStyle(element, false, element.rect).width, 'max-content');
      assert.equal(wrapperStyle(element, false, element.rect).maxWidth, 'max(0px, calc(100% - 104px))');
      assert.equal(wrapperStyle(element, true, element.rect).maxWidth, '100%');
      element.style.sizing.maxWidth = 200;
      assert.equal(wrapperStyle(element, false, element.rect).maxWidth, 'min(200px, max(0px, calc(100% - 104px)))');
      element.style.sizing.width = 'fixed';
      assert.equal(wrapperStyle(element, false, element.rect).width, 800);
      assert.equal(wrapperStyle(element, false, element.rect).maxWidth, 200);
    }
  });

  await test('new text and shapes use readable defaults for the boundary background', () => {
    const rect = { x: 0, y: 0, width: 100, height: 40 };
    for (const background of ['#ffffff', '#f7f7f5', '#e9e9e9', '#fff']) {
      const text = elementGeometry.forBackground(elementGeometry.createTextElement(rect), background);
      assert.equal(text.style.textStyle.color, '#1a1a1a');
    }
    for (const background of ['#1f1f1f', '#121212', '#000000']) {
      const text = elementGeometry.forBackground(elementGeometry.createTextElement(rect), background);
      const line = elementGeometry.forBackground(elementGeometry.createElement('line', rect), background);
      assert.equal(text.style.textStyle.color, '#f2f2f2');
      assert.equal(line.style.stroke.color, '#f2f2f2');
    }
  });

  await test('canvas views reuse resolution and indices until the project revision changes', () => {
    const layer = makeElement('layer', { children: [makeElement('child')] });
    const page = makePage('p', 'Page', '/', [makeSize('base', 'Desktop', 800, 600), makeSize('phone', 'Phone', 390, 844)], [layer]);
    const project = makeProject([page]);
    const first = canvas.canvasView(project);
    const tree = sizes.resolveForSize(project, page, 'base');
    assert.equal(canvas.canvasView(project), first);
    assert.equal(sizes.resolveForSize(project, page, 'base'), tree);
    assert.notEqual(sizes.resolveForSize(project, page, 'phone'), tree);
    assert.equal(elementGeometry.elementIndex(tree), elementGeometry.elementIndex(tree));
    assert.equal(canvas.canvasBoxes(project, first.boundaries), first.boxes);
    assert.equal(first.boxes.length, 4);
    const refreshed = structuredClone(project);
    assert.equal(canvas.canvasView(refreshed), first, 'unchanged native snapshots reuse the revision view');
    assert.equal(sizes.resolveForSize(refreshed, refreshed.pages[0], 'base'), tree);
    assert.equal(canvas.canvasBoxes(refreshed, first.boundaries), first.boxes);
    project.revision++;
    layer.rect.x = 24;
    const next = canvas.canvasView(project);
    assert.notEqual(next, first);
    assert.notEqual(sizes.resolveForSize(project, page, 'base'), tree);
    assert.equal(tree[0].rect.x, 0, 'cached trees never alias editable project data');
    assert.equal(next.boxes[0].rect.x, next.boundaries[0].rect.x + 24);
  });

  await test('boundary culling includes a margin and still retains all model hit boxes', () => {
    const viewport = { x: 0, y: 0, width: 800, height: 600 };
    assert.equal(canvas.boundaryVisible({ x: 900, y: 0, width: 100, height: 100 }, viewport, 200), true);
    assert.equal(canvas.boundaryVisible({ x: 1001, y: 0, width: 100, height: 100 }, viewport, 200), false);
    assert.equal(canvas.boundaryVisible({ x: -400, y: -400, width: 100, height: 100 }, viewport, 200), false);
    assert.equal(canvas.boundaryVisible({ x: -400, y: -400, width: 2000, height: 2000 }, viewport, 200), true);
    const page = makePage('p', 'Distant', '/', [{ ...makeSize('base', 'Desktop', 800, 600), x: 10000, y: 10000 }], [makeElement('layer')]);
    const view = canvas.canvasView(makeProject([page]));
    assert.equal(canvas.boundaryVisible(view.boundaries[0].rect, viewport, 200), false);
    assert.equal(view.boxes[0].elementId, 'layer');
    assert.equal(view.boxes[0].rect.x, view.boundaries[0].rect.x);
  });

  // -------------------------------------------------------------------------
  // sizes.ts
  // -------------------------------------------------------------------------

  await test('size helpers read the base size and name the boundary', () => {
    const page = makePage('p', 'Home', '/', [makeSize('base', 'Desktop', 1440, 900), makeSize('phone', 'Phone', 390, 844, 'screen')], []);
    assert.equal(sizes.baseSizeId(page), 'base');
    assert.deepEqual([500, 700, 1023, 1024].map(sizes.sizeLabel), ['Phone', 'Tablet', 'Tablet', 'Desktop']);
    assert.equal(sizes.hasManySizes(page), true);
    assert.equal(sizes.hasManySizes(makePage('one', 'One', '/one', [makeSize('only', 'Desktop', 800, 600)], [])), false);
    assert.equal(sizes.boundaryLabel(page, page.sizes[1]), 'Home · Phone');
    assert.equal(sizes.boundaryLabel(makePage('one', 'One', '/one', [makeSize('only', 'Desktop', 800, 600)], []), makeSize('only', 'Desktop', 800, 600)), 'One');
    assert.deepEqual(sizes.SIZE_PRESETS.map(preset => [preset.preset, preset.width, preset.height]), [
      ['phone', 390, 844], ['tablet', 820, 1180], ['desktop', 1440, 900],
    ]);
  });

  await test('resolveForSize applies overrides without writing them back', () => {
    const child = makeElement('child', { rect: { x: 10, y: 10, width: 20, height: 20 }, text: 'Child' });
    const root = makeElement('root', {
      rect: { x: 50, y: 60, width: 200, height: 100 },
      text: 'Root',
      children: [child],
      overrides: {
        phone: {
          rect: { x: 5, y: 6, width: 120, height: 80 },
          style: { ...baseStyle(), fill: '#ff0000' },
          layout: { kind: 'stack', direction: 'column', gap: 4, padding: 8, align: 'start', justify: 'start', wrap: false },
          hidden: false,
          dismissed: false,
        },
      },
    });
    child.overrides = { phone: { hidden: true, dismissed: true } };
    const page = makePage('p', 'Home', '/', [makeSize('base', 'Desktop', 1440, 900), makeSize('phone', 'Phone', 390, 844, 'screen')], [root]);
    const project = makeProject([page]);
    const phone = sizes.resolveForSize(project, page, 'phone');
    assert.deepEqual(phone[0].rect, { x: 5, y: 6, width: 120, height: 80 });
    assert.equal(phone[0].style.fill, '#ff0000');
    assert.equal(phone[0].layout.kind, 'stack');
    assert.equal(phone[0].children[0].visible, false);
    // The project state is untouched.
    assert.deepEqual(root.rect, { x: 50, y: 60, width: 200, height: 100 });
    assert.equal(root.style.fill, '#ffffff');
    assert.equal(root.children[0].visible, true);
    assert.deepEqual(root.overrides.phone.layout.kind, 'stack');
  });

  await test('placeholders, origins and per-size inspection read the resolved tree', () => {
    const parent = makeElement('parent', { rect: { x: 100, y: 200, width: 300, height: 200 } });
    const child = makeElement('child', { rect: { x: 10, y: 20, width: 50, height: 30 } });
    parent.children = [child];
    child.overrides = { phone: { hidden: true, dismissed: false } };
    const page = makePage('p', 'Home', '/', [makeSize('base', 'Desktop', 1440, 900), makeSize('phone', 'Phone', 390, 844)], [parent]);
    child.overrides.phone.dismissed = true;
    assert.deepEqual(sizes.elementOrigin(sizes.resolveForSize(makeProject([page]), page, 'base'), 'child'), { x: 110, y: 220 });
    assert.equal(sizes.elementOrigin(sizes.resolveForSize(makeProject([page]), page, 'base'), 'gone'), null);
  });

  // -------------------------------------------------------------------------
  // links.ts
  // -------------------------------------------------------------------------

  const master = (name = 'Card') => makeElement('master', {
    name,
    rect: { x: 0, y: 0, width: 200, height: 100 },
    children: [makeElement('master-child', { rect: { x: 10, y: 12, width: 60, height: 24 }, text: 'Inside' })],
  });
  const linkedProject = (copies = []) => {
    const definition = { id: 'linked', name: 'Card', master: master() };
    const pages = copies.length
      ? [makePage('p', 'Home', '/', [makeSize('base', 'Desktop', 1440, 900)], copies)]
      : [];
    return makeProject(pages, { linked: { linked: definition } });
  };

  await test('nested ids follow the same algorithm as the Rust core', () => {
    const id = '12345678-1234-4234-8234-1234567890ab';
    assert.equal(links.nestedElementId(id, [0]), '12345678-1234-4234-8234-12343e44d4c0');
    assert.equal(links.nestedElementId(id, [1, 2]), '12345678-1234-4234-8234-12344a75f0a3');
    assert.equal(links.nestedElementId(id, [0, 1, 2]), '12345678-1234-4234-8234-1234cc84f503');
    assert.equal(links.nestedElementId(id, [0]), links.nestedElementId(id, [0]));
    assert.notEqual(links.nestedElementId(id, [0]), links.nestedElementId(id, [1]));
  });

  await test('a linked copy resolves with its own id and position and the definition content', () => {
    const copy = makeElement('copy', {
      link: 'linked', name: 'Card', rect: { x: 300, y: 40, width: 200, height: 100 }, visible: false, locked: true,
      meta: { requests: [{ id: 'request', prompt: 'Keep me', attachments: [], createdAt: 'now', outcome: null, jobId: null }], context: [] },
    });
    const project = linkedProject([copy]);
    const definition = links.linkedDefinition(project, copy);
    assert.equal(definition.name, 'Card');
    assert.equal(links.linkedName(project, copy), 'Card');
    assert.equal(links.linkedName(project, makeElement('plain')), null);
    const resolved = links.resolveElement(project, copy);
    assert.equal(resolved.id, 'copy');
    assert.equal(resolved.link, null);
    assert.equal(links.linkedName(project, resolved), 'Card');
    assert.deepEqual(resolved.rect, { x: 300, y: 40, width: 200, height: 100 });
    assert.equal(resolved.visible, false);
    assert.equal(resolved.locked, true);
    assert.equal(resolved.children[0].text, 'Inside');
    assert.equal(resolved.children[0].id, links.nestedElementId('copy', [0]));
    assert.equal(resolved.meta.requests[0].prompt, 'Keep me');
    assert.equal(master().children[0].id, 'master-child', 'the definition is not rewritten');
    assert.deepEqual(links.resolveElements(project, [copy]).map(item => item.id), ['copy']);
  });

  await test('nested layer ids and structure checks drive linking', () => {
    const copy = makeElement('copy', { link: 'linked', rect: { x: 300, y: 40, width: 200, height: 100 } });
    const project = linkedProject([copy]);
    const nested = links.nestedLayerIds(project);
    assert.ok(nested.has(links.nestedElementId('copy', [0])));
    assert.equal(links.isNestedLayer(project, links.nestedElementId('copy', [0])), true);
    assert.equal(links.isNestedLayer(project, 'copy'), false);
    assert.equal(links.sameKind(makeElement('a', { kind: 'text' }), makeElement('b', { kind: 'text' })), true);
    assert.equal(links.sameKind(makeElement('a', { kind: 'text' }), makeElement('b', { kind: 'box' })), false);
    const first = makeElement('a', { text: 'One', children: [makeElement('a-child')] });
    const second = makeElement('b', { text: 'Two', children: [makeElement('b-child')] });
    assert.equal(links.sameStructure(first, second), true);
    assert.equal(links.sameStructure(first, makeElement('c')), false);
    assert.equal(links.linkNeedsChoice([first, second]), true, 'different text needs a winner');
    assert.equal(links.linkNeedsChoice([first, { ...first, id: 'other' }]), false);
    assert.equal(links.copySummary(makeElement('a', { name: 'Named' })), 'Named');
    assert.equal(links.copySummary(makeElement('a', { text: 'First line\nSecond' })), 'First line');
    assert.equal(links.copySummary(makeElement('a', { kind: 'image', text: '' })), 'Picture');
    assert.equal(links.copySummary(makeElement('a', { kind: 'code', language: 'css', text: '' })), 'css code');
    assert.equal(links.copySummary(makeElement('a', { kind: 'ellipse', text: '' })), 'Ellipse');
  });

  // -------------------------------------------------------------------------
  // elementGeometry.ts
  // -------------------------------------------------------------------------

  await test('element creation gives each kind sensible defaults', () => {
    assert.equal(GRID_SPACING, 8);
    assert.equal(geometry.MIN_SIZE, 1);
    const input = elementGeometry.createElement('input', { x: 0, y: 0, width: 100, height: 40 });
    assert.equal(input.inputType, 'text');
    assert.equal(input.text, 'Input');
    const select = elementGeometry.createElement('select', { x: 0, y: 0, width: 100, height: 40 });
    assert.deepEqual(select.options, ['Option 1', 'Option 2', 'Option 3']);
    const code = elementGeometry.createElement('code', { x: 0, y: 0, width: 100, height: 40 });
    assert.equal(code.language, 'html');
    assert.ok(code.source.includes('<div>'));
    for (const kind of ['link', 'video', 'audio', 'embed']) {
      assert.equal(elementGeometry.createElement(kind, { x: 0, y: 0, width: 10, height: 10 }).url, 'https://example.com');
    }
    const text = elementGeometry.createTextElement({ x: 1, y: 2, width: 3, height: 4 }, 'Hi');
    assert.equal(text.kind, 'text');
    assert.equal(text.text, 'Hi');
    assert.equal(text.style.fill, null);
    assert.deepEqual(elementGeometry.roundRect({ x: 1.4, y: 1.6, width: 2.5, height: 3.5 }), { x: 1, y: 2, width: 3, height: 4 });
    assert.equal(geometry.snapValue(13, 8), 16);
    assert.deepEqual(elementGeometry.snapPoint({ x: 4, y: 12 }, 8), { x: 8, y: 16 });
    assert.deepEqual(geometry.unionBounds([]), { x: 0, y: 0, width: 0, height: 0 });
    assert.deepEqual(geometry.unionBounds([{ x: 0, y: 0, width: 10, height: 10 }, { x: 20, y: 5, width: 10, height: 10 }]), { x: 0, y: 0, width: 30, height: 15 });
  });

  await test('a cloned element never shares ids', () => {
    const original = makeElement('root', { children: [makeElement('child', { children: [makeElement('grandchild')] })] });
    const clone = elementGeometry.cloneElement(original);
    assert.notEqual(clone.id, original.id);
    assert.notEqual(clone.children[0].id, original.children[0].id);
    assert.notEqual(clone.children[0].children[0].id, original.children[0].children[0].id);
    assert.equal(clone.children[0].text, original.children[0].text);
  });

  await test('element paths, locations and world rectangles use parent origins', () => {
    const grandchild = makeElement('grandchild', { rect: { x: 5, y: 6, width: 30, height: 20 }, text: 'Deep' });
    const child = makeElement('child', { rect: { x: 10, y: 20, width: 100, height: 60 }, children: [grandchild] });
    const root = makeElement('root', { rect: { x: 100, y: 200, width: 300, height: 200 }, children: [child] });
    const other = makeElement('other', { rect: { x: 500, y: 0, width: 50, height: 50 } });
    const page = makePage('p', 'Home', '/', [makeSize('base', 'Desktop', 1440, 900)], [root, other]);
    const project = makeProject([page]);
    assert.equal(elementFacts.findElementIn(page.elements, 'grandchild').text, 'Deep');
    assert.equal(elementFacts.findElementIn(page.elements, 'gone'), null);
    const path = elementGeometry.elementPath(project, 'p', 'grandchild');
    assert.deepEqual(path.map(entry => entry.element.id), ['root', 'child', 'grandchild']);
    assert.deepEqual(path[1].origin, { x: 110, y: 220 });
    assert.deepEqual(path[1].parentOrigin, { x: 100, y: 200 });
    assert.deepEqual(path[2].origin, { x: 115, y: 226 });
    assert.deepEqual(path[2].parentOrigin, { x: 110, y: 220 });
    assert.equal(path[2].depth, 2);
    assert.deepEqual(elementGeometry.elementParents(project, 'p', 'grandchild'), ['root', 'child']);
    assert.deepEqual(elementGeometry.elementChildrenOf(project, 'p', 'child').map(item => item.id), ['grandchild']);
    assert.deepEqual(elementGeometry.elementChildrenOf(project, 'p', null).map(item => item.id), ['root', 'other']);
    assert.deepEqual(elementGeometry.elementWorldRect(project, 'p', 'grandchild', { x: 1000, y: 2000 }), { x: 1115, y: 2226, width: 30, height: 20 });
    assert.equal(elementGeometry.elementWorldRect(project, 'p', 'gone', { x: 0, y: 0 }), null);
    assert.deepEqual(elementGeometry.elementBoundsHeight([makeElement('a', { rect: { x: 0, y: 0, width: 10, height: 100 }, children: [makeElement('b', { rect: { x: 0, y: 50, width: 10, height: 200 } })] })], 10), 250);
    assert.equal(elementGeometry.elementBoundsHeight([], 10), 10);
  });

  await test('page elements resolve linked copies and the selected size', () => {
    const copy = makeElement('copy', { link: 'linked', rect: { x: 300, y: 40, width: 200, height: 100 } });
    copy.overrides = { phone: { rect: { x: 20, y: 30, width: 120, height: 80 }, hidden: false, dismissed: false } };
    const page = makePage('p', 'Home', '/', [makeSize('base', 'Desktop', 1440, 900), makeSize('phone', 'Phone', 390, 844)], [copy]);
    const project = makeProject([page], { linked: { linked: { id: 'linked', name: 'Card', master: master() } } });
    const base = elementGeometry.pageElements(project, 'p');
    assert.equal(base[0].children[0].text, 'Inside');
    assert.deepEqual(elementGeometry.pageElements(project, 'p', 'phone')[0].rect, { x: 20, y: 30, width: 120, height: 80 });
    assert.equal(elementGeometry.findPageElement(project, 'p', 'copy').id, 'copy');
    assert.equal(elementGeometry.findPageElement(project, 'p', 'gone'), null);
    assert.equal(elementGeometry.pageElements(project, 'gone').length, 0);
  });

  await test('move and resize math snaps, anchors and keeps a minimum size', () => {
    const rect = { x: 10, y: 20, width: 100, height: 80 };
    assert.deepEqual(elementGeometry.resizedElementRect(rect, 'se', { x: 150, y: 140 }, false, GRID_SPACING), { x: 10, y: 20, width: 140, height: 120 });
    assert.deepEqual(elementGeometry.resizedElementRect(rect, 'nw', { x: 0, y: 0 }, false, GRID_SPACING), { x: 0, y: 0, width: 110, height: 100 });
    assert.deepEqual(elementGeometry.resizedElementRect(rect, 'e', { x: 150, y: 400 }, false, GRID_SPACING), { x: 10, y: 20, width: 140, height: 80 });
    assert.deepEqual(elementGeometry.resizedElementRect(rect, 'n', { x: 0, y: 0 }, false, GRID_SPACING), { x: 10, y: 0, width: 100, height: 100 });
    assert.deepEqual(elementGeometry.resizedElementRect(rect, 'se', { x: -100, y: -100 }, false, GRID_SPACING), { x: 10, y: 20, width: 1, height: 1 });
    assert.deepEqual(elementGeometry.resizedElementRect(rect, 'nw', { x: 500, y: 500 }, false, GRID_SPACING), { x: 109, y: 99, width: 1, height: 1 });
    assert.deepEqual(elementGeometry.ratioResizedRect(rect, 'se', { x: 400, y: 300 }, false, GRID_SPACING), { x: 10, y: 20, width: 390, height: 312 });
    assert.deepEqual(elementGeometry.rectFromPoints({ x: 30, y: 40 }, { x: 10, y: 20 }), { x: 10, y: 20, width: 20, height: 20 });
    const boundaryOrigin = { x: 100, y: 200 };
    const parentOrigin = { x: 50, y: 60 };
    assert.deepEqual(elementGeometry.elementLocalRect({ x: 160, y: 280, width: 100, height: 80 }, boundaryOrigin, parentOrigin), rect);
  });

  await test('transform keys resize from the centre, keep ratios and lock axes', () => {
    const rect = { x: 10, y: 20, width: 100, height: 80 };
    assert.deepEqual(elementGeometry.resizedRectWithKeys(rect, 'se', { x: 210, y: 180 }, { ...{ snap: false }, spacing: GRID_SPACING }), { x: 10, y: 20, width: 200, height: 160 });
    assert.deepEqual(elementGeometry.resizedRectWithKeys(rect, 'se', { x: 210, y: 180 }, { ...{ snap: false, fromCentre: true }, spacing: GRID_SPACING }), { x: -40, y: -20, width: 200, height: 160 });
    assert.deepEqual(elementGeometry.resizedRectWithKeys(rect, 'nw', { x: 0, y: 0 }, { ...{ snap: false, fromCentre: true }, spacing: GRID_SPACING }), { x: 5, y: 10, width: 110, height: 100 });
    assert.deepEqual(elementGeometry.resizedRectWithKeys(rect, 'se', { x: 400, y: 300 }, { ...{ snap: false, ratio: true }, spacing: GRID_SPACING }), { x: 10, y: 20, width: 390, height: 312 });
    assert.deepEqual(elementGeometry.resizedRectWithKeys(rect, 'se', { x: 400, y: 300 }, { ...{ snap: false, ratio: true, fromCentre: true }, spacing: GRID_SPACING }), { x: -135, y: -96, width: 390, height: 312 });
    assert.deepEqual(elementGeometry.drawRectWithKeys({ x: 100, y: 100 }, { x: 143, y: 121 }, { ...{ snap: true }, spacing: GRID_SPACING }), { x: 100, y: 100, width: 44, height: 20 });
    assert.deepEqual(elementGeometry.drawRectWithKeys({ x: 100, y: 100 }, { x: 140, y: 120 }, { ...{ square: true }, spacing: GRID_SPACING }), { x: 100, y: 100, width: 40, height: 40 });
    assert.deepEqual(elementGeometry.drawRectWithKeys({ x: 100, y: 100 }, { x: 140, y: 120 }, { ...{ fromCentre: true }, spacing: GRID_SPACING }), { x: 60, y: 80, width: 80, height: 40 });
    assert.deepEqual(elementGeometry.drawRectWithKeys({ x: 100, y: 100 }, { x: 140, y: 120 }, { ...{ square: true, fromCentre: true }, spacing: GRID_SPACING }), { x: 60, y: 60, width: 80, height: 80 });
    assert.deepEqual(elementGeometry.axisLockedDelta({ x: 10, y: 3 }), { x: 10, y: 0 });
    assert.deepEqual(elementGeometry.axisLockedDelta({ x: 2, y: -9 }), { x: 0, y: -9 });
    const snapped = elementGeometry.snappedLineEnd({ x: 0, y: 0 }, { x: 10, y: 3 }, 15);
    const length = Math.hypot(10, 3);
    assert.ok(Math.abs(snapped.x - Math.cos(Math.PI / 12) * length) < 1e-9);
    assert.ok(Math.abs(snapped.y - Math.sin(Math.PI / 12) * length) < 1e-9);
    assert.deepEqual(elementGeometry.snappedLineEnd({ x: 0, y: 0 }, { x: 10, y: 0 }), { x: 10, y: 0 });
  });

  await test('element labels and pictures read the project store', () => {
    const stored = makeElement('image', { kind: 'image', image: 'pic', text: 'Alt text' });
    assert.equal(elementGeometry.pictureUri({ pictures: { pic: 'data:image/png;base64,abc' } }, 'pic'), 'data:image/png;base64,abc');
    assert.equal(elementGeometry.pictureUri({ importedIdentity: { assets: [{ id: 'a', dataUri: 'data:image/png;base64,a', name: 'Logo' }] } }, 'a'), 'data:image/png;base64,a');
    assert.equal(elementGeometry.pictureUri({}, 'gone'), null);
    assert.equal(elementGeometry.pictureUri({}, null), null);
    assert.equal(elementGeometry.pictureName({ importedIdentity: { assets: [{ id: 'a', dataUri: '', name: 'Logo' }] } }, 'a'), 'Logo');
    assert.equal(elementGeometry.pictureName({}, 'abcdefghijklmnop'), 'Picture abcdefgh');
    assert.equal(elementFacts.elementLabel(stored), 'Alt text');
    assert.equal(elementFacts.elementLabel(makeElement('a', { text: 'Line one\nLine two' })), 'Line one');
    assert.equal(elementFacts.elementLabel(makeElement('a', { kind: 'code', language: 'css', text: '' })), 'css code');
    assert.equal(elementFacts.elementLabel(makeElement('a', { kind: 'ellipse', text: '' })), 'Ellipse');
    assert.equal(elementGeometry.elementKindLabel(makeElement('a', { kind: 'input', inputType: 'email' })), 'email input');
    assert.equal(elementGeometry.elementKindLabel(makeElement('a', { kind: 'ellipse' })), 'Ellipse');
  });

  // -------------------------------------------------------------------------
  // minimap.ts
  // -------------------------------------------------------------------------

  await test('the minimap fits content, maps the viewport and hides when everything fits', () => {
    const boundary = { pageId: 'p', sizeId: 's', label: 'Desktop', kind: 'page', rect: { x: 100, y: 50, width: 400, height: 300 } };
    const reference = [{ id: 'r', rect: { x: -80, y: 20, width: 100, height: 80 }, kind: { kind: 'note', text: 'Note' } }];
    const content = minimap.overviewRects([boundary], reference);
    assert.deepEqual(content, [boundary.rect, reference[0].rect]);

    const camera = { x: 0, y: 0, zoom: 1 };
    assert.deepEqual(minimap.minimapWorld(content, camera, { width: 800, height: 600 }), { x: -80, y: 20, width: 580, height: 330 });
    const empty = minimap.minimapWorld([], camera, { width: 800, height: 600 });
    assert.equal(Math.abs(empty.x), 0);
    assert.equal(Math.abs(empty.y), 0);
    assert.equal(empty.width, 800);
    assert.equal(empty.height, 600);

    assert.equal(minimap.contentFitsViewport(content, { x: -100, y: 0, width: 700, height: 500 }), true);
    assert.equal(minimap.contentFitsViewport(content, { x: 0, y: 0, width: 200, height: 100 }), false);
    assert.equal(minimap.contentFitsViewport([], { x: 0, y: 0, width: 1, height: 1 }), true);

    const world = { x: -80, y: 20, width: 580, height: 330 };
    const transform = minimap.minimapTransform(world);
    assert.ok(transform.scale > 0);
    const inside = minimap.minimapPoint(transform, { x: 0, y: 0 });
    assert.deepEqual(minimap.minimapWorldPoint(transform, inside), { x: 0, y: 0 });
    assert.ok(inside.x >= 0 && inside.x <= minimap.MINIMAP_WIDTH);
    assert.ok(inside.y >= 0 && inside.y <= minimap.MINIMAP_HEIGHT);

    assert.deepEqual(minimap.cameraCentredOn({ x: 50, y: 25 }, { x: 10, y: 20, zoom: 2 }, { width: 800, height: 600 }), { x: 300, y: 250, zoom: 2 });
  });

  // -------------------------------------------------------------------------
  // shortcuts.ts
  // -------------------------------------------------------------------------

  await test('no two keymap bindings can fire for the same key event', () => {
    const keyToCode = {
      '=': 'Equal', '+': 'Equal', '-': 'Minus', '_': 'Minus', ']': 'BracketRight', '[': 'BracketLeft',
      '0': 'Digit0', ' ': 'Space', Enter: 'Enter', Delete: 'Delete', Backspace: 'Backspace', Tab: 'Tab',
      F1: 'F1', F2: 'F2', ArrowLeft: 'ArrowLeft', ArrowRight: 'ArrowRight', ArrowUp: 'ArrowUp', ArrowDown: 'ArrowDown',
    };
    const eventFor = (combo, shift) => {
      const key = combo.key ?? 'Unidentified';
      const code = combo.code ?? keyToCode[key] ?? (/^[a-z]$/i.test(key) ? `Key${key.toUpperCase()}` : undefined);
      return {
        key, code, ctrlKey: !!combo.mod, metaKey: false, altKey: !!combo.alt, shiftKey: shift,
      };
    };
    const matches = (combo, event) => {
      const command = event.ctrlKey || event.metaKey;
      if (!!combo.mod !== command) return false;
      if (!!combo.alt !== event.altKey) return false;
      if (combo.shift !== undefined && combo.shift !== event.shiftKey) return false;
      if (combo.code && event.code === combo.code) return true;
      if (combo.key !== undefined) {
        const key = event.key === 'Unidentified' && event.code === 'Tab' ? 'Tab' : event.key;
        if (key.toLowerCase() === combo.key.toLowerCase()) return true;
      }
      return false;
    };
    const conflicts = [];
    const seen = new Set();
    for (const binding of shortcuts.KEYMAP) {
      for (const combo of binding.keys) {
        for (const shift of combo.shift === undefined ? [false, true] : [combo.shift]) {
          const event = eventFor(combo, shift);
          const key = JSON.stringify(event);
          if (seen.has(key)) continue;
          seen.add(key);
          // Text-layer editing and everything else are separate contexts, as in shortcutFor.
          for (const active of [other => !other.scope && other.inText, other => !other.scope && !other.textOnly]) {
            const fired = shortcuts.KEYMAP.filter(other => active(other) && other.keys.some(candidate => matches(candidate, event)));
            if (new Set(fired.map(other => other.action)).size > 1) {
              conflicts.push(`${key} → ${fired.map(other => other.action).join(', ')}`);
            }
          }
        }
      }
    }
    assert.deepEqual(conflicts, []);
  });

  await test('shortcutFor respects text fields, composition and prevented events', () => {
    const event = (patch) => ({ key: 'k', code: 'KeyK', ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, ...patch });
    assert.equal(shortcuts.shortcutFor(event({ ctrlKey: true }), false), 'askAI');
    assert.equal(shortcuts.shortcutFor(event({ metaKey: true }), false), 'askAI');
    assert.equal(shortcuts.shortcutFor(event({ ctrlKey: true }), true), null, 'text fields keep Ctrl+K');
    assert.equal(shortcuts.shortcutFor(event({ ctrlKey: true, key: 's', code: 'KeyS' }), true), 'save', 'save works in text fields');
    assert.equal(shortcuts.shortcutFor(event({ ctrlKey: true, defaultPrevented: true }), false), null);
    assert.equal(shortcuts.shortcutFor(event({ ctrlKey: true, isComposing: true }), false), null);
    assert.equal(shortcuts.shortcutFor(event({ key: 'Enter', code: 'Enter' }), false), 'selectChild');
    assert.equal(shortcuts.shortcutFor(event({ key: 'Enter', code: 'Enter', shiftKey: true }), false), 'selectParent');
    assert.equal(shortcuts.shortcutFor(event({ key: 'Unidentified', code: 'Tab', shiftKey: true }), false), null);
    assert.equal(shortcuts.shortcutFor(event({ key: 'Tab', code: 'Tab' }), false), null, 'Tab keeps normal focus movement');
    assert.equal(shortcuts.shortcutFor(event({ key: '\\', code: 'Backslash', ctrlKey: true }), false), 'hidePanels');
    assert.equal(shortcuts.shortcutFor(event({ key: 'c', code: 'KeyC', ctrlKey: true, altKey: true }), false), 'copyStyle');
    assert.equal(shortcuts.shortcutFor(event({ key: 'v', code: 'KeyV', ctrlKey: true, altKey: true }), false), 'pasteStyle');
    assert.equal(shortcuts.shortcutFor(event({ key: 'c', code: 'KeyC', ctrlKey: true, altKey: true, shiftKey: true }), false), 'paintFormat');
    assert.equal(shortcuts.shortcutFor(event({ key: '1', code: 'Digit1', ctrlKey: true, altKey: true }), false), 'saveView');
    assert.equal(shortcuts.shortcutFor(event({ key: '9', code: 'Digit9', ctrlKey: true, altKey: true }), false), 'saveView');
    assert.equal(shortcuts.shortcutFor(event({ key: '4', code: 'Digit4', altKey: true }), false), 'goToView');
    assert.equal(shortcuts.shortcutFor(event({ key: 'm', code: 'KeyM', shiftKey: true }), false), 'minimap');
    assert.equal(shortcuts.shortcutFor(event({ key: '1', code: 'Digit1', ctrlKey: true, altKey: true, shiftKey: true }), false), null, 'Save view ignores Shift');
    assert.equal(shortcuts.viewSlot({ key: '7', code: 'Digit7' }), 7);
    assert.equal(shortcuts.viewSlot({ key: '3', code: 'Numpad3' }), 3);
    assert.equal(shortcuts.viewSlot({ key: '0', code: 'Digit0' }), null);
    assert.equal(shortcuts.viewSlot({ key: 'Unidentified', code: 'Digit2' }), 2);
    assert.equal(shortcuts.viewSlot({ key: 'a', code: 'KeyA' }), null);
    assert.equal(shortcuts.shortcutFor(event({ key: 'q', code: 'KeyQ' }), false), null);
    assert.equal(shortcuts.shortcutLabel('not-an-action'), '');
    assert.equal(shortcuts.withShortcut('Request', 'askAI'), 'Request · Ctrl/Cmd+K');
    assert.ok(shortcuts.SHORTCUT_GROUPS.find(group => group.group === 'Tools').items.length > 0);
  });
} finally {
  await modules.cleanup();
}
