import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { chooseFile, chooseFolder, savedState, verifyArchive } from './file-dialog-workflow.mjs';
import { assertCanvasOnly } from './workflow.mjs';
import { waitFor } from './webdriver.mjs';
import { inspectChromiumPage } from '../export/chromium.mjs';

const panel = 'section[aria-label="Layers"]';
const workspace = driver => driver.invoke('get_workspace');
const hash = value => createHash('sha256').update(value).digest('hex');
const settled = driver => driver.executeAsync(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve(true)))));
const pending = project => project.pages[0].regions.filter(region => !region.completed);

async function previewReady(driver) {
  return waitFor(async () => {
    const project = (await workspace(driver)).project, page = project.pages[0];
    if (!page.content) return false;
    const expected = hash(JSON.stringify({ html: page.content.html, css: page.content.css, javascript: page.content.javascript }));
    return driver.execute(digest => {
      const preview = document.querySelector('.static-preview');
      return preview?.dataset.previewReady === 'true' && preview.dataset.sourceHash === digest && !!preview.querySelector('iframe')?.contentDocument?.querySelector('#layer-stage');
    }, expected);
  }, 'The current native layer source did not finish rendering');
}

async function openLayers(driver) {
  if (!await driver.execute(() => !!document.querySelector('section[aria-label="Layers"]'))) await driver.button('Layers');
  await driver.ready(panel);
}

async function closeLayers(driver) {
  if (await driver.execute(() => !!document.querySelector('section[aria-label="Layers"]'))) await driver.click(`${panel} button[aria-label="Close Layers"]`);
}

async function selectRequest(driver, physical, id) {
  await openLayers(driver);
  const selector = `${panel} .layer-row[data-region-id=${JSON.stringify(id)}] button.layer-select`;
  await driver.ready(selector); await physical.click(await physical.center(selector));
  await waitFor(() => driver.execute(regionId => document.querySelector(`section[aria-label="Layers"] .layer-row[data-region-id="${regionId}"]`)?.dataset.selected === 'true', id), 'The requested layer did not own selection');
}

async function selectorForId(driver, id) {
  return driver.execute(value => {
    const doc = document.querySelector('.static-preview iframe')?.contentDocument;
    const node = doc?.getElementById(value); if (!node) throw new Error(`Missing generated layer ${value}`);
    const parts = [];
    for (let item = node; item && item !== doc.body; item = item.parentElement) {
      const peers = [...item.parentElement.children].filter(peer => peer.localName === item.localName);
      parts.unshift(`${item.localName}:nth-of-type(${peers.indexOf(item) + 1})`);
    }
    return ['body', ...parts].join(' > ');
  }, id);
}

async function selectSite(driver, physical, id) {
  await previewReady(driver); await openLayers(driver);
  const target = await selectorForId(driver, id);
  const row = `${panel} button[data-layer-selector=${JSON.stringify(target)}]`;
  await driver.ready(row); await physical.click(await physical.center(row));
  await waitFor(() => driver.execute(selector => {
    const selection = document.querySelector('[data-testid="direct-element-selection"]');
    return selection?.dataset.selector === selector && selection.dataset.sourceHash === document.querySelector('.static-preview')?.dataset.sourceHash;
  }, target), 'The exact current-source website layer did not own selection');
  return target;
}

async function arrange(driver, name) {
  const before = (await workspace(driver)).project.revision;
  await driver.click(`${panel} button[aria-label=${JSON.stringify(name)}]`);
  await waitFor(async () => (await workspace(driver)).project.revision > before, `${name} was not acknowledged by Rust`);
  await settled(driver);
}

async function changedBy(driver, action, message) {
  const before = (await workspace(driver)).project.revision;
  await action();
  await waitFor(async () => (await workspace(driver)).project.revision > before, message);
  await settled(driver);
}

