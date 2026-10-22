import assert from 'node:assert/strict';
import { writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { waitFor } from './webdriver.mjs';

export async function interactionWorkflow(driver, physical, step, evidence) {
  const project = async () => (await driver.invoke('get_workspace')).project;
  const original = await project();
  const canvas = '[data-testid="canvas"]';
  const zoom = () => driver.execute(() => Number(document.querySelector('[data-testid="canvas"]').dataset.zoom));
  const focusCanvas = () => driver.execute(() => document.querySelector('[data-testid="canvas"]').focus());
  const blank = () => driver.execute(() => {
    const node = document.querySelector('[data-testid="workspace-scroll"]'); const r = node.getBoundingClientRect();
    for (let y = r.top + 60; y < r.bottom - 120; y += 65) for (let x = r.left + 70; x < r.right - 160; x += 90) {
      if ([0, 50, 100].every(dx => document.elementFromPoint(x + dx, y)?.dataset.testid === 'canvas')) return { x, y };
    }
    throw new Error('No unobstructed canvas point for physical interactions');
  });
  const regions = async () => (await project()).pages[0].regions;
  await physical.initialize(evidence);
  await step('Desegnos appears in the actual native window, page title and editor wordmark', async () => {
    assert.equal((await physical.inspect()).window.title, 'Desegnos');
    assert.equal(await driver.execute(() => document.title), 'Desegnos');
    assert.equal(await driver.execute(() => document.querySelector('.wordmark').textContent), 'Desegnos');
  });
  await step('Left-button dragging pans the workspace without creating or editing requests', async () => {
    await driver.press('[data-testid="canvas"]', '\uE011');
    const before = await project(); const point = await blank(); const from = await physical.rect(canvas);
    await physical.resetEvents();
    await physical.drag(point, { x: point.x + 75, y: point.y + 40 });
    const after = await physical.rect(canvas);
    assert.ok(Math.abs(after.left - from.left - 75) < 2);
    assert.ok(Math.abs(after.top - from.top - 40) < 2);
    assert.deepEqual(await project(), before);
    await physical.assertPointerGesture();
    // A drag followed by a click is not a pair of stationary clicks.
    await physical.click({ x: point.x + 75, y: point.y + 40 });
    await driver.executeAsync(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve(true)))));
    assert.deepEqual(await project(), before, 'Finishing a pan then clicking must not open a request');
  });
  await step('Ctrl+wheel zooms at the pointer while ordinary and Shift wheels still scroll', async () => {
    const point = await blank(); const before = await project(); const size = await driver.execute(() => ({ width: innerWidth, height: innerHeight, font: getComputedStyle(document.querySelector('.wordmark')).fontSize }));
    const from = await physical.rect(canvas); const startZoom = await zoom();
    const anchor = physical.canvasPoint(from, point, startZoom);
    await physical.resetEvents(); await physical.wheel(point, 'up', ['Control'], 2);
    await waitFor(async () => await zoom() > startZoom, 'Ctrl+wheel did not zoom the canvas');
    const to = await physical.rect(canvas); const endZoom = await zoom(); const anchored = physical.canvasPoint(to, point, endZoom);
    assert.ok(Math.abs(anchor.x - anchored.x) < 2 && Math.abs(anchor.y - anchored.y) < 2, JSON.stringify({anchor, anchored}));
    assert.deepEqual(await driver.execute(() => ({ width: innerWidth, height: innerHeight, font: getComputedStyle(document.querySelector('.wordmark')).fontSize })), size);
    assert.ok((await physical.events()).some(event => event.type === 'wheel' && event.trusted));
    const scrollBefore = await physical.rect(canvas); await physical.wheel(point, 'down');
    await waitFor(async () => (await physical.rect(canvas)).top < scrollBefore.top, 'Ordinary wheel did not scroll vertically');
    const horizontal = await physical.rect(canvas); await physical.wheel(point, 'down', ['Shift']);
    await waitFor(async () => (await physical.rect(canvas)).left < horizontal.left, 'Shift+wheel did not scroll horizontally');
    assert.deepEqual(await project(), before);
    await writeFile(join(evidence, 'interaction-wheel.json'), JSON.stringify({startZoom,endZoom,anchor,anchored,size}, null, 2));
  });
  await step('Zoom and help shortcuts operate outside prompts and preserve project data', async () => {
    await focusCanvas(); await physical.keys('1', 1, ['Control']); await waitFor(async () => await zoom() === 1, 'Ctrl+1 actual size failed');
    await physical.keys('equal', 1, ['Control']); await waitFor(async () => await zoom() === 1.25, 'Ctrl+plus failed');
    await physical.keys('minus', 1, ['Control']); await waitFor(async () => await zoom() === 1, 'Ctrl+minus failed');
    await physical.keys('2', 1, ['Control']); await waitFor(async () => await zoom() === 2, 'Ctrl+2 failed');
    await physical.keys('0', 1, ['Control']);
    await waitFor(() => driver.execute(() => { const g = document.querySelector('[data-testid="screen-guide"]').getBoundingClientRect(); const w = document.querySelector('[data-testid="workspace-scroll"]').getBoundingClientRect(); return g.left >= w.left + 40 && g.right <= w.right - 40 && g.top >= w.top + 40 && g.bottom <= w.bottom - 40; }), 'Ctrl+0 did not fit the complete guide');
    await physical.keys('F1'); await waitFor(() => driver.execute(() => !!document.querySelector('.shortcut-help')), 'F1 did not show shortcut help');
    assert.ok(await driver.execute(() => document.querySelector('.shortcut-help').textContent.includes('Ctrl/Cmd+Shift+Z')));
    await physical.captureWindow(join(evidence, 'desegnos-shortcuts-window.png'));
    await physical.keys('Escape'); await waitFor(() => driver.execute(() => !document.querySelector('dialog[open]')), 'Escape did not close help');
    assert.deepEqual((await project()).pages, original.pages);
    await driver.select('Zoom', '0.75'); await driver.press('[data-testid="canvas"]', '\uE011');
  });
  await step('Abandoned empty boxes disappear, recent typing survives and keyboard undo/redo restores selections', async () => {
    const firstPoint = await blank(); await physical.doubleClick(firstPoint);
    await waitFor(async () => (await regions()).length === original.pages[0].regions.length + 1, 'First empty request was not created');
    const first = (await regions()).at(-1);
    assert.equal(await driver.execute(id => !!document.querySelector(`[data-region-id="${id}"] textarea`), first.id), true);
    const secondPoint = await blank(); await physical.doubleClick(secondPoint);
    await waitFor(async () => (await regions()).length === original.pages[0].regions.length + 2, 'Second empty request was not created');
    const second = (await regions()).at(-1);
    assert.equal((await regions()).find(item => item.id === first.id).completed, true);
    assert.equal(await driver.execute(id => !!document.querySelector(`[data-region-id="${id}"] textarea`), first.id), false);
    // Type then immediately switch selection; the queued text commit must precede cleanup.
    await physical.raw({action:'text',text:'Keep this x'});
    await waitFor(async () => (await regions()).at(-1).prompt === 'Keep this x', 'Native text did not commit');
    await physical.keys('Z', 1, ['Control']);
    await waitFor(async () => (await regions()).at(-1).prompt !== 'Keep this x', 'Text undo did not update the request');
    assert.equal((await regions()).length, original.pages[0].regions.length + 2);
    await physical.keys('Z', 1, ['Control','Shift']);
    await waitFor(async () => (await regions()).at(-1).prompt === 'Keep this x', 'Text redo was intercepted by project redo');
    await physical.doubleClick(await blank());
    await waitFor(async () => (await regions()).length === original.pages[0].regions.length + 3, 'Third request was not created');
    const accepted = (await regions()).find(item => item.id === second.id);
    assert.equal(accepted.prompt, 'Keep this x'); assert.equal(!!accepted.completed, false);
    await focusCanvas(); const prior = await regions();
    await physical.keys('Z', 1, ['Control']);
    await waitFor(async () => (await regions()).length === prior.length - 1, 'Ctrl+Z did not undo area creation');
    await physical.keys('Z', 1, ['Control','Shift']);
    await waitFor(async () => (await regions()).length === prior.length, 'Ctrl+Shift+Z did not redo area creation');
    // Restore the authoring fixture through user-visible undo; no direct state replacement.
    for (let index = 0; index < 40 && JSON.stringify((await project()).pages) !== JSON.stringify(original.pages); index++) {
      const revision = (await project()).revision; await focusCanvas(); await physical.keys('Z', 1, ['Control']);
      await waitFor(async () => (await project()).revision > revision, 'Undo failed while restoring the authoring fixture');
    }
    assert.deepEqual((await project()).pages, original.pages);
  });
  await physical.cleanup();
  return project();
}
