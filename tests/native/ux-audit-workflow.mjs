import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import { basename, dirname, join, resolve, relative, sep } from 'node:path';
import { pathToFileURL } from 'node:url';
import { chooseFile, chooseFolder, savedState, verifyArchive } from './file-dialog-workflow.mjs';
import { assertCanvasOnly } from './workflow.mjs';
import { waitFor, xpathString } from './webdriver.mjs';
import { inspectChromiumPage } from '../export/chromium.mjs';

const workspace = driver => driver.invoke('get_workspace');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const requestCount = provider => provider.generationRequests().length;
const png = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNQCi3/DwADeQHuOPZfDAAAAABJRU5ErkJggg==';
const png2 = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAYAAABytg0kAAAAEUlEQVR4nGNQCi3/D8IMMAYAP/QHtZw6VX0AAAAASUVORK5CYII=';

async function closed(driver) {
  await waitFor(() => driver.execute(() => !document.querySelector('dialog[open]')), 'The dialog did not close');
}

async function closeDialog(driver) {
  await driver.click('(//dialog[@open])[last()]//button[starts-with(@aria-label,"Close ")]', 'xpath');
  await closed(driver);
}

async function jobAt(driver, projectId, statuses) {
  return waitFor(async () => {
    const value = await driver.invoke('get_generation', { projectId });
    return statuses.includes(value?.status) && value;
  }, `Generation did not reach ${statuses.join('/')}`, 25_000);
}

async function previewReady(driver, heading) {
  await waitFor(async () => {
    const active = await driver.execute(() => document.querySelector('[role="tablist"][aria-label="Project pages"] [aria-selected="true"]')?.id.replace('page-tab-', ''));
    const page = (await workspace(driver)).project.pages.find(page => page.id === active);
    if (!page?.content) return false;
    const digest = hash(JSON.stringify({ html: page.content.html, css: page.content.css, javascript: page.content.javascript }));
    return driver.execute((text, expected) => {
      const preview = document.querySelector('.static-preview'), frame = preview?.querySelector('iframe');
      const content = frame?.contentDocument;
      return preview?.dataset.previewReady === 'true' && preview.dataset.sourceHash === expected && !!content?.querySelector('main') && (!text || content.querySelector('h1')?.textContent === text);
    }, heading ?? null, digest);
  }, 'The accepted source did not become ready on the canvas');
}

async function childPoint(driver, selector, text = false) {
  return driver.execute((query, actualText) => {
    const frame = document.querySelector('.static-preview iframe');
    const element = frame?.contentDocument?.querySelector(query);
    if (!frame || !element) throw new Error(`Missing generated element ${query}`);
    const outer = frame.getBoundingClientRect();
    const zoom = Number(document.querySelector('[data-testid="canvas"]').dataset.zoom);
    let rect = element.getBoundingClientRect();
    if (actualText) {
      const walker = frame.contentDocument.createTreeWalker(element, NodeFilter.SHOW_TEXT);
      let node; while ((node = walker.nextNode())) if (node.textContent.trim()) break;
      if (!node) throw new Error('The text editor test needs a real text run');
      const range = frame.contentDocument.createRange();
      range.setStart(node, 0); range.setEnd(node, Math.min(node.textContent.length, 8));
      rect = range.getClientRects()[0];
    }
    const local = actualText ? { x: rect.x + Math.min(rect.width / 2, 16), y: rect.y + rect.height / 2 } : { x: rect.x + 8, y: rect.y + 8 };
    const point = { x: outer.x + local.x * zoom, y: outer.y + local.y * zoom };
    if (!document.elementFromPoint(point.x, point.y)?.closest('[data-testid="canvas"]')) throw new Error(`Generated element ${query} is outside the visible canvas`);
    return point;
  }, selector, text);
}

async function clickHeading(driver, physical) {
  await previewReady(driver);
  await physical.click(await childPoint(driver, 'h1', true));
  await driver.ready('.element-toolbar button[title^="Change text"]');
}

async function chooseInterpretation(driver, precise) {
  await driver.menu(['AI style', precise ? 'Precise — follow placement closely' : 'Flexible — small adjustments only']);
  await waitFor(async () => (await workspace(driver)).project.generationMode === (precise ? 'strict' : 'fuzzy'), 'The global interpretation choice did not persist');
}

async function openRequest(driver,pageId) {
  const active=await driver.execute(()=>document.querySelector('[role=tab][aria-selected=true]')?.id.replace('page-tab-',''));
  if(active!==pageId) await driver.page(pageId);
  const page=(await workspace(driver)).project.pages.find(page=>page.id===pageId);
  const request=page.regions.find(region=>!region.completed);
  assert.ok(request,'The page needs a pending request');
  await driver.button('Layers');
  await driver.click(`section[aria-label="Layers"] .layer-row[data-region-id="${request.id}"] button.layer-select`);
  await driver.button('Close Layers');
  await driver.click(`.annotation[data-region-id="${request.id}"] textarea`);
}

