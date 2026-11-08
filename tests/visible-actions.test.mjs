import assert from 'node:assert/strict';
import test from 'node:test';
import { compileModules } from './typescript.mjs';

const modules = await compileModules(['src/visibleActions.ts']);
try {
  const { projectHasPreviewPages, showsOpener } = await modules.load('visibleActions.js');

  await test('an opener is shown only while the surface it opens is closed', () => {
    assert.equal(showsOpener({ available: true, open: false }), true);
    assert.equal(showsOpener({ available: true, open: true }), false, 'the open surface has its own Close');
    assert.equal(showsOpener({ available: false, open: false }), false);
    assert.equal(showsOpener({ available: false, open: true }), false);
  });

  await test('Preview is offered only while a project has web output', () => {
    const project = outputs => ({ pages: outputs.map((output, index) => ({ id: `page-${index}`, output })) });
    assert.equal(projectHasPreviewPages(null), false);
    assert.equal(projectHasPreviewPages({ pages: [] }), false);
    assert.equal(projectHasPreviewPages(project(['slide', 'image'])), false);
    assert.equal(projectHasPreviewPages(project(['image', 'webpage'])), true);
    assert.equal(projectHasPreviewPages(project(['webpage'])), true);
  });

} finally {
  await modules.cleanup();
}
