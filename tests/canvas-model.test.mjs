import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { join } from 'node:path';
import { compileModules, root } from './typescript.mjs';

const modules = await compileModules([
  'src/projectCanvas/sizes.ts',
  'src/projectCanvas/links.ts',
  'src/projectCanvas/elementGeometry.ts',
  'src/shortcuts.ts',
]);
try {
  const sizes = await modules.load('projectCanvas/sizes.js');
  const links = await modules.load('projectCanvas/links.js');
  const elementGeometry = await modules.load('projectCanvas/elementGeometry.js');
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
  const makePage = (id, name, path, sizes, elements) => ({ id, name, path, included: true, output: 'webpage', sizes, appearance: 'light', elements });
  const makeProject = (pages = [], patch = {}) => ({
    schemaVersion: 14, id: 'project', name: 'Test', revision: 1,
    viewport: { width: 800, height: 600 }, appearance: 'light', showGrid: true,
    gridSettings: { spacing: 8, unit: 'pixels', snap: true, rulers: true }, generationMode: 'fuzzy',
    homePageId: pages[0]?.id ?? 'home', pages, reference: [], pointers: [], linked: {},
    brief: { purpose: '', audience: '', brandName: '', tone: '' }, latestGeneration: null,
    decisions: [], exceptions: [], appliedJobIds: [], extensions: {}, ...patch,
  });

  // -------------------------------------------------------------------------
  // sizes.ts
  // -------------------------------------------------------------------------

  await test('size helpers read the base size and name the boundary', () => {
    const page = makePage('p', 'Home', '/', [makeSize('base', 'Desktop', 1440, 900), makeSize('phone', 'Phone', 390, 844, 'screen')], []);
    assert.equal(sizes.baseSize(page).id, 'base');
    assert.equal(sizes.baseSizeId(page), 'base');
    assert.equal(sizes.hasManySizes(page), true);
    assert.equal(sizes.hasManySizes(makePage('one', 'One', '/one', [makeSize('only', 'Desktop', 800, 600)], [])), false);
    assert.equal(sizes.findSize(page, 'phone').label, 'Phone');
    assert.equal(sizes.findSize(page, 'gone'), null);
    assert.equal(sizes.findSize(page, null), null);
    assert.equal(sizes.isBaseSize(page, 'base'), true);
    assert.equal(sizes.isBaseSize(page, 'phone'), false);
    assert.equal(sizes.isBaseSize(page, null), false);
    assert.equal(sizes.presetLabel('phone'), 'Phone');
    assert.equal(sizes.presetLabel('tablet'), 'Tablet');
    assert.equal(sizes.presetLabel('desktop'), 'Desktop');
    assert.equal(sizes.presetLabel('custom', 100, 200), '100 × 200');
    assert.equal(sizes.boundaryName(page, page.sizes[1]), 'Home · Phone');
    assert.equal(sizes.boundaryName(makePage('one', 'One', '/one', [makeSize('only', 'Desktop', 800, 600)], []), makeSize('only', 'Desktop', 800, 600)), 'One');
    assert.deepEqual(sizes.SIZE_PRESETS.map(preset => [preset.preset, preset.width, preset.height]), [
      ['phone', 390, 844], ['tablet', 820, 1180], ['desktop', 1440, 900],
    ]);
    assert.equal(sizes.sameSizeRect(page.sizes[0], { ...page.sizes[0] }), true);
    assert.equal(sizes.sameSizeRect(page.sizes[0], page.sizes[1]), false);
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
    assert.equal(sizes.hiddenInSize(child, 'phone'), true);
    assert.equal(sizes.placeholderInSize(child, 'phone'), true);
    assert.equal(sizes.placeholderInSize(child, 'base'), false);
    child.overrides.phone.dismissed = true;
    assert.equal(sizes.placeholderInSize(child, 'phone'), false);
    assert.deepEqual(sizes.overrideFor(child, 'phone'), child.overrides.phone);
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
    assert.equal(elementGeometry.ELEMENT_GRID, 8);
    assert.equal(elementGeometry.MIN_ELEMENT_SIZE, 1);
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
    assert.equal(elementGeometry.snapValue(13), 16);
    assert.deepEqual(elementGeometry.snapPoint({ x: 4, y: 12 }), { x: 8, y: 16 });
    assert.equal(elementGeometry.sameRect({ x: 0, y: 0, width: 1, height: 1 }, { x: 0, y: 0, width: 1, height: 1 }), true);
    assert.equal(elementGeometry.sameRect({ x: 0, y: 0, width: 1, height: 1 }, { x: 0, y: 0, width: 2, height: 1 }), false);
    assert.deepEqual(elementGeometry.unionRects([]), { x: 0, y: 0, width: 0, height: 0 });
    assert.deepEqual(elementGeometry.unionRects([{ x: 0, y: 0, width: 10, height: 10 }, { x: 20, y: 5, width: 10, height: 10 }]), { x: 0, y: 0, width: 30, height: 15 });
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
    assert.equal(elementGeometry.findElementIn(page.elements, 'grandchild').text, 'Deep');
    assert.equal(elementGeometry.findElementIn(page.elements, 'gone'), null);
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
    assert.deepEqual(elementGeometry.elementSiblings(project, 'p', 'grandchild').map(item => item.id), ['grandchild']);
    assert.deepEqual(elementGeometry.elementSiblings(project, 'p', 'child').map(item => item.id), ['child']);
    assert.deepEqual(elementGeometry.elementWorldRect(project, 'p', 'grandchild', { x: 1000, y: 2000 }), { x: 1115, y: 2226, width: 30, height: 20 });
    assert.deepEqual(elementGeometry.elementParentWorldOrigin(project, 'p', 'grandchild', { x: 1000, y: 2000 }), { x: 1110, y: 2220 });
    assert.equal(elementGeometry.elementWorldRect(project, 'p', 'gone', { x: 0, y: 0 }), null);
    assert.deepEqual(elementGeometry.elementLocation(project, 'p', 'grandchild'), { pageId: 'p', parentId: 'child', index: 0, element: grandchild });
    assert.equal(elementGeometry.anyElementLocation(project, 'other').pageId, 'p');
    assert.equal(elementGeometry.anyElementLocation(project, 'gone'), null);
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
    assert.equal(elementGeometry.findElementPage(project, 'copy').id, 'p');
    assert.equal(elementGeometry.findElementPage(project, 'gone'), null);
    assert.equal(elementGeometry.pageElements(project, 'gone').length, 0);
  });

  await test('move and resize math snaps, anchors and keeps a minimum size', () => {
    assert.deepEqual(elementGeometry.movedElementRect({ x: 10, y: 13, width: 30, height: 30 }, { x: 5, y: 6 }, true), { x: 16, y: 16, width: 30, height: 30 });
    assert.deepEqual(elementGeometry.movedElementRect({ x: 10, y: 13, width: 30, height: 30 }, { x: 5, y: 6 }, false), { x: 15, y: 19, width: 30, height: 30 });
    assert.deepEqual(elementGeometry.draggedElementRect({ x: 10, y: 10, width: 30, height: 30 }, { x: 5, y: 5 }, { x: 20, y: 20 }, false), { x: 25, y: 25, width: 30, height: 30 });
    const rect = { x: 10, y: 20, width: 100, height: 80 };
    assert.deepEqual(elementGeometry.resizedElementRect(rect, 'se', { x: 150, y: 140 }, false), { x: 10, y: 20, width: 140, height: 120 });
    assert.deepEqual(elementGeometry.resizedElementRect(rect, 'nw', { x: 0, y: 0 }, false), { x: 0, y: 0, width: 110, height: 100 });
    assert.deepEqual(elementGeometry.resizedElementRect(rect, 'e', { x: 150, y: 400 }, false), { x: 10, y: 20, width: 140, height: 80 });
    assert.deepEqual(elementGeometry.resizedElementRect(rect, 'n', { x: 0, y: 0 }, false), { x: 10, y: 0, width: 100, height: 100 });
    assert.deepEqual(elementGeometry.resizedElementRect(rect, 'se', { x: -100, y: -100 }, false), { x: 10, y: 20, width: 1, height: 1 });
    assert.deepEqual(elementGeometry.resizedElementRect(rect, 'nw', { x: 500, y: 500 }, false), { x: 109, y: 99, width: 1, height: 1 });
    assert.deepEqual(elementGeometry.ratioResizedRect(rect, 'se', { x: 400, y: 300 }, false), { x: 10, y: 20, width: 390, height: 312 });
    assert.equal(elementGeometry.layoutChildren({ kind: 'free' }), false);
    assert.equal(elementGeometry.layoutChildren({ kind: 'stack', direction: 'row', gap: 0, padding: 0, align: 'start', justify: 'start', wrap: false }), true);
    assert.deepEqual(elementGeometry.rectFromPoints({ x: 30, y: 40 }, { x: 10, y: 20 }), { x: 10, y: 20, width: 20, height: 20 });
    const snappedLine = elementGeometry.lineEnd({ x: 0, y: 0 }, { x: 10, y: 3 }, true);
    assert.ok(Math.abs(snappedLine.y) < 1e-9);
    assert.ok(Math.abs(snappedLine.x - Math.hypot(10, 3)) < 1e-9);
    assert.deepEqual(elementGeometry.lineEnd({ x: 0, y: 0 }, { x: 10, y: 3 }, false), { x: 10, y: 3 });
    const boundaryOrigin = { x: 100, y: 200 };
    const parentOrigin = { x: 50, y: 60 };
    assert.deepEqual(elementGeometry.elementCanvasRect(rect, boundaryOrigin, parentOrigin), { x: 160, y: 280, width: 100, height: 80 });
    assert.deepEqual(elementGeometry.elementLocalRect({ x: 160, y: 280, width: 100, height: 80 }, boundaryOrigin, parentOrigin), rect);
    assert.deepEqual(elementGeometry.reparentedElementRect(rect, boundaryOrigin, parentOrigin, boundaryOrigin, { x: 0, y: 0 }), { x: 60, y: 80, width: 100, height: 80 });
    const world = elementGeometry.elementCanvasRect(rect, boundaryOrigin, parentOrigin);
    assert.deepEqual(elementGeometry.elementLocalRect(world, boundaryOrigin, { x: 0, y: 0 }), { x: 60, y: 80, width: 100, height: 80 });
  });

  await test('element labels and pictures read the project store', () => {
    const stored = makeElement('image', { kind: 'image', image: 'pic', text: 'Alt text' });
    assert.equal(elementGeometry.pictureUri({ pictures: { pic: 'data:image/png;base64,abc' } }, 'pic'), 'data:image/png;base64,abc');
    assert.equal(elementGeometry.pictureUri({ importedIdentity: { assets: [{ id: 'a', dataUri: 'data:image/png;base64,a', name: 'Logo' }] } }, 'a'), 'data:image/png;base64,a');
    assert.equal(elementGeometry.pictureUri({}, 'gone'), null);
    assert.equal(elementGeometry.pictureUri({}, null), null);
    assert.equal(elementGeometry.pictureName({ importedIdentity: { assets: [{ id: 'a', dataUri: '', name: 'Logo' }] } }, 'a'), 'Logo');
    assert.equal(elementGeometry.pictureName({}, 'abcdefghijklmnop'), 'Picture abcdefgh');
    assert.equal(elementGeometry.elementLabel(stored), 'Alt text');
    assert.equal(elementGeometry.elementLabel(makeElement('a', { text: 'Line one\nLine two' })), 'Line one');
    assert.equal(elementGeometry.elementLabel(makeElement('a', { kind: 'code', language: 'css', text: '' })), 'css code');
    assert.equal(elementGeometry.elementLabel(makeElement('a', { kind: 'ellipse', text: '' })), 'Ellipse');
    assert.equal(elementGeometry.elementKindLabel(makeElement('a', { kind: 'input', inputType: 'email' })), 'email input');
    assert.equal(elementGeometry.elementKindLabel(makeElement('a', { kind: 'ellipse' })), 'Ellipse');
    assert.equal(elementGeometry.commonValue([]), undefined);
    assert.equal(elementGeometry.commonValue([1, 1]), 1);
    assert.equal(elementGeometry.commonValue([1, 2]), undefined);
    assert.equal(elementGeometry.commonValue([{ a: 1 }, { a: 1 }]).a, 1);
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
          const fired = shortcuts.KEYMAP.filter(other => other.keys.some(candidate => matches(candidate, event)));
          if (new Set(fired.map(other => other.action)).size > 1) {
            conflicts.push(`${key} → ${fired.map(other => other.action).join(', ')}`);
          }
        }
      }
    }
    assert.deepEqual(conflicts, []);
  });

  await test('every Keyboard entry in CANVAS.md has a binding and display label', async () => {
    const doc = await readFile(join(root, 'CANVAS.md'), 'utf8');
    const section = doc.split('## Keyboard')[1]?.split('\n## ')[0] ?? '';
    const rows = section.split('\n').filter(line => line.trim().startsWith('|')).slice(2)
      .map(line => line.trim().split('|').slice(1, -1).map(cell => cell.trim()));
    const expected = {
      'Select / Frame / Boundary / Pointer / Text / Pan': [['selectTool', 'V'], ['frameTool', 'F'], ['boundaryTool', 'B'], ['pointerTool', 'P'], ['textTool', 'T'], ['panTool', 'H']],
      'Rectangle / Ellipse / Line / Image': [['rectTool', 'R'], ['ellipseTool', 'O'], ['lineTool', 'L'], ['imageTool', 'Shift+Ctrl/Cmd+K']],
      'Note (reference)': [['noteTool', 'N']],
      'Insert any element': [['insert', 'Ctrl/Cmd+I']],
      'Copy / Paste / Paste with choice / Duplicate': [['copy', 'Ctrl/Cmd+C'], ['paste', 'Ctrl/Cmd+V'], ['pasteChoice', 'Shift+Ctrl/Cmd+V'], ['duplicate', 'Ctrl/Cmd+D']],
      'Link / Unlink': [['link', 'Ctrl/Cmd+Alt+K'], ['unlink', 'Ctrl/Cmd+Alt+B']],
      'Group / Ungroup': [['group', 'Ctrl/Cmd+G'], ['ungroup', 'Shift+Ctrl/Cmd+G']],
      'Bring forward / backward': [['forward', 'Ctrl/Cmd+]'], ['backward', 'Ctrl/Cmd+[']],
      'Select all / child / parent': [['selectAll', 'Ctrl/Cmd+A'], ['selectChild', 'Enter'], ['selectParent', 'Shift+Enter']],
      'Nudge / far nudge': [['nudge', 'Arrow keys'], ['nudgeFar', 'Shift+Arrow keys']],
      'Delete': [['delete', 'Delete or Backspace']],
      'Zoom in / out / 100% / fit all / fit selection': [['zoomIn', 'Ctrl/Cmd+='], ['zoomOut', 'Ctrl/Cmd+-'], ['zoomReset', 'Ctrl/Cmd+0'], ['fitAll', 'Shift+1'], ['fitSelection', 'Shift+2']],
      'Play / Preview export': [['play', 'Shift+Space'], ['preview', 'Ctrl/Cmd+Enter']],
      'Hide panels': [['hidePanels', 'Tab']],
      'Ask AI on selection': [['askAI', 'Ctrl/Cmd+K']],
      'Export': [['export', 'Ctrl/Cmd+E']],
      'Undo / Redo': [['undo', 'Ctrl/Cmd+Z'], ['redo', 'Shift+Ctrl/Cmd+Z']],
    };
    assert.ok(rows.length >= 15, 'the Keyboard table was found');
    for (const [action, display] of Object.entries(expected)) {
      assert.ok(rows.some(row => row[0] === action), `CANVAS.md still lists ${action}`);
    }
    for (const row of rows) {
      const bindings = expected[row[0]];
      assert.ok(bindings, `CANVAS.md Keyboard row is mapped: ${row[0]}`);
      for (const [action, display] of bindings) {
        assert.equal(shortcuts.shortcutLabel(action), display, `${action} display`);
        assert.ok(shortcuts.SHORTCUT_HELP.some(item => item[0] === shortcuts.KEYMAP.find(binding => binding.action === action).label && item[1] === display));
        assert.equal(shortcuts.KEYMAP.filter(binding => binding.action === action).length, 1, `${action} has one binding`);
      }
    }
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
    assert.equal(shortcuts.shortcutFor(event({ key: 'Tab', code: 'Tab' }), false), 'hidePanels');
    assert.equal(shortcuts.shortcutFor(event({ key: 'q', code: 'KeyQ' }), false), null);
    assert.equal(shortcuts.shortcutLabel('not-an-action'), '');
    assert.equal(shortcuts.withShortcut('Ask AI', 'askAI'), 'Ask AI · Ctrl/Cmd+K');
    assert.ok(shortcuts.SHORTCUT_GROUPS.find(group => group.group === 'Tools').items.length > 0);
    assert.ok(shortcuts.TOOL_SHORTCUTS.selectTool);
    assert.ok(shortcuts.TOOL_SHORTCUTS.rectangleTool === 'R');
  });
} finally {
  await modules.cleanup();
}
