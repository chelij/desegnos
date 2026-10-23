import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
const temporary = await mkdtemp(new URL('../.draft-review-', import.meta.url));
try {
  const compiler = new URL('bin/tsc', import.meta.resolve('typescript/package.json')).pathname;
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022', '--outDir', temporary, 'src/generationDraftState.ts'], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const { pendingDrafts } = await import(pathToFileURL(join(temporary, 'generationDraftState.js')));
  await test('draft counts deduplicate repair copies and exclude applied or dismissed results', () => {
    const draft = { id: 'one', pageId: 'page', code: 'saved output' };
    const job = { drafts: [draft, { ...draft, id: 'copy' }, { ...draft, id: 'done', code: 'done', applied: true }, { ...draft, id: 'hidden', code: 'hidden', dismissed: true }] };
    assert.deepEqual(pendingDrafts(job), [draft]);
    assert.deepEqual(pendingDrafts(null), []);
  });
} finally { await rm(temporary, { recursive: true, force: true }); }
