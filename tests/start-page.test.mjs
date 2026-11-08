import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

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
    { path: '/home/design/Shop.desegnos', name: 'Shop', pinned: false, openedAtMs: Date.now() - 3 * 60 * 60_000 },
    { path: '/home/design/Studio.desegnos', name: 'Studio', pinned: true, openedAtMs: Date.now() - 60_000 },
  ];
  function render(overrides = {}) {
    const calls = [];
    const tree = StartPage({
      busy: false,
      create: template => calls.push(['create', template]),
      openProject: () => calls.push(['open-project']),
      recentProjects: recents,
      openRecent: path => calls.push(['open', path]),
      forgetRecent: path => calls.push(['forget', path]),
      pinRecent: (path, pinned) => calls.push(['pin', path, pinned]),
      recovery: null,
      showStartPageOnLaunch: true,
      changeShowStartPage: value => calls.push(['show-start-page', value]),
      ...overrides,
    });
    return { tree, calls };
  }

  await test('each tile creates its own template through one native call', () => {
    const { tree, calls } = render();
    for (const [label, template] of [['Blank canvas', 'blank'], ['Website', 'website'], ['App screen', 'appScreen'], ['Slide deck', 'slideDeck'], ['Social image', 'socialImage']]) {
      button(tree, label).props.onClick();
      assert.deepEqual(calls.at(-1), ['create', template]);
    }
    button(tree, 'New').props.onClick();
    assert.deepEqual(calls.at(-1), ['create', 'blank']);
  });

  await test('Open browses for a file, and Preferences shows the launch toggle inline', () => {
    const { tree, calls } = render();
    const html = renderToStaticMarkup(tree);
    button(tree, 'Open').props.onClick();
    assert.deepEqual(calls.at(-1), ['open-project']);
    assert.match(html, /Show start page on launch/);
    const toggle = all(tree).find(node => node.type === 'input' && node.props.type === 'checkbox');
    assert.equal(toggle.props.checked, true);
    toggle.props.onChange({ target: { checked: false } });
    assert.deepEqual(calls.at(-1), ['show-start-page', false]);
  });

  await test('recents list pinned projects first and wire every action', () => {
    const { tree, calls } = render();
    const html = renderToStaticMarkup(tree);
    assert.match(html, /<table\b/);
    assert.match(html, />Name</);
    assert.match(html, />Location</);
    assert.match(html, />Last opened</);
    assert.ok(html.indexOf('>Studio<') < html.indexOf('>Shop<'), 'pinned projects lead the table');
    assert.match(html, />\/home\/design</);
    assert.match(html, />1 min ago</);
    assert.match(html, />3 hr ago</);
    assert.match(html, /aria-label="Unpin Studio"/);
    assert.match(html, /aria-label="Pin Shop"/);
    assert.match(html, /aria-label="Remove Shop from recent projects"/);
    button(tree, 'Studio').props.onClick();
    assert.deepEqual(calls.at(-1), ['open', '/home/design/Studio.desegnos']);
    const row = all(tree).find(node => node.type === 'tr' && all(node).some(child => child === 'Shop'));
    row.props.onDoubleClick();
    assert.deepEqual(calls.at(-1), ['open', '/home/design/Shop.desegnos']);
    const pin = all(tree).find(node => node.type === 'button' && node.props['aria-label'] === 'Unpin Studio');
    pin.props.onClick();
    assert.deepEqual(calls.at(-1), ['pin', '/home/design/Studio.desegnos', false]);
    const remove = all(tree).find(node => node.type === 'button' && node.props['aria-label'] === 'Remove Shop from recent projects');
    remove.props.onClick();
    assert.deepEqual(calls.at(-1), ['forget', '/home/design/Shop.desegnos']);
  });

  await test('an empty Recent is left off the page', () => {
    const html = renderToStaticMarkup(render({ recentProjects: [] }).tree);
    assert.doesNotMatch(html, />Recent</);
  });

  await test('recovery appears before recents, and tiles wait while a project is opening', () => {
    const withRecovery = render({ recovery: React.createElement('div', { className: 'recovery-list' }, 'Unsaved changes') });
    const html = renderToStaticMarkup(withRecovery.tree);
    assert.ok(html.indexOf('Unsaved work') < html.indexOf('>Recent<'));
    assert.match(html, /Unsaved changes/);
    const busy = render({ busy: true });
    for (const label of ['Blank canvas', 'Website', 'App screen', 'Slide deck', 'Social image', 'New', 'Open']) {
      assert.equal(button(busy.tree, label).props.disabled, true, label);
    }
  });
} finally {
  await rm(temporary, { recursive: true, force: true });
}
