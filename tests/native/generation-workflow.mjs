import assert from 'node:assert/strict';
import { directEditingWorkflow } from './direct-editing-workflow.mjs';
import { createHash, randomUUID } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { waitFor, xpathString } from './webdriver.mjs';
import { assertCanvasOnly, dataBlocks } from './workflow.mjs';
import { verifyArchive } from './file-dialog-workflow.mjs';
export { assertNoSyntheticKeys } from './provider-server.mjs';

const previewFrame = '.live-canvas-page iframe';
const generationDialog = '.workspace-surface .generation-dialog';
const hash = bytes => createHash('sha256').update(bytes).digest('hex');

function dialogButton(driver, text) {
  if (['Test connection', 'Replace key', 'Remove key'].includes(text)) return driver.menu([text], 'Account options');
  if (text === 'Refresh job') return driver.menu(['Advanced', 'Refresh job'], 'Generation options');
  if (text === 'AI settings') return driver.menu(['AI settings'], 'Generation options');
  if (text === 'Discard candidate') return driver.menu(['Discard result'], 'Generation options');
  if (text === 'Discard request') return driver.menu(['Discard request'], 'Generation options');
  if (text === 'Apply reviewed changes') text = 'Apply changes';
  return driver.click(`(//section[contains(@class,"workspace-surface")]//button[normalize-space(.)=${xpathString(text)}])[last()]`, 'xpath');
}

async function closeDialog(driver) {
  const remaining = await driver.execute(() => {
    const dialogs = [...document.querySelectorAll('.workspace-surface')];
    const close = (document.querySelector('.workspace-banner:last-child') ?? dialogs.at(-1))?.querySelector('button[aria-label^="Close "]');
    if (!close) throw new Error('No open native editor dialog to close');
    close.click();
    return dialogs.length - 1;
  });
  await waitFor(() => driver.execute(count => document.querySelectorAll('.workspace-surface').length === count, remaining), 'The top editor dialog did not close');
}

async function latest(driver, projectId) { return driver.invoke('get_generation', { projectId }); }

async function waitJob(driver, projectId, status, timeout = 20_000) {
  const expected = Array.isArray(status) ? status : [status];
  const job = await waitFor(async () => { const value = await latest(driver, projectId); return expected.includes(value?.status) && value; }, `Generation did not reach ${expected.join(' or ')}`, timeout);
  if (await driver.execute(() => !!document.querySelector('.workspace-surface .generation-dialog'))) {
    if (!expected.includes('prepared')) await dialogButton(driver, 'Refresh job');
    await waitFor(() => driver.execute(states => states.includes(document.querySelector('.workspace-surface .generation-status')?.dataset.jobStatus), expected), 'The native generation status did not reach its visible review');
  }
  return job;
}

function assertAttachment(record, job) {
  assert.ok(record, 'The real native provider request was not captured');
  assert.deepEqual(Buffer.from(record.markdown, 'utf8'), Buffer.from(job.requirements.markdown, 'utf8'));
  assert.equal(record.sha256, job.requirements.sha256);
  assert.deepEqual(record.metadata, { generationMode: job.generationMode, jobId: job.jobId, requirementsSha256: job.requirements.sha256 });
  assert.equal(record.body.max_output_tokens, job.settings.maxOutputTokens);
  assert.equal(record.body.reasoning.effort, job.settings.reasoningEffort);
  assert.equal(record.body.store, false);
  assert.equal(record.body.stream, true);
  assert.equal(record.body.service_tier, 'default');
  assert.deepEqual(record.body.tools, [{type:'web_search'}]);
  assert.equal(record.body.text.format.strict, true);
  assert.equal(record.body.text.format.schema.additionalProperties, false);
}

async function installPreviewObserver(driver) {
  await driver.execute(selector => {
    if (window.__desegniFixtureObserver) window.removeEventListener('message', window.__desegniFixtureObserver);
    window.__desegniFixtureMessages = []; window.__desegniFixtureLoads = 0;
    window.__desegniFixtureObserver = event => {
      const frame = document.querySelector(selector);
      if (event.source !== frame?.contentWindow || typeof event.data !== 'object' || !event.data) return;
      if (!['desegni-native-fixture', 'desegni-native-inline-script-ran'].includes(event.data.type)) return;
      window.__desegniFixtureMessages.push({ origin: event.origin, ...event.data });
      window.__desegniFixtureMessages = window.__desegniFixtureMessages.slice(-64);
    };
    window.addEventListener('message', window.__desegniFixtureObserver);
  }, previewFrame);
}

function messages(driver) { return driver.execute(() => window.__desegniFixtureMessages ?? []); }

