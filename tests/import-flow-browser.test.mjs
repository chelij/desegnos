import { runInBrowser, tauriStub } from './browserFixture.mjs';

// The real import panel and website import hook in a real DOM; only the native commands are stubbed.
const script = `
import React, { useState } from 'react';
import { createRoot } from 'react-dom/client';
import { flushSync } from 'react-dom';
import { ImportContext } from '/src/ImportContext.tsx';

const check = (ok, message) => { if (!ok) throw Error(message); };
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const until = async (read, message) => {
  for (let i = 0; i < 300; i++) { const value = read(); if (value) return value; await sleep(10); }
  throw Error('Timed out waiting for ' + (typeof message === 'function' ? message() : message) + errors.join(' | '));
};
const errors = [];
addEventListener('error', event => errors.push(String(event.message)));
console.error = (...args) => { errors.push(args.map(String).join(' ').slice(0, 300)); };
const pickCalls = calls => calls.filter(call => /pick|fetch/.test(call.command)).length;

function mountFlow({ connected = true, previous = false, initialSource = null, pageCount = 0, fail = '', deferred = '', pageWarnings = ['A picture was skipped.'], answerFrames = true } = {}) {
  const calls = [], kept = [], imported = [], answered = new Set();
  let closed = 0, completeDeferred;
  const source = { id: 'shared', name: 'Studio', files: [{ id: 'html', name: 'index.html', textBytes: 50, asset: null }], skipped: ['secret.txt: credential'], photos: [], pages: [], content: [] };
  const website = { id: 'website', name: 'Studio', baseUrl: 'http://localhost:1234/', extractorUrl: 'http://localhost:1234/extract',
    pages: Array.from({ length: pageCount }, (_, i) => ({ route: i ? '/page' + i : '/', title: i ? 'Page ' + i : 'Home', url: 'http://localhost:1234/page' + i + '.html' })),
    totalPages: pageCount, skipped: ['large.png: size limit'], identitySource: source, identityError: null };
  const candidate = { summary: 'A studio site', brandName: 'Studio', tone: 'Calm', findings: [{ topic: 'Brand name', value: 'Studio', confidence: 'high', evidence: ['html'] }], logoAssetIds: [], content: [], warnings: [] };
  const analysis = { jobId: 'analysis', sourceId: 'shared', source, fileIds: ['html'], status: 'complete', message: '', receivedBytes: 0, candidate, error: null, authentication: 'apiKey', chatgptAccountId: null };
  globalThis.__invoke = async (command, args) => {
    if (command === 'record_usage') return null;
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
  const container = document.createElement('div'); document.body.append(container);
  const root = createRoot(container);
  const settings = connected => ({ readiness: { ready: connected }, credential: { configured: connected }, configurationError: null, settings: {}, providers: [], authentication: 'apiKey', chatgpt: { accounts: [], activeAccountId: null } });
  function Host() {
    const [configured, setConfigured] = useState(connected);
    return <ImportContext initialSource={initialSource} busy={false} settings={settings(configured)} openSettings={() => setConfigured(true)}
      importInto={async (...args) => { kept.push(args); }} onPagesImported={result => imported.push(result)} close={() => { closed += 1; }} />;
  }
  flushSync(() => root.render(<Host />));
  const timer = setInterval(() => {
    const frame = container.querySelector('iframe');
    if (!answerFrames || !frame) return;
    const url = new URL(frame.src), id = url.searchParams.get('id');
    if (answered.has(id)) return;
    answered.add(id);
    window.dispatchEvent(new MessageEvent('message', { origin: url.origin, data: { type: 'desegnos-extract', id, capture: { url: url.searchParams.get('url'), scripts: [], elements: [] } } }));
  }, 5);
  const text = () => container.textContent;
  const button = label => [...container.querySelectorAll('button')].find(item => item.textContent.trim() === label);
  const waitButton = label => until(() => { const found = button(label); return found && !found.disabled ? found : null; }, () => 'enabled button ' + label + ' in: ' + text().slice(0, 300) + ' ## ');
  return {
    calls, kept, imported, text, button, waitButton, closed: () => closed, resolve: () => completeDeferred(),
    checkbox: () => container.querySelector('input[type=checkbox]'),
    async pick(label = 'Choose folder…') { (await waitButton(label)).click(); },
    stop() { clearInterval(timer); root.unmount(); container.remove(); },
  };
}

const tests = {
  async 'brand-only source skips pages and keeps the existing review'() {
    const flow = mountFlow();
    await flow.pick(); (await flow.waitButton('Read my brand'));
    check(!/pages found/.test(flow.text()), 'no pages are offered');
    flow.button('Read my brand').click();
    (await flow.waitButton('Keep brand choices')).click();
    await until(() => flow.kept.length === 1, 'the brand to be kept');
    check(flow.kept[0][0] === 'analysis', 'the analysis was kept');
    check(pickCalls(flow.calls) === 1, 'one source read');
    check(JSON.stringify(flow.calls.find(call => call.command === 'analyze_identity').args) === JSON.stringify({ sourceId: 'shared', fileIds: ['html'] }), 'analysis arguments');
    return flow;
  },
  async 'pages import without AI and retain warnings and skipped files in the panel'() {
    const flow = mountFlow({ connected: false, pageCount: 2 });
    await flow.pick('Choose ZIP…'); await flow.waitButton('Connect AI');
    check(/2 pages found/.test(flow.text()), 'two pages found');
    check(!flow.checkbox(), 'no brand option without AI');
    check(!flow.button('Import 2 pages').disabled, 'import is enabled');
    flow.button('Import 2 pages').click();
    await until(() => flow.imported.length === 1 && flow.text().includes('A picture was skipped.'), 'pages to import');
    check(!flow.calls.some(call => call.command === 'analyze_identity'), 'no brand analysis');
    check(flow.closed() === 0, 'warnings keep the panel open');
    check(flow.text().includes('secret.txt: May contain private information'), 'skipped file explained');
    return flow;
  },
  async 'a clean pages-only import closes the panel'() {
    const flow = mountFlow({ connected: false, pageCount: 2, pageWarnings: [] });
    await flow.pick('Choose ZIP…');
    (await flow.waitButton('Import 2 pages')).click();
    await until(() => flow.closed() === 1, 'the panel to close');
    check(flow.imported.length === 1, 'one import');
    return flow;
  },
  async 'one source imports pages then analyzes its brand by default'() {
    const flow = mountFlow({ pageCount: 1 });
    await flow.pick('Choose HTML files…'); await flow.waitButton('Import page');
    check(flow.checkbox().checked, 'brand reading is on by default');
    check(flow.text().includes('Also read brand and content'), 'brand option shown');
    flow.button('Import page').click();
    await flow.waitButton('Keep brand choices');
    const commands = flow.calls.map(call => call.command);
    check(commands.indexOf('import_website_pages') < commands.indexOf('analyze_identity'), 'pages import before the brand is read');
    check(pickCalls(flow.calls) === 1, 'one source read');
    const picked = flow.calls.find(call => call.command === 'pick_website_import_source').args;
    check(picked.kind === 'file' && typeof picked.extractor === 'string' && picked.extractor.length > 0, 'file pick passes the extractor');
    check(flow.imported.length === 1, 'one import');
    return flow;
  },
  async 'unchecking brand reading imports only pages even with AI connected'() {
    const flow = mountFlow({ pageCount: 1 });
    await flow.pick(); await flow.waitButton('Import page');
    flow.checkbox().click();
    await until(() => !flow.checkbox().checked, 'the checkbox to clear');
    flow.button('Import page').click();
    await until(() => flow.imported.length === 1, 'pages to import');
    await sleep(100);
    check(!flow.calls.some(call => call.command === 'analyze_identity'), 'no brand analysis');
    return flow;
  },
  async 'an import failure stays beside its retry and does not start brand analysis'() {
    const flow = mountFlow({ pageCount: 1, fail: 'import_website_pages' });
    await flow.pick(); (await flow.waitButton('Import page')).click();
    await until(() => flow.text().includes('Import failed here'), 'the failure');
    check(flow.button('Import page'), 'retry stays available');
    check(!flow.calls.some(call => call.command === 'analyze_identity') && flow.closed() === 0, 'nothing else started');
    return flow;
  },
  async 'brand failure retries on the same source without importing pages twice'() {
    const flow = mountFlow({ pageCount: 1, fail: 'analyze_identity' });
    await flow.pick(); (await flow.waitButton('Import page')).click();
    await flow.waitButton('Read my brand');
    await until(() => flow.text().includes('Import failed here'), 'the brand failure');
    check(!flow.button('Import page'), 'pages are not offered again');
    flow.button('Read my brand').click();
    await until(() => flow.calls.filter(call => call.command === 'analyze_identity').length === 2, 'the retry');
    check(flow.calls.filter(call => call.command === 'import_website_pages').length === 1, 'pages imported once');
    check(pickCalls(flow.calls) === 1, 'one source read');
    return flow;
  },
  async 'connecting AI after reading uses the already chosen source'() {
    const flow = mountFlow({ pageCount: 1, connected: false });
    await flow.pick(); (await flow.waitButton('Connect AI')).click();
    await until(() => flow.text().includes('Also read brand and content'), 'the brand option');
    flow.button('Import page').click();
    await flow.waitButton('Keep brand choices');
    check(pickCalls(flow.calls) === 1, 'one source read');
    return flow;
  },
  async 'Stop during rendering never applies pages or starts AI'() {
    const flow = mountFlow({ pageCount: 1, answerFrames: false });
    await flow.pick(); (await flow.waitButton('Import page')).click();
    (await flow.waitButton('Stop')).click();
    await until(() => flow.calls.some(call => call.command === 'cancel_website_import'), 'the cancel');
    await sleep(100);
    check(!flow.calls.some(call => call.command === 'import_website_pages' || call.command === 'analyze_identity'), 'nothing applied after Stop');
    return flow;
  },
  async 'Stop ignores a late source read'() {
    const flow = mountFlow({ pageCount: 1, deferred: 'prepare_website_import_url' });
    const input = await until(() => document.querySelector('input:not([type=checkbox])'), 'the address field');
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, 'example.com');
    input.dispatchEvent(new Event('input', { bubbles: true }));
    (await flow.waitButton('Read')).click();
    (await flow.waitButton('Stop')).click();
    await sleep(50);
    flow.resolve(); await sleep(100);
    check(!flow.button('Import page'), 'the late source is ignored');
    check(!flow.calls.some(call => call.command === 'analyze_identity'), 'no analysis');
    return flow;
  },
  async 'brand-only source without AI offers a connection and no disabled brand control'() {
    const flow = mountFlow({ connected: false });
    await flow.pick(); await flow.waitButton('Connect AI');
    check(!flow.button('Read my brand'), 'no disabled brand button');
    return flow;
  },
  async 'resuming a previous brand keeps its review without another source read'() {
    const flow = mountFlow({ previous: true });
    (await flow.waitButton('Continue with my previous brand')).click();
    await flow.waitButton('Keep brand choices');
    check(pickCalls(flow.calls) === 0, 'no source read');
    return flow;
  },
  async 'an existing identity source opens the review flow without another picker'() {
    const flow = mountFlow({ initialSource: { id: 'own', name: 'Own site', files: [], skipped: [], photos: [], pages: [] } });
    await flow.waitButton('Read my brand');
    check(pickCalls(flow.calls) === 0, 'no picker');
    return flow;
  },
};

try {
  for (const [name, run] of Object.entries(tests)) {
    let flow;
    try { flow = await run(); } catch (error) { throw Error(name + ': ' + error.message); }
    flow?.stop();
  }
  document.querySelector('#result').textContent = 'PASS: ' + Object.keys(tests).length + ' import flows';
} catch (error) { document.querySelector('#result').textContent = 'FAIL: ' + error.message; }
`;
console.log(await runInBrowser({ name: 'import-flow', script, plugins: [tauriStub] }));
