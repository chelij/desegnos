// Exercise the real settings component in a headless browser with account-scoped fixtures.
// The Rust transport tests separately verify discovery and submission over loopback HTTP.
import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { promisify } from 'node:util';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';

const root = fileURLToPath(new URL('../', import.meta.url));
await mkdir(join(root, 'test-results'), { recursive: true });
const folder = await mkdtemp(join(root, 'test-results/ai-models-'));
const fixture = join(folder, 'index.html');
let server;
try {
  await writeFile(fixture, `<!doctype html><html><body style="width:380px;padding:16px"><div id="root"></div><pre id="result">pending</pre>
  <script type="module">
  import React from 'react';
  import { createRoot } from 'react-dom/client';
  import { AiSettings } from '/src/AiSettings.tsx';
  import '/src/styles.css';
  import '/src/brand.css';
  import '/src/notice.css';
  import '/src/ui.css';
  const check = (ok, message) => { if (!ok) throw Error(message); };
  const wait = async (read, message) => {
    for (let i = 0; i < 200; i++) { if (read()) return; await new Promise(resolve => setTimeout(resolve, 20)); }
    throw Error(message);
  };
  const model = (id, efforts = ['low', 'medium', 'high']) => ({id, name: id, reasoningEfforts: efforts});
  const account = id => ({id, label: id, connected: true, planEnabled: true, storageRepairRequired: false, storage: 'session', email: null});
  const emptyKey = {configured: false, storage: 'none', maskedIdentifier: null, vaultError: null};
  let current = {
    authentication: 'chatgpt', provider: 'openai',
    providers: ['openai','anthropic','google','openrouter','local'].map(id => ({id, name:id, available:true, signInSupported:id === 'openai', capabilities:{webSearch:true}})),
    settings: {model:'gpt-6.1-sol', reasoningEffort:'medium', maxOutputTokens:32768, spendingLimitUsd:2, imageGeneration:false},
    catalogue: [model('gpt-6.1-sol'), model('future-account-model', ['adaptive', 'max'])],
    credential: emptyKey, imageKey: emptyKey, imageAccess: null, imageAccessChecking: false, configurationError:null,
    chatgpt: {accounts:[account('first')], activeAccountId:'first', models:[], modelsError:null,
      signIn:{status:'idle', message:null}, storageError:null, showWelcome:false, message:null}
  };
  const reads = [];
  let saved;
  let connectedUrl;
  const root = createRoot(document.getElementById('root'));
  const refresh = async flag => { reads.push(flag); return current; };
  const update = async (command, args) => {
    if (command === 'save_ai_settings') {
      if (args.connectionOnly) { connectedUrl = args.settings.endpoint; current = {...current, settings:{...current.settings, endpoint:connectedUrl}}; }
      else { saved = structuredClone(args.settings); current = {...current, settings:saved}; }
    }
    else {
      check(command === 'get_ai_settings' && args.refreshModels === true, 'Refresh must request account discovery');
      current = {...current, catalogue:[...current.catalogue.map(item => item.id === 'future-account-model' ? model(item.id, ['next_effort']) : item), model('just-added-account-model')]};
    }
    render(); return current;
  };
  function render() { root.render(React.createElement(AiSettings, {current, refresh, update, manageUsage:async()=>{}, pollingError:''})); }
  const picker = () => document.querySelector('.ai-model-row select');
  const ids = () => [...picker().options].filter(option => !option.disabled).map(option => option.value);
  try {
    render();
    await wait(() => picker(), 'Model picker did not load');
    check(reads[0] === true, 'Opening settings must refresh account models');
    check(ids().includes('future-account-model'), 'New account model was filtered out');
    const sameLine = (first, second) => {
      const a = first.getBoundingClientRect(), b = second.getBoundingClientRect();
      return Math.abs(a.top + a.height/2 - b.top - b.height/2) < 3;
    };
    check(sameLine(document.querySelector('[aria-label="Refresh models"]'), document.querySelector('.ai-usage-link')), 'Model Refresh and Usage must share one line');
    check(sameLine(document.querySelector('label[for="ai-image-generation"]'), document.getElementById('ai-image-status')), 'Image generation label and status must share one line');
    check(sameLine(document.getElementById('ai-image-status'), document.querySelector('.image-generation-access .ai-refresh')), 'Image status and Refresh must share one line');
    picker().value = 'future-account-model';
    picker().dispatchEvent(new Event('change', {bubbles:true}));
    await wait(() => document.querySelector('.ai-save-actions button.primary'), 'Selection was not editable');
    document.querySelector('.generation-settings-form').requestSubmit();
    await wait(() => saved?.model === 'future-account-model', 'Selected model did not save');
    check(saved.reasoningEffort === 'adaptive', 'Advertised reasoning option was not selected');
    await wait(() => !document.querySelector('[aria-label="Refresh models"]').disabled, 'Refresh stayed disabled after save');
    document.querySelector('[aria-label="Refresh models"]').click();
    await wait(() => ids().includes('just-added-account-model'), 'Refreshed model did not appear');
    await wait(() => document.querySelector('.ai-save-actions button.primary'), 'Changed reasoning options did not produce an editable draft');
    document.querySelector('.generation-settings-form').requestSubmit();
    await wait(() => saved?.reasoningEffort === 'next_effort', 'Changed reasoning option did not save');
    check(!document.body.textContent.includes('Catalogue verified'), 'Plan models must not claim static catalogue verification');
    current = {...current, catalogue:[model('other-account-model')], chatgpt:{...current.chatgpt, accounts:[account('second')], activeAccountId:'second'}};
    render();
    await wait(() => ids().length === 1 && ids()[0] === 'other-account-model', 'Previous account models survived account switch');
    for (const provider of ['openai', 'anthropic', 'google', 'openrouter', 'local']) {
      const id = 'vendor/future-' + provider + ':latest';
      const quoted = provider === 'openrouter' || provider === 'local';
      const pricing = {model:id, inputUsdPerMillion:provider === 'local' ? 0 : 20, outputUsdPerMillion:provider === 'local' ? 0 : 80, requestUsd:provider === 'local' ? 0 : 0.01, imageUsd:provider === 'local' ? 0 : 0.02};
      current = {...current, provider, authentication:'apiKey', credential:{...emptyKey, configured:true, storage:'session'},
        settings:{...current.settings, model:'unavailable', reasoningEffort:'none', maxOutputTokens:32768, pricing:undefined, endpoint:provider === 'local' ? 'http://127.0.0.1:11434/v1' : undefined, localAuthentication:false},
        catalogue:[{...model(id, ['none']), pricingRequired:provider !== 'openrouter', defaultPricing:quoted ? pricing : undefined, maxOutputTokens:4096}]};
      render();
      await wait(() => ids().includes(id), provider + ': discovery did not reach picker');
      check(!document.querySelector('[aria-label="Refresh models"]').disabled, provider + ': refresh must work without an available saved model');
      picker().value = id;
      picker().dispatchEvent(new Event('change', {bubbles:true}));
      await wait(() => document.querySelector(provider === 'openrouter' ? '.ai-save-actions button.primary' : '[aria-label="Input price ceiling"]'), provider + ': pricing draft did not appear');
      const form = document.querySelector('.generation-settings-form');
      if (!quoted) check(!form.checkValidity(), provider + ': missing prices must block submission');
      else check(form.checkValidity(), provider + ': advertised prices must prefill the form');
      for (const [label, value] of quoted ? [] : [['Input price ceiling','20'], ['Output price ceiling','80']]) {
        const input = document.querySelector('[aria-label="' + label + '"]');
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, value);
        input.dispatchEvent(new Event('input', {bubbles:true}));
        await wait(() => input.value === value, 'Price draft did not update');
        await new Promise(resolve => setTimeout(resolve, 20));
      }
      form.requestSubmit();
      await wait(() => saved?.model === id, provider + ': new model did not save');
      check(saved.pricing.inputUsdPerMillion === pricing.inputUsdPerMillion && saved.pricing.outputUsdPerMillion === pricing.outputUsdPerMillion, provider + ': price ceilings did not save');
      if (provider === 'openrouter') check(saved.pricing.requestUsd === 0.01 && saved.pricing.imageUsd === 0.02, 'OpenRouter fees did not save');
      if (provider === 'local') {
        const address = document.querySelector('[aria-label="Local server URL"]');
        check(address.value === 'http://127.0.0.1:11434/v1', 'Local default address was not shown');
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(address, 'http://127.0.0.1:8080/v1');
        address.dispatchEvent(new Event('input', {bubbles:true}));
        await wait(() => document.querySelector('[aria-label="Refresh models"]').disabled, 'Unsaved local address did not block model refresh');
        address.closest('form').requestSubmit();
        await wait(() => connectedUrl === 'http://127.0.0.1:8080/v1', 'Local address did not connect');
      }
      check(saved.maxOutputTokens === 4096, provider + ': output cap ignored provider metadata');
      await wait(() => !document.querySelector('[aria-label="Refresh models"]').disabled, provider + ': refresh stayed disabled');
      document.querySelector('[aria-label="Refresh models"]').click();
      await wait(() => ids().includes('just-added-account-model'), provider + ': refresh did not update model list');
    }
    document.getElementById('result').textContent = 'PASS: account/provider discovery, selection, prices, save, refresh, isolation, and single-line controls';
  } catch (error) { document.getElementById('result').textContent = 'FAIL: ' + error.stack; }
  </script></body></html>`);
  server = await createServer({ root, cacheDir: join(folder, 'cache'),
    server: { host: '127.0.0.1', port: 0, strictPort: false }, optimizeDeps: { entries: [fixture] } });
  await server.listen();
  const port = server.httpServer.address().port;
  const { stdout } = await promisify(execFile)(process.env.CHROMIUM ?? 'chromium', [
    '--headless', '--no-first-run', '--disable-background-networking', '--disable-dev-shm-usage',
    '--no-default-browser-check', '--password-store=basic', '--disable-extensions',
    '--user-data-dir=' + join(folder, 'profile'), '--dump-dom', '--virtual-time-budget=15000',
    'http://127.0.0.1:' + port + fixture.slice(root.length - 1),
  ], { timeout: 45000, maxBuffer: 2 * 1024 * 1024 });
  const result = stdout.match(/<pre id="result">([\s\S]*?)<\/pre>/)?.[1];
  assert.match(result ?? stdout.slice(-2000), /^PASS:/);
  console.log(result);
} finally {
  await server?.close();
  await rm(folder, { recursive: true, force: true });
}