async function frameState(driver) {
  return waitFor(() => driver.execute(selector => {
    const frame = document.querySelector(selector); if (!frame) return null;
    const rect = frame.getBoundingClientRect();
    return { url: frame.src, sandbox: frame.getAttribute('sandbox'), left: rect.left, top: rect.top, width: rect.width, height: rect.height, clientLeft: frame.clientLeft, clientTop: frame.clientTop };
  }, previewFrame), 'The actual isolated preview iframe did not mount');
}

async function setScripts(driver, physical, enabled) {
  if (enabled) {
    if (!await driver.execute(() => !!document.querySelector('.live-canvas-page'))) await driver.menu(['Live preview']);
    return frameState(driver);
  }
  if (await driver.execute(() => !!document.querySelector('.live-canvas-page'))) await driver.button('Exit live');
}

async function clickChild(driver, physical, rect) {
  await driver.execute(selector => document.querySelector(selector).scrollIntoView({ block: 'center' }), previewFrame);
  const frame = await frameState(driver);
  const point = { x: frame.left + frame.clientLeft + rect.x + rect.width / 2, y: frame.top + frame.clientTop + rect.y + rect.height / 2 };
  assert.equal(await driver.execute((selector, at) => document.elementFromPoint(at.x, at.y) === document.querySelector(selector), previewFrame, point), true, 'The real preview target is covered or outside the viewport');
  await physical.click(point);
}