async function siteState(driver) {
  return driver.execute(() => {
    const doc = document.querySelector('.static-preview iframe').contentDocument;
    const ids = ['layer-stage', 'layer-backdrop', 'blue-card', 'orange-card', 'nested-group', 'green-card', 'purple-card', 'zero-child', 'contents-child'];
    const backdrop = doc.getElementById('layer-backdrop'), backdropStyle = doc.defaultView.getComputedStyle(backdrop);
    return {
      order: [...doc.querySelector('#layer-stage').children].map(node => node.id),
      nestedOrder: [...doc.querySelector('#nested-group').children].map(node => node.id),
      boxes: Object.fromEntries(ids.map(id => { const r = doc.getElementById(id).getBoundingClientRect(); return [id, { x: r.x, y: r.y, width: r.width, height: r.height }]; })),
      outerHit: doc.elementFromPoint(330, 260)?.closest('#blue-card,#orange-card')?.id,
      nestedHit: doc.elementFromPoint(300, 520)?.closest('#green-card,#purple-card')?.id,
      backdrop: { inlineStyle: backdrop.getAttribute('style'), zIndex: backdropStyle.zIndex, pointerEvents: backdropStyle.pointerEvents },
    };
  });
}

function sameLayout(actual, expected) {
  assert.deepEqual(actual.order, expected.order, 'Arranging layers changed source reading order');
  assert.deepEqual(actual.nestedOrder, expected.nestedOrder, 'Arranging a group changed nested reading order');
  assert.deepEqual(actual.backdrop, expected.backdrop, 'Arranging a card changed its negative, noninteractive backdrop');
  for (const id of Object.keys(expected.boxes)) for (const dimension of ['x', 'y', 'width', 'height']) {
    assert.ok(Math.abs(actual.boxes[id][dimension] - expected.boxes[id][dimension]) <= .5, `Arranging ${id} changed ${dimension}: ${JSON.stringify({ actual, expected })}`);
  }
}

async function clickSite(driver, physical, x, y, expectedId) {
  await closeLayers(driver);
  const point = await driver.execute((localX, localY) => {
    const frame = document.querySelector('.static-preview iframe'), rect = frame.getBoundingClientRect();
    const zoom = Number(document.querySelector('[data-testid="canvas"]').dataset.zoom);
    const at = { x: rect.x + localX * zoom, y: rect.y + localY * zoom };
    if (!document.elementFromPoint(at.x, at.y)?.closest('[data-testid="canvas"]')) throw new Error('The layer overlap is not visible in the owned canvas');
    return at;
  }, x, y);
  await physical.click(point);
  const selector = await selectorForId(driver, expectedId);
  await waitFor(() => driver.execute(target => document.querySelector('[data-testid="direct-element-selection"]')?.dataset.selector === target, selector), `A trusted overlap click did not select ${expectedId}`);
}

async function appearance(driver, id) {
  return driver.execute(value => {
    const doc = document.querySelector('.static-preview iframe').contentDocument, node = doc.getElementById(value);
    if (!node) throw new Error(`Missing visual layer ${value}`);
    const fields = ['backgroundColor', 'color', 'width', 'height', 'fontFamily', 'fontSize', 'paddingTop', 'borderRadius'];
    const css = doc.defaultView.getComputedStyle(node), before = doc.defaultView.getComputedStyle(node, '::before'), after = doc.defaultView.getComputedStyle(node, '::after'), marker = doc.defaultView.getComputedStyle(node.querySelector('li'), '::marker');
    return { normal: Object.fromEntries(fields.map(name => [name, css[name]])),
      before: { content: before.content, backgroundColor: before.backgroundColor, color: before.color, fontSize: before.fontSize },
      after: { content: after.content, backgroundColor: after.backgroundColor, width: after.width, height: after.height, borderRadius: after.borderRadius },
      marker: { content: marker.content, color: marker.color, fontSize: marker.fontSize } };
  }, id);
}