async function allFiles(directory) {
  const files = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    assert.equal(entry.isSymbolicLink(), false, 'Exported websites must not use source symlinks');
    const path = join(directory, entry.name);
    if (entry.isDirectory()) files.push(...await allFiles(path));
    else { assert.equal(entry.isFile(), true); files.push(path); }
  }
  return files;
}

async function chromium(url, evidencePath) {
  const result = await inspectChromiumPage({ url, outputDirectory: dirname(evidencePath), label: basename(evidencePath, '.html'),
    waitForExpression: "document.documentElement.dataset.localClick === 'passed' && document.documentElement.dataset.localImage === 'true' && document.documentElement.dataset.localCss === 'true' && document.documentElement.dataset.localStylesheet === 'true'",
    expression: 'document.documentElement.outerHTML' });
  await writeFile(evidencePath, result.value);
  await writeFile(`${evidencePath}.report.json`, JSON.stringify({ ...result, value: undefined }, null, 2));
  return result.value;
}

function exportBody(about) {
  return `<main><h1>${about ? 'About our standalone website' : 'Our standalone website'}</h1><p id="fixture-status">Local website preview</p><button id="fixture-action" type="button">Existing generated action</button><a id="fixture-retained" href="#retained">Retained page action</a><form id="fixture-form"><input name="probe" value="local"></form><a id="export-navigation" href="${about ? '/' : '/about'}">${about ? 'Home' : 'About'}</a><img id="export-image" alt="Portable local pixel" src="${png}" srcset="${png} 1x, ${png2} 2x" style="display:block;width:32px;height:32px"><style>.export-css-image{width:32px;height:32px;background-color:#225577;background-image:url("${png}")}</style><div class="export-css-image" aria-label="Portable background"></div><button id="export-action" type="button">Try local action</button><p id="export-script-status">Waiting for local action</p><script>addEventListener('DOMContentLoaded',()=>{document.documentElement.dataset.desegnosRuntime=String(!!(window.__TAURI__||window.__TAURI_INTERNALS__||window.ipc));document.getElementById('export-action').addEventListener('click',()=>{document.getElementById('export-script-status').textContent='Local click script works';document.documentElement.dataset.localClick='passed'});document.getElementById('export-action').click();setTimeout(()=>{const image=document.getElementById('export-image');document.documentElement.dataset.localImage=String(image.complete&&image.naturalWidth===1);document.documentElement.dataset.localCss=String(getComputedStyle(document.querySelector('.export-css-image')).backgroundImage.includes('url('));document.documentElement.dataset.localStylesheet=String(getComputedStyle(document.body).backgroundColor==='rgb(22, 35, 41)');},100);if(location.hash==='#follow-navigation')document.getElementById('export-navigation').click();});</script></main>`;
}

async function replaceMain(driver, physical, about) {
  await previewReady(driver);
  const current=(await workspace(driver)).project;
  const active=await driver.execute(()=>document.querySelector('[role=tab][aria-selected=true]').id.replace('page-tab-',''));
  await driver.invoke('edit_page_source',{projectId:current.id,pageId:active,revision:current.revision,html:exportBody(about)});
  // Save returns the real native workspace to the UI; tab changes alone do not read Rust state.
  await driver.button('Save');
  await previewReady(driver, about ? 'About our standalone website' : 'Our standalone website');
}

