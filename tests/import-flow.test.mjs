import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const root = fileURLToPath(new URL('../', import.meta.url));
const temporary = await mkdtemp(join(root, '.import-flow-'));
const all = tree => !tree || typeof tree !== 'object' ? [] : [tree, ...[tree.props?.children].flat(Infinity).flatMap(all)];
const text = node => typeof node === 'string' || typeof node === 'number' ? String(node) : Array.isArray(node) ? node.map(text).join('') : text(node?.props?.children ?? '');
const button = (tree, label) => all(tree).find(node => node.type === 'button' && text(node) === label);
const settle = async () => { for (let i = 0; i < 30; i++) await Promise.resolve(); };
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--types', 'vite/client', '--jsx', 'react-jsx', '--module', 'esnext', '--target', 'ES2022', '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/ImportContext.tsx'], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const code = (await Promise.all(['errorMessage.js', 'generated/limits.js', 'usePoll.js', 'WebsiteImport.js', 'ImportContext.js'].map(file => readFile(join(temporary, file), 'utf8')))).map(code => code.replace(/^import .*;$/gm, '').replace(/^export /gm, '')).join('\n');
  function setup({ connected = true, previous = false, initialSource = null, pageCount = 0, fail = '', deferred = '', pageWarnings = ['A picture was skipped.'] } = {}) {
    let cursor = 0, closed = 0, configured = connected, completeDeferred;
    const state = [], dependencies = [], effects = [], cleanups = [], calls = [], kept = [], imported = [], listeners = new Set();
    const useState = initial => { const index = cursor++; if (!(index in state)) state[index] = initial; return [state[index], value => { state[index] = typeof value === 'function' ? value(state[index]) : value; }]; };
    const useRef = initial => { const index = cursor++; return state[index] ??= { current: initial }; };
    const useEffect = (effect, deps) => { const index = cursor++; if (!dependencies[index] || deps.some((dep, i) => dep !== dependencies[index][i])) { dependencies[index] = deps; effects.push(() => { cleanups[index]?.(); cleanups[index] = effect(); }); } };
    const source = { id: 'shared', name: 'Studio', files: [{ id: 'html', name: 'index.html', textBytes: 50, asset: null }], skipped: ['secret.txt: credential'], photos: [], pages: [], content: [] };
    const website = { id: 'website', name: 'Studio', baseUrl: 'http://localhost:1234/', extractorUrl: 'http://localhost:1234/extract', pages: Array.from({ length: pageCount }, (_, i) => ({ route: i ? `/page${i}` : '/', title: i ? `Page ${i}` : 'Home', url: `http://localhost:1234/page${i}.html` })), totalPages: pageCount, skipped: ['large.png: size limit'], identitySource: source, identityError: null };
    const candidate = { summary: 'A studio site', brandName: 'Studio', tone: 'Calm', findings: [{ topic: 'Brand name', value: 'Studio', confidence: 'high', evidence: ['html'] }], logoAssetIds: [], content: [], warnings: [] };
    const analysis = { jobId: 'analysis', sourceId: 'shared', source, fileIds: ['html'], status: 'complete', message: '', receivedBytes: 0, candidate, error: null, authentication: 'apiKey', chatgptAccountId: null };
    const native = async (command, args) => {
      calls.push({ command, args });
      if (command === fail) throw Error('Import failed here');
      if (command === deferred) await new Promise(resolve => { completeDeferred = resolve; });
      if (command === 'get_identity_analysis') return previous ? { ...analysis, jobId: 'previous' } : null;
      if (command === 'prepare_identity_selection') return { fits: true, fileIds: ['html'], message: null, needsSettings: false };
      if (command === 'pick_website_import_source' || command === 'prepare_website_import_url') return website;
      if (command === 'analyze_identity') return analysis;
      if (command === 'import_website_pages') return { workspace: {}, pages: pageCount, layers: 3, pictures: 0, warnings: pageWarnings };
      return null;
    };
    const jsx = (type, props) => typeof type === 'function' ? type(props) : ({ type, props });
    const window = { addEventListener: (_, listener) => listeners.add(listener), removeEventListener: (_, listener) => listeners.delete(listener), setTimeout: () => 1, clearTimeout() {} };
    const bindings = { useState, useEffect, useRef, useMemo: fn => fn(), useCallback: fn => fn, native, window, EXTRACTOR_SCRIPT: 'extract', CAPTURE_TIMEOUT_MS: 1, _jsx: jsx, _jsxs: jsx, _Fragment: 'fragment', Icon: 'icon', Notice: 'notice', ProblemBanner: 'problem', userError: error => error?.message ?? String(error), needsAiSettings: () => false, groupIdentityPages: () => [], selectedPageContent: () => ({ photos: [], items: [] }), findingSection: () => 'Other', parseColours: () => [], parseFonts: () => [], parseTone: () => [] };
    const Component = new Function(...Object.keys(bindings), `${code}\nreturn ImportContext;`)(...Object.values(bindings));
    const render = () => {
      cursor = 0;
      const tree = Component({ initialSource, busy: false, settings: { readiness: { ready: configured }, credential: { configured }, configurationError: null, settings: {}, providers: [], authentication: 'apiKey', chatgpt: { accounts: [], activeAccountId: null } }, openSettings() { configured = true; }, importInto: async (...args) => kept.push(args), onPagesImported: result => imported.push(result), close() { closed += 1; } });
      effects.splice(0).forEach(effect => effect());
      return tree;
    };
    async function pump() {
      let tree;
      for (let i = 0; i < pageCount * 2 + 4; i++) {
        await settle(); tree = render();
        const frame = all(tree).find(node => node.type === 'iframe');
        if (frame) {
          const url = new URL(frame.props.src);
          listeners.forEach(listener => listener({ origin: url.origin, data: { type: 'desegnos-extract', id: url.searchParams.get('id'), capture: { url: url.searchParams.get('url'), scripts: [], elements: [] } } }));
        }
      }
      await settle(); return render();
    }
    async function pick(label = 'Choose folder…') { button(render(), label).props.onClick(); await settle(); render(); await settle(); return render(); }
    return { render, calls, kept, imported, pick, pump, source, resolve: () => completeDeferred(), closed: () => closed };
  }
  await test('brand-only source skips pages and keeps the existing review', async () => {
    const flow = setup();
    let tree = await flow.pick();
    assert.ok(button(tree, 'Read my brand'));
    assert.doesNotMatch(text(tree), /pages found/);
    button(tree, 'Read my brand').props.onClick(); await settle(); tree = flow.render();
    assert.ok(button(tree, 'Keep brand choices'));
    tree.props.onSubmit({ preventDefault() {} }); await settle();
    assert.equal(flow.kept.length, 1);
    assert.equal(flow.kept[0][0], 'analysis');
    assert.equal(flow.calls.filter(call => /pick|fetch/.test(call.command)).length, 1);
    assert.deepEqual(flow.calls.find(call => call.command === 'analyze_identity').args, { sourceId: 'shared', fileIds: ['html'] });
  });
  await test('pages import without AI and retain warnings and skipped files in the panel', async () => {
    const flow = setup({ connected: false, pageCount: 2 });
    let tree = await flow.pick('Choose ZIP…');
    assert.ok(button(tree, 'Connect AI'));
    assert.match(text(tree), /2 pages found/);
    assert.equal(all(tree).some(node => node.type === 'input' && node.props.type === 'checkbox'), false);
    assert.equal(button(tree, 'Import 2 pages').props.disabled, false);
    button(tree, 'Import 2 pages').props.onClick(); tree = await flow.pump();
    assert.equal(flow.imported.length, 1);
    assert.equal(flow.calls.some(call => call.command === 'analyze_identity'), false);
    assert.equal(flow.closed(), 0);
    assert.ok(all(tree).some(node => node.props?.message === 'A picture was skipped.'));
    assert.match(text(tree), /secret.txt: May contain private information/);
  });
  await test('a clean pages-only import closes the panel', async () => {
    const flow = setup({ connected: false, pageCount: 2, pageWarnings: [] });
    let tree = await flow.pick('Choose ZIP…');
    button(tree, 'Import 2 pages').props.onClick(); tree = await flow.pump(); tree = await flow.pump();
    assert.equal(flow.imported.length, 1);
    assert.equal(flow.closed(), 1);
  });
  await test('one source imports pages then analyzes its brand by default', async () => {
    const flow = setup({ pageCount: 1 });
    let tree = await flow.pick('Choose HTML files…');
    const option = all(tree).find(node => node.type === 'input' && node.props.type === 'checkbox');
    assert.equal(option.props.checked, true);
    assert.match(text(tree), /Also read brand and content/);
    button(tree, 'Import page').props.onClick(); tree = await flow.pump();
    assert.ok(button(tree, 'Keep brand choices'));
    const commands = flow.calls.map(call => call.command);
    assert.ok(commands.indexOf('import_website_pages') < commands.indexOf('analyze_identity'));
    assert.equal(commands.filter(command => /pick|fetch/.test(command)).length, 1);
    assert.deepEqual(flow.calls.find(call => call.command === 'pick_website_import_source').args, { kind: 'file', extractor: 'extract' });
    assert.deepEqual(flow.calls.find(call => call.command === 'analyze_identity').args, { sourceId: 'shared', fileIds: ['html'] });
    assert.equal(flow.imported.length, 1);
  });
  await test('unchecking brand reading imports only pages even with AI connected', async () => {
    const flow = setup({ pageCount: 1 });
    let tree = await flow.pick();
    all(tree).find(node => node.type === 'input' && node.props.type === 'checkbox').props.onChange({ target: { checked: false } });
    tree = flow.render();
    button(tree, 'Import page').props.onClick(); await flow.pump();
    assert.equal(flow.imported.length, 1);
    assert.equal(flow.calls.some(call => call.command === 'analyze_identity'), false);
  });
  await test('an import failure stays beside its retry and does not start brand analysis', async () => {
    const flow = setup({ pageCount: 1, fail: 'import_website_pages' });
    button(await flow.pick(), 'Import page').props.onClick(); const tree = await flow.pump();
    assert.match(text(tree), /Import failed here/);
    assert.ok(button(tree, 'Import page'));
    assert.equal(flow.calls.some(call => call.command === 'analyze_identity'), false);
    assert.equal(flow.closed(), 0);
  });
  await test('brand failure retries on the same source without importing pages twice', async () => {
    const flow = setup({ pageCount: 1, fail: 'analyze_identity' });
    button(await flow.pick(), 'Import page').props.onClick();
    let tree = await flow.pump();
    assert.ok(all(tree).some(node => node.props?.message === 'Import failed here'));
    assert.ok(button(tree, 'Read my brand'));
    assert.equal(button(tree, 'Import page'), undefined);
    button(tree, 'Read my brand').props.onClick(); await settle(); tree = flow.render();
    assert.equal(flow.calls.filter(call => call.command === 'import_website_pages').length, 1);
    assert.equal(flow.calls.filter(call => call.command === 'analyze_identity').length, 2);
    assert.equal(flow.calls.filter(call => /pick|fetch/.test(call.command)).length, 1);
  });
  await test('connecting AI after reading uses the already chosen source', async () => {
    const flow = setup({ pageCount: 1, connected: false });
    button(await flow.pick(), 'Connect AI').props.onClick();
    let tree = flow.render();
    assert.match(text(tree), /Also read brand and content/);
    button(tree, 'Import page').props.onClick(); tree = await flow.pump();
    assert.ok(button(tree, 'Keep brand choices'));
    assert.equal(flow.calls.filter(call => /pick|fetch/.test(call.command)).length, 1);
  });
  await test('Stop during rendering never applies pages or starts AI', async () => {
    const flow = setup({ pageCount: 1 });
    button(await flow.pick(), 'Import page').props.onClick();
    button(flow.render(), 'Stop').props.onClick(); await settle(); flow.render();
    assert.equal(flow.calls.some(call => call.command === 'cancel_website_import'), true);
    assert.equal(flow.calls.some(call => call.command === 'import_website_pages' || call.command === 'analyze_identity'), false);
  });
  await test('Stop ignores a late source read', async () => {
    const flow = setup({ pageCount: 1, deferred: 'prepare_website_import_url' });
    let tree = flow.render();
    all(tree).find(node => node.type === 'input').props.onChange({ target: { value: 'example.com' } });
    button(flow.render(), 'Read').props.onClick();
    button(flow.render(), 'Stop').props.onClick(); await settle();
    flow.resolve(); await settle(); tree = flow.render();
    assert.equal(button(tree, 'Import page'), undefined);
    assert.equal(flow.calls.some(call => call.command === 'analyze_identity'), false);
  });
  await test('brand-only source without AI offers a connection and no disabled brand control', async () => {
    const flow = setup({ connected: false });
    const tree = await flow.pick();
    assert.ok(button(tree, 'Connect AI'));
    assert.equal(button(tree, 'Read my brand'), undefined);
  });
  await test('resuming a previous brand keeps its review without another source read', async () => {
    const flow = setup({ previous: true });
    flow.render(); await settle();
    button(flow.render(), 'Continue with my previous brand').props.onClick(); await settle();
    assert.ok(button(flow.render(), 'Keep brand choices'));
    assert.equal(flow.calls.filter(call => /pick|fetch/.test(call.command)).length, 0);
  });
  await test('an existing identity source opens the review flow without another picker', async () => {
    const flow = setup({ initialSource: { id: 'own', name: 'Own site', files: [], skipped: [], photos: [], pages: [] } });
    assert.ok(button(flow.render(), 'Read my brand'));
    assert.equal(flow.calls.filter(call => /pick|fetch/.test(call.command)).length, 0);
  });
} finally { await rm(temporary, { recursive: true, force: true }); }
