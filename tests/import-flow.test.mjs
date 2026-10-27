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
  function setup(connected = true, previous = false, initialSource = null) {
    let cursor = 0;
    const state = [], dependencies = [], effects = [], calls = [], kept = [];
    const useState = initial => { const index = cursor++; if (!(index in state)) state[index] = initial; return [state[index], value => { state[index] = typeof value === 'function' ? value(state[index]) : value; }]; };
    const useRef = initial => { const index = cursor++; return state[index] ??= { current: initial }; };
    const useEffect = (effect, deps) => { const index = cursor++; if (!dependencies[index] || deps.some((dep, i) => dep !== dependencies[index][i])) { dependencies[index] = deps; effects.push(effect); } };
    const source = { id: 'shared', name: 'Studio', files: [{ id: 'html', name: 'index.html', textBytes: 50, asset: null }], skipped: [], photos: [], pages: [], content: [] };
    const candidate = { summary: 'A studio site', brandName: 'Studio', tone: 'Calm', findings: [{ topic: 'Brand name', value: 'Studio', confidence: 'high', evidence: ['html'] }], logoAssetIds: [], content: [], warnings: [] };
    const analysis = { jobId: 'analysis', sourceId: 'shared', source, fileIds: ['html'], status: 'complete', message: '', receivedBytes: 0, candidate, error: null, authentication: 'apiKey', chatgptAccountId: null };
    const native = async (command, args) => {
      calls.push({ command, args });
      if (command === 'get_identity_analysis') return previous ? { ...analysis, jobId: 'previous' } : null;
      if (command === 'prepare_identity_selection') return { fits: true, fileIds: ['html'], message: null, needsSettings: false };
      if (command === 'pick_identity_source') return source;
      if (command === 'fetch_identity_source') return source;
      if (command === 'analyze_identity') return analysis;
      return source;
    };
    const jsx = (type, props) => ({ type, props });
    const bindings = { useState, useEffect, useRef, useMemo: fn => fn(), useCallback: fn => fn, native, _jsx: jsx, _jsxs: jsx, _Fragment: 'fragment', Icon: 'icon', Notice: 'notice', ProblemBanner: 'problem', userError: String, needsAiSettings: () => false, groupIdentityPages: () => [], selectedPageContent: () => ({ photos: [], items: [] }), findingSection: () => 'Other', parseColours: () => [], parseFonts: () => [], parseTone: () => [] };
    const Component = new Function(...Object.keys(bindings), `${code}\nreturn ImportContext;`)(...Object.values(bindings));
    const render = (overrides = {}) => {
      cursor = 0;
      const tree = Component({ initialSource, busy: false, settings: { credential: { configured: connected }, configurationError: null, settings: {}, providers: [], authentication: 'apiKey', chatgpt: { accounts: [], activeAccountId: null } }, openSettings() {}, importInto: async (...args) => kept.push(args), importWebsite: overrides.importWebsite, close() { throw Error('Merged panel must stay open'); }, ...overrides });
      effects.splice(0).forEach(effect => effect());
      return tree;
    };
    return { render, calls, kept };
  }
  await test('one folder selection supplies brand analysis and keeping choices submits it', async () => {
    const { render, calls, kept } = setup();
    button(render(), 'Choose folder…').props.onClick(); await settle();
    let tree = render(); await settle(); tree = render();
    assert.ok(button(tree, 'Read my brand'));
    button(tree, 'Read my brand').props.onClick(); await settle(); tree = render();
    assert.ok(button(tree, 'Keep brand choices'));
    tree.props.onSubmit({ preventDefault() {} }); await settle(); tree = render();
    assert.equal(kept.length, 1);
    assert.equal(kept[0][0], 'analysis');
    assert.equal(calls.filter(call => /pick|fetch/.test(call.command)).length, 1);
    assert.deepEqual(calls.find(call => call.command === 'analyze_identity').args, { sourceId: 'shared', fileIds: ['html'] });
  });
  await test('a folder pick still offers the brand connection action without AI', async () => {
    const { render } = setup(false);
    button(render(), 'Choose ZIP or HTML files…').props.onClick(); await settle();
    render(); await settle();
    const tree = render();
    assert.ok(button(tree, 'Connect AI'));
    assert.equal(button(tree, 'Read my brand'), undefined);
  });
  await test('resuming a previous brand keeps its review without asking for source files again', async () => {
    const { render, calls } = setup(true, true);
    render(); await settle();
    button(render(), 'Continue with my previous brand').props.onClick(); await settle();
    const tree = render();
    assert.ok(button(tree, 'Keep brand choices'));
    assert.doesNotMatch(JSON.stringify(tree), /Choose another location/);
    assert.equal(calls.filter(call => call.command === 'pick_identity_source' || call.command === 'fetch_identity_source').length, 0);
  });
  await test('an existing website source opens the brand flow immediately and offers page import', async () => {
    let pageImport = 0;
    const { render, calls } = setup(true, false, { id: 'own', name: 'Own site', files: [], skipped: [], photos: [], pages: [], content: [] });
    const tree = render({ importWebsite: () => { pageImport += 1; } }); await settle();
    assert.equal(calls.filter(call => /pick|fetch/.test(call.command)).length, 0);
    assert.ok(button(tree, 'Read my brand'));
    button(tree, 'Import a website as pages…').props.onClick();
    assert.equal(pageImport, 1);
  });
  await test('the menu keeps brand and website imports as separate routes', async () => {
    const app = await readFile(join(root, 'src/App.tsx'), 'utf8');
    assert.equal((app.match(/label: 'Import context…'/g) ?? []).length, 1);
    assert.equal((app.match(/label: 'Import website…'/g) ?? []).length, 1);
    assert.match(app, /label: 'Import context…'.*setShowImport\(true\)/);
    assert.match(app, /function startWebsiteImport\(\).*setShowWebsiteImport\(true\)/);
    assert.match(app, /label: 'Open project folder…'.*open\(true\)/);
  });
} finally { await rm(temporary, { recursive: true, force: true }); }
