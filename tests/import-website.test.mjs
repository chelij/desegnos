import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const root = fileURLToPath(new URL('../', import.meta.url));
const temporary = await mkdtemp(join(root, '.import-website-'));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--jsx', 'react-jsx', '--module', 'esnext', '--target', 'ES2022', '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/ImportWebsite.tsx'], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout || String(result.error));
  const source = (await readFile(join(temporary, 'ImportWebsite.js'), 'utf8')).replace(/^import .*;$/gm, '').replace(/^export /gm, '');
  const selection = { token: 'snapshot', review: { updated: ['/'], added: ['/new'], kept: [{ id: 'keep', path: '/keep' }, { id: 'home', path: '/' }] } };
  function setup(response = selection) {
    const state = [], calls = [], applied = [];
    let cursor = 0, previousDeps, cleanup;
    const effects = [];
    const useEffect = (effect, deps) => {
      if (!previousDeps || deps.some((value, index) => value !== previousDeps[index])) {
        previousDeps = deps; effects[0] = effect;
      }
    };
    const useState = initial => { const index = cursor++; if (!(index in state)) state[index] = initial; return [state[index], value => { state[index] = typeof value === 'function' ? value(state[index]) : value; }]; };
    const native = async (command, args) => { calls.push({ command, args }); return response; };
    const jsx = (type, props) => ({ type, props });
    const Component = new Function('useState', 'useEffect', 'native', '_jsx', '_jsxs', '_Fragment', `${source}\nreturn ImportWebsite;`)(useState, useEffect, native, jsx, jsx, 'fragment');
    const render = (revision = 1) => { cursor = 0; return Component({ sourceId: 'shared-source', revision, homePageId: 'home', busy: false, beforePick: async () => {}, apply: async (...args) => applied.push(args) }); };
    const prepare = async (revision = 1) => { render(revision); effects.splice(0).forEach(effect => { cleanup?.(); cleanup = effect(); }); await settle(); };
    return { render, prepare, calls, applied };
  }
  function all(tree) {
    if (!tree || typeof tree !== 'object') return [];
    return [tree, ...[tree.props?.children].flat(Infinity).flatMap(all)];
  }
  function button(tree, text) { return all(tree).find(node => node.type === 'button' && node.props.children === text); }
  async function settle() { for (let index = 0; index < 12; index++) await Promise.resolve(); }

  await test('review lists updates and additions, keeps absent pages by default, and imports only toggled removals', async () => {
    const { render, prepare, calls, applied } = setup();
    let tree = render();
    assert.equal(button(tree, 'Import pages'), undefined);
    await prepare();
    tree = render();
    assert.deepEqual(calls, [{ command: 'prepare_website_import', args: { sourceId: 'shared-source' } }]);
    const checkboxes = all(tree).filter(node => node.type === 'input');
    // The home page is never offered for removal.
    assert.equal(checkboxes.length, 1);
    assert.ok(checkboxes.every(node => node.props.checked === false));
    const text = JSON.stringify(tree);
    for (const label of ['Update', 'Add', 'Remove pages not in this import', '/new', '/keep']) assert.ok(text.includes(label));
    checkboxes[0].props.onChange({ target: { checked: true } });
    tree = render();
    button(tree, 'Import pages').props.onClick(); await settle();
    assert.deepEqual(applied, [['snapshot', ['keep']]]);
  });
  await test('brand changes refresh the page review from the same snapshot without another picker', async () => {
    const { prepare, calls } = setup();
    await prepare(1);
    await prepare(2);
    assert.deepEqual(calls, [1, 2].map(() => ({ command: 'prepare_website_import', args: { sourceId: 'shared-source' } })));
  });
  await test('sources without HTML offer no page action', async () => {
    const { render, prepare, calls } = setup(null);
    await prepare();
    assert.equal(render(), null);
    assert.equal(calls.length, 1);
  });
  await test('page review reuses the selected source and the app mounts one import side panel', async () => {
    const { render, prepare, calls, applied } = setup();
    await prepare();
    button(render(), 'Import pages').props.onClick(); await settle();
    assert.equal(calls[0].args.sourceId, 'shared-source');
    assert.deepEqual(applied, [['snapshot', []]]);
    const app = await readFile(join(root, 'src/App.tsx'), 'utf8');
    assert.match(app, /label: 'Import…', disabled: !project/);
    assert.match(app, /showImport && <WorkspaceSurface place=\{project \? 'panel' : 'start'\}/);
  });
} finally { await rm(temporary, { recursive: true, force: true }); }
