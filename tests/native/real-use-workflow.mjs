// Real-use suite: the flows the owner actually uses (see scripts/usage-report.mjs), run on
// copies of their real projects and unsaved work, driven by real pointer and keyboard input.
// driver.invoke is only used to read state and to install the synthetic AI key.
//
// Observed flows (generation history, 2026-10-18 – 10-21, 23 jobs):
//  - Unsaved work is recovered after restarts; a real recovery once failed to load.
//  - Select an element on a real page and ask for a fix, then discard and retry until it fits
//    (6 element fixes, 4 retries straight after a discard).
//  - Draw boxes on a blank page and describe each one in several lines (12 jobs).
//  - Pictures of the owner's own work go into requests (now sent as previews).
//  - Typing must not disturb the controls (the toolbar once blinked on every key).
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { chooseFile, savedState } from './file-dialog-workflow.mjs';
import { waitFor } from './webdriver.mjs';

const workspace = driver => driver.invoke('get_workspace');
const generation = (driver, projectId) => driver.invoke('get_generation', { projectId });

export const statusChecks = [];
async function jobAt(driver, projectId, statuses, previous = null) {
  return waitFor(async () => {
    const started = Date.now();
    let value;
    try { value = await generation(driver, projectId); }
    catch (error) { statusChecks.push({ at: new Date(started).toISOString(), ms: Date.now() - started, error: String(error.message).slice(0, 80) }); return false; }
    statusChecks.push({ at: new Date(started).toISOString(), ms: Date.now() - started, status: value?.status });
    return statuses.includes(value?.status) && value.jobId !== previous && value;
  }, `Generation did not reach ${statuses.join('/')}`, 60_000);
}

async function previewReady(driver) {
  await waitFor(() => driver.execute(() => {
    const preview = document.querySelector('.static-preview');
    return preview?.dataset.previewReady === 'true' && !!preview.querySelector('iframe')?.contentDocument?.body;
  }), 'The page did not show on the canvas', 20_000);
}

async function noRecoveryError(driver) {
  const text = await driver.execute(() => [...document.querySelectorAll('.notice-error')].map(item => item.textContent).join(' '));
  assert.doesNotMatch(text, /Recovery could not be loaded/, text);
}

// Centre of an element inside the page preview, in window coordinates.
async function previewPoint(driver, selector) {
  return driver.execute(query => {
    const frame = document.querySelector('.static-preview iframe');
    const elements = [...(frame?.contentDocument?.querySelectorAll(query) ?? [])];
    const outer = frame.getBoundingClientRect(), zoom = Number(document.querySelector('[data-testid="canvas"]').dataset.zoom);
    for (const element of elements) {
      const rect = element.getBoundingClientRect();
      if (rect.width < 24 || rect.height < 24) continue;
      const point = { x: outer.x + (rect.x + Math.min(rect.width / 2, 120)) * zoom, y: outer.y + (rect.y + Math.min(rect.height / 2, 80)) * zoom };
      if (document.elementFromPoint(point.x, point.y)?.closest('[data-testid="canvas"]')) return point;
    }
    return null;
  }, selector);
}

// Counts every time a toolbar button is enabled or disabled, to catch flicker while typing.
function watchToolbar(driver) {
  return driver.execute(() => {
    window.__toolbarToggles = [];
    const observer = new MutationObserver(records => { for (const record of records) window.__toolbarToggles.push(record.target.getAttribute('aria-label') ?? record.target.textContent.trim()); });
    for (const button of document.querySelectorAll('header button, [role="toolbar"] button')) observer.observe(button, { attributes: true, attributeFilter: ['disabled'] });
    window.__toolbarObserver = observer; return true;
  });
}
const toolbarToggles = driver => driver.execute(() => { window.__toolbarObserver?.disconnect(); return window.__toolbarToggles; });

async function typeLines(physical, lines) {
  for (const [index, line] of lines.entries()) {
    if (index) await physical.keys('Return', 1, ['Shift']);
    await physical.raw({ action: 'text', text: line });
  }
}

async function closeProject(driver) {
  const before = (await workspace(driver)).openProjects?.length ?? 1;
  await driver.menu(['Close project']);
  // Unsaved work asks first; a saved project closes straight away.
  const asked = await waitFor(async () => {
    if (await driver.execute(() => [...document.querySelectorAll('.workspace-surface button')].some(button => button.textContent.trim() === 'Discard and continue'))) return 'asked';
    const open = (await workspace(driver)).openProjects?.length ?? 0;
    return open < before && 'closed';
  }, 'Close project neither asked nor closed');
  if (asked === 'asked') {
    await driver.button('Discard and continue');
    await waitFor(async () => ((await workspace(driver)).openProjects?.length ?? 0) < before, 'Discarding did not close the project');
  }
}