export async function layersWorkflow(driver, physical, step, evidence, provider) {
  const output = join(evidence, 'layers'); await mkdir(output, { recursive: true });
  const projectPath = join(output, 'Native layers.desegnos');
  let projectId, originalRequests, baseline, copiedId, copiedAppearance, savedProject;

  await step('Create two overlapping requests through trusted canvas drags', async () => {
    await driver.button('Blank website'); await driver.fill('Project name', 'Native layers'); await driver.button('Create project');
    projectId = await waitFor(async () => (await workspace(driver)).project?.id, 'The layer project did not open');
    await physical.initialize(evidence); await driver.button('Fit');
    const view = await physical.rect('.workspace-scroll');
    for (const [index, offset] of [0, 90].entries()) {
      await driver.click('[aria-label="Canvas tools"] button[title^="Add request"]');
      await physical.drag({ x: view.left + 140 + offset, y: view.top + 130 + offset / 2 }, { x: view.left + 440 + offset, y: view.top + 300 + offset / 2 });
      await driver.fill(`Request ${index + 1}`, index ? 'Place an orange card in front and a nested green and purple group below.' : 'Create a blue card with a name field, local link, graphic and badge.');
      await waitFor(async () => pending((await workspace(driver)).project).length === index + 1 && pending((await workspace(driver)).project)[index].prompt.trim(), 'The described request did not persist');
    }
    await driver.click('[aria-label="Canvas tools"] button[title^="Select"]');
    originalRequests = pending((await workspace(driver)).project);
    await assertCanvasOnly(driver);
  });

  await step('Request layer order changes hit testing without changing placement or text', async () => {
    await selectRequest(driver, physical, originalRequests[0].id); await arrange(driver, 'Bring to front');
    let current = pending((await workspace(driver)).project);
    assert.ok(current[0].zIndex > current[1].zIndex);
    assert.deepEqual(current.map(({ id, rect, prompt }) => ({ id, rect, prompt })), originalRequests.map(({ id, rect, prompt }) => ({ id, rect, prompt })));
    const hit = await driver.execute((first, second) => {
      const a = document.querySelector(`.annotation[data-region-id="${first}"]`).getBoundingClientRect(), b = document.querySelector(`.annotation[data-region-id="${second}"]`).getBoundingClientRect();
      const at = { x: Math.max(a.left, b.left) + 24, y: Math.max(a.top, b.top) + 24 };
      if (at.x >= Math.min(a.right, b.right) || at.y >= Math.min(a.bottom, b.bottom)) throw new Error('The requests must really overlap');
      return { at, id: document.elementFromPoint(at.x, at.y)?.closest('.annotation')?.dataset.regionId };
    }, current[0].id, current[1].id);
    assert.equal(hit.id, current[0].id); await closeLayers(driver); await physical.click(hit.at);
    assert.equal(await driver.execute(() => document.activeElement?.closest('.annotation')?.dataset.regionId), current[0].id);
    await selectRequest(driver, physical, current[0].id); await arrange(driver, 'Send to back');
    current = pending((await workspace(driver)).project); assert.ok(current[0].zIndex < current[1].zIndex);
    await physical.captureWindow(join(output, '01-request-layers.png'));
  });

  await step('Trusted layer copy and paste use the system clipboard and participate in request history', async () => {
    await selectRequest(driver, physical, originalRequests[0].id);
    const before = (await workspace(driver)).project;
    await physical.keys('C', 1, ['Control']);
    await waitFor(() => driver.execute(() => document.querySelector('.status-bar')?.textContent.includes('Layer copied.')), 'The real clipboard copy was not acknowledged');
    assert.equal((await workspace(driver)).project.revision, before.revision, 'Copy must not edit the project');
    await physical.keys('V', 1, ['Control']);
    const copied = await waitFor(async () => { const values = pending((await workspace(driver)).project); return values.length === 3 && values.find(value => !before.pages[0].regions.some(item => item.id === value.id)); }, 'The system clipboard request was not pasted');
    assert.equal(copied.prompt, originalRequests[0].prompt); assert.equal(copied.element ?? null, null);
    assert.equal(copied.rect.width, originalRequests[0].rect.width); assert.equal(copied.rect.height, originalRequests[0].rect.height);
    assert.equal(copied.rect.x, originalRequests[0].rect.x + 24); assert.equal(copied.rect.y, originalRequests[0].rect.y + 24);
    await physical.keys('Z', 1, ['Control']); await waitFor(async () => pending((await workspace(driver)).project).length === 2, 'Undo did not remove the pasted request');
    await physical.keys('Z', 1, ['Control', 'Shift']); await waitFor(async () => pending((await workspace(driver)).project).some(region => region.id === copied.id), 'Redo did not restore the same pasted request');
  });

  await step('Generate the controlled overlapping website through normal native preparation and application', async () => {
    await closeLayers(driver); await driver.invoke('set_ai_key', { key: provider.key(), storage: 'session' });
    provider.setMode('complete', { layerAudit: true }); await driver.button('Generate');
    const job = await waitFor(async () => { const value = await driver.invoke('get_generation', { projectId }); return value?.status === 'applied' && value; }, 'The native layer fixture did not apply', 25_000);
    assert.equal(provider.generationRequests().length, 1); assert.equal(provider.generationRequests()[0].markdown, job.requirements.markdown);
    await previewReady(driver); await driver.click('section[aria-label="Generated changes"] button.primary'); await driver.button('Fit'); await settled(driver);
    baseline = await siteState(driver); assert.equal(baseline.outerHit, 'orange-card'); assert.equal(baseline.nestedHit, 'purple-card');
    assert.equal(baseline.backdrop.zIndex, '-1'); assert.equal(baseline.backdrop.pointerEvents, 'none');
    // These wrappers have no selectable box; their visible children still need
    // actual Layers rows that select the exact current-source child.
    await selectSite(driver, physical, 'zero-child'); await selectSite(driver, physical, 'contents-child');
    await closeLayers(driver); await driver.button('Fit'); await settled(driver);
    await clickSite(driver, physical, 330, 260, 'orange-card');
    await physical.captureWindow(join(output, '02-generated-overlap.png'));
  });

  await step('The floating Layers popover fits both themes and a 760px window with trusted keyboard focus', async () => {
    const before = (await workspace(driver)).project;
    const original = await driver.command('GET', '/window/rect');
    try {
      for (const theme of ['Light', 'Dark']) {
        await closeLayers(driver); await driver.menu(['View', 'Editor appearance', theme]);
        await waitFor(() => driver.execute(value => document.documentElement.dataset.theme === value, theme.toLowerCase()), 'The editor theme did not apply');
        const target = await selectSite(driver, physical, 'blue-card'); await closeLayers(driver); await openLayers(driver); await settled(driver);
        assert.equal(await driver.execute(() => document.activeElement?.dataset.layerSelector), target, 'Opening Layers did not focus the selected row');
        const layout = await driver.execute(() => {
          const rect = document.querySelector('section[aria-label="Layers"]').getBoundingClientRect();
          return { left: rect.left, right: rect.right, top: rect.top, bottom: rect.bottom, width: window.innerWidth, height: window.innerHeight, overflow: document.documentElement.scrollWidth > window.innerWidth + 1 };
        });
        assert.ok(layout.left >= 0 && layout.right <= layout.width + 1 && layout.top >= 0 && layout.bottom <= layout.height + 1, JSON.stringify(layout)); assert.equal(layout.overflow, false);
        await physical.captureWindow(join(output, `layers-${theme.toLowerCase()}.png`));
      }
      await driver.command('POST', '/window/rect', { width: 760, height: 900 });
      await waitFor(() => driver.execute(() => window.innerWidth >= 740 && window.innerWidth <= 760), 'The owned editor did not reach 760px');
      await physical.initialize(evidence); await openLayers(driver); await settled(driver);
      const layout = await physical.rect(panel), viewport = await driver.execute(() => ({ width: innerWidth, height: innerHeight, scrollWidth: document.documentElement.scrollWidth }));
      assert.ok(layout.left >= 0 && layout.right <= viewport.width + 1 && layout.top >= 0 && layout.bottom <= viewport.height + 1, JSON.stringify({ layout, viewport })); assert.ok(viewport.scrollWidth <= viewport.width + 1);
      await physical.keys('End'); assert.equal(await driver.execute(() => !!document.activeElement?.matches('section[aria-label="Layers"] button.layer-select')), true);
      await physical.keys('Home'); await physical.keys('Down');
      assert.equal(await driver.execute(() => !!document.activeElement?.matches('section[aria-label="Layers"] button.layer-select')), true);
      await physical.captureWindow(join(output, 'layers-760px-keyboard.png'));
      await physical.keys('Escape'); await waitFor(() => driver.execute(() => !document.querySelector('section[aria-label="Layers"]') && document.activeElement?.hasAttribute('data-layers-trigger')), 'Escape did not close Layers and restore its trigger focus');
      assert.deepEqual((await workspace(driver)).project, before);
    } finally {
      await driver.command('POST', '/window/rect', { width: original.width, height: original.height, x: original.x, y: original.y }); await settled(driver); await physical.initialize(evidence); await driver.button('Fit'); await settled(driver);
    }
  });

  await step('Website layer controls change actual overlap hits while boxes and DOM order remain unchanged', async () => {
    await selectSite(driver, physical, 'blue-card'); await arrange(driver, 'Bring forward'); await previewReady(driver);
    let current = await siteState(driver); sameLayout(current, baseline); assert.equal(current.outerHit, 'blue-card');
    await clickSite(driver, physical, 330, 260, 'blue-card');
    await selectSite(driver, physical, 'blue-card'); await arrange(driver, 'Send backward'); await previewReady(driver);
    current = await siteState(driver); sameLayout(current, baseline); assert.equal(current.outerHit, 'orange-card');
    await selectSite(driver, physical, 'blue-card'); await arrange(driver, 'Bring to front'); await previewReady(driver);
    current = await siteState(driver); sameLayout(current, baseline); assert.equal(current.outerHit, 'blue-card');
    await selectSite(driver, physical, 'blue-card'); await arrange(driver, 'Send to back'); await previewReady(driver);
    current = await siteState(driver); sameLayout(current, baseline); assert.equal(current.outerHit, 'orange-card');
    await selectSite(driver, physical, 'blue-card');
    await changedBy(driver, () => physical.keys('bracketright', 1, ['Control', 'Shift']), 'The trusted Bring to front shortcut did not arrange the selected layer'); await previewReady(driver);
    current = await siteState(driver); sameLayout(current, baseline); assert.equal(current.outerHit, 'blue-card');
    await changedBy(driver, () => physical.keys('bracketleft', 1, ['Control', 'Shift']), 'The trusted Send to back shortcut did not arrange the selected layer'); await previewReady(driver);
    current = await siteState(driver); sameLayout(current, baseline); assert.equal(current.outerHit, 'orange-card');
    await physical.captureWindow(join(output, '03-website-layers.png'));
  });

  await step('Nested layers stay within their group and trusted Undo and Redo restore the visible order', async () => {
    await selectSite(driver, physical, 'green-card'); await arrange(driver, 'Bring to front'); await previewReady(driver);
    let current = await siteState(driver); sameLayout(current, baseline); assert.equal(current.nestedHit, 'green-card'); assert.equal(current.outerHit, 'orange-card');
    await clickSite(driver, physical, 300, 520, 'green-card'); await openLayers(driver);
    await changedBy(driver, () => physical.keys('Z', 1, ['Control']), 'Undo did not persist the previous nested order'); await previewReady(driver);
    current = await siteState(driver); sameLayout(current, baseline); assert.equal(current.nestedHit, 'purple-card');
    await changedBy(driver, () => physical.keys('Z', 1, ['Control', 'Shift']), 'Redo did not persist the nested layer order'); await previewReady(driver);
    current = await siteState(driver); sameLayout(current, baseline); assert.equal(current.nestedHit, 'green-card');
    await physical.captureWindow(join(output, '04-nested-history.png'));
  });

  await step('Copy and paste preserve current styling, CSS pseudo-elements, native controls and remapped SVG references', async () => {
    await selectSite(driver, physical, 'blue-card'); copiedAppearance = await appearance(driver, 'blue-card');
    assert.match(copiedAppearance.before.content, /Copied badge/); assert.equal(copiedAppearance.after.width, '14px');
    assert.equal(copiedAppearance.marker.color, 'rgb(250, 220, 80)'); assert.equal(copiedAppearance.marker.fontSize, '19px');
    const before = (await workspace(driver)).project;
    await driver.click(`${panel} button[aria-label="Copy layer"]`); await settled(driver);
    await driver.ready(`${panel} button[aria-label="Copy layer"]`);
    assert.equal((await workspace(driver)).project.revision, before.revision);
    await driver.click(`${panel} button[aria-label="Paste layer"]`);
    await waitFor(async () => (await workspace(driver)).project.revision > before.revision, 'The real clipboard site paste was not persisted'); await previewReady(driver);
    copiedId = await waitFor(() => driver.execute(() => {
      const selection = document.querySelector('[data-testid="direct-element-selection"]'), preview = document.querySelector('.static-preview');
      if (!selection || selection.dataset.sourceHash !== preview?.dataset.sourceHash) return false;
      const node = preview.querySelector('iframe').contentDocument.querySelector(selection.dataset.selector);
      return node?.getAttribute('aria-label') === 'Blue card' && node.id !== 'blue-card' && node.id;
    }), 'The pasted element did not own the current-source selection');
    assert.match(copiedId, /^desegnos-copy-/); assert.deepEqual(await appearance(driver, copiedId), copiedAppearance);
    const relationships = await driver.execute(id => {
      const doc = document.querySelector('.static-preview iframe').contentDocument, copy = doc.getElementById(id), input = copy.querySelector('input'), clip = copy.querySelector('clipPath'), shape = copy.querySelector('svg rect');
      const allIds = [...doc.querySelectorAll('[id]')].map(node => node.id), before = doc.getElementById('blue-card').getBoundingClientRect(), after = copy.getBoundingClientRect();
      return { unique: new Set(allIds).size === allIds.length, inputId: input.id, labelFor: copy.querySelector('label').htmlFor, href: copy.querySelector('a').getAttribute('href'), value: input.value,
        clipId: clip.id, clipPath: doc.defaultView.getComputedStyle(shape).clipPath, sourceInput: doc.getElementById('card-input')?.value, sourceClip: !!doc.getElementById('card-clip'), offset: { x: after.x - before.x, y: after.y - before.y } };
    }, copiedId);
    assert.equal(relationships.unique, true); assert.notEqual(relationships.inputId, 'card-input'); assert.equal(relationships.labelFor, relationships.inputId); assert.equal(relationships.href, `#${relationships.inputId}`);
    assert.equal(relationships.value, 'A real copied input'); assert.equal(relationships.sourceInput, relationships.value); assert.equal(relationships.sourceClip, true);
    assert.notEqual(relationships.clipId, 'card-clip'); assert.ok(relationships.clipPath.includes(`#${relationships.clipId}`)); assert.deepEqual(relationships.offset, { x: 24, y: 24 });
    await writeFile(join(output, 'copied-appearance.json'), JSON.stringify({ copiedId, copiedAppearance, relationships }, null, 2));
    await physical.captureWindow(join(output, '05-system-clipboard-copy.png'));
  });

  await step('Paste Undo and Redo restore the same copied identity and its visible badge', async () => {
    await closeLayers(driver); await changedBy(driver, () => driver.button('Undo'), 'Undo did not remove the copied website layer'); await previewReady(driver);
    assert.equal(await driver.execute(id => !!document.querySelector('.static-preview iframe').contentDocument.getElementById(id), copiedId), false);
    await changedBy(driver, () => driver.button('Redo'), 'Redo did not restore the copied website layer'); await previewReady(driver);
    assert.deepEqual(await appearance(driver, copiedId), copiedAppearance);
    assert.equal(provider.generationRequests().length, 1, 'Layer editing must not request AI generation');
  });

  await step('Trusted text Ctrl+C and Ctrl+V stay native and plain-text clipboard data cannot paste a site layer', async () => {
    await driver.click('[aria-label="Canvas tools"] button[title^="Add request"]');
    const view = await physical.rect('.workspace-scroll'); await physical.click({ x: view.left + 720, y: view.top + 95 });
    await driver.fill('Request 4', 'Ordinary text stays ordinary text.'); await settled(driver);
    const before = (await workspace(driver)).project; assert.equal(pending(before).length, 1);
    await physical.keys('A', 1, ['Control']); await physical.keys('C', 1, ['Control']);
    await physical.raw({ action: 'text', text: 'Temporary replacement' }); await physical.keys('A', 1, ['Control']); await physical.keys('V', 1, ['Control']);
    await waitFor(async () => pending((await workspace(driver)).project)[0]?.prompt === 'Ordinary text stays ordinary text.', 'The native text clipboard did not restore the selected text');
    assert.deepEqual((await workspace(driver)).project.pages[0].content, before.pages[0].content);
    assert.equal((await workspace(driver)).project.pages[0].regions.length, before.pages[0].regions.length);
    await selectSite(driver, physical, 'blue-card'); const invalidBefore = (await workspace(driver)).project;
    await physical.keys('V', 1, ['Control']); await driver.ready('.element-edit-error[role="alert"]');
    assert.match(await driver.execute(() => document.querySelector('.element-edit-error').textContent), /clipboard|layer|copied/i);
    assert.deepEqual((await workspace(driver)).project, invalidBefore); await driver.click('.element-edit-error button');
    const events = await physical.events(); assert.ok(events.some(event => event.type === 'keydown' && event.trusted && event.key.toLowerCase() === 'v'));
    await physical.captureWindow(join(output, '06-text-clipboard.png'));
  });

  await step('Save and reopen preserve actual layer source, references and request order', async () => {
    await closeLayers(driver); await chooseFile(physical, () => driver.button('Save'), projectPath, output, 'layers-save');
    savedProject = (await savedState(driver, projectPath)).project;
    await verifyArchive(projectPath, savedProject, await driver.invoke('get_project_notes'));
    await chooseFile(physical, () => driver.menu(['Project', 'Open project']), projectPath, output, 'layers-open');
    await waitFor(async () => (await workspace(driver)).project?.id === savedProject.id && (await workspace(driver)).path === projectPath, 'The saved layer project did not reopen');
    assert.deepEqual((await workspace(driver)).project, savedProject); await previewReady(driver); assert.deepEqual(await appearance(driver, copiedId), copiedAppearance);
    await physical.captureWindow(join(output, '07-reopened-layers.png'));
  });

  await step('Exported ordinary files preserve copied layers and pseudo-elements without native runtime', async () => {
    const parent = join(output, 'export-parent'); await mkdir(parent, { recursive: true });
    await chooseFolder(physical, () => driver.button('Export'), parent, output, 'layers-export');
    const directory = await waitFor(async () => { const entries = (await readdir(parent, { withFileTypes: true })).filter(item => item.isDirectory()); return entries.length === 1 && join(parent, entries[0].name); }, 'Export did not create the real website folder');
    const index = join(directory, 'index.html'); assert.match(await readFile(index, 'utf8'), /Blue card/);
    const result = await inspectChromiumPage({ url: pathToFileURL(index).href, outputDirectory: output, label: 'exported-layers', expression: `(() => {
      const node = document.getElementById(${JSON.stringify(copiedId)}), input = node.querySelector('input'), clip = node.querySelector('clipPath');
      return { native: !!(window.__TAURI__ || window.__TAURI_INTERNALS__), background: getComputedStyle(node).backgroundColor, before: getComputedStyle(node, '::before').content, after: getComputedStyle(node, '::after').width,
        markerColor: getComputedStyle(node.querySelector('li'), '::marker').color, markerSize: getComputedStyle(node.querySelector('li'), '::marker').fontSize,
        input: input.value, labelFor: node.querySelector('label').htmlFor, inputId: input.id, href: node.querySelector('a').getAttribute('href'), clipId: clip.id, clipPath: getComputedStyle(node.querySelector('svg rect')).clipPath,
        ids: [...document.querySelectorAll('[id]')].map(item => item.id) };
    })()` });
    assert.equal(result.value.native, false); assert.equal(result.value.background, copiedAppearance.normal.backgroundColor); assert.match(result.value.before, /Copied badge/); assert.equal(result.value.after, '14px');
    assert.equal(result.value.markerColor, copiedAppearance.marker.color); assert.equal(result.value.markerSize, copiedAppearance.marker.fontSize);
    assert.equal(result.value.input, 'A real copied input'); assert.equal(result.value.labelFor, result.value.inputId); assert.equal(result.value.href, `#${result.value.inputId}`); assert.ok(result.value.clipPath.includes(`#${result.value.clipId}`));
    assert.equal(new Set(result.value.ids).size, result.value.ids.length); assert.equal(result.profileRemoved, true);
    await writeFile(join(output, 'exported-layer-report.json'), JSON.stringify(result, null, 2));
  });

  await step('Leave one acknowledged unsaved request edit for the shared native crash-recovery check', async () => {
    const archiveHash = hash(await readFile(projectPath)), request = pending((await workspace(driver)).project)[0];
    await selectRequest(driver, physical, request.id); await closeLayers(driver);
    await driver.fill('Request 4', 'Recovery keeps the copied website layers and this unsaved request.');
    await waitFor(async () => { const value = await workspace(driver); return value.dirty && value.project.revision > savedProject.revision && pending(value.project)[0]?.prompt.includes('unsaved request'); }, 'The final unsaved layer edit was not journaled');
    assert.equal(hash(await readFile(projectPath)), archiveHash); assert.equal(provider.generationRequests().length, 1); assert.deepEqual(provider.errors(), []); await assertCanvasOnly(driver);
  });
  return (await workspace(driver)).project;
}