export async function uxAuditWorkflow(driver, physical, step, evidence, provider) {
  const output = join(resolve(evidence), 'ux-audit'); await mkdir(output, { recursive: true, mode: 0o700 });
  const projectPath = join(output, 'Audit website.desegnos');
  const longPrompt = 'Create a welcoming studio homepage with a clear heading and one useful action.\n' + Array.from({ length: 85 }, (_, index) => `Detail ${index + 1}: use clear labels and welcoming, readable content.`).join('\n');
  let projectId, homeId, aboutId, cancelled, applied, savedProject, exportedRoot;

  await step('Fresh onboarding explains the creation loop and offers described cards without sidebars', async () => {
    assert.equal((await workspace(driver)).project, null);
    await driver.ready('//button[.//strong[normalize-space(.)="Blank website"]]', 'xpath');
    await driver.ready('//button[.//strong[normalize-space(.)="Use my existing brand"]]', 'xpath');
    assert.match(await driver.execute(() => document.querySelector('.new-project-intro')?.textContent), /describe.*generate.*edit.*export/i);
    assert.equal(await driver.execute(() => !!document.querySelector('input[name="projectName"]')), false);
    await assertCanvasOnly(driver); await physical.initialize(output);
    await physical.captureWindow(join(output, '01-onboarding-window.png'));
  });

  await step('A new mobile Light page fits the screen and always shows a disabled Generate action', async () => {
    await driver.click('//button[.//strong[normalize-space(.)="Blank website"]]', 'xpath');
    await driver.fill('Project name', 'Audit website'); await driver.button('Mobile'); await driver.button('Create project');
    const value = await waitFor(async () => (await workspace(driver)).project, 'The mobile project did not open');
    projectId = value.id; homeId = value.homePageId;
    assert.deepEqual(value.pages[0].viewport, { width: 390, height: 844 }); assert.equal(value.pages[0].appearance, 'light'); assert.equal(value.showGrid, true);
    await driver.ready('[data-testid="canvas"]');
    assert.equal(await driver.execute(() => [...document.querySelectorAll('.shell-primary-actions button')].find(button => button.textContent.trim() === 'Generate')?.disabled), true);
    const fit = await driver.execute(() => { const guide = document.querySelector('[data-testid="screen-guide"]').getBoundingClientRect(), view = document.querySelector('.free-workspace').getBoundingClientRect(); return { top: guide.top, bottom: guide.bottom, viewportTop: view.top, viewportBottom: view.bottom, dockTop: document.querySelector('[aria-label="Canvas tools"]').getBoundingClientRect().top }; });
    assert.ok(fit.top >= fit.viewportTop - 1 && fit.bottom <= fit.viewportBottom + 1 && fit.bottom - fit.top >= (fit.viewportBottom - fit.viewportTop) * 0.75, JSON.stringify(fit));
    await driver.ready('.canvas-empty-hint button'); await physical.captureWindow(join(output, '02-empty-mobile-window.png'));
  });

  await step('Visible canvas tools provide Select, Add request, Pan and zoom while rulers default to pixels', async () => {
    const tools = '[role="toolbar"][aria-label="Canvas tools"]'; await driver.ready(tools);
    for (const text of ['Select', 'Add request', 'Pan', 'Fit']) await driver.ready(`//div[@role="toolbar" and @aria-label="Canvas tools"]//button[normalize-space(.)=${xpathString(text)} or @aria-label=${xpathString(text)}]`, 'xpath');
    const originalZoom = await driver.execute(() => Number(document.querySelector('[data-testid="canvas"]').dataset.zoom));
    await driver.button('Zoom in'); await waitFor(() => driver.execute(value => Number(document.querySelector('[data-testid="canvas"]').dataset.zoom) > value, originalZoom), 'Visible Zoom in did not zoom the canvas');
    await driver.button('Zoom out'); await driver.button('Fit');
    await driver.click(`${tools} button[aria-label="Pan"], ${tools} button[title^="Pan"]`);
    await waitFor(() => driver.execute(() => !!document.querySelector('.free-workspace.tool-pan')), 'Visible Pan did not select its tool');
    await driver.click(`${tools} button[aria-label="Select"], ${tools} button[title^="Select"]`);
    await driver.button('Grid and rulers');
    assert.equal(await driver.execute(() => [...document.querySelectorAll('dialog[open] select')].find(select => [...select.options].some(option => option.value === 'pixels'))?.value), 'pixels');
    await closeDialog(driver); await physical.captureWindow(join(output, '03-visible-tools-window.png'));
  });

  await step('The empty-state Add request action, readable long prompts and one global interpretation reach Generate', async () => {
    const emptyPoint = await driver.execute(() => { const guide = document.querySelector('[data-testid="screen-guide"]').getBoundingClientRect(); return { x: guide.x + guide.width / 4, y: guide.y + 35 }; });
    await physical.doubleClick(emptyPoint);
    await waitFor(async () => (await workspace(driver)).project.pages[0].regions.length === 1, 'Double-click did not add exactly one request');
    await openRequest(driver, homeId);
    await waitFor(() => driver.execute(() => document.activeElement?.tagName === 'TEXTAREA'), 'An empty request could not be reopened from the overview');
    assert.equal((await workspace(driver)).project.pages[0].regions.length, 1);
    await physical.keys('Escape'); await driver.button('Undo');
    await waitFor(async () => (await workspace(driver)).project.pages[0].regions.length === 0, 'Undo did not remove the empty request');
    await driver.click('.canvas-empty-hint button'); await driver.fill('Request 1', longPrompt);
    await driver.click('[role="toolbar"][aria-label="Canvas tools"] button[aria-label="Select"], [role="toolbar"][aria-label="Canvas tools"] button[title^="Select"]');
    await waitFor(async () => (await workspace(driver)).project.pages[0].regions[0]?.prompt === longPrompt, 'The long request did not persist');
    const promptLayout = await driver.execute(() => {
      const input = document.querySelector('.annotation textarea'), box = input.closest('.annotation');
      const a=input.getBoundingClientRect(), b=box.getBoundingClientRect();
      return {inside:a.left>=b.left-1&&a.right<=b.right+1&&a.top>=b.top-1&&a.bottom<=b.bottom+1,scrollable:input.scrollHeight>input.clientHeight};
    });
    assert.equal(promptLayout.inside,true,'Long prompt text must remain within its request');
    assert.equal(promptLayout.scrollable,true,'Long prompts need internal scrolling');
    await driver.button('Fit');
    await chooseInterpretation(driver, true); await chooseInterpretation(driver, false); await chooseInterpretation(driver, true);
    assert.equal(await driver.execute(() => [...document.querySelectorAll('.shell-primary-actions button')].find(button => button.textContent.trim() === 'Generate')?.disabled), false);
    await physical.captureWindow(join(output, '04-request-readability-window.png'));
  });

  await step('Browser-style tabs and Layers expose each page’s pending requests', async () => {
    await driver.button('Add page'); await driver.fill('Page name', 'About');
    await driver.click('dialog[open] .page-address summary'); await driver.fill('Page path', '/about'); await driver.button('Add page'); await closed(driver);
    aboutId = await waitFor(async () => (await workspace(driver)).project.pages.find(page => page.path === '/about')?.id, 'The About page did not open');
    await driver.click('.canvas-empty-hint button'); await driver.fill('Request 1', 'Create a clear About page for the same studio.');
    await waitFor(async () => (await workspace(driver)).project.pages.find(page => page.id === aboutId).regions[0]?.prompt.includes('About page'), 'The second page request did not persist');
    await openRequest(driver, homeId);
    await waitFor(() => driver.execute(id => document.getElementById(`page-tab-${id}`)?.getAttribute('aria-selected') === 'true' && !!document.querySelector('.annotation.selected textarea'), homeId), 'Layers did not select the page request');
    await physical.captureWindow(join(output,'05-page-request-navigation-window.png'));
  });

  await step('AI settings connect a session key in reading order and show truthful connection and budget states', async () => {
    await driver.button('AI settings');
    await driver.fill('OpenAI API key', provider.key());
    await driver.click('//fieldset[legend[normalize-space(.)="Keep this key"]]//label[contains(normalize-space(.),"Current session only")]/input', 'xpath');
    await driver.button('Connect key');
    await waitFor(async () => (await driver.invoke('get_ai_settings')).credential.configured, 'The synthetic session key did not connect');
    await driver.button('Test access'); await waitFor(() => driver.execute(() => document.querySelector('.ai-settings [role="status"]')?.textContent.includes('Connected')), 'Test access did not show its result');
    const positions = await driver.execute(() => [document.querySelector('.ai-settings label'), document.querySelector('.api-key-connection'), document.querySelector('.ai-access-test')].map(element => element.getBoundingClientRect().top));
    assert.ok(positions.every((position, index) => index === 0 || position > positions[index - 1]));
    await driver.click('details.ai-more summary');
    await driver.fill('API budget per generation (USD)', '10'); await driver.button('Save AI settings');
    await waitFor(() => driver.execute(() => document.querySelector('.ai-settings [role="status"]')?.textContent.includes('saved')), 'The bottom save did not acknowledge AI settings');
    assert.match(await driver.execute(() => document.querySelector('.generation-settings-form').textContent), /not a guaranteed billing cap/);
    await physical.captureWindow(join(output, '06-ai-connection-window.png')); await closeDialog(driver);
    assert.equal((await driver.invoke('get_ai_settings')).credential.configured,true);
  });

  await step('Cancel stops a held generation visibly and preserves the current website and requests', async () => {
    const before = (await workspace(driver)).project;
    provider.setMode('waiting', { editorAudit: true }); await driver.button('Generate');
    cancelled = await waitFor(async () => { const value = await driver.invoke('get_generation', { projectId }); return value?.status === 'generating' && provider.generationRequests().some(record => record.metadata.jobId === value.jobId) && value; }, 'The held generation did not reach the provider');
    await driver.ready('.canvas-pill[aria-label="Generation status"]');
    assert.equal(await driver.execute(() => !!document.querySelector('dialog[open]')), false);
    await driver.click('//*[@role="region" and @aria-label="Generation status"]//button[starts-with(normalize-space(.),"Cancel")]', 'xpath');
    await jobAt(driver, projectId, ['cancelled']); provider.release(cancelled.jobId);
    assert.deepEqual((await workspace(driver)).project.pages, before.pages);
    await physical.captureWindow(join(output, '07-cancelled-generation-window.png'));
  });

  await step('Generation stays in a non-modal strip while real panning works, then applies both pages with the exact instructions', async () => {
    provider.setMode('waiting', { editorAudit: true }); await driver.button('Generate');
    const held = await waitFor(async () => { const value = await driver.invoke('get_generation', { projectId }); return value?.status === 'generating' && value.jobId !== cancelled.jobId && provider.generationRequests().some(record => record.metadata.jobId === value.jobId) && value; }, 'The second generation did not reach the held provider');
    assert.equal(await driver.execute(() => !!document.querySelector('dialog[open]')), false);
    assert.match(await driver.execute(() => document.querySelector('.canvas-pill[aria-label="Generation status"]').textContent), /\d+:\d{2}|elapsed/i);
    await driver.click('[role="toolbar"][aria-label="Canvas tools"] button[aria-label="Pan"], [role="toolbar"][aria-label="Canvas tools"] button[title^="Pan"]');
    const view = await physical.rect('.free-workspace'); const start = { x: view.left + 80, y: view.top + 120 };
    const before = await driver.execute(() => { const element = document.querySelector('.workspace-scroll'); return [element.scrollLeft, element.scrollTop]; });
    await physical.drag(start, { x: start.x + 55, y: start.y + 35 });
    await waitFor(() => driver.execute(previous => { const element = document.querySelector('.workspace-scroll'); return element.scrollLeft !== previous[0] || element.scrollTop !== previous[1]; }, before), 'The canvas could not pan while generation was held');
    await physical.captureWindow(join(output, '08-nonmodal-generation-window.png'));
    await driver.button('Fit'); await driver.click('[role="toolbar"][aria-label="Canvas tools"] button[aria-label="Select"], [role="toolbar"][aria-label="Canvas tools"] button[title^="Select"]');
    provider.release(held.jobId);
    await waitFor(async () => (await driver.invoke('get_generation', { projectId }))?.receivedBytes > 0, 'The held reply did not start streaming');
    provider.release(held.jobId);
    applied = await jobAt(driver, projectId, ['applied']);
    const record = provider.generationRequests().find(item => item.metadata.jobId === applied.jobId);
    assert.equal(record.markdown, applied.requirements.markdown); assert.equal(hash(record.markdown), applied.requirements.sha256); assert.equal(record.metadata.generationMode, 'strict');
    assert.equal(applied.requirements.pageCount, 2); assert.equal((await workspace(driver)).project.pages.every(page => page.content?.html.includes('Generated native preview')), true);
    await previewReady(driver, 'Generated native preview'); await driver.ready('.canvas-pill[aria-label="Generation status"]');
    await writeFile(join(output, 'submitted-instructions.md'), record.markdown);
  });

  await step('On-canvas result actions preserve one-step Undo and Redo without a sidebar', async () => {
    const bar = '//*[@role="region" and @aria-label="Generation status"]';
    for (const label of ['Undo', 'Try again']) await driver.ready(`${bar}//button[normalize-space(.)=${xpathString(label)}]`, 'xpath');
    await physical.captureWindow(join(output, '09-applied-review-window.png'));
    const requests = requestCount(provider); await driver.click(`${bar}//button[normalize-space(.)="Undo"]`, 'xpath');
    await waitFor(async () => (await workspace(driver)).project.pages.every(page => !page.content), 'Result Undo did not restore the previous website');
    await driver.button('Redo'); await previewReady(driver, 'Generated native preview'); assert.equal(requestCount(provider), requests);
    if (await driver.execute(() => !![...document.querySelectorAll('.canvas-pill[aria-label="Generation status"] button')].find(button=>button.textContent.trim()==='Keep'))) await driver.click(`${bar}//button[normalize-space(.)="Keep"]`, 'xpath');
    await assertCanvasOnly(driver);
  });

  await step('A draft typed during generation is committed before apply and stale output cannot overwrite it', async () => {
    await clickHeading(driver, physical); await physical.rightClick(await childPoint(driver, 'h1', true)); await driver.click('.editor-menu [aria-label="Ask AI"]');
    await waitFor(() => driver.execute(() => document.activeElement?.tagName === 'TEXTAREA'), 'Ask AI did not focus its request');
    await physical.raw({ action: 'text', text: 'Refine this heading while keeping its meaning.' });
    await waitFor(async () => (await workspace(driver)).project.pages.find(page => page.id === homeId).regions.some(region => region.prompt === 'Refine this heading while keeping its meaning.'), 'Typing in Ask AI lost focus or failed to persist');
    provider.setMode('waiting', { editorAudit: true }); await driver.button('Generate');
    const held = await waitFor(async () => { const job = await driver.invoke('get_generation', { projectId }); return job?.status === 'generating' && provider.generationRequests().some(item => item.metadata.jobId === job.jobId) && job; }, 'Draft preservation generation did not start');
    await driver.button('Fit');
    await physical.doubleClick(await childPoint(driver, 'h1', true)); await driver.ready('.inline-text-editor[role="textbox"]');
    await physical.keys('A', 1, ['Control']); await physical.raw({ action: 'text', text: 'My unfinished edit must survive' });
    await waitFor(() => driver.execute(() => document.querySelector('.inline-text-editor')?.textContent === 'My unfinished edit must survive'), 'The native typing did not settle in the inline editor');
    provider.release(held.jobId);
    await waitFor(async () => (await driver.invoke('get_generation', { projectId }))?.receivedBytes > 0, 'The draft-test reply did not start streaming');
    provider.release(held.jobId); await jobAt(driver, projectId, ['blocked']);
    await previewReady(driver, 'My unfinished edit must survive');
    await physical.captureWindow(join(output, '09b-draft-preserved-window.png'));
    await driver.button('Undo'); await previewReady(driver, 'Generated native preview');
    provider.setMode('complete', { editorAudit: true }); await driver.button('Generate'); applied = await jobAt(driver, projectId, ['applied']);
    await previewReady(driver, 'Generated native preview');
    await driver.ready('.canvas-pill[aria-label="Generation status"]');
    await physical.doubleClick(await childPoint(driver, 'h1', true)); await driver.ready('.inline-text-editor[role="textbox"]');
    await physical.keys('A', 1, ['Control']); await physical.raw({ action: 'text', text: 'Result review preserves newer typing' });
    await waitFor(() => driver.execute(() => document.querySelector('.inline-text-editor')?.textContent === 'Result review preserves newer typing'), 'The native typing did not settle in the inline editor');
    await driver.click('//*[@role="region" and @aria-label="Generation status"]//button[normalize-space(.)="Undo"]','xpath');
    await previewReady(driver, 'Result review preserves newer typing');
    assert.equal((await workspace(driver)).project.latestGeneration.jobId, applied.jobId, 'Result Undo undid a newer local edit');
    await driver.button('Undo'); await previewReady(driver, 'Generated native preview');
  });

  await step('Double-click edits actual heading text inline, Enter commits and Escape cancels without an AI request', async () => {
    const requests = requestCount(provider); await physical.doubleClick(await childPoint(driver, 'h1', true)); await driver.ready('[data-testid="inline-text-editor"], .inline-text-editor[role="textbox"]');
    assert.equal(await driver.execute(() => !!document.querySelector('dialog[open]')), false);
    await physical.keys('A', 1, ['Control']); await physical.raw({ action: 'text', text: 'Edited directly on the page' }); await physical.keys('Return');
    await previewReady(driver, 'Edited directly on the page');
    await driver.button('Undo'); await previewReady(driver, 'Generated native preview'); await driver.button('Redo'); await previewReady(driver, 'Edited directly on the page');
    await physical.doubleClick(await childPoint(driver, 'h1', true)); await driver.ready('.inline-text-editor[role="textbox"]'); await physical.keys('A', 1, ['Control']); await physical.raw({ action: 'text', text: 'Cancelled text change' }); await physical.keys('Escape');
    await previewReady(driver, 'Edited directly on the page'); assert.equal(requestCount(provider), requests);
    await clickHeading(driver, physical); assert.deepEqual(await driver.execute(() => [...document.querySelectorAll('.element-toolbar button')].map(button => button.getAttribute('aria-label') || button.textContent.trim())), ['Edit', 'Ask AI', 'Delete layer']);
    assert.match(await driver.execute(() => document.querySelector('.workspace-measurement').textContent), /px/);
    const overlap = await driver.execute(() => { const a = document.querySelector('.element-toolbar').getBoundingClientRect(), b = document.querySelector('.element-selection').getBoundingClientRect(); return Math.max(0, Math.min(a.right,b.right)-Math.max(a.left,b.left))*Math.max(0,Math.min(a.bottom,b.bottom)-Math.max(a.top,b.top)); });
    assert.equal(overlap, 0, 'The restored element toolbar hides the selected heading');
    await physical.captureWindow(join(output, '10-inline-edit-selection-window.png'));
  });

  await step('The right-click Ask AI action and Ctrl+K open the same on-page request without sending anything', async () => {
    const requests = requestCount(provider); await physical.rightClick(await childPoint(driver, 'h1', true)); await driver.click('.editor-menu [aria-label="Ask AI"]');
    const request = await waitFor(async () => (await workspace(driver)).project.pages.find(page => page.id === homeId).regions.find(region => !region.completed && region.element?.tag === 'h1'), 'Ask AI did not create a source-bound request');
    await driver.fill(`Request ${(await workspace(driver)).project.pages.find(page => page.id === homeId).regions.findIndex(region => region.id === request.id) + 1}`, 'Make the heading warmer while preserving its meaning.');
    await clickHeading(driver, physical); await physical.keys('K', 1, ['Control']);
    await waitFor(() => driver.execute(() => document.activeElement?.tagName === 'TEXTAREA' && !!document.activeElement.closest('[data-testid="canvas"]')), 'Ctrl+K did not focus its on-canvas AI request');
    assert.equal(requestCount(provider), requests); await physical.captureWindow(join(output, '11-ask-ai-window.png'));
  });

  await step('Page tabs preserve completed work while Layers reveals and focuses a pending request', async () => {
    await driver.page(aboutId); await previewReady(driver,'Generated native preview');
    assert.ok((await workspace(driver)).project.pages.find(page=>page.id===aboutId).regions.every(region=>region.completed));
    await openRequest(driver,homeId);
    await waitFor(()=>driver.execute(()=>!!document.querySelector('.annotation.selected textarea')),'The pending request was not revealed');
    await physical.captureWindow(join(output,'12-request-navigation-window.png'));
  });

  await step('Native Save writes a real archive and recent-project history stores its actual name and path', async () => {
    await chooseFile(physical, () => driver.button('Save'), projectPath, output, 'audit-save');
    savedProject = (await savedState(driver, projectPath)).project; await verifyArchive(projectPath, savedProject, await driver.invoke('get_project_notes'));
    const recent = await driver.invoke('get_recent_projects'); assert.deepEqual(recent.projects[0], { name: 'Audit website', path: projectPath });
    await driver.menu(['New project']);
    await driver.ready('.recent-projects'); assert.match(await driver.execute(() => document.querySelector('.recent-projects').textContent), /Audit website/);
    await physical.captureWindow(join(output, '13-real-recent-projects-window.png')); await closeDialog(driver);
  });


  await step('Native Close flushes an unfinished inline edit before asking about unsaved changes', async () => {
    assert.equal((await workspace(driver)).dirty, false);
    await physical.doubleClick(await childPoint(driver, 'h1', true)); await driver.ready('.inline-text-editor[role="textbox"]');
    await physical.keys('A', 1, ['Control']); await physical.raw({ action: 'text', text: 'Closing preserves this draft' });
    assert.equal((await workspace(driver)).dirty, false, 'The regression requires an uncommitted inline draft');
    await physical.raw({ action: 'close' });
    const dialog = await waitFor(async () => (await physical.inspect()).windows.find(window => window.title === 'Unsaved project changes'), 'Close did not show the native unsaved-changes confirmation');
    assert.equal((await workspace(driver)).dirty, true);
    await physical.captureWindow(join(output, '13b-unsaved-inline-close-window.png'));
    await physical.raw({ action: 'key', key: 'Escape', windowId: dialog.id });
    await previewReady(driver, 'Closing preserves this draft');
    await driver.button('Undo'); await previewReady(driver, 'Edited directly on the page');
    await driver.button('Save'); savedProject = (await savedState(driver, projectPath)).project;
  });

  await step('Opening an actual recent project restores its saved source and pending requests', async () => {
    await driver.menu(['New project']); await driver.click('//button[.//strong[normalize-space(.)="Blank website"]]', 'xpath');
    await driver.fill('Project name', 'Temporary audit page'); await driver.button('Create project'); await closed(driver);
    await waitFor(async () => (await workspace(driver)).project.id !== projectId, 'The temporary project did not open');
    await driver.menu(['New project']);
    await driver.click(`//section[contains(@class,"recent-projects")]//button[@title=${xpathString(projectPath)}]`, 'xpath');
    await waitFor(async () => (await workspace(driver)).project.id === projectId || driver.execute(() => !!document.querySelector('dialog[aria-label="Save your changes?"][open]')), 'The recent project neither opened nor requested its explicit discard choice');
    if (await driver.execute(() => !!document.querySelector('dialog[aria-label="Save your changes?"][open]'))) await driver.button('Discard and continue');
    await closed(driver); await waitFor(async () => (await workspace(driver)).project.id === projectId, 'The actual recent project did not reopen');
    assert.deepEqual((await workspace(driver)).project, savedProject); await driver.page(homeId); await previewReady(driver, 'Edited directly on the page');
  });

  await step('Local replacement builds two real portable pages with image sources, CSS assets, internal links and a click script', async () => {
    const requests = requestCount(provider);
    await openRequest(driver,homeId);
    await waitFor(() => driver.execute(() => !!document.querySelector('.annotation.selected textarea')), 'The temporary export request was not selected');
    await driver.click('.element-toolbar [aria-label="Delete layer"]');
    if(await driver.execute(()=>!!document.querySelector('dialog[open]'))) { await driver.button('Remove'); await closed(driver); }
    await driver.button('Fit'); await replaceMain(driver, physical, false); await driver.page(aboutId); await replaceMain(driver, physical, true); await driver.page(homeId);
    const value = (await workspace(driver)).project; assert.equal(value.pages.length, 2);
    for (const page of value.pages) { assert.ok(page.content.html.includes('srcset=')); assert.ok(page.content.html.includes('background-image:')); assert.ok(page.content.html.includes('Local click script works')); }
    assert.equal(requestCount(provider), requests); await driver.button('Save'); await savedState(driver, projectPath);
  });

  await step('Export writes a usable standalone website through the native destination picker and shows Open folder', async () => {
    const parent = join(output, 'export-parent'); await mkdir(parent, { recursive: true, mode: 0o700 });
    await chooseFolder(physical, () => driver.button('Export'), parent, output, 'audit-export');
    await driver.ready('.export-notice');
    const folders = await waitFor(async () => { const entries = await readdir(parent, { withFileTypes: true }); return entries.filter(entry => entry.isDirectory()).length === 1 && entries; }, 'Export did not create its website folder');
    assert.equal(folders.length, 1); exportedRoot = join(parent, folders[0].name);
    const files = await allFiles(exportedRoot), names = files.map(path => relative(exportedRoot, path).split(sep).join('/'));
    assert.ok(names.includes('index.html')); assert.ok(names.includes('about/index.html')); assert.ok(names.includes('styles.css'));
    const home = await readFile(join(exportedRoot, 'index.html'), 'utf8'), about = await readFile(join(exportedRoot, 'about/index.html'), 'utf8');
    assert.match(home, /Our standalone website/); assert.match(about, /About our standalone website/); assert.ok(files.some(path => /\.png$/i.test(path)));
    assert.doesNotMatch(home, /data:image\/png/); assert.doesNotMatch(about, /data:image\/png/);
    assert.equal(await driver.execute(() => [...document.querySelectorAll('[role="status"]')].some(element => /export/i.test(element.textContent) && element.textContent.includes('Open folder'))), true);
    await driver.ready('//button[normalize-space(.)="Open folder"]', 'xpath');
    await physical.captureWindow(join(output, '16-export-notice-window.png'));
    await writeFile(join(output, 'exported-files.json'), JSON.stringify(await Promise.all(files.map(async path => ({ path: relative(exportedRoot, path), sha256: hash(await readFile(path)) }))), null, 2));
  });

  await step('Ordinary Chromium opens exported files with working local images, scripts and internal navigation, without Desegnos runtime', async () => {
    const index = pathToFileURL(join(exportedRoot, 'index.html')).href;
    const home = await chromium(index, join(output, 'exported-home-browser.html'));
    assert.match(home, /<h1>Our standalone website<\/h1>/); assert.match(home, /data-desegnos-runtime="false"/); assert.match(home, /data-local-click="passed"/); assert.match(home, /data-local-image="true"/); assert.match(home, /data-local-css="true"/); assert.match(home, /data-local-stylesheet="true"/);
    const about = await chromium(`${index}#follow-navigation`, join(output, 'exported-navigation-browser.html'));
    assert.match(about, /<h1>About our standalone website<\/h1>/); assert.match(about, /data-local-click="passed"/); assert.match(about, /data-desegnos-runtime="false"/);
  });

  await step('Cancelling Export from the project menu preserves the website, archive and provider history', async () => {
    const before = (await workspace(driver)).project, archive = hash(await readFile(projectPath)), requests = requestCount(provider);
    await chooseFile(physical, () => driver.menu(['Export website…']), null, output, 'audit-export-cancel', true);
    assert.deepEqual((await workspace(driver)).project, before); assert.equal(hash(await readFile(projectPath)), archive); assert.equal(requestCount(provider), requests);
    assert.deepEqual(provider.errors(), []); await assertCanvasOnly(driver);
    await writeFile(join(output, 'final-project.json'), JSON.stringify(before, null, 2));
  });
  return (await workspace(driver)).project;
}
