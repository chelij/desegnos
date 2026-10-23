import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const root = fileURLToPath(new URL('../', import.meta.url));
const temporary = await mkdtemp(join(root, '.import-flow-'));
const all = tree => !tree || typeof tree !== 'object' ? [] : [tree, ...[tree.props?.children].flat(Infinity).flatMap(all)];
const button = (tree, label) => all(tree).find(node => node.type === 'button' && node.props.children === label);
const settle = async () => { for (let i = 0; i < 20; i++) await Promise.resolve(); };
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--types', 'vite/client', '--jsx', 'react-jsx', '--module', 'esnext', '--target', 'ES2022', '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/ImportContext.tsx'], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const code = (await readFile(join(temporary, 'ImportContext.js'), 'utf8')).replace(/^import .*;$/gm, '').replace(/^export /gm, '');
  function setup(connected = true, previous = false) {
    let cursor = 0;
    const state = [], dependencies = [], effects = [], calls = [], kept = [];
    const useState = initial => { const index = cursor++; if (!(index in state)) state[index] = initial; return [state[index], value => { state[index] = typeof value === 'function' ? value(state[index]) : value; }]; };
    const useRef = initial => { const index = cursor++; return state[index] ??= { current: initial }; };
    const useEffect = (effect, deps) => { const index = cursor++; if (!dependencies[index] || deps.some((dep, i) => dep !== dependencies[index][i])) { dependencies[index] = deps; effects.push(effect); } };
    const source = { id: 'shared', name: 'Studio', files: [{ id: 'html', name: 'index.html', textBytes: 50 }], skipped: [], photos: [], pages: [] };
    const candidate = { findings: [{ topic: 'Brand name', value: 'Studio', confidence: 'high', evidence: ['html'] }], logoAssetIds: [], content: [], warnings: [] };
    const native = async (command, args) => {
      calls.push({ command, args });
      if (command === 'get_identity_analysis') return previous ? { jobId: 'previous', source, fileIds: ['html'], status: 'complete', candidate } : null;
      if (command === 'prepare_identity_selection') return { fits: true, fileIds: ['html'] };
      if (command === 'analyze_identity') return { jobId: 'analysis', source, status: 'complete', candidate };
      return source;
    };
    const jsx = (type, props) => ({ type, props });
    const bindings = { useState, useEffect, useRef, useMemo: fn => fn(), useCallback: fn => fn, native, _jsx: jsx, _jsxs: jsx, _Fragment: 'fragment', ImportWebsite: 'page-review', WebsiteChoice: 'website-choice', Notice: 'notice', ProblemBanner: 'problem', userError: String, needsAiSettings: () => false, groupIdentityPages: () => [], selectedPageContent: () => ({ photos: [], items: [] }), findingSection: () => 'Other', parseColours: () => [], parseFonts: () => [], parseTone: () => [] };
    const Component = new Function(...Object.keys(bindings), `${code}\nreturn ImportContext;`)(...Object.values(bindings));
    const render = () => {
      cursor = 0;
      const tree = Component({ busy: false, settings: { credential: { configured: connected }, settings: {}, providers: [], chatgpt: { accounts: [] } }, openSettings() {}, importInto: async (...args) => kept.push(args), close() { throw Error('Merged panel must stay open'); }, pages: { homePageId: 'home', revision: 1, beforePick() {}, apply() {} } });
      effects.splice(0).forEach(effect => effect());
      return tree;
    };
    return { render, calls, kept };
  }
  await test('one folder selection supplies page review and brand analysis; keeping brand leaves pages available', async () => {
    const { render, calls, kept } = setup();
    button(render(), 'Choose folder…').props.onClick(); await settle();
    let tree = render(); await settle(); tree = render();
    assert.equal(all(tree).find(node => node.type === 'page-review').props.sourceId, 'shared');
    button(tree, 'Read my brand').props.onClick(); await settle(); tree = render();
    tree.props.onSubmit({ preventDefault() {} }); await settle(); tree = render();
    assert.equal(kept.length, 1);
    assert.ok(all(tree).find(node => node.type === 'page-review'));
    assert.equal(calls.filter(call => /pick|fetch/.test(call.command)).length, 1);
    assert.deepEqual(calls.find(call => call.command === 'analyze_identity').args, { sourceId: 'shared', fileIds: ['html'] });
  });
  await test('pages remain available without AI and brand offers the connection action', async () => {
    const { render } = setup(false);
    button(render(), 'Choose ZIP or HTML files…').props.onClick(); await settle();
    const tree = render();
    assert.ok(all(tree).find(node => node.type === 'page-review'));
    assert.ok(button(tree, 'Connect AI'));
    assert.equal(button(tree, 'Read my brand'), undefined);
  });
  await test('resuming a previous brand keeps its review without requesting unavailable original page files', async () => {
    const { render } = setup(true, true);
    render(); await settle();
    button(render(), 'Continue with my previous brand').props.onClick();
    const tree = render();
    assert.ok(button(tree, 'Keep brand choices'));
    assert.equal(all(tree).find(node => node.type === 'page-review'), undefined);
  });
  await test('menu has one import entry and preserves project-folder and start-brand routes', async () => {
    const app = await readFile(join(root, 'src/App.tsx'), 'utf8');
    assert.equal((app.match(/label: 'Import…'/g) ?? []).length, 1);
    assert.doesNotMatch(app, /Import website…|Import context…|showWebsiteImport/);
    assert.match(app, /label: 'Open project folder…'.*open\(true\)/);
    assert.match(app, /function startBrandImport\(\).*setImportAsNew\(true\)/);
  });
} finally { await rm(temporary, { recursive: true, force: true }); }
