import assert from 'node:assert/strict';
import test from 'node:test';
import { compileModules } from './typescript.mjs';

const modules = await compileModules(['src/projectCanvas/behaviours.ts']);
try {
  const {
    isBehaviour, behaviourFor, behaviourCommand, behaviourStateLabel, playShouldPreview,
  } = await modules.load('projectCanvas/behaviours.js');

  const page = (id = 'home', output = 'webpage') => ({ id, output });
  const project = (pointers, pages = [page()]) => ({ pages, pointers });
  const elementSource = (pageId = 'home', elementId = 'a', sizeId = null) => ({ kind: 'element', pageId, elementId, sizeId });
  const boundarySource = (pageId = 'home', sizeId = null) => ({ kind: 'boundary', pageId, sizeId });
  const pointer = (patch = {}) => {
    const from = patch.from ?? elementSource();
    return { id: 'behaviour', kind: 'change', from, to: { ...from }, comment: 'Rotates to point at the mouse', wiring: null, ...patch };
  };

  await test('a change pointer back to its own layer or boundary is a behaviour', () => {
    assert.equal(isBehaviour(pointer()), true);
    assert.equal(isBehaviour(pointer({ from: elementSource('home', 'a', 'desktop') })), true);
    assert.equal(isBehaviour(pointer({ from: boundarySource() })), true);
    // Sizes are overrides of one layer, not a different source.
    const sized = pointer({ from: elementSource('home', 'a', 'desktop'), to: elementSource('home', 'a', 'phone') });
    assert.equal(isBehaviour(sized), true);
  });

  await test('a pointer to anything else is not a behaviour', () => {
    assert.equal(isBehaviour(pointer({ to: elementSource('home', 'b') })), false);
    assert.equal(isBehaviour(pointer({ to: elementSource('other', 'a') })), false);
    assert.equal(isBehaviour(pointer({ to: boundarySource('other') })), false);
    assert.equal(isBehaviour(pointer({ from: elementSource(), to: boundarySource() })), false);
    assert.equal(isBehaviour(pointer({ from: boundarySource(), to: elementSource() })), false);
    assert.equal(isBehaviour(pointer({ to: { kind: 'reference', itemId: 'note' } })), false);
    assert.equal(isBehaviour(pointer({ to: { kind: 'url', url: 'https://example.com' } })), false);
    assert.equal(isBehaviour(pointer({ kind: 'reference' })), false);
  });

  await test('behaviourFor finds the layer or boundary behaviour and nothing else', () => {
    const other = pointer({ id: 'other', from: elementSource('home', 'b'), to: elementSource('home', 'b') });
    const line = pointer({ id: 'line', from: elementSource('home', 'a'), to: elementSource('home', 'b') });
    const layer = pointer({ id: 'layer' });
    const boundary = pointer({ id: 'boundary', from: boundarySource() });
    const design = project([other, line, layer, boundary]);
    assert.equal(behaviourFor(design, elementSource('home', 'a', 'phone'))?.id, 'layer');
    assert.equal(behaviourFor(design, boundarySource())?.id, 'boundary');
    assert.equal(behaviourFor(design, elementSource('home', 'c')), null);
    assert.equal(behaviourFor(project(undefined), boundarySource()), null);
    assert.equal(behaviourFor(project([]), boundarySource()), null);
  });

  await test('typing, changing and clearing a behaviour is one add, update or remove', () => {
    assert.equal(behaviourCommand(null, elementSource(), '   '), null);
    const added = behaviourCommand(null, elementSource(), '  Follows the mouse  ');
    assert.equal(added.type, 'add_pointer');
    assert.equal(added.pointer.kind, 'change');
    assert.equal(added.pointer.comment, 'Follows the mouse');
    assert.deepEqual(added.pointer.from, elementSource());
    assert.deepEqual(added.pointer.to, elementSource());
    assert.equal(added.pointer.wiring, null);
    assert.equal(typeof added.pointer.id, 'string');
    assert.ok(added.pointer.id.length > 0);

    assert.equal(behaviourCommand(pointer(), boundarySource(), 'Rotates to point at the mouse'), null);
    const updated = behaviourCommand(pointer(), elementSource(), 'Fades in on scroll');
    assert.deepEqual(updated, { type: 'update_pointer', pointer_id: 'behaviour', comment: 'Fades in on scroll' });
    const removed = behaviourCommand(pointer(), elementSource(), '');
    assert.deepEqual(removed, { type: 'remove_pointers', ids: ['behaviour'] });
    assert.equal(behaviourCommand(null, elementSource(), ''), null);
  });

  await test('the dock says Built or Runs after Preview or Export and nothing for clashes', () => {
    assert.equal(behaviourStateLabel('wired'), 'Built');
    assert.equal(behaviourStateLabel('needsExporting'), 'Runs after Preview or Export');
    assert.equal(behaviourStateLabel('needsGenerating'), 'Runs after Preview or Export');
    assert.equal(behaviourStateLabel('unwired'), 'Runs after Preview or Export');
    for (const clash of ['codeChanged', 'contradicts', 'broken']) assert.equal(behaviourStateLabel(clash), '');
  });

  await test('Play previews a web or app screen that carries behaviours or non-simple changes', () => {
    assert.equal(playShouldPreview(project([]), 'home'), false);
    assert.equal(playShouldPreview(project([pointer()]), 'home'), true);
    assert.equal(playShouldPreview(project([pointer({ from: boundarySource() })]), 'home'), true);
    assert.equal(playShouldPreview(project([pointer(), page('home', 'appScreen')]), 'home'), true);
    // A layer on another boundary is not the boundary Play would run.
    assert.equal(playShouldPreview(project([pointer({ from: elementSource('other', 'a'), to: elementSource('other', 'a') })], [page('home'), page('other')]), 'home'), false);
    // Slides and images render without code, so Play keeps its own behaviour.
    assert.equal(playShouldPreview(project([pointer()], [page('home', 'slide')]), 'home'), false);
    assert.equal(playShouldPreview(project([pointer()], [page('home', 'image')]), 'home'), false);
    assert.equal(playShouldPreview(project([pointer()]), 'missing'), false);
  });

  await test('a change pointer without a simple Play action also previews', () => {
    const change = pointer({ id: 'change', from: elementSource(), to: elementSource('home', 'b') });
    assert.equal(playShouldPreview(project([change]), 'home'), true);
    assert.equal(playShouldPreview(project([{ ...change, play: { kind: 'goTo', boundary: 'about' } }]), 'home'), false);
    // Reading reference is not a change, so Play stays.
    assert.equal(playShouldPreview(project([pointer({ kind: 'reference', to: { kind: 'reference', itemId: 'note' } })]), 'home'), false);
    // A behaviour still previews even if generation also classified a Play action for it.
    const behaviour = pointer({ play: { kind: 'toggle', element: 'menu' } });
    assert.equal(playShouldPreview(project([behaviour]), 'home'), true);
  });
} finally {
  await modules.cleanup();
}
