import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { join } from 'node:path';
import { compileModules, root } from './typescript.mjs';

// Rust writes tests/fixtures/resolution.json (desegnos-core/tests/resolution_fixture.rs); the
// canvas's own resolver must produce the same trees as the core's exports and AI requests do.
const modules = await compileModules(['src/projectCanvas/sizes.ts']);
try {
  const { resolveForSize } = await modules.load('projectCanvas/sizes.js');
  const { project, cases } = JSON.parse(await readFile(join(root, 'tests/fixtures/resolution.json'), 'utf8'));
  const withoutNullLinks = value => JSON.parse(JSON.stringify(value, (key, item) => (key === 'link' && item === null ? undefined : item)));
  for (const { pageId, sizeId, elements } of cases) {
    await test(`resolved elements for size ${sizeId} match the core`, () => {
      const page = project.pages.find(item => item.id === pageId);
      assert.deepEqual(withoutNullLinks(resolveForSize(structuredClone(project), page, sizeId)), withoutNullLinks(elements));
    });
  }
} finally {
  await modules.cleanup();
}
