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
  function setup() {
    const state = [], calls = [], applied = [];
    let cursor = 0;
    const useState = initial => { const index = cursor++; if (!(index in state)) state[index] = initial; return [state[index], value => { state[index] = typeof value === 'function' ? value(state[index]) : value; }]; };
    const native = async (command, args) => { calls.push({ command, args }); return selection; };
    const jsx = (type, props) => ({ type, props });
    const Component = new Function('useState', 'native', '_jsx', '_jsxs', '_Fragment', `${source}\nreturn ImportWebsite;`)(useState, native, jsx, jsx, 'fragment');
    const render = () => { cursor = 0; return Component({ homePageId: 'home', busy: false, beforePick: async () => {}, apply: async (...args) => applied.push(args) }); };
    return { render, calls, applied };
  }
  function all(tree) {
    if (!tree || typeof tree !== 'object') return [];
    return [tree, ...[tree.props?.children].flat(Infinity).flatMap(all)];
  }
  function button(tree, text) { return all(tree).find(node => node.type === 'button' && node.props.children === text); }
  async function settle() { await Promise.resolve(); await Promise.resolve(); }

  await test('review lists updates and additions, keeps absent pages by default, and imports only toggled removals', async () => {
    const { render, calls, applied } = setup();
    let tree = render();
    assert.equal(button(tree, 'Import'), undefined);
    button(tree, 'Choose ZIP or HTML files…').props.onClick(); await settle();
    tree = render();
    assert.deepEqual(calls, [{ command: 'prepare_website_import', args: { kind: 'files' } }]);
    const checkboxes = all(tree).filter(node => node.type === 'input');
    assert.equal(checkboxes.length, 2);
    assert.ok(checkboxes.every(node => node.props.checked === false));
    assert.equal(checkboxes[1].props.disabled, true);
    const text = JSON.stringify(tree);
    for (const label of ['Update', 'Add', 'Keep', '/new', '/keep']) assert.ok(text.includes(label));
    checkboxes[0].props.onChange({ target: { checked: true } });
    tree = render();
    button(tree, 'Import').props.onClick(); await settle();
    assert.deepEqual(applied, [['snapshot', ['keep']]]);
  });
  await test('folder selection uses the native folder route and the app mounts review in a side panel', async () => {
    const { render, calls, applied } = setup();
    button(render(), 'Choose folder…').props.onClick(); await settle();
    button(render(), 'Import').props.onClick(); await settle();
    assert.equal(calls[0].args.kind, 'folder');
    assert.deepEqual(applied, [['snapshot', []]]);
    const app = await readFile(join(root, 'src/App.tsx'), 'utf8');
    assert.match(app, /label: 'Import website…', disabled: !project/);
    assert.match(app, /showWebsiteImport && project && <WorkspaceSurface place="panel"/);
  });
} finally { await rm(temporary, { recursive: true, force: true }); }
