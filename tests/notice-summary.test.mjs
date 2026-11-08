import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import test from 'node:test';

const temporary = await mkdtemp(new URL('../.notice-summary-', import.meta.url));
try {
  const compiler = new URL('bin/tsc', import.meta.resolve('typescript/package.json')).pathname;
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022', '--outDir', temporary, 'src/noticeSummary.ts'], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const { summarizePageChanges } = await import(pathToFileURL(join(temporary, 'noticeSummary.js')));

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
  await rm(temporary, { recursive: true, force: true });
}
