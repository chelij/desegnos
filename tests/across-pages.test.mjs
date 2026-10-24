import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';

const root = fileURLToPath(new URL('../', import.meta.url));
const temporary = await mkdtemp(join(root, '.across-pages-'));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022', '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/acrossPages.ts', 'src/elementTargets.ts'], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout || String(result.error));
  const { ACROSS_PAGES_OFF, MAX_ACROSS_PAGES, acrossPagesChangedLabel, acrossPagesFor, acrossPagesLabel, acrossSelectionKey, chosenAcrossPages, setAcrossPages, toggleAcrossPage } = await import(pathToFileURL(join(temporary, 'acrossPages.js')).href);

  const page = id => ({ kind: 'page', pageId: id });
  const file = path => ({ kind: 'siteFile', path });
  const match = (document, label) => ({ document, target: { id: 'abc', selector: 'body > header:nth-of-type(1)', tag: 'header', label }, label, identicalHtml: true });
  const target = (id, document) => ({ id, selector: 'body > header:nth-of-type(1)', tag: 'header', label: '', ...(document ? { document } : {}) });
  const matches = [match(page('one'), 'Home'), match(page('two'), 'About'), match(file('shared/index.html'), 'shared/index.html')];

  await test('a new selection starts with the toggle off and the same element keeps it', () => {
    const home = target('aaa', page('home'));
    assert.equal(acrossSelectionKey(home), 'page:home|aaa');
    assert.equal(acrossSelectionKey(null), '');
    assert.deepEqual(acrossPagesFor(null, ACROSS_PAGES_OFF), ACROSS_PAGES_OFF);
    const first = acrossPagesFor(home, ACROSS_PAGES_OFF);
    assert.equal(first.enabled, false);
    const on = setAcrossPages(first, true);
    assert.equal(on.enabled, true);
    assert.equal(acrossPagesFor(home, on), on, 'the same element keeps the toggle through its edits');
    const other = acrossPagesFor(target('bbb', page('home')), on);
    assert.deepEqual(other, { key: 'page:home|bbb', enabled: false, excluded: [] });
    const framed = acrossPagesFor(target('aaa', file('shared/index.html')), on);
    assert.deepEqual(framed, { key: 'site:shared/index.html|aaa', enabled: false, excluded: [] });
  });

  await test('the toggle chooses every matched page until one is unticked', () => {
    const on = setAcrossPages(acrossPagesFor(target('aaa', page('home')), ACROSS_PAGES_OFF), true);
    assert.deepEqual(chosenAcrossPages(ACROSS_PAGES_OFF, matches), [], 'off changes only the selected page');
    assert.deepEqual(chosenAcrossPages(on, matches), [page('one'), page('two'), file('shared/index.html')]);
    const withoutAbout = toggleAcrossPage(on, page('two'), false);
    assert.deepEqual(withoutAbout.excluded, ['page:two']);
    assert.deepEqual(chosenAcrossPages(withoutAbout, matches), [page('one'), file('shared/index.html')]);
    assert.deepEqual(chosenAcrossPages(toggleAcrossPage(withoutAbout, page('two'), true), matches), chosenAcrossPages(on, matches), 'ticking again restores it');
    assert.deepEqual(toggleAcrossPage(toggleAcrossPage(on, page('two'), false), page('two'), false).excluded, ['page:two'], 'unticking twice does not repeat the page');
  });

  await test('the chosen list follows the native order and stops at the limit', () => {
    const on = setAcrossPages(acrossPagesFor(target('aaa'), ACROSS_PAGES_OFF), true);
    const many = Array.from({ length: MAX_ACROSS_PAGES + 5 }, (_, index) => match(page(`p${index}`), `Page ${index}`));
    const chosen = chosenAcrossPages(on, many);
    assert.equal(chosen.length, MAX_ACROSS_PAGES);
    assert.deepEqual(chosen.slice(0, 2), [page('p0'), page('p1')]);
    assert.deepEqual(chosenAcrossPages({ ...on, enabled: false, excluded: [] }, many), [], 'off never sends page documents');
  });

  await test('labels name the page count', () => {
    assert.equal(acrossPagesLabel(1), 'Also on 1 page');
    assert.equal(acrossPagesLabel(4), 'Also on 4 pages');
    assert.equal(acrossPagesChangedLabel(1), 'Changed on 1 page');
    assert.equal(acrossPagesChangedLabel(3), 'Changed on 3 pages');
  });
} finally {
  await rm(temporary, { recursive: true, force: true });
}
