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
  const { summarizeOutsideChanges, summarizeReportedOutsideChanges, summarizePageChanges } = await import(pathToFileURL(join(temporary, 'noticeSummary.js')));

  await test('reported outside changes name their document once', () => {
    assert.equal(summarizeReportedOutsideChanges([]), '');
    assert.equal(summarizeReportedOutsideChanges([{ document: '', description: ' ' }]), '');
    assert.equal(
      summarizeReportedOutsideChanges([
        { document: 'Home', description: 'Added the tooltip controller.' },
        { document: 'Home', description: 'Added the tooltip controller.' },
        { document: 'Atlas', description: 'Shared its tooltip styles' },
      ]),
      'Also changed outside this area: Home: Added the tooltip controller and Atlas: Shared its tooltip styles',
    );
    assert.equal(
      summarizeReportedOutsideChanges([{ document: '', description: 'Updated the page script' }]),
      'Also changed outside this area: Updated the page script',
    );
  });

  await test('reported changes replace derived labels and blank reports fall back', () => {
    const events = [{ kind: 'outsideChanges', message: 'Also changed: page styles, page script' }];
    assert.equal(
      summarizeOutsideChanges(events, [{ document: 'Home', description: 'Added the tooltip controller' }]),
      'Also changed outside this area: Home: Added the tooltip controller',
    );
    assert.equal(summarizeOutsideChanges(events, []), 'Also changed page styles and page script');
    assert.equal(summarizeOutsideChanges(events, [{ document: 'Home', description: ' ' }]), 'Also changed page styles and page script');
  });
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
