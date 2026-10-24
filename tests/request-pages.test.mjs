import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

const temporary = await mkdtemp(new URL('../.request-pages-', import.meta.url));
try {
  const compiler = new URL('bin/tsc', import.meta.resolve('typescript/package.json'));
  const result = spawnSync(process.execPath, [compiler.pathname, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022', '--outDir', temporary, 'src/requestPages.ts'], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const { pageGroups, pageGroupPath, coversPage, coversSelection, coversGroup, pageSelectionLabel, normalizeSelections, toggleAll, toggleGroup, togglePage } = await import(pathToFileURL(join(temporary, 'requestPages.js')).href);

  const pages = [
    { id: 'home', name: 'Home', path: '/' },
    { id: 'projects', name: 'Projects', path: '/projects' },
    { id: 'atlas', name: 'Atlas', path: '/projects/atlas' },
    { id: 'borealis', name: 'Borealis', path: '/projects/borealis' },
    { id: 'about', name: 'About', path: '/about' },
  ];
  const groups = Object.fromEntries(pageGroups(pages).map(group => [group.path, group.pages.map(page => page.id)]));
  const page = id => pages.find(item => item.id === id);

  await test('no chosen pages means only this page', () => {
    assert.equal(pageSelectionLabel([], pages), 'This page');
    assert.deepEqual(normalizeSelections([], pages), []);
    assert.equal(coversSelection([], page('atlas')), false);
  });

  await test('pages are grouped by their path prefix', () => {
    assert.equal(pageGroupPath('/projects/atlas'), '/projects');
    assert.equal(pageGroupPath('/about'), '/');
    assert.deepEqual(groups['/projects'], ['atlas', 'borealis']);
    assert.deepEqual(groups['/'], ['home', 'projects', 'about']);
  });

  await test('a group is chosen by prefix and labelled with a wildcard', () => {
    const chosen = toggleGroup([], { path: '/projects', pages: [page('projects'), page('atlas'), page('borealis')] }, pages);
    assert.deepEqual(chosen, [{ kind: 'prefix', path: 'projects' }]);
    assert.equal(pageSelectionLabel(chosen, pages), 'projects/*');
    assert.equal(coversPage(chosen[0], page('atlas')), true);
    assert.equal(coversPage(chosen[0], page('about')), false);
    assert.deepEqual(toggleGroup(chosen, { path: '/projects', pages: [page('projects'), page('atlas'), page('borealis')] }, pages), []);
  });

  await test('choosing pages inside a group expands the remaining pages', () => {
    const group = { path: '/projects', pages: [page('projects'), page('atlas'), page('borealis')] };
    const chosen = toggleGroup([], group, pages);
    const withoutAtlas = togglePage(chosen, page('atlas'), pages);
    assert.equal(coversSelection(withoutAtlas, page('atlas')), false);
    assert.equal(coversSelection(withoutAtlas, page('projects')), true);
    assert.equal(coversSelection(withoutAtlas, page('borealis')), true);
    assert.equal(pageSelectionLabel(withoutAtlas, pages), '2 pages');
    // The selected group entry is replaced by explicit pages, never an exclusion.
    assert.ok(withoutAtlas.every(item => item.kind === 'page'));
    const restored = togglePage(withoutAtlas, page('atlas'), pages);
    assert.equal(coversSelection(restored, page('atlas')), true);
    assert.equal(pageSelectionLabel(togglePage(restored, page('projects'), pages), pages), '2 pages');
  });

  await test('specific pages keep a page-name label', () => {
    const chosen = togglePage([], page('atlas'), pages);
    assert.deepEqual(chosen, [{ kind: 'page', pageId: 'atlas' }]);
    assert.equal(pageSelectionLabel(chosen, pages), 'Atlas');
    assert.deepEqual(togglePage(chosen, page('atlas'), pages), []);
    const two = togglePage(chosen, page('about'), pages);
    assert.equal(pageSelectionLabel(two, pages), '2 pages');
    assert.equal(coversGroup(two, { path: '/projects', pages: [page('projects'), page('atlas'), page('borealis')] }), false);
  });

  await test('all pages covers every page and toggles back to this page', () => {
    const all = toggleAll([]);
    assert.deepEqual(all, [{ kind: 'all' }]);
    assert.equal(pageSelectionLabel(all, pages), 'All pages');
    assert.ok(pages.every(item => coversSelection(all, item)));
    assert.deepEqual(toggleAll(all), []);
  });

  await test('unchecking a group inside All pages keeps the other pages explicit', () => {
    const group = { path: '/projects', pages: [page('projects'), page('atlas'), page('borealis')] };
    const chosen = toggleGroup(toggleAll([]), group, pages);
    assert.deepEqual(chosen, [{ kind: 'page', pageId: 'home' }, { kind: 'page', pageId: 'about' }]);
    assert.equal(pageSelectionLabel(chosen, pages), '2 pages');
  });

  await test('a page covered by a group is not repeated', () => {
    const chosen = normalizeSelections([{ kind: 'prefix', path: 'projects' }, { kind: 'page', pageId: 'atlas' }, { kind: 'page', pageId: 'atlas' }], pages);
    assert.deepEqual(chosen, [{ kind: 'prefix', path: 'projects' }]);
    assert.deepEqual(normalizeSelections([{ kind: 'all' }, { kind: 'page', pageId: 'atlas' }], pages), [{ kind: 'all' }]);
  });

  await test('the request box and Ask AI toolbar both offer the picker', async () => {
    const canvas = await readFile(new URL('../src/Canvas.tsx', import.meta.url), 'utf8');
    assert.match(canvas, /<RequestPages pages=\{pages\} hostPageId=\{hostPageId\}/);
    assert.match(canvas, /<RequestPages pages=\{pages\} hostPageId=\{page\.id\} selection=\{requestPages\}/);
    assert.match(canvas, /void pickElement\(selections, requestPages\)/);
    const app = await readFile(new URL('../src/App.tsx', import.meta.url), 'utf8');
    assert.match(app, /type: 'set_region_pages'/);
    assert.match(app, /pages=\{project\.pages\}/);
  });
} finally {
  await rm(temporary, { recursive: true, force: true });
}