export async function realUseWorkflow(driver, physical, step, evidence, provider) {
  const manifest = JSON.parse(await readFile(join(evidence, 'real-use-manifest.json'), 'utf8'));
  assert.ok(manifest.recoveries.length + manifest.projects.length > 0, `No real projects or recovery were found in ${manifest.source}`);
  await driver.invoke('set_ai_key', { key: provider.key(), storage: 'session' });
  let current;

  await step(`All ${manifest.recoveries.length} pieces of real unsaved work stay out of the way after a restart and recover from the menu`, async () => {
    if (!manifest.recoveries.length) return;
    await driver.ready('.startup');
    await noRecoveryError(driver);
    // The owner asked for no recovery notice; recovery lives in Menu → Recover unsaved projects.
    assert.equal(await driver.execute(() => !!document.querySelector('.recovery-notice')), false, 'The start screen still shows a recovery notice');
    for (let opened = 0; opened < manifest.recoveries.length; opened++) {
      await driver.menu(['Recover unsaved projects']);
      if (!opened) {
        const offered = await driver.execute(() => document.querySelectorAll('.workspace-surface .recovery-list > section').length);
        assert.equal(offered, manifest.recoveries.length);
        await physical.captureWindow(join(evidence, 'real-01-recovery-offered.png'));
      }
      await driver.click('.workspace-surface .recovery-list > section:first-child button');
      await waitFor(async () => ((await workspace(driver)).openProjects?.length ?? 0) === opened + 1, 'A recovery did not open in its own tab group', 15_000);
    }
    await noRecoveryError(driver);
    const state = await workspace(driver);
    assert.equal(state.openProjects.length, manifest.recoveries.length);
    assert.equal(state.recoveries.length, 0);
    if (state.project.pages.some(page => page.content)) await previewReady(driver);
    await physical.captureWindow(join(evidence, 'real-02-recovered.png'));
  });

  await step('Real projects open from their files, show their pages, and close without touching the originals', async () => {
    for (let open = (await workspace(driver)).openProjects?.length ?? 0; open > 0; open--) await closeProject(driver);
    await waitFor(async () => !(await workspace(driver)).project, 'Recovered projects did not close');
    for (const project of manifest.projects) {
      await chooseFile(physical, () => driver.button('Open project'), project.path, evidence, `real-open-${project.name}`);
      const state = await waitFor(async () => { const value = await workspace(driver); return value.path === project.path && value; }, `${project.name} did not open`, 15_000);
      assert.equal(state.dirty, false);
      if (state.project.pages.some(page => page.content)) await previewReady(driver);
      await physical.captureWindow(join(evidence, `real-03-open-${project.name}.png`));
      if (project !== manifest.projects.at(-1)) await closeProject(driver);
    }
    current = (await workspace(driver)).project;
  });

  // The window crashed twice (2026-10-21 13:53 and 15:43) in Split view: WebKitGTK 2.54 aborts in
  // AccessibilityObjectAtspi::selectionChanged when the caret moves in an editable while the
  // desktop's accessibility client listens. Run with DESEGNOS_NATIVE_TEST_ACCESSIBILITY=1.
  await step('Moving the caret through the real page source in Split view keeps the window alive', async () => {
    const view = label => driver.click(`//div[@aria-label="Editor view"]/button[normalize-space(.)=${JSON.stringify(label)}]`, 'xpath');
    await view('Split');
    for (const language of ['HTML', 'CSS', 'JavaScript']) {
      const editor = `[contenteditable="true"][aria-label="${language} code"]`;
      await driver.click(`//*[@role="tablist" and @aria-label="Code files"]/*[@role="tab" and starts-with(normalize-space(.), ${JSON.stringify(language)})]`, 'xpath');
      await driver.ready(editor);
      await driver.click(editor);
      for (const [key, modifiers] of [['End', ['Control']], ['Home', []], ['End', []], ['Left', []], ['Right', []], ['Home', ['Control']], ['End', ['Control']], ['A', ['Control']], ['Right', []], ['Left', ['Shift']]]) {
        await physical.keys(key, 1, modifiers);
        assert.ok(await driver.execute(() => document.readyState === 'complete'), `The window stopped answering after ${modifiers.join('+')}${modifiers.length ? '+' : ''}${key} in the ${language} source`);
      }
    }
    await physical.captureWindow(join(evidence, 'real-03b-split-caret.png'));
    await view('Design');
  });

  await step('Typing a several-line request about a real element keeps the toolbar still and saves quickly', async () => {
    current = (await workspace(driver)).project;
    assert.ok(current?.pages.some(page => page.content), 'The real project needs a built page');
    await previewReady(driver); await driver.button('Fit');
    await physical.initialize(evidence);
    const point = await previewPoint(driver, 'img, h1, h2, section');
    assert.ok(point, 'No visible element to select on the real page');
    await physical.click(point);
    await physical.rightClick(point); await driver.click('.editor-menu [aria-label="Ask AI"]');
    await waitFor(() => driver.execute(() => document.activeElement?.tagName === 'TEXTAREA'), 'Ask AI did not focus its request');
    await watchToolbar(driver);
    // When each keystroke reaches the text box: the gaps are the app's cost per key.
    await driver.execute(() => {
      window.__inputTimes = []; document.activeElement.addEventListener('input', () => window.__inputTimes.push(performance.now()));
      // Per key: time in key handlers, then time until the next frame is drawn.
      window.__phases = []; let down = 0;
      document.addEventListener('keydown', () => { down = performance.now(); }, true);
      document.activeElement.addEventListener('input', () => { const input = performance.now(); requestAnimationFrame(() => window.__phases.push({ handlers: Math.round(input - down), toFrame: Math.round(performance.now() - input) })); });
      return true;
    });
    const lines = ['make the background behind this calmer', 'keep my photo exactly as it is'];
    const started = Date.now();
    await typeLines(physical, lines);
    const typed = Date.now();
    const expected = lines.join('\n');
    // Xvfb draws without a GPU, so a burst of keys reaches the box slowly; what matters is how
    // soon the request is saved after the last key arrives.
    const settled = await waitFor(async () => (await workspace(driver)).project.pages.some(page => page.regions.some(region => !region.completed && region.prompt === expected)), 'The typed request was not saved', 15_000).then(() => true, () => false);
    const afterLastKey = Math.round(await driver.execute(() => performance.now() - window.__inputTimes.at(-1)));
    const times = await driver.execute(() => window.__inputTimes);
    const gaps = times.slice(1).map((time, index) => Math.round(time - times[index])).sort((a, b) => a - b);
    const phases = await driver.execute(() => window.__phases);
    const median = values => values.sort((a, b) => a - b)[values.length >> 1];
    await writeFile(join(evidence, 'real-typing-keys.json'), JSON.stringify({ inputs: times.length, medianGapMs: gaps[gaps.length >> 1], maxGapMs: gaps.at(-1), firstToLastMs: Math.round(times.at(-1) - times[0]), handlersMedianMs: median(phases.map(item => item.handlers)), toFrameMedianMs: median(phases.map(item => item.toFrame)) }, null, 2));
    assert.ok(settled, 'The typed request was not saved');
    const saved = afterLastKey;
    // One change is fine (Generate turns on once the request has text); back and forth is flicker.
    const toggles = await toolbarToggles(driver);
    const flicker = [...new Set(toggles)].filter(name => toggles.filter(other => other === name).length > 1);
    assert.deepEqual(flicker, [], `Toolbar buttons flickered while typing: ${toggles.join(', ')}`);
    await writeFile(join(evidence, 'real-typing.json'), JSON.stringify({ characters: expected.length, typingMs: typed - started, keysArrivedMs: Date.now() - typed - saved, savedAfterLastKeyMs: saved }, null, 2));
    assert.ok(saved < 1_500, `Saving the typed request took ${saved} ms after the last key arrived`);
  });

  await step('An element fix on the real page can be discarded, retried and kept, each attempt sending a small request', async () => {
    const projectId = current.id, before = provider.generationRequests().length;
    provider.setMode('complete', { editorAudit: true });
    await driver.button('Generate');
    const first = await jobAt(driver, projectId, ['applied']);
    const sent = provider.generationRequests().slice(before);
    assert.equal(sent.length, 1);
    assert.match(sent[0].markdown, /^Requirements format: 14$/m);
    await writeFile(join(evidence, 'real-element-fix-request.md'), sent[0].markdown);
    const bar = '//*[contains(concat(" ", normalize-space(@class), " "), " generation-notice ")]';
    await driver.click(`${bar}//button[normalize-space(.)="Undo"]`, 'xpath');
    await waitFor(async () => (await workspace(driver)).project.pages.some(page => page.regions.some(region => !region.completed && region.prompt)), 'Discarding the result did not bring the request back');
    await driver.button('Generate');
    const second = await jobAt(driver, projectId, ['applied'], first.jobId);
    assert.equal(provider.generationRequests().length, before + 2);
    if (await driver.execute(() => [...document.querySelectorAll('.notice-host .generation-notice button')].some(button => button.getAttribute('aria-label') === 'Dismiss notice'))) await driver.click(`${bar}//button[@aria-label="Dismiss notice"]`, 'xpath');
    await writeFile(join(evidence, 'real-element-fix.json'), JSON.stringify({ attempts: 2, requestBytes: provider.generationRequests().slice(before).map(record => record.requestBytes), finalJob: second.jobId }, null, 2));
    await physical.captureWindow(join(evidence, 'real-04-element-fix-kept.png'));
  });

  await step('A blank page gets boxes described in several lines, with a large photo attached, and generates', async () => {
    await driver.menu(['New project']);
    await driver.button('Blank website'); await driver.fill('Project name', 'Real use boxes'); await driver.button('Create project');
    await waitFor(async () => (await workspace(driver)).project?.name === 'Real use boxes', 'The blank project did not open');
    if (await driver.execute(() => [...document.querySelectorAll('button')].some(button => button.textContent.trim() === 'Skip tips'))) await driver.button('Skip tips');
    const projectId = (await workspace(driver)).project.id;
    await driver.button('Fit');
    // The page frame is the parent of its "Screen · W × H" label.
    const pageFrame = await driver.execute(() => { const rect = document.querySelector('.screen-guide-label').parentElement.getBoundingClientRect(); return { left: rect.left, top: rect.top, width: rect.width, height: rect.height }; });
    const spots = [{ x: pageFrame.left + 80, y: pageFrame.top + 80 }, { x: pageFrame.left + pageFrame.width * 0.6, y: pageFrame.top + pageFrame.height * 0.65 }];
    const requests = [['Desegnos logo', 'big text saying the name', 'smaller text saying it is a page builder'], ['Github link and call to action']];
    for (const [index, lines] of requests.entries()) {
      const free = await driver.execute(point => { const hit = document.elementFromPoint(point.x, point.y); return !!hit?.closest('[data-testid="canvas"]') && !hit.closest('.annotation'); }, spots[index]);
      assert.ok(free, `Box ${index + 1} would land on another box or off the canvas`);
      await physical.doubleClick(spots[index]);
      await waitFor(() => driver.execute(() => document.activeElement?.tagName === 'TEXTAREA'), 'The new box did not take typing');
      await typeLines(physical, lines);
      await waitFor(async () => (await workspace(driver)).project.pages[0].regions.some(region => region.prompt === lines.join('\n')), `Box ${index + 1} was not saved`);
    }
    // A photo straight from a camera: far larger than any former limit.
    const photo = join(evidence, 'camera-photo.jpg');
    execFileSync('/usr/bin/convert', ['-size', '4000x3000', 'plasma:', '-quality', '95', photo]);
    const first = (await workspace(driver)).project.pages[0].regions[0];
    await driver.click(`.annotation[data-region-id="${first.id}"] textarea`);
    await chooseFile(physical, () => driver.click('button[aria-label="Attach files to request 1"]'), photo, evidence, 'real-attach-photo');
    await waitFor(async () => (await workspace(driver)).project.pages[0].regions[0].attachments?.length === 1, 'The photo was not attached');
    const before = provider.generationRequests().length;
    provider.setMode('complete', { editorAudit: true, placeAttachments: true });
    await driver.button('Generate');
    statusChecks.length = 0;
    try { await jobAt(driver, projectId, ['applied']); }
    finally { await writeFile(join(evidence, 'real-photo-status-checks.json'), JSON.stringify(statusChecks, null, 1)); }
    const sent = provider.generationRequests()[before];
    const previews = sent.body.input[0].content.filter(item => item.type === 'input_image');
    assert.equal(previews.length, 1, 'The AI did not get a preview of the attached photo');
    assert.equal(previews[0].detail, 'low');
    assert.ok(previews[0].image_url.length < 70 * 1024, `The preview is ${previews[0].image_url.length} bytes`);
    const project = (await workspace(driver)).project;
    assert.equal(Object.keys(project.pictures ?? {}).length, 1, 'Only the displayed copy should be kept');
    const kept = Buffer.from(Object.values(project.pictures)[0].split(',')[1], 'base64');
    const size = execFileSync('/usr/bin/identify', ['-format', '%w %h', '-'], { input: kept }).toString().trim();
    assert.equal(size, '640 480', 'The kept photo should be twice its 320 px display width');
    assert.ok(project.pages[0].regions.every(region => !region.attachments?.some(item => item.kind === 'image')), 'The original photo was kept');
    await previewReady(driver);
    await physical.captureWindow(join(evidence, 'real-05-boxes-generated.png'));
  });

  await step('The new project saves, reopens unchanged and keeps its photo', async () => {
    const before = (await workspace(driver)).project;
    const destination = (await driver.invoke('get_workspace')).path;
    await driver.button('Save');
    await savedState(driver, destination);
    await closeProject(driver);
    await chooseFile(physical, () => driver.button('Open project'), destination, evidence, 'real-reopen');
    const after = await waitFor(async () => { const value = await workspace(driver); return value.path === destination && value.project; }, 'The saved project did not reopen');
    assert.deepEqual(after.pages, before.pages);
    assert.deepEqual(after.pictures, before.pictures);
    await previewReady(driver);
  });

  return (await workspace(driver)).project;
}

