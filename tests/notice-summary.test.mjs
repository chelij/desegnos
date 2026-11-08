import assert from 'node:assert/strict';
import test from 'node:test';
import { compileModules } from './typescript.mjs';

const modules = await compileModules(['src/noticeSummary.ts']);
try {
  const { summarizePageChanges } = await modules.load('noticeSummary.js');

  await test('page edits appear in the changed-items summary', () => {
    const candidate = { requests: [{ status: 'implemented', operations: [
      { kind: 'setPage', pageId: 'home', name: 'Home', path: '/', background: '#05060b' },
      { kind: 'setPage', pageId: 'lineup', name: 'Lineup', path: '/lineup', background: null },
    ] }] };
    const summary = summarizePageChanges(candidate, { pages: [] });
    assert.match(summary, /Home: name, path \/ and background #05060b/);
    assert.match(summary, /Lineup: name and path \/lineup/);
  });
} finally {
  await modules.cleanup();
}