export async function dialogLayout(driver, output, name) {
  const result = await driver.execute(() => {
    const dialog = [...document.querySelectorAll('.workspace-surface')].at(-1);
    const bounds = dialog.getBoundingClientRect();
    const style = getComputedStyle(document.documentElement);
    const palette = Object.fromEntries(['text', 'muted', 'background', 'canvas-bed', 'border', 'surface', 'surface-raised', 'surface-muted', 'selected', 'accent', 'accent-text', 'danger', 'error-surface', 'warning-surface', 'focus'].map(key => [key, style.getPropertyValue('--' + key).trim()]));
    const canvasStyle = getComputedStyle(document.querySelector('[data-testid="canvas"]'));
    for (const key of ['annotation-surface', 'annotation-focus', 'alignment-color']) palette[key] = canvasStyle.getPropertyValue('--' + key).trim();
    const actions = dialog.querySelector('.credential-status .inline-actions');
    const explanation = dialog.querySelector('.credential-status .field-help');
    const credentialHelpGap = actions && explanation ? explanation.getBoundingClientRect().top - actions.getBoundingClientRect().bottom : null;
    return { theme: document.documentElement.dataset.theme, viewport: innerWidth, bodyWidth: document.body.scrollWidth, dialog: { left: bounds.left, right: bounds.right, clientWidth: dialog.clientWidth, scrollWidth: dialog.scrollWidth }, credentialHelpGap, palette };
  });
  assert.equal(result.viewport, 760);
  assert.ok(result.bodyWidth <= result.viewport && result.dialog.left >= 0 && result.dialog.right <= result.viewport && result.dialog.scrollWidth <= result.dialog.clientWidth, 'The BYOK dialog overflows the supported desktop window');
  if (result.credentialHelpGap !== null) assert.ok(result.credentialHelpGap >= 7, 'The saved-key explanation overlaps its access buttons');
  const luminance = color => {
    assert.match(color, /^#(?:[0-9a-f]{3}|[0-9a-f]{6})$/i);
    if (color.length === 4) color = '#' + [...color.slice(1)].map(value => value + value).join('');
    const values = [1, 3, 5].map(index => parseInt(color.slice(index, index + 2), 16) / 255).map(value => value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4);
    return values[0] * 0.2126 + values[1] * 0.7152 + values[2] * 0.0722;
  };
  const pairs = [['text', 'surface'], ['text', 'surface-raised'], ['text', 'surface-muted'], ['text', 'background'], ['text', 'selected'], ['muted', 'surface'], ['muted', 'surface-raised'], ['muted', 'background'], ['danger', 'error-surface'], ['danger', 'surface'], ['text', 'warning-surface'], ['accent-text', 'accent'], ['focus', 'surface'], ['focus', 'surface-raised'], ['text', 'canvas-bed'], ['muted', 'canvas-bed'], ['muted', 'surface-muted'], ['muted', 'selected'], ['border', 'canvas-bed'], ['border', 'surface'], ['focus', 'canvas-bed'], ['focus', 'selected'], ['annotation-focus', 'annotation-surface'], ['alignment-color', 'canvas-bed'], ['alignment-color', 'annotation-surface']];
  for (const key of ['background', 'surface', 'surface-raised', 'surface-muted', 'canvas-bed', 'text', 'muted', 'border', 'annotation-surface']) {
    let color = result.palette[key];
    if (color.length === 4) color = '#' + [...color.slice(1)].map(value => value + value).join('');
    assert.match(color, /^#([0-9a-f]{2})\1\1$/i, `${name}: ${key} must use a neutral grey`);
  }
  result.contrast = pairs.map(([foreground, background]) => {
    const a = luminance(result.palette[foreground]), b = luminance(result.palette[background]);
    const ratio = (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
    const minimum = ['focus', 'border', 'annotation-focus', 'alignment-color'].includes(foreground) ? 3 : 4.5;
    assert.ok(ratio >= minimum, `${name}: ${foreground}/${background} contrast ${ratio} is below ${minimum}`);
    return { foreground, background, ratio, minimum };
  });
  await writeFile(join(output, `${name}.json`), JSON.stringify(result, null, 2));
  await writeFile(join(output, `${name}.png`), Buffer.from(await driver.screenshot(), 'base64'));
}

async function settingsLayout(driver, physical, step, output) {
  await step('AI settings fit 760px in both themes with measured contrast and native keyboard dismissal', async () => {
    const original = await driver.command('GET', '/window/rect');
    const theme = await driver.execute(() => localStorage.getItem('desegni.editor-theme') ?? 'system');
    await driver.command('POST', '/window/rect', { width: 760, height: 900 });
    try {
      for (const selected of ['light', 'dark']) {
        await driver.select('Editor theme', selected);
        await waitFor(() => driver.execute(value => document.documentElement.dataset.theme === value, selected), 'The editor theme did not change');
        const point = await driver.execute(() => { const button = document.querySelector('.app-bar button[aria-label="AI settings"]'); const r = button.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; });
        await physical.click(point);
        await waitFor(() => driver.execute(() => !!document.querySelector('.ai-settings form')), 'AI settings did not finish loading');
        await dialogLayout(driver, output, `settings-${selected}-760px`);
        await physical.keys('Tab');
        assert.equal(await driver.execute(() => !!document.activeElement?.closest('.workspace-surface')), true);
        await physical.keys('Escape');
        await waitFor(() => driver.execute(() => !document.querySelector('.workspace-surface')), 'Escape did not close AI settings');
        assert.equal(await driver.execute(() => document.activeElement?.getAttribute('aria-label')), 'AI settings');
      }
    } finally {
      if (await driver.execute(() => !!document.querySelector('.workspace-surface'))) await closeDialog(driver);
      await driver.command('POST', '/window/rect', original); await driver.select('Editor theme', theme);
    }
  });
}

async function credentialWorkflow(driver, provider, step) {
  await step('AI settings expose verified model capabilities and persist the selected native limits', async () => {
    await driver.button('AI settings');
    await waitFor(() => driver.execute(() => !!document.querySelector('.workspace-surface .ai-settings input[type="number"]')), 'The native AI settings form did not load');
    const initial = await driver.invoke('get_ai_settings');
    assert.equal(initial.credential.storage, 'none'); assert.equal(initial.credential.configured, false);
    const model = initial.catalogue.find(value => value.id === 'gpt-6.1-sol');
    assert.ok(model && model.verifiedAt === '2026-10-18');
    assert.deepEqual(model.reasoningEfforts, ['low', 'medium', 'high']);
    await driver.click('details.ai-advanced > summary');
    await driver.fill('Maximum output tokens', '4096');
    await driver.select('Reasoning effort', 'low');
    await driver.fill('API budget per generation (USD)', '1.50');
    await dialogButton(driver, 'Save AI settings');
    await waitFor(async () => (await driver.invoke('get_ai_settings')).settings.maxOutputTokens === 4096, 'AI settings were not acknowledged');
    assert.deepEqual((await driver.invoke('get_ai_settings')).settings, { model: 'gpt-6.1-sol', maxOutputTokens: 4096, reasoningEffort: 'low', spendingLimitUsd: 1.5, imageGeneration: initial.settings.imageGeneration });
    assert.equal(provider.requests().length, 0);
  });
  await step('An unavailable system vault requires an explicit session choice and clears the submitted key', async () => {
    await driver.fill('OpenAI API key', provider.key());
    await dialogButton(driver, 'Connect');
    await waitFor(() => driver.execute(() => /System credential storage is unavailable/i.test([...document.querySelectorAll('.workspace-surface .ai-settings [role="alert"]')].map(element => element.textContent).join(' '))), 'The isolated vault failure was not exposed in the key form');
    const failed = await driver.invoke('get_ai_settings');
    assert.equal(failed.credential.configured, false); assert.equal(failed.credential.storage, 'none');
    assert.equal(await driver.execute(() => document.querySelector('.workspace-surface input[type="password"]')?.value ?? ''), '');
    await driver.menu(['Where to keep the key', 'Current session only'], 'Account options');
    await driver.fill('OpenAI API key', provider.key());
    await dialogButton(driver, 'Connect');
    await waitFor(async () => (await driver.invoke('get_ai_settings')).credential.storage === 'session', 'Explicit session credential was not configured');
    const view = await driver.invoke('get_ai_settings');
    assert.equal(view.credential.configured, true); assert.equal(view.credential.maskedIdentifier.endsWith('A001'), true);
    assert.equal(JSON.stringify(view).includes(provider.key()), false);
    assert.equal(await driver.execute(() => document.querySelector('.workspace-surface input[type="password"]')?.value ?? ''), '');
  });
  await step('Test connection, replace and remove use the real provider transport without generation or secret views', async () => {
    await dialogButton(driver, 'Test connection');
    await waitFor(() => provider.modelRequests().length === 1, 'The actual model access GET was not sent');
    await dialogButton(driver, 'Replace key');
    await driver.fill('Replacement API key', provider.key('replacement'));
    provider.expectKey('replacement');
    await dialogButton(driver, 'Connect');
    await waitFor(async () => (await driver.invoke('get_ai_settings')).credential.maskedIdentifier?.endsWith('B002'), 'Replacement session key was not acknowledged');
    await dialogButton(driver, 'Test connection');
    await waitFor(() => provider.modelRequests().length === 2, 'Replacement key model access did not complete');
    await dialogButton(driver, 'Remove key');
    await waitFor(async () => !(await driver.invoke('get_ai_settings')).credential.configured, 'Remove key did not clear the session');
    provider.expectKey('primary');
    await driver.fill('OpenAI API key', provider.key());
    await dialogButton(driver, 'Connect');
    await waitFor(async () => (await driver.invoke('get_ai_settings')).credential.configured, 'The test session key was not restored');
    assert.equal(provider.generationRequests().length, 0);
    assert.deepEqual(provider.errors(), []);
    await closeDialog(driver);
  });
}

export async function generationWorkflow(driver, physical, step, evidence, provider) {
  const output = join(evidence, 'generation'); await mkdir(output, { recursive: true });
  await physical.initialize(output);
  await settingsLayout(driver, physical, step, output);
  await credentialWorkflow(driver, provider, step);
  const workspace = () => driver.invoke('get_workspace');
  const job = async () => latest(driver, (await workspace()).project.id);
  const marker = `canvas-${randomUUID()}`;
  let initial; let applied; let first; let target; let revised; let retained;
  const clearDialogs = async () => { while (await driver.execute(() => !!document.querySelector('.workspace-surface'))) await closeDialog(driver); };
  const request = async text => {
    await clearDialogs();
    const project = (await workspace()).project;
    let area = project.pages[0].regions.find(region => !region.completed);
    if (!area) {
      await driver.execute(() => document.getElementById('page-canvas').focus());
      await physical.raw({ action: 'key', key: 'Return', modifiers: ['Shift'] });
      area = await waitFor(async () => (await workspace()).project.pages[0].regions.find(region => !region.completed), 'A fresh request did not appear');
    }
    const current = (await workspace()).project;
    const number = current.pages[0].regions.findIndex(region => region.id === area.id) + 1;
    await driver.fill(`Request ${number}`, text);
    await waitFor(async () => (await workspace()).project.pages[0].regions.find(region => region.id === area.id)?.prompt === text, 'The revision prompt did not persist');
    return area;
  };
  const completed = async () => {
    const value = await waitFor(async () => { const value = await job(); return value?.status === 'applied' && value; }, 'The result did not appear automatically', 25000);
    await waitFor(() => driver.execute(() => !document.querySelector('dialog[open]')), 'Generation opened a modal');
    return job();
  };
  await step('One Generate click freezes the exact requirements and global mode, sends once and blurs only submitted areas', async () => {
    initial = (await workspace()).project;
    const count = provider.generationRequests().length;
    provider.setMode('waiting', { marker });
    await driver.button('Strict');
    await driver.click('.shell-primary-actions button.primary');
    first = await waitFor(async () => { const value = await job(); return value?.status === 'generating' && value; }, 'One Generate click did not start generation');
    await waitFor(() => driver.execute(() => !!document.querySelector('.generation-log .generation-status')), 'The progress window did not open');
    await waitFor(() => provider.generationRequests().length === count + 1, 'The native request did not reach the provider');
    assertAttachment(provider.generationRequests().at(-1), first);
    assert.equal(first.generationMode, 'strict');
    const state = await driver.execute(() => {
      const dialog = document.querySelector('.workspace-surface');
      const masks = [...document.querySelectorAll('[data-testid="generation-blur"]')];
      return { maskCount: masks.length, masks: masks.map(mask => getComputedStyle(mask).backdropFilter),
        workspaceFilter: getComputedStyle(document.querySelector('.workspace')).filter,
        backdropFilter: document.querySelector('dialog[open]') ? 'modal' : 'none',
        applyButton: [...dialog.querySelectorAll('button')].some(button => /Apply|Send and generate/.test(button.textContent)) };
    });
    assert.equal(state.maskCount, first.requirements.regionCount);
    assert.ok(state.masks.every(value => value.includes('blur')));
    assert.equal(state.workspaceFilter, 'none'); assert.equal(state.backdropFilter, 'none'); assert.equal(state.applyButton, false);
    await physical.captureWindow(join(output, 'generating-only-areas-window.png'));
    await writeFile(join(output, 'scoped-blur.json'), JSON.stringify(state, null, 2));
    provider.release(first.jobId);
    await waitFor(async () => (await job()).receivedBytes > 0, 'Streaming progress did not update');
    provider.release(first.jobId);
    first = await completed(); applied = (await workspace()).project;
    assert.deepEqual(applied.pages[0].content, first.candidate.pages[0].content);
    assert.ok(applied.pages[0].regions.filter(region => region.prompt.trim()).every(region => region.completed));
    assert.equal(applied.latestGeneration.applicationStatus, 'applied');
    assert.deepEqual(applied.decisions, initial.decisions, 'Automatic application silently accepted identity proposals');
    assert.equal(await driver.execute(() => document.querySelectorAll('.annotation textarea').length), initial.pages[0].regions.filter(region => !region.prompt.trim()).length);
    assert.equal(provider.generationRequests().length, count + 1);
    await waitFor(() => driver.execute(() => !!document.querySelector('.static-preview iframe')?.contentDocument?.querySelector('#fixture-action')), 'The generated site did not render on the canvas');
    await physical.captureWindow(join(output, 'generated-canvas-window.png'));
  });
  await directEditingWorkflow(driver, physical, step, output, provider);
  await step('A trusted element click opens an on-canvas prompt with a source-bound structural target at 75% zoom', async () => {
    await driver.select('Zoom', '0.75');
    await driver.press('[data-testid="canvas"]', '\uE011');
    const point = await driver.execute(() => {
      const frame = document.querySelector('.static-preview iframe'); const f = frame.getBoundingClientRect();
      const r = frame.contentDocument.querySelector('#fixture-action').getBoundingClientRect();
      const zoom = Number(document.querySelector('[data-testid="canvas"]').dataset.zoom);
      return { x: f.x + (r.x + r.width / 2) * zoom, y: f.y + (r.y + r.height / 2) * zoom };
    });
    const beforeClick = (await workspace()).project;
    await physical.click(point);
    await driver.executeAsync(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve(true)))));
    assert.deepEqual((await workspace()).project, beforeClick, 'A single click on a generated element must not open a prompt');
    // Start a distinct pair within the same button after testing a single click.
    await physical.doubleClick({ x: point.x + 24, y: point.y });
    target = await waitFor(async () => (await workspace()).project.pages[0].regions.find(region => !region.completed && region.element), 'Double-clicking the generated button did not select its real element');
    assert.equal(target.element.tag, 'button'); assert.match(target.element.selector, /button:nth-of-type\(1\)$/);
    assert.equal(target.element.label, 'Try generated interaction');
    assert.equal(target.element.sourceSha256, hash(Buffer.from(JSON.stringify({ html: applied.pages[0].content.html, css: applied.pages[0].content.css, javascript: applied.pages[0].content.javascript }))));
    assert.deepEqual(target.rect, { x: 20, y: 116, width: 240, height: 42 });
    assert.equal(await driver.execute(() => document.activeElement?.closest('[data-testid="canvas"]') !== null), true);
    assert.equal(await driver.execute(() => document.querySelector('.static-preview iframe').getAttribute('sandbox')), 'allow-same-origin');
    await physical.captureWindow(join(output, 'element-prompt-window.png'));
  });
  await step('Keyboard selection advances beyond nested button content and lets the user return to the button', async () => {
    await driver.execute(() => document.getElementById('page-canvas').focus());
    await physical.keys('Return');
    await waitFor(() => driver.execute(() => document.querySelector('[data-testid="direct-element-selection"]')?.dataset.selector.endsWith('a:nth-of-type(1)')), 'Enter became trapped in the nested button span');
    assert.equal((await workspace()).project.pages[0].regions.filter(region => !region.completed && region.element).length, 0, 'Keyboard selection opened a prompt');
    const point = await driver.execute(() => {
      const frame = document.querySelector('.static-preview iframe'); const f = frame.getBoundingClientRect(); const r = frame.contentDocument.querySelector('#fixture-action').getBoundingClientRect();
      const zoom = Number(document.querySelector('[data-testid="canvas"]').dataset.zoom); return { x: f.x + (r.x + r.width - 6) * zoom, y: f.y + (r.y + r.height / 2) * zoom };
    });
    await physical.doubleClick(point);
    target = await waitFor(async () => (await workspace()).project.pages[0].regions.find(region => !region.completed && region.element?.tag === 'button'), 'The button could not be selected again');
    assert.equal((await workspace()).project.pages[0].regions.filter(region => !region.completed && region.element && !region.prompt.trim()).length, 1);
  });
  await step('Element revision sends its target and existing source, blurs that element, then updates the canvas automatically', async () => {
    await request('Change this button label to Create my page. Keep everything else.');
    provider.setMode('waiting', { marker, buttonLabel: 'Create my page' });
    const count = provider.generationRequests().length;
    await driver.click('.shell-primary-actions button.primary');
    const value = await waitFor(async () => { const value = await job(); return value?.status === 'generating' && value; }, 'The element revision did not start');
    assert.equal(value.requirements.regionCount, 1);
    const blocks = dataBlocks(value.requirements.markdown);
    assert.deepEqual(blocks.find(block => block.regionId === target.id).elementTarget, target.element);
    assert.deepEqual(blocks.find(block => block.pageId === applied.homePageId && 'content' in block).content, applied.pages[0].content);
    assert.deepEqual(value.areas.map(area => area.rect), [target.rect]);
    await physical.captureWindow(join(output, 'generating-element-window.png'));
    await waitFor(() => provider.generationRequests().length === count + 1, 'The targeted request was not received');
    provider.release(value.jobId); await waitFor(async () => (await job()).receivedBytes > 0, 'The target response did not stream'); provider.release(value.jobId);
    revised = await completed();
    const result = (await workspace()).project;
    assert.deepEqual(result.pages[0].content, revised.candidate.pages[0].content);
    assert.ok(result.pages[0].content.html.includes('Create my page'));
    assert.equal(result.pages[0].content.html.replace('Create my page', 'Try generated interaction'), applied.pages[0].content.html);
    assert.equal(result.pages[0].content.css, applied.pages[0].content.css);
    assert.equal(result.pages[0].content.javascript, applied.pages[0].content.javascript);
    assert.equal(await driver.execute(() => document.querySelectorAll('[data-testid="generation-blur"]').length), 0);
  });
  await step('Visible Undo restores the previous site and revision prompt; Redo restores the result without another provider request', async () => {
    const count = provider.generationRequests().length;
    await driver.button('Undo last change');
    await waitFor(async () => (await workspace()).project.pages[0].content.html === applied.pages[0].content.html, 'Undo did not restore the previous site');
    assert.equal((await workspace()).project.pages[0].regions.find(region => region.id === target.id).completed ?? false, false);
    await driver.button('Redo');
    await waitFor(async () => (await workspace()).project.pages[0].content.html.includes('Create my page'), 'Redo did not restore the revised site');
    assert.equal(provider.generationRequests().length, count);
  });
  await step('Explicit retry after an editor reload updates the canvas automatically and never repeats the request on its own', async () => {
    await request('Keep the current native button.'); provider.setMode('truncated');
    await driver.click('.shell-primary-actions button.primary');
    const failed = await waitFor(async () => { const value = await job(); return value?.status === 'unknown' && value; }, 'The interrupted fixture did not finish');
    const count = provider.generationRequests().length;
    await clearDialogs();
    await driver.execute(() => { window.__beforeWorkflowReload = true; });
    await physical.raw({ action: 'key', key: 'R', modifiers: ['Control', 'Shift'] });
    await waitFor(async () => { try { return await driver.execute(() => !window.__beforeWorkflowReload && !!document.querySelector('[data-testid="canvas"]')); } catch { return false; } }, 'The native editor did not reload', 12000);
    await physical.initialize(output);
    await driver.execute(() => {
      window.__desegniTestErrors = [];
      window.addEventListener('error', event => window.__desegniTestErrors.push({ kind: 'error', message: event.message }));
      window.addEventListener('unhandledrejection', event => window.__desegniTestErrors.push({ kind: 'rejection', message: String(event.reason) }));
    });
    assert.equal((await job()).jobId, failed.jobId); await delay(300);
    assert.equal(provider.generationRequests().length, count);
    await driver.button('Generation status'); await driver.button('Details');
    await driver.button('Retry same snapshot');
    provider.setMode('complete', { marker, buttonLabel: 'Create my page', chunkDelayMs: 2 });
    await driver.button('Send paid retry');
    const retried = await completed();
    assert.equal(retried.jobId, failed.jobId); assert.equal(retried.attempts.length, 2);
    assert.equal(retried.requirements.sha256, failed.requirements.sha256);
    assert.equal(provider.generationRequests().length, count + 1);
  });
  await step('Tall canvas content keeps a stable extent through prompt edits and pauses high-specificity CSS animation', async () => {
    await request('Keep this tall page and its button.');
    provider.setMode('complete', { marker, extraCss: 'body{min-height:100vh;margin:8px}main{min-height:1700px}#fixture-action{animation:fixture-motion 1s infinite!important;animation-play-state:running!important}@keyframes fixture-motion{to{transform:translateX(10px)}}', chunkDelayMs: 2 });
    await driver.click('.shell-primary-actions button.primary'); await completed();
    await waitFor(() => driver.execute(() => document.querySelector('.static-preview iframe')?.clientHeight >= 1700), 'The canvas did not expand for tall content');
    const height = await driver.execute(() => document.querySelector('.static-preview iframe').clientHeight);
    await delay(500); assert.equal(await driver.execute(() => document.querySelector('.static-preview iframe').clientHeight), height);
    assert.equal(await driver.execute(() => document.querySelector('.static-preview iframe').contentDocument.getAnimations().every(animation => animation.playState === 'paused')), true);
    await request('Keep the tall preview while I type this revision.');
    assert.equal(await driver.execute(() => document.querySelector('.static-preview iframe').clientHeight), height);
    assert.ok((await workspace()).project.pages[0].content.css.includes('animation-play-state:running!important'));
  });
  await step('Invalid, refused, interrupted and conflicting output keeps the existing site and requires an explicit next action', async () => {
    for (const [kind, expected] of [['malformed', 'failed'], ['refusal', 'failed'], ['truncated', 'unknown'], ['conflict', 'blocked'], ['auth', 'failed'], ['quota', 'failed'], ['rate-limit', 'failed']]) {
      await request(`A controlled ${kind} revision`);
      const before = (await workspace()).project.pages[0].content;
      const count = provider.generationRequests().length; provider.setMode(kind);
      await driver.click('.shell-primary-actions button.primary');
      const failed = await waitFor(async () => { const value = await job(); return value?.status === expected && value; }, `The ${kind} outcome did not appear`);
      assert.deepEqual((await workspace()).project.pages[0].content, before);
      assert.equal(failed.attempts.length, 1); await delay(800); assert.equal(provider.generationRequests().length, count + 1);
      assert.equal(await driver.execute(() => document.querySelectorAll('[data-testid="generation-blur"]').length), 0);
      await clearDialogs();
    }
  });
  await step('Cancel stops waiting and keeps the previous site; a late provider result cannot apply', async () => {
    await request('A request that will be cancelled'); provider.setMode('waiting');
    const before = (await workspace()).project.pages[0].content;
    await driver.click('.shell-primary-actions button.primary');
    const running = await waitFor(async () => { const value = await job(); return value?.status === 'generating' && value; }, 'Cancel fixture did not start');
    await driver.button('Cancel generation');
    await waitFor(async () => (await job()).status === 'cancelled', 'Cancel did not complete');
    provider.release(running.jobId); await delay(500);
    assert.deepEqual((await workspace()).project.pages[0].content, before);
    await clearDialogs();
  });
  await step('An edit made while generation is running fails the stale-source guard and never replaces the current site', async () => {
    const area = await request('Original frozen request'); provider.setMode('waiting');
    const before = (await workspace()).project.pages[0].content;
    await driver.click('.shell-primary-actions button.primary');
    const running = await waitFor(async () => { const value = await job(); return value?.status === 'generating' && value; }, 'The stale fixture did not start');
    await clearDialogs();
    const project = (await workspace()).project;
    await driver.fill(`Request ${project.pages[0].regions.findIndex(region => region.id === area.id) + 1}`, 'Keep this newer request');
    await waitFor(async () => (await workspace()).project.pages[0].regions.find(region => region.id === area.id).prompt === 'Keep this newer request', 'The newer request did not save');
    provider.release(running.jobId); await waitFor(async () => (await job()).receivedBytes > 0, 'Stale response did not stream'); provider.release(running.jobId);
    await waitFor(() => driver.execute(() => document.querySelector('.generation-progress [data-job-status]')?.dataset.jobStatus === 'blocked'), 'Stale application did not show its blocked outcome');
    assert.match((await job()).message, /stale|changed/i);
    assert.deepEqual((await workspace()).project.pages[0].content, before);
    await clearDialogs();
  });
  await step('The progress window fits both grey themes at 760px and leaves unaffected canvas sharp', async () => {
    await request('A compact-window progress check'); provider.setMode('waiting');
    await driver.click('.shell-primary-actions button.primary');
    await waitFor(async () => (await job()).status === 'generating', 'Progress layout fixture did not start');
    const original = await driver.command('GET', '/window/rect');
    const theme = await driver.execute(() => localStorage.getItem('desegni.editor-theme') ?? 'system');
    try {
      await driver.command('POST', '/window/rect', { width: 760, height: 900 });
      for (const selected of ['light', 'dark']) {
        await clearDialogs(); await driver.select('Editor theme', selected); await driver.button('Generation status');
        await dialogLayout(driver, output, `progress-${selected}-760px`);
        await physical.captureWindow(join(output, `progress-${selected}-window.png`));
      }
    } finally { await clearDialogs(); await driver.command('POST', '/window/rect', original); await driver.select('Editor theme', theme); }
    await driver.button('Generation status'); await driver.button('Cancel generation'); await clearDialogs();
  });
  await step('Static canvas rejects forged messages and stale element fingerprints without modifying the project', async () => {
    const before = (await workspace()).project;
    await driver.execute(() => window.postMessage({ type: 'desegni-select', selector: 'body', pageId: 'forged', rect: { x: 0, y: 0, width: 1, height: 1 } }, '*'));
    await delay(150); assert.deepEqual((await workspace()).project, before);
    await assert.rejects(() => driver.invoke('edit_project', { revision: before.revision, command: { type: 'add_element_region', page_id: before.homePageId, rect: target.rect, element: target.element } }), /page changed|Select the element/);
    assert.deepEqual((await workspace()).project, before);
  });
  // Finish with a valid site whose links point to a retained, script-enabled package.
  await request('Keep the native button and its explanation.');
  retained = await driver.invoke('create_preview', { pageId: applied.homePageId, jobId: null, runScripts: true });
  provider.setMode('complete', { marker, retainedPreviewUrl: retained.url, chunkDelayMs: 2 });
  await driver.click('.shell-primary-actions button.primary'); await completed();
  await step('Selectable canvas keeps authored scripts paused and invalidates selection after navigation to an enabled package', async () => {
    await waitFor(() => driver.execute(() => !!document.querySelector('.static-preview iframe')?.contentDocument?.querySelector('#fixture-action')), 'The canvas did not render');
    await driver.execute(() => { window.__canvasMessages = []; window.addEventListener('message', event => { if (event.source === document.querySelector('.static-preview iframe')?.contentWindow) window.__canvasMessages.push(event.data); }); });
    const before = (await workspace()).project;
    await delay(200); assert.deepEqual(await driver.execute(() => window.__canvasMessages), []);
    await driver.execute(url => { document.querySelector('.static-preview iframe').contentWindow.location.href = url; }, retained.url);
    await waitFor(() => driver.execute(() => !!document.querySelector('.static-preview .preview-error')), 'Navigating the canvas did not invalidate selection');
    await delay(300); assert.deepEqual(await driver.execute(() => window.__canvasMessages), []);
    assert.deepEqual((await workspace()).project, before); assert.deepEqual(provider.networkProbes(), []);
  });
  await step('Explicit interactive preview stays opaque and denies parent DOM, native IPC and network', async () => {
    await driver.menu(['Live preview']); await installPreviewObserver(driver); await setScripts(driver, physical, true);
    const ready = await waitFor(async () => (await messages(driver)).find(value => value.event === 'ready'), 'The explicit interactive preview did not execute');
    assert.equal(ready.origin, 'null'); assert.equal(ready.detail.parentDenied, true);
    assert.equal(ready.detail.tauriAbsent, true); assert.equal(ready.detail.internalsAbsent, true); assert.equal(ready.detail.ipcAbsent, true);
    await clickChild(driver, physical, ready.detail.action);
    await waitFor(async () => (await messages(driver)).find(value => value.event === 'interaction'), 'The preview interaction did not run');
    await setScripts(driver, physical, false);
    await waitFor(() => driver.execute(() => !document.querySelector('.live-canvas-page') && !!document.querySelector('.static-preview iframe')), 'Exit live did not restore the static canvas');
    assert.equal(await driver.execute(() => document.querySelector('.static-preview iframe').getAttribute('sandbox')), 'allow-same-origin');
    await delay(300); assert.deepEqual(provider.networkProbes(), []);
    await clearDialogs(); await driver.invoke('release_preview', { sessionId: retained.sessionId });
  });
  await step('Save preserves the generated site and automatic dossier in the real project archive', async () => {
    await driver.button('Save'); await waitFor(async () => !(await workspace()).dirty, 'The generated project did not save');
    const value = await workspace(); await verifyArchive(value.path, value.project, await driver.invoke('get_project_notes'));
    assert.equal(value.project.latestGeneration.applicationStatus, 'applied');
    assert.deepEqual(provider.errors(), []); assert.deepEqual(provider.networkProbes(), []);
    await writeFile(join(output, 'native-views.json'), JSON.stringify({ project: value.project, job: await job(), requests: provider.requests() }, null, 2), { mode: 0o600 });
  });
  // Leave a genuine editable request for the account-bound native coordinator contracts.
  await request('Keep the native button and explain its behavior.');
  return (await workspace()).project;
}
