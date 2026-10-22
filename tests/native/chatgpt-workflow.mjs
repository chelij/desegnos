import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdir, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { waitFor, xpathString } from './webdriver.mjs';
import { dialogLayout } from './generation-workflow.mjs';
import { assertCanvasOnly } from './workflow.mjs';

const settings = driver => driver.invoke('get_ai_settings');
const job = (driver, projectId) => driver.invoke('get_generation', { projectId });
async function button(driver, name) {
  // Account/transport cases deliberately drive the native coordinator; the canvas suite covers user one-click sending/application.
  if (['Send and generate', 'Apply reviewed changes', 'Retry using ChatGPT plan'].includes(name)) {
    const project = (await driver.invoke('get_workspace')).project;
    const current = await job(driver, project.id);
    if (name === 'Retry using ChatGPT plan') await driver.click('(//dialog[@open]//button[normalize-space(.)="Keep current job"])[last()]', 'xpath');
    await driver.invoke(name === 'Send and generate' ? 'start_generation' : name === 'Apply reviewed changes' ? 'apply_generation' : 'retry_generation', {
      jobId: current.jobId, ...(name === 'Apply reviewed changes' ? { acceptedDecisionIndices: [] } : {}),
    });
    await driver.menu(['Advanced', 'Refresh job'], 'Generation options'); return;
  }
  if (['Add account', 'Reconnect', 'Test connection', 'Sign out'].includes(name)) return driver.menu([name], 'Account options');
  if (name === 'Refresh job') return driver.menu(['Advanced', 'Refresh job'], 'Generation options');
  if (name === 'AI settings') return driver.menu(['AI settings'], 'Generation options');
  if (name === 'Discard candidate') return driver.menu(['Discard result'], 'Generation options');
  if (name === 'Discard request') return driver.menu(['Discard request'], 'Generation options');
  if (name === 'Apply reviewed changes') name = 'Apply changes';
  await waitFor(() => driver.execute(text => {
    const top = [...document.querySelectorAll('dialog[open]')].at(-1);
    const target = [...(top?.querySelectorAll('button') ?? [])].findLast(button => button.textContent.trim() === text);
    if (!target || target.disabled || !target.getClientRects().length) return false;
    target.scrollIntoView({ block: 'center' }); return true;
  }, name), `The visible ${name} control did not become available`, 15_000);
  await driver.click(`(//dialog[@open]//button[normalize-space(.)=${xpathString(name)}])[last()]`, 'xpath');
}
const hash = value => createHash('sha256').update(value).digest('hex');
async function close(driver) {
  await driver.execute(() => { [...document.querySelectorAll('dialog[open]')].at(-1).querySelector('button[aria-label^="Close "]').click(); });
  if (await driver.execute(() => !!document.querySelector('dialog[open].generation-progress-modal') && !document.querySelector('dialog[open] .generation-dialog'))) await driver.execute(() => document.querySelector('dialog[open].generation-progress-modal button[aria-label^="Close "]').click());
}
async function openSettings(driver) {
  if (await driver.execute(() => !!document.querySelector('dialog[open] .generation-dialog'))) await button(driver, 'AI settings');
  else await driver.button('AI settings');
  await waitFor(() => driver.execute(() => !!document.querySelector('dialog[open] .ai-settings form')), 'ChatGPT settings did not mount');
}
async function selectMethod(driver, value) {
  const provider = value === 'chatgpt' ? 'openai-chatgpt' : 'openai';
  await driver.select('Provider', provider);
  await waitFor(async () => (await settings(driver)).authentication === value, 'The native authentication preference did not update');
  await waitFor(() => driver.execute(choice => { const select = document.querySelector('.ai-provider select'); return select?.value === choice && !select.disabled; }, provider), 'The editor did not accept the generation method');
}
async function sessionStorage(driver) {
  await driver.menu(['Where to keep the sign-in', 'Current session only'], 'Account options');
  // Reopen the submenu to verify the selected storage, rather than only clicking it.
  await driver.menu(['Where to keep the sign-in'], 'Account options');
  await waitFor(() => driver.execute(() => !!document.querySelector('[role="menuitemradio"][aria-label="Current session only"][aria-checked="true"]')), 'The editor did not accept session sign-in storage');
  for (let level = 0; level < 2; level += 1) {
    await driver.command('POST', '/actions', { actions: [{ type: 'key', id: 'keyboard', actions: [{ type: 'keyDown', value: '\uE00C' }, { type: 'keyUp', value: '\uE00C' }] }] });
    await driver.command('DELETE', '/actions');
  }
}

