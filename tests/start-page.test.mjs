import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

// The start page is the Home page of the main window: Blank canvas, templates, then recents.
const root = fileURLToPath(new URL('../', import.meta.url));
const temporary = await mkdtemp(join(root, '.start-page-'));
const all = tree => tree == null ? [] : typeof tree !== 'object' ? [tree] : [tree, ...[tree.props?.children].flat(Infinity).flatMap(all)];
const button = (tree, label) => all(tree).find(node => node.type === 'button' && all(node).some(child => child === label));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const compiled = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--jsx', 'react-jsx', '--module', 'esnext', '--target', 'ES2022', '--moduleResolution', 'bundler', '--skipLibCheck', '--types', 'vite/client', '--outDir', temporary, 'src/StartPage.tsx'], { cwd: root, encoding: 'utf8' });
  assert.equal(compiled.status, 0, compiled.stderr || compiled.stdout || String(compiled.error));
  await writeFile(join(temporary, 'style-stub.js'), 'export {};\n');
  for (const [file, replacements] of [
    ['StartPage.js', [["'./icons'", "'./icons.js'"], ["'./startPage.css'", "'./style-stub.js'"]]],
  ]) {
    const path = join(temporary, file);
    const source = await readFile(path, 'utf8');
    await writeFile(path, replacements.reduce((value, [from, to]) => value.replaceAll(from, to), source));
  }
  const { StartPage } = await import(pathToFileURL(join(temporary, 'StartPage.js')).href);

  const recents = [
    { path: '/home/design/Studio.desegnos', name: 'Studio', pinned: true, openedAtMs: Date.now() - 60_000 },
    { path: '/home/design/Shop.desegnos', name: 'Shop', pinned: false, openedAtMs: Date.now() - 3 * 60 * 60_000 },
  ];
  function render(overrides = {}) {
    const calls = [];
    const tree = StartPage({
      busy: false,
      create: template => calls.push(['create', template]),
      recentProjects: recents,
      openRecent: path => calls.push(['open', path]),
      forgetRecent: path => calls.push(['forget', path]),
      pinRecent: (path, pinned) => calls.push(['pin', path, pinned]),
      recovery: null,
      ...overrides,
    });
    return { tree, calls };
  }

  await test('Blank canvas comes first, then the four templates, then recent projects', () => {
    const { tree } = render();
    const html = renderToStaticMarkup(tree);
    const order = ['Blank canvas', 'Website', 'App screen', 'Slide deck', 'Social image', 'Recent projects'].map(label => {
      const at = html.indexOf(`>${label}<`);
      assert.ok(at >= 0, `${label} is on the page`);
      return at;
    });
    assert.deepEqual(order, [...order].sort((a, b) => a - b));
    assert.equal((html.match(/class="start-tile"/g) ?? []).length, 5);
    assert.equal((html.match(/class="start-preview /g) ?? []).length, 5, 'every tile draws an outline preview');
    assert.doesNotMatch(html, /<dialog|role="dialog"/);
  });

  await test('each tile creates its own template through one native call', () => {
    const { tree, calls } = render();
    for (const [label, template] of [['Blank canvas', 'blank'], ['Website', 'website'], ['App screen', 'appScreen'], ['Slide deck', 'slideDeck'], ['Social image', 'socialImage']]) {
      button(tree, label).props.onClick();
      assert.deepEqual(calls.at(-1), ['create', template]);
    }
  });

  await test('recents show name, location and last opened with pin and remove actions', () => {
    const { tree, calls } = render();
    const html = renderToStaticMarkup(tree);
    assert.match(html, />Studio</);
    assert.match(html, />\/home\/design</);
    assert.match(html, />1 min ago</);
    assert.match(html, />Shop</);
    assert.match(html, />3 hr ago</);
    assert.match(html, /class="start-recent pinned"/);
    assert.match(html, /aria-label="Unpin Studio"/);
    assert.match(html, /aria-label="Pin Shop"/);
    button(tree, 'Studio').props.onClick();
    assert.deepEqual(calls.at(-1), ['open', '/home/design/Studio.desegnos']);
    const pin = all(tree).find(node => node.type === 'button' && node.props['aria-label'] === 'Unpin Studio');
    pin.props.onClick();
    assert.deepEqual(calls.at(-1), ['pin', '/home/design/Studio.desegnos', false]);
    const remove = all(tree).find(node => node.type === 'button' && node.props['aria-label'] === 'Remove Shop from recent projects');
    remove.props.onClick();
    assert.deepEqual(calls.at(-1), ['forget', '/home/design/Shop.desegnos']);
  });

  await test('recovery appears on the start page before recents, and an empty list stays quiet', () => {
    const withRecovery = render({ recovery: React.createElement('div', { className: 'recovery-list' }, 'Unsaved changes') });
    const html = renderToStaticMarkup(withRecovery.tree);
    assert.ok(html.indexOf('Unsaved work') < html.indexOf('Recent projects'));
    assert.match(html, /class="recovery-list"/);
    const empty = renderToStaticMarkup(render({ recentProjects: [] }).tree);
    assert.match(empty, />No recent projects</);
    assert.doesNotMatch(empty, /class="start-recent"/);
  });

  await test('template tiles wait while a project is opening', () => {
    const { tree } = render({ busy: true });
    const tiles = all(tree).filter(node => node.type === 'button' && String(node.props.className ?? '').includes('start-tile'));
    assert.equal(tiles.length, 5);
    assert.ok(tiles.every(node => node.props.disabled === true));
  });
} finally {
  await rm(temporary, { recursive: true, force: true });
}
