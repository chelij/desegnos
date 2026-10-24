import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

const temporary = await mkdtemp(new URL('../.page-tabs-', import.meta.url));
try {
  const compiler = new URL('bin/tsc', import.meta.resolve('typescript/package.json'));
  const result = spawnSync(process.execPath, [compiler.pathname, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022', '--outDir', temporary, 'src/pageTabs.ts', 'src/shortcuts.ts'], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const { pageTabsKey, visiblePageTabs, showPageTab, closePageTab, loadPageTabs, savePageTabs } = await import(pathToFileURL(join(temporary, 'pageTabs.js')).href);
  const { editorShortcut, SHORTCUT_HELP, TOOL_SHORTCUTS, withShortcut } = await import(pathToFileURL(join(temporary, 'shortcuts.js')).href);

  const pages = ['home', 'about', 'pricing'];
  function storage(initial = {}) {
    const values = new Map(Object.entries(initial));
    return {
      values,
      getItem: key => values.get(key) ?? null,
      setItem: (key, value) => { values.set(key, value); },
    };
  }
  const key = (options = {}) => ({ key: 'w', code: 'KeyW', ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, ...options });

  await test('a project with no saved tab state shows every page', () => {
    assert.deepEqual(loadPageTabs('project-1', pages), pages);
    assert.deepEqual(loadPageTabs('project-1', pages, storage()), pages);
    assert.deepEqual(visiblePageTabs(undefined, pages), pages);
  });

  await test('closing a page tab only hides it and the last tab stays open', () => {
    const closed = closePageTab(pages, 'about', 'home');
    assert.deepEqual(closed, { open: ['home', 'pricing'], active: 'home' });
    const last = closePageTab(['home'], 'home', 'home');
    assert.deepEqual(last, { open: ['home'], active: 'home' });
    const unknown = closePageTab(pages, 'missing', 'home');
    assert.deepEqual(unknown, { open: pages, active: 'home' });
    assert.deepEqual(pages, ['home', 'about', 'pricing'], 'closing never changes the project pages');
  });

  await test('closing the active tab activates its neighbour', () => {
    assert.equal(closePageTab(pages, 'about', 'about').active, 'pricing');
    assert.equal(closePageTab(pages, 'pricing', 'pricing').active, 'about');
    assert.equal(closePageTab(pages, 'home', 'home').active, 'about');
  });

  await test('reopening a hidden page puts it back in page order', () => {
    const hidden = closePageTab(pages, 'about', 'home').open;
    assert.deepEqual(showPageTab(hidden, 'about', pages), pages);
    assert.equal(showPageTab(pages, 'home', pages), pages, 'an open tab is left untouched');
    assert.deepEqual(visiblePageTabs(['pricing', 'gone'], pages), ['pricing'], 'removed pages drop out of the strip');
    assert.deepEqual(visiblePageTabs(['gone'], pages), pages, 'a project with no surviving tabs reopens every page');
  });

  await test('open page tabs persist per project and survive a restart', () => {
    const store = storage();
    savePageTabs('project-1', ['home', 'pricing'], store);
    savePageTabs('project-2', ['about'], store);
    assert.deepEqual(loadPageTabs('project-1', pages, store), ['home', 'pricing']);
    assert.deepEqual(loadPageTabs('project-2', pages, store), ['about']);
    assert.deepEqual(loadPageTabs('project-3', pages, store), pages);
    assert.equal(store.values.has(pageTabsKey('project-1')), true);
  });

  await test('damaged or unavailable tab storage falls back to every page', () => {
    assert.deepEqual(loadPageTabs('project-1', pages, storage({ [pageTabsKey('project-1')]: '{not json' })), pages);
    assert.deepEqual(loadPageTabs('project-1', pages, storage({ [pageTabsKey('project-1')]: '{"home":true}' })), pages);
    assert.deepEqual(loadPageTabs('project-1', pages, storage({ [pageTabsKey('project-1')]: '["about", 3]' })), ['about']);
    const unavailable = { getItem: () => { throw new Error('storage disabled'); }, setItem: () => { throw new Error('storage disabled'); } };
    assert.deepEqual(loadPageTabs('project-1', pages, unavailable), pages);
    assert.doesNotThrow(() => savePageTabs('project-1', ['home'], unavailable));
  });

  await test('Ctrl/Cmd+W closes the active page tab and the shortcut is documented', () => {
    assert.equal(editorShortcut(key({ ctrlKey: true }), false), 'closePageTab');
    assert.equal(editorShortcut(key({ metaKey: true }), false), 'closePageTab');
    assert.equal(editorShortcut(key({ ctrlKey: true }), true), 'closePageTab', 'closing does not depend on where the cursor is');
    assert.equal(editorShortcut(key({ ctrlKey: true, shiftKey: true }), false), null);
    assert.equal(editorShortcut(key(), false), null);
    assert.ok(SHORTCUT_HELP.some(([label, keys]) => label === 'Close page tab' && /Ctrl\/Cmd\+W/.test(keys)));
  });

  await test('tool tooltips use the documented shortcut list', () => {
    for (const [action, keys] of Object.entries(TOOL_SHORTCUTS)) {
      assert.ok(SHORTCUT_HELP.some(([, text]) => text.includes(keys)), `${action} (${keys}) is missing from SHORTCUT_HELP`);
    }
    assert.equal(withShortcut('Ask AI', 'askAI'), 'Ask AI · Ctrl/Cmd+K');
    assert.equal(withShortcut('Close'), 'Close');
  });

  await test('the strip wires the close button, the page list and the shortcut', async () => {
    const { readFile } = await import('node:fs/promises');
    const app = await readFile(new URL('../src/App.tsx', import.meta.url), 'utf8');
    assert.match(app, /className="page-tab-close close-button icon-button sm"/);
    assert.match(app, /aria-label=\{`Close \$\{item\.name\} tab`\}/);
    assert.match(app, /className="project-tab-close close-button icon-button sm"/);
    assert.match(app, /<MenuButton label="Pages"/);
    assert.match(app, /shortcut === 'closePageTab'/);
    assert.match(app, /closePageTabInStrip\(page\.id\)/);
  });
} finally {
  await rm(temporary, { recursive: true, force: true });
}