async function signedIn(driver, previousActive = null) {
  const view = await waitFor(async () => { const value = await settings(driver); return value.chatgpt.signIn.status === 'complete' && value.chatgpt.activeAccountId !== previousActive && value; }, 'Signed loopback OAuth did not complete', 20_000);
  await waitFor(() => driver.execute(() => !!document.querySelector('dialog[aria-label="You\'re using your ChatGPT plan"][open]')), 'The first plan-use confirmation did not appear');
  await button(driver, 'Got it');
  await waitFor(async () => !(await settings(driver)).chatgpt.showWelcome, 'Welcome acknowledgment was not saved');
  return view;
}
async function terminal(driver, projectId, expected) {
  const value = await waitFor(async () => { const value = await job(driver, projectId); return value?.status === expected && value; }, `ChatGPT request did not reach ${expected}`, 20_000);
  await button(driver, 'Refresh job');
  await waitFor(() => driver.execute(status => document.querySelector('dialog[open] .generation-status')?.dataset.jobStatus === status, expected), 'The visible ChatGPT job did not refresh');
  return value;
}
async function prepare(driver, provider, mode) {
  let project = (await driver.invoke('get_workspace')).project;
  if (project.generationMode !== mode) await driver.invoke('edit_project', { revision: project.revision, command: { type: 'set_generation_mode', mode } });
  project = (await driver.invoke('get_workspace')).project;
  const before = provider.generationRequests().length;
  // These checks exercise frozen native job/account contracts independently of the one-click canvas workflow.
  for (const page of project.pages) for (const area of page.regions.filter(area => area.completed && !area.element && area.prompt.trim())) {
    await driver.invoke('edit_project', { revision: (await driver.invoke('get_workspace')).project.revision, command: { type: 'update_region', page_id: page.id, region_id: area.id, rect: area.rect, prompt: area.prompt } });
  }
  project = (await driver.invoke('get_workspace')).project;
  await driver.invoke('prepare_generation');
  await driver.button('Generation status'); await driver.button('Details');
  await waitFor(() => driver.execute(() => !!document.querySelector('dialog[open] .generation-dialog')), 'The ChatGPT request review did not open');
  const request = await terminal(driver, project.id, 'prepared');
  assert.equal(request.authentication, 'chatgpt'); assert.ok(request.chatgptAccountId);
  assert.equal(request.generationMode, mode); assert.equal(request.attempts.length, 0);
  assert.equal(provider.generationRequests().length, before);
  assert.equal(await driver.source('Generation REQUIREMENTS.md source'), null);
  await driver.menu(['Advanced', 'What will be sent'], 'Generation options');
  assert.equal(await driver.source('Generation REQUIREMENTS.md source'), request.requirements.markdown);
  assert.equal(hash(request.requirements.markdown), request.requirements.sha256);
  await driver.menu(['Advanced', 'Request details'], 'Generation options');
  assert.equal(await driver.execute(() => document.querySelector('.generation-facts').textContent.includes('Estimated job cost')), false);
  return { project, request };
}
function attachment(record, request) {
  assert.equal(record.authentication, 'chatgpt');
  assert.equal(record.markdown, request.requirements.markdown);
  assert.deepEqual(record.metadata, { generationMode: request.generationMode, jobId: request.jobId, requirementsSha256: request.requirements.sha256 });
  assert.equal(record.body.reasoning.effort, request.settings.reasoningEffort);
  assert.equal('max_output_tokens' in record.body, false); assert.equal('service_tier' in record.body, false);
}

