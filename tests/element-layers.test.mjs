import assert from 'node:assert/strict';
import test from 'node:test';
import { compileModules } from './typescript.mjs';

const modules = await compileModules(['src/projectCanvas/elementLayers.tsx']);
try {
  const { layerDropPosition, layerDropTarget, resolveLayerDrop } = await modules.load('projectCanvas/elementLayers.js');
  const layer = (id, kind, children = []) => ({ id, kind, name: id, text: '', language: '', visible: true, locked: false, children });
  const source = (parentId, index, id = 'x') => ({ pageId: 'page', sizeId: 'size', id, parentId, index });
  const target = (parentId, index) => ({ pageId: 'page', sizeId: 'size', parentId, index });

  await test('a layer row drops before, after, or into a group', () => {
    const group = layer('group', 'group', [layer('a', 'text'), layer('b', 'text')]);
    assert.equal(layerDropPosition(group, 0), 'before');
    assert.equal(layerDropPosition(group, 0.3), 'before');
    assert.equal(layerDropPosition(group, 0.5), 'into');
    assert.equal(layerDropPosition(group, 1), 'after');
    // Boxes hold children as well.
    assert.equal(layerDropPosition(layer('box', 'box'), 0.5), 'into');
    // A leaf only ever splits into before and after.
    const text = layer('text', 'text');
    assert.equal(layerDropPosition(text, 0.49), 'before');
    assert.equal(layerDropPosition(text, 0.5), 'after');
    assert.equal(layerDropPosition(text, 0.1), 'before');
  });

  await test('a row offset maps to the model drop target', () => {
    const group = layer('group', 'group', [layer('a', 'text'), layer('b', 'text')]);
    assert.deepEqual(layerDropTarget(group, 0.5, 'page', 'size', 'parent', 3), { pageId: 'page', sizeId: 'size', parentId: 'group', index: 2 });
    // Rows are drawn in reverse model order, so the row above index 3 inserts after it.
    assert.deepEqual(layerDropTarget(group, 0.1, 'page', 'size', 'parent', 3), { pageId: 'page', sizeId: 'size', parentId: 'parent', index: 4 });
    assert.deepEqual(layerDropTarget(layer('text', 'text'), 0.9, 'page', 'size', null, 0), { pageId: 'page', sizeId: 'size', parentId: null, index: 0 });
  });

  await test('dropping resolves sibling order, group moves, and self-drops', () => {
    // Downward within a parent: the grabbed layer still occupies its old slot.
    assert.deepEqual(resolveLayerDrop(source(null, 1), target(null, 4)), { parentId: null, index: 3 });
    // Upward within a parent lands on the target index.
    assert.deepEqual(resolveLayerDrop(source(null, 4), target(null, 1)), { parentId: null, index: 1 });
    // Out of a group and into one.
    assert.deepEqual(resolveLayerDrop(source('group', 2), target(null, 0)), { parentId: null, index: 0 });
    assert.deepEqual(resolveLayerDrop(source(null, 2), target('group', 0)), { parentId: 'group', index: 0 });
    // A layer cannot become a child of itself, and another page or size is not a drop.
    assert.equal(resolveLayerDrop(source(null, 1, 'group'), target('group', 0)), null);
    assert.equal(resolveLayerDrop({ ...source(null, 1), pageId: 'other' }, target(null, 0)), null);
    assert.equal(resolveLayerDrop({ ...source(null, 1), sizeId: 'other' }, target(null, 0)), null);
  });
} finally {
  await modules.cleanup();
}