export async function chatgptWorkflow(driver, physical, step, evidence, provider) {
  const output = join(evidence, 'chatgpt'); await mkdir(output, { recursive: true });
  let first; let second; let accepted;
  await step('ChatGPT authorization can be cancelled with no generation or account replacement', async () => {
    await openSettings(driver); await selectMethod(driver, 'chatgpt'); await sessionStorage(driver);
    provider.oauth.setMode('pending'); const before = provider.generationRequests().length;
    await button(driver, 'Sign in with ChatGPT');
    await waitFor(async () => (await settings(driver)).chatgpt.signIn.status === 'awaitingBrowser', 'The real callback listener did not enter pending sign-in');
    await close(driver); await openSettings(driver);
    await button(driver, 'Cancel sign-in');
    const view = await settings(driver);
    assert.equal(view.chatgpt.signIn.status, 'cancelled'); assert.deepEqual(view.chatgpt.accounts, []);
    assert.equal(provider.generationRequests().length, before);
  });
  await step('A real signed OAuth callback verifies PKCE and exposes only verified supported account models', async () => {
    await sessionStorage(driver);
    provider.oauth.setMode('complete', 'first'); await button(driver, 'Sign in with ChatGPT');
    const view = await signedIn(driver); first = view.chatgpt.accounts.find(value => value.id === view.chatgpt.activeAccountId);
    assert.equal(first.storage, 'session'); assert.equal(first.connected, true); assert.equal(first.planEnabled, true);
    assert.deepEqual(view.catalogue.map(value => value.id), ['gpt-6.1-sol']);
    assert.deepEqual(view.chatgpt.models.map(value => value.id), ['future-unverified-model', 'gpt-6.1-sol']);
    assert.equal(await driver.execute(() => [...document.querySelectorAll('dialog[open] input[type="number"]')].length), 0);
    await button(driver, 'Test connection');
    await waitFor(() => driver.execute(() => document.querySelector('.ai-settings .operation-status')?.textContent === 'Connection checked.'), 'ChatGPT Test connection did not finish');
    const authorization = provider.oauth.records().findLast(value => value.path === '/api/accounts/authorize');
    const exchange = provider.oauth.records().findLast(value => value.grantType === 'authorization_code');
    assert.ok(authorization.initial && exchange.pkceValidated && exchange.redirectValidated && exchange.dynamicClientExcluded);
  });
  await step('ChatGPT account settings fit both 760px themes with measured contrast and keyboard focus', async () => {
    const original = await driver.command('GET', '/window/rect');
    const theme = await driver.execute(() => localStorage.getItem('desegni.editor-theme') ?? 'system');
    await close(driver);
    await driver.command('POST', '/window/rect', { width: 760, height: 900 });
    try {
      for (const selected of ['light', 'dark']) {
        await driver.select('Editor theme', selected);
        await waitFor(() => driver.execute(value => document.documentElement.dataset.theme === value, selected), 'The ChatGPT theme did not change');
        const point = await driver.execute(() => { const button = document.querySelector('.app-bar button[aria-label="AI settings"]'); const r = button.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; });
        await physical.click(point);
        await waitFor(() => driver.execute(() => !!document.querySelector('dialog[open] .ai-settings form')), 'The physically opened ChatGPT settings did not mount');
        await dialogLayout(driver, output, `settings-${selected}-760px`);
        await physical.keys('Tab');
        assert.equal(await driver.execute(() => !!document.activeElement?.closest('dialog[open]')), true);
        await physical.keys('Escape');
        await waitFor(() => driver.execute(() => !document.querySelector('dialog[open]')), 'Escape did not dismiss ChatGPT settings');
        assert.equal(await driver.execute(() => document.activeElement?.getAttribute('aria-label')), 'AI settings');
      }
    } finally { await driver.command('POST', '/window/rect', original); await driver.select('Editor theme', theme); }
    await openSettings(driver);
  });
  await step('Separate ChatGPT registrations remain selectable even when they share an email address', async () => {
    await sessionStorage(driver);
    provider.oauth.setMode('complete', 'second'); await button(driver, 'Add account');
    const view = await signedIn(driver, first.id); second = view.chatgpt.accounts.find(value => value.id === view.chatgpt.activeAccountId);
    assert.notEqual(second.id, first.id); assert.equal(view.chatgpt.accounts.length, 2);
    assert.equal(second.email, first.email); assert.notEqual(second.label, first.label);
    await close(driver);
  });
  await step('Changing the selected account and API preference cannot redirect a prepared ChatGPT job', async () => {
    provider.setMode('complete', { chunkDelayMs: 2, chunkSize: 8192, omitContentType: true, largeStream: true, finalizedItems: true, emptyTerminalOutput: true });
    const prepared = await prepare(driver, provider, 'strict');
    assert.equal(prepared.request.chatgptAccountId, second.id);
    await openSettings(driver); await driver.select('ChatGPT account', first.id);
    await selectMethod(driver, 'apiKey'); assert.equal((await settings(driver)).credential.configured, true);
    await close(driver); await button(driver, 'Send and generate');
    const ready = await terminal(driver, prepared.project.id, 'ready');
    attachment(provider.generationRequests().at(-1), prepared.request);
    assert.equal(provider.generationRequests().at(-1).responseContentType, null);
    assert.deepEqual(provider.generationRequests().at(-1).responsePhases, []);
    assert.equal(provider.generationRequests().at(-1).responseTextParts, 1);
    assert.deepEqual(provider.generationRequests().at(-1).responseWireIndices, [1]);
    assert.equal(provider.generationRequests().at(-1).responseFinalizedItems, 2);
    assert.equal(provider.generationRequests().at(-1).responseEmptyAddedItems, 0);
    assert.equal(provider.generationRequests().at(-1).responseTerminalItems, 0);
    assert.ok(Buffer.byteLength(JSON.stringify(ready.candidate), 'utf8') > 65_536);
    assert.ok(ready.receivedBytes > 65_536);
    assert.ok(ready.candidate.pages[0].content.css.includes('Synthetic stream transport padding.'));
    const secondAuthorization = provider.oauth.records().filter(value => value.path === '/api/accounts/authorize' && value.profile === 'second').at(-1);
    assert.equal(provider.generationRequests().at(-1).clientId, secondAuthorization.clientId);
    assert.equal(ready.chatgptAccountId, second.id); assert.equal(ready.authentication, 'chatgpt');
    await button(driver, 'Apply reviewed changes');
    accepted = await waitFor(async () => { const project = (await driver.invoke('get_workspace')).project; return project.appliedJobIds.includes(ready.jobId) && project; }, 'The reviewed ChatGPT candidate did not commit');
    assert.deepEqual(accepted.pages.find(page => page.id === accepted.homePageId).content, ready.candidate.pages[0].content);
    assert.ok(accepted.appliedJobIds.includes(ready.jobId)); await close(driver);
    await openSettings(driver); await selectMethod(driver, 'chatgpt'); await close(driver);
  });
  await step('Unmatched text parts retain bounded correlation details and never change the site', async () => {
    provider.setMode('complete', { chunkDelayMs: 2, chunkSize: 8192, omitContentType: true, phased: true, splitParts: true, sparse: true, omitFinalizedCommentary: true, missingContentPart: true });
    const before = provider.generationRequests().length;
    const prepared = await prepare(driver, provider, 'strict');
    await driver.menu(['Overview'], 'Generation options'); await button(driver, 'Send and generate');
    const failed = await terminal(driver, prepared.project.id, 'failed');
    assert.equal(failed.candidate, null); assert.equal(failed.attempts.length, 1);
    assert.equal(failed.providerError.responseFormat.rejection, 'textLocation');
    assert.deepEqual(failed.providerError.responseFormat.correlation, { terminalOutput: 'array', terminalItems: 1, terminalTextParts: 2,
      streamedParts: 3, finalizedItems: 3, emptyAddedItems: 1, cause: 'contentPartAbsent', outputIndex: 9, contentIndex: 7 });
    assert.deepEqual((await driver.invoke('get_workspace')).project, prepared.project);
    assert.equal(await driver.execute(() => !!document.querySelector('[aria-label="Provider response details"]')), false);
    const original = await driver.command('GET', '/window/rect');
    const theme = await driver.execute(() => localStorage.getItem('desegni.editor-theme') ?? 'system');
    await close(driver); await driver.command('POST', '/window/rect', { width: 760, height: 900 });
    try {
      for (const selected of ['light', 'dark']) {
        await driver.select('Editor theme', selected); await driver.button('Generation status'); await driver.button('Details');
        await driver.menu(['Advanced', 'Request details'], 'Generation options');
        const details = await waitFor(() => driver.execute(() => document.querySelector('[aria-label="Provider response details"]')?.textContent), 'Stream matching details did not appear');
        assert.match(details, /contentPartAbsent/); assert.match(details, /Output 9 · Content 7/);
        assert.match(details, /3 text parts · 3 finalized items · 1 empty added item/);
        assert.doesNotMatch(details, /msg_|rs_|Preparing the requested page/);
        await driver.execute(() => document.querySelector('[aria-label="Provider response details"]').scrollIntoView({ block: 'center' }));
        await dialogLayout(driver, output, `correlation-${selected}-760px`);
        await physical.captureWindow(join(output, `correlation-${selected}-window.png`));
        await close(driver);
      }
    } finally {
      if (await driver.execute(() => !!document.querySelector('dialog[open]'))) await close(driver);
      await driver.command('POST', '/window/rect', original); await driver.select('Editor theme', theme);
    }
    assert.equal(provider.generationRequests().length, before + 1);
    await driver.button('Generation status'); await driver.button('Details'); await driver.menu(['Overview'], 'Generation options');
    await button(driver, 'Discard candidate'); await terminal(driver, prepared.project.id, 'discarded'); await close(driver);
  });
  await step('Non-stream replies preserve safe diagnostics, show the actual failure, and retry only on explicit approval', async () => {
    const explanation = 'Controlled provider: this request cannot use text.format.';
    for (const [name, kind, expected, options] of [
      ['non-stream-error', 'non-stream-error', 'failed', {}],
      ['non-stream-message', 'non-stream-error', 'failed', { code: null, explanation }],
      ['non-stream-html', 'non-stream-html', 'unknown', { largeReply: true }],
    ]) {
      provider.setMode(kind, options);
      const before = provider.generationRequests().length;
      const prepared = await prepare(driver, provider, 'strict');
      await driver.menu(['Overview'], 'Generation options');
      await button(driver, 'Send and generate');
      const failed = await terminal(driver, prepared.project.id, expected);
      assert.equal(failed.candidate, null); assert.equal(failed.attempts.length, 1);
      assert.equal(failed.providerError.httpStatus, 200);
      assert.equal(failed.providerError.requestId, kind === 'non-stream-error' ? 'req_native_non_stream_error' : 'req_native_non_stream_html');
      assert.equal(failed.providerError.responseFormat.bodyKind, kind === 'non-stream-error' ? 'jsonError' : 'html');
      assert.ok(failed.receivedBytes > 0);
      assert.equal(failed.failureKind, name === 'non-stream-message' ? 'requestRejected' : kind === 'non-stream-error' ? 'chatgptUnsupportedCapability' : 'unexpectedResponseFormat');
      assert.equal(failed.providerError.responseFormat.providerMessage ?? null, name === 'non-stream-message' ? explanation : null);
      if (name === 'non-stream-html') {
        assert.equal(failed.providerError.responseFormat.bodyTruncated, true);
        assert.equal(failed.providerError.responseFormat.limitBytes, 65_536);
        assert.ok(failed.receivedBytes > 65_536);
      }
      assert.doesNotMatch(failed.message, /invalid or inconsistent structured/);
      assert.doesNotMatch(JSON.stringify(failed.providerError), /Bearer|Do not retain/);
      assert.equal(await driver.execute(() => !!document.querySelector('[aria-label="Provider response details"]')), false, 'Technical diagnostics must stay hidden in the default review');
      assert.deepEqual((await driver.invoke('get_workspace')).project, prepared.project, 'A rejected response changed the website or dossier');
      const original = await driver.command('GET', '/window/rect');
      const theme = await driver.execute(() => localStorage.getItem('desegni.editor-theme') ?? 'system');
      await close(driver); await driver.command('POST', '/window/rect', { width: 760, height: 900 });
      try {
        for (const selected of ['light', 'dark']) {
          await driver.select('Editor theme', selected); await driver.button('Generation status'); await driver.button('Details');
          await terminal(driver, prepared.project.id, expected);
          await driver.menu(['Advanced', 'Request details'], 'Generation options');
          const details = await waitFor(() => driver.execute(() => document.querySelector('[aria-label="Provider response details"]')?.textContent), 'The response diagnostics did not appear');
          assert.match(details, /HTTP status200/);
          assert.match(details, kind === 'non-stream-error' ? /text\.format/ : /text\/html/);
          assert.match(details, /req_native_non_stream/);
          if (name === 'non-stream-message') assert.ok(details.includes(explanation), 'The provider explanation was lost when no error code was supplied');
          if (name === 'non-stream-html') {
            assert.match(details, /partial reply/);
            assert.match(details, /Read limit65,?536 bytes/);
          }
          assert.doesNotMatch(details, /Bearer|Do not retain/);
          await driver.execute(() => document.querySelector('[aria-label="Provider response details"]').scrollIntoView({ block: 'center' }));
          await dialogLayout(driver, output, `${name}-${selected}-760px`);
          await physical.captureWindow(join(output, `${name}-${selected}-window.png`));
          await close(driver);
        }
      } finally {
        if (await driver.execute(() => !!document.querySelector('dialog[open]'))) await close(driver);
        await driver.command('POST', '/window/rect', original); await driver.select('Editor theme', theme);
      }
      assert.equal(provider.generationRequests().length, before + 1, 'Reading diagnostics repeated a provider request');
      await driver.button('Generation status'); await driver.button('Details'); await terminal(driver, prepared.project.id, expected);
      if (kind === 'non-stream-html') {
        await button(driver, 'Retry same snapshot');
        assert.equal(provider.generationRequests().length, before + 1, 'Opening retry confirmation submitted a request');
        provider.setMode('complete'); await button(driver, 'Retry using ChatGPT plan');
        const ready = await terminal(driver, prepared.project.id, 'ready');
        assert.equal(ready.attempts.length, 2); assert.equal(ready.attempts[0].status, 'unknown');
        assert.equal(ready.providerError, null);
        assert.equal(ready.requirements.markdown, prepared.request.requirements.markdown);
        assert.equal(ready.requirements.sha256, prepared.request.requirements.sha256);
        assert.equal(ready.chatgptAccountId, prepared.request.chatgptAccountId);
        attachment(provider.generationRequests().at(-1), prepared.request);
        assert.equal(provider.generationRequests().length, before + 2);
      }
      await button(driver, 'Discard candidate'); await terminal(driver, prepared.project.id, 'discarded'); await close(driver);
    }
  });
  await step('Pre-stream and terminal-stream ChatGPT limits reject candidates without API billing fallback', async () => {
    for (const [kind, mode] of [['plan-limit', 'fuzzy'], ['plan-stream-limit', 'strict']]) {
      provider.setMode(kind, { chunkDelayMs: 5 });
      const before = provider.generationRequests().length;
      const prepared = await prepare(driver, provider, mode); assert.equal(prepared.request.chatgptAccountId, first.id);
      await button(driver, 'Send and generate');
      const failed = await terminal(driver, prepared.project.id, 'failed');
      assert.equal(failed.failureKind, 'chatgptUsageLimit'); assert.equal(failed.candidate, null); assert.equal(failed.attempts.length, 1);
      assert.equal(provider.generationRequests().length, before + 1); attachment(provider.generationRequests().at(-1), prepared.request);
      assert.equal(await driver.execute(() => document.querySelector('.generation-actions button.primary')?.textContent.trim()), 'Manage usage');
      await button(driver, 'Manage usage');
      await close(driver);
      const original = await driver.command('GET', '/window/rect');
      const theme = await driver.execute(() => localStorage.getItem('desegni.editor-theme') ?? 'system');
      await driver.command('POST', '/window/rect', { width: 760, height: 900 });
      try {
        for (const selected of ['light', 'dark']) {
          await driver.select('Editor theme', selected);
          await waitFor(() => driver.execute(value => document.documentElement.dataset.theme === value, selected), 'The plan-limit theme did not change');
          await driver.button('Generation status'); await driver.button('Details');
          await terminal(driver, prepared.project.id, 'failed');
          await dialogLayout(driver, output, `${kind}-${selected}-760px`);
          await close(driver);
        }
      } finally {
        if (await driver.execute(() => !!document.querySelector('dialog[open]'))) await close(driver);
        await driver.command('POST', '/window/rect', original);
        await driver.select('Editor theme', theme);
      }
      await driver.button('Generation status'); await driver.button('Details');
      await terminal(driver, prepared.project.id, 'failed');
      await button(driver, 'Discard candidate'); await terminal(driver, prepared.project.id, 'discarded'); await close(driver);
    }
  });
  await step('Sign-out cancels the bound request, revokes its session, and blocks a retry through an existing API key', async () => {
    provider.setMode('slow'); const prepared = await prepare(driver, provider, 'fuzzy');
    await button(driver, 'Send and generate');
    await waitFor(async () => provider.generationRequests().at(-1)?.metadata.jobId === prepared.request.jobId, 'The plan request did not reach the controlled HTTP transport');
    await openSettings(driver); await button(driver, 'Sign out');
    await waitFor(async () => !(await settings(driver)).chatgpt.accounts.find(value => value.id === first.id).connected, 'Sign-out did not clear native access');
    assert.ok(provider.oauth.records().some(value => value.path === '/api/accounts/oauth/revoke' && value.confirmed));
    await selectMethod(driver, 'apiKey'); await close(driver);
    const cancelled = await terminal(driver, prepared.project.id, 'cancelled'); assert.equal(cancelled.candidate, null);
    const count = provider.generationRequests().length;
    await assert.rejects(() => driver.invoke('retry_generation', { jobId: prepared.request.jobId }), /Reconnect|reconnect|Sign in|signed out/);
    assert.equal(provider.generationRequests().length, count);
    assert.equal((await job(driver, prepared.project.id)).attempts.length, 1);
    provider.release(prepared.request.jobId);
    await button(driver, 'Discard candidate'); await terminal(driver, prepared.project.id, 'discarded'); await close(driver);
  });
  await step('Reconnect reuses the registration and host mapping without repeating the welcome acknowledgment', async () => {
    await openSettings(driver); await selectMethod(driver, 'chatgpt'); await sessionStorage(driver);
    provider.oauth.setMode('complete', 'first'); await button(driver, 'Reconnect');
    const view = await waitFor(async () => { const value = await settings(driver); return value.chatgpt.signIn.status === 'complete' && value.chatgpt.accounts.find(account => account.id === first.id).connected && value; }, 'Returning ChatGPT sign-in did not complete');
    assert.equal(view.chatgpt.showWelcome, false); assert.equal(view.chatgpt.accounts.length, 2);
    const authorizations = provider.oauth.records().filter(value => value.path === '/api/accounts/authorize' && value.profile === 'first');
    assert.equal(authorizations.at(-1).initial, false); assert.equal(authorizations.at(-1).clientId, authorizations.find(value => value.kind === 'complete').clientId);
    assert.equal(authorizations.at(-1).hostId, authorizations[0].hostId);
  });
  await step('A verified identity without plan permission remains visible and cannot submit generation', async () => {
    const before = provider.generationRequests().length;
    provider.oauth.setMode('no-permission', 'identity-only');
    await button(driver, 'Add account');
    const view = await waitFor(async () => { const value = await settings(driver); return value.chatgpt.signIn.status === 'complete' && value.chatgpt.accounts.length === 3 && value; }, 'Identity-only OAuth did not complete');
    const account = view.chatgpt.accounts.find(value => value.id === view.chatgpt.activeAccountId);
    assert.equal(account.connected, true); assert.equal(account.planEnabled, false); assert.equal(view.chatgpt.showWelcome, false);
    assert.deepEqual(view.catalogue, []); assert.deepEqual(view.chatgpt.models, []);
    await assert.rejects(() => driver.invoke('prepare_generation'), /grant plan usage|plan usage|plan permission/);
    assert.equal(provider.generationRequests().length, before);
    await waitFor(() => driver.execute(() => [...document.querySelectorAll('.chatgpt-accounts [role="alert"]')].some(element => element.textContent.includes('Plan usage not authorized'))), 'The editor did not show denied plan permission');
    await driver.select('ChatGPT account', first.id);
    await waitFor(async () => (await settings(driver)).catalogue.some(model => model.id === 'gpt-6.1-sol'), 'Account switching did not refresh model discovery');
  });
  await step('Repair secure storage saves a retained rotation through the real UI without replaying OAuth or generation', async () => {
    const flag = join(process.env.XDG_DATA_HOME, 'io.github.chelij.desegni/generation/chatgpt/native-test-vault-write-failure');
    await driver.menu(['Where to keep the sign-in', 'Save securely on this device'], 'Account options');
    provider.oauth.setMode('complete', 'first', { expiresIn: 30 });
    await button(driver, 'Reconnect');
    await waitFor(async () => { const view = await settings(driver); return view.chatgpt.signIn.status === 'complete' && view.chatgpt.accounts.find(account => account.id === first.id).storage === 'system'; }, 'The fault-test account did not accept native system-storage mode');
    const before = provider.generationRequests().length;
    provider.oauth.setRefreshHook(() => writeFile(flag, 'Controlled native vault write failure.\n', { mode: 0o600 }));
    try {
      await button(driver, 'Test connection');
      await waitFor(async () => (await settings(driver)).chatgpt.accounts.find(account => account.id === first.id).storageRepairRequired, 'The rotated bundle was not retained after the controlled secure-write failure');
      await waitFor(() => driver.execute(() => [...document.querySelectorAll('dialog[open] button')].some(button => button.textContent.trim() === 'Repair secure storage' && !button.disabled)), 'The native storage repair action did not appear');
      const oauthCount = provider.oauth.records().length;
      const modelCount = provider.modelRequests().length;
      provider.oauth.setRefreshHook(null); await rm(flag);
      await button(driver, 'Repair secure storage');
      await waitFor(async () => !(await settings(driver)).chatgpt.accounts.find(account => account.id === first.id).storageRepairRequired, 'The exact retained bundle was not saved by Repair');
      assert.equal(provider.oauth.records().length, oauthCount, 'Repair replayed an OAuth request');
      assert.equal(provider.modelRequests().length, modelCount, 'Repair made a model HTTP request');
      assert.equal(provider.generationRequests().length, before, 'Repair made a generation request');
      assert.equal((await settings(driver)).chatgpt.modelsError, null);
    } finally { provider.oauth.setRefreshHook(null); await rm(flag, { force: true }); }
    await writeFile(flag, 'Controlled native vault removal failure.\n', { mode: 0o600 });
    try {
      await button(driver, 'Sign out');
      await waitFor(() => driver.execute(() => document.querySelector('.chatgpt-account-status')?.textContent.includes('Not connected')), 'The editor kept stale connected status after native sign-out disabled access but vault removal failed');
      assert.equal((await settings(driver)).chatgpt.accounts.find(account => account.id === first.id).connected, false);
      assert.equal(provider.generationRequests().length, before);
    } finally { await rm(flag, { force: true }); }
    await sessionStorage(driver); provider.oauth.setMode('complete', 'first');
    await button(driver, 'Reconnect');
    await waitFor(async () => { const view = await settings(driver); return view.chatgpt.signIn.status === 'complete' && view.chatgpt.accounts.find(account => account.id === first.id).storage === 'session'; }, 'Session reconnect did not retire the synthetic system credential');
    await close(driver); await assertCanvasOnly(driver);
    accepted = (await driver.invoke('get_workspace')).project;
    assert.deepEqual(provider.errors(), []);
    await writeFile(join(output, 'oauth-http-evidence.json'), JSON.stringify(provider.oauth.records(), null, 2), { mode: 0o600 });
    await writeFile(join(output, 'provider-requests.json'), JSON.stringify(provider.requests(), null, 2), { mode: 0o600 });
    await writeFile(join(output, 'native-views.json'), JSON.stringify({ settings: await settings(driver), project: accepted, job: await job(driver, accepted.id) }, null, 2), { mode: 0o600 });
    await writeFile(join(output, 'compose.png'), Buffer.from(await driver.screenshot(), 'base64'));
  });
  return accepted;
}
