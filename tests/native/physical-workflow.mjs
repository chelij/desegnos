import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { writeFile } from 'node:fs/promises';
import { join, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { waitFor } from './webdriver.mjs';
import { assertCanvasOnly, reviewRequirements, assertGenerationModes } from './workflow.mjs';

const helper = fileURLToPath(new URL('./x11-input.py', import.meta.url));

function assertRectClose(actual, expected, tolerance = 0.000001) {
  for (const key of ['x', 'y', 'width', 'height']) {
    assert.ok(Math.abs(actual[key] - expected[key]) <= tolerance, `${key}: expected ${expected[key]}, received ${actual[key]}`);
  }
}

export class PhysicalInput {
  constructor(driver, pid) { this.driver = driver; this.pid = pid; }

  raw(request) {
    return new Promise((accept, reject) => {
      const child = spawn('/usr/bin/python3', [helper], { stdio: ['pipe', 'pipe', 'pipe'] });
      let stdout = ''; let stderr = '';
      const timeout = setTimeout(() => { child.kill('SIGKILL'); reject(new Error('The isolated X11 input helper timed out')); }, 10_000);
      child.stdout.on('data', chunk => { stdout += chunk; if (stdout.length > 512_000) child.kill('SIGKILL'); });
      child.stderr.on('data', chunk => { stderr += chunk; if (stderr.length > 32_000) child.kill('SIGKILL'); });
      child.once('error', error => { clearTimeout(timeout); reject(error); });
      child.once('close', code => {
        clearTimeout(timeout);
        if (code !== 0) { reject(new Error(`Isolated X11 input failed: ${stderr || `exit ${code}`}`)); return; }
        try { accept(JSON.parse(stdout)); } catch (error) { reject(error); }
      });
      child.stdin.end(JSON.stringify({ ...request, pid: this.pid }));
    });
  }

  inspect() { return this.raw({ action: 'inspect' }); }

  async captureWindow(destination, fromScreen = true) {
    const evidence = process.env.DESEGNOS_NATIVE_TEST_EVIDENCE;
    assert.ok(evidence && resolve(destination).startsWith(resolve(evidence) + sep), 'Window captures must stay in this test evidence directory');
    const native = await this.inspect();
    await new Promise((accept, reject) => {
      const child = spawn('/usr/bin/import', ['-display', native.display, ...(fromScreen ? ['-screen'] : []), '-window', String(native.window.id), destination], { stdio: ['ignore', 'ignore', 'pipe'] });
      let stderr = '';
      const timeout = setTimeout(() => { child.kill('SIGKILL'); reject(new Error('The owned-window capture timed out')); }, 10_000);
      child.stderr.on('data', chunk => { stderr += chunk; if (stderr.length > 32_000) child.kill('SIGKILL'); });
      child.once('error', error => { clearTimeout(timeout); reject(error); });
      child.once('close', code => { clearTimeout(timeout); if (code === 0) accept(); else reject(new Error(`Owned-window capture failed: ${stderr || code}`)); });
    });
    return { display: native.display, pid: native.pid, window: native.window, fromScreen };
  }

  async initialize(evidence) {
    const native = await this.inspect();
    const windowRect = await this.driver.command('GET', '/window/rect');
    const viewport = await this.driver.execute(() => ({ width: window.innerWidth, height: window.innerHeight, scale: window.devicePixelRatio }));
    assert.ok(viewport.scale > 0 && Number.isFinite(viewport.scale));
    assert.ok(Math.abs(native.window.x - windowRect.x * viewport.scale) <= 2, 'The X11 and native driver window positions disagree');
    assert.ok(Math.abs(native.window.y - windowRect.y * viewport.scale) <= 2, 'The X11 and native driver window positions disagree');
    await this.driver.execute(() => {
      if (window.__desegnosRecordNativeEvent) {
        for (const type of ['pointermove', 'pointerdown', 'pointerup', 'keydown', 'keyup', 'wheel', 'click', 'dblclick', 'contextmenu']) {
          document.removeEventListener(type, window.__desegnosRecordNativeEvent, true);
        }
      }
      window.__desegnosNativeEvents = [];
      window.__desegnosRecordNativeEvent = event => {
        window.__desegnosNativeEvents.push({ type: event.type, trusted: event.isTrusted, key: event.key, button: event.button, target: event.target?.tagName, targetClass: event.target?.className,
          clientX: event.clientX, clientY: event.clientY, screenX: event.screenX, screenY: event.screenY });
        window.__desegnosNativeEvents = window.__desegnosNativeEvents.slice(-64);
      };
      for (const type of ['pointermove', 'pointerdown', 'pointerup', 'keydown', 'keyup', 'wheel', 'click', 'dblclick', 'contextmenu']) {
        document.addEventListener(type, window.__desegnosRecordNativeEvent, true);
      }
    });
    const rootPoint = { x: native.window.x + Math.floor(native.window.width / 2), y: native.window.y + Math.floor(native.window.height / 2) };
    await this.raw({ action: 'move', point: { x: rootPoint.x - 1, y: rootPoint.y - 1 } });
    await this.raw({ action: 'move', point: rootPoint });
    const observed = await waitFor(() => this.driver.execute(() => window.__desegnosNativeEvents.filter(event => event.type === 'pointermove').at(-1)), 'The real X11 pointer did not reach the native webview');
    assert.equal(observed.trusted, true, 'The calibration event was not a trusted operating-system pointer event');
    assert.ok(Math.abs(observed.screenX * viewport.scale - rootPoint.x) <= 1);
    assert.ok(Math.abs(observed.screenY * viewport.scale - rootPoint.y) <= 1);
    this.origin = { x: rootPoint.x - observed.clientX * viewport.scale, y: rootPoint.y - observed.clientY * viewport.scale };
    this.scale = viewport.scale;
    assert.ok(this.origin.x >= native.window.x - 1 && this.origin.y >= native.window.y - 1, 'The webview origin lies outside its owned X11 window');
    assert.ok(this.origin.x + viewport.width * this.scale <= native.window.x + native.window.width + 1);
    assert.ok(this.origin.y + viewport.height * this.scale <= native.window.y + native.window.height + 1);
    await writeFile(join(evidence, 'pointer-geometry.json'), JSON.stringify({ native, windowRect, viewport, observed, origin: this.origin }, null, 2));
  }

  rootPoint(point) {
    assert.ok(this.origin, 'Calibrate the owned native window before physical input');
    return { x: Math.round(this.origin.x + point.x * this.scale), y: Math.round(this.origin.y + point.y * this.scale) };
  }

  canvasPoint(canvas, point, zoom) {
    const root = this.rootPoint(point);
    return { x: ((root.x - this.origin.x) / this.scale - canvas.left) / zoom + canvas.worldLeft,
      y: ((root.y - this.origin.y) / this.scale - canvas.top) / zoom + canvas.worldTop };
  }

  worldPoint(canvas, point, zoom) {
    return { x: canvas.left + (point.x - canvas.worldLeft) * zoom, y: canvas.top + (point.y - canvas.worldTop) * zoom };
  }

  doubleClick(point) { return this.raw({ action: 'click', count: 2, point: this.rootPoint(point) }); }
  click(point) { return this.raw({ action: 'click', point: this.rootPoint(point) }); }
  rightClick(point) { return this.raw({ action: 'click', button: 3, point: this.rootPoint(point) }); }
  drag(from, to, modifiers = []) { return this.raw({ modifiers, action: 'drag', from: this.rootPoint(from), to: this.rootPoint(to) }); }
  path(points, cancelDrag = false) { return this.raw({ action: 'drag', from: this.rootPoint(points[0]), to: this.rootPoint(points.at(-1)), path: points.map(point => this.rootPoint(point)), cancelDrag }); }
  wheel(point, direction, modifiers = [], count = 1) { return this.raw({ action: 'wheel', point: this.rootPoint(point), direction, modifiers, count }); }
  keys(key, count = 1, modifiers = []) { return this.raw({ modifiers, action: 'key', key, count }); }
  resetEvents() { return this.driver.execute(() => { window.__desegnosNativeEvents = []; }); }
  events() { return this.driver.execute(() => window.__desegnosNativeEvents); }

  rect(selector) {
    return this.driver.execute(value => {
      const element = document.querySelector(value);
      if (!element) throw new Error(`Missing physical input target ${value}`);
      const rect = element.getBoundingClientRect();
      return { left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom, width: rect.width, height: rect.height,
        worldLeft: Number(element.dataset.worldLeft ?? 0), worldTop: Number(element.dataset.worldTop ?? 0), zoom: Number(element.dataset.zoom ?? 1) };
    }, selector);
  }

  async center(selector) {
    const rect = await this.rect(selector);
    const point = { x: (rect.left + rect.right) / 2, y: (rect.top + rect.bottom) / 2 };
    assert.equal(await this.driver.execute((value, at) => !!document.elementFromPoint(at.x, at.y)?.closest(value), selector, point), true, `Physical target ${selector} is covered or outside the viewport`);
    return point;
  }

  async assertPointerGesture() {
    const events = await waitFor(async () => {
      const events = await this.events();
      return events.some(event => event.type === 'pointerup' && event.trusted) && events;
    }, 'The trusted pointer release did not reach the webview');
    assert.ok(events.some(event => event.type === 'pointerdown' && event.trusted));
    assert.ok(events.some(event => event.type === 'pointerup' && event.trusted));
    assert.equal(events.filter(event => event.type.startsWith('pointer')).every(event => event.trusted), true);
  }

  cleanup() {
    return this.driver.execute(() => {
      for (const type of ['pointermove', 'pointerdown', 'pointerup', 'keydown', 'keyup', 'wheel', 'click', 'dblclick', 'contextmenu']) {
        document.removeEventListener(type, window.__desegnosRecordNativeEvent, true);
      }
      delete window.__desegnosRecordNativeEvent; delete window.__desegnosNativeEvents;
    });
  }
}

export async function physicalAuthoring(driver, physical, step, evidence) {
  const region = async index => (await driver.invoke('get_workspace')).project.pages[0].regions[index];
  await step('Physical input is restricted to the app-owned Xvfb window', () => physical.initialize(evidence));

  await step('A real pointer drag creates an on-canvas request with placement intent at 75% zoom', async () => {
    await driver.button('Grid and rulers');
    await driver.click(driver.field('Show grid'), 'xpath');
    await driver.click(driver.field('Snap requests to the grid'), 'xpath');
    await driver.button('Save grid settings');
    await waitFor(async () => { const { project } = await driver.invoke('get_workspace'); return !project.showGrid && !project.gridSettings.snap; }, 'Disable snap and grid visibility deliberately before the exact physical gesture');
    const canvas = await physical.rect('[data-testid="canvas"]');
    const at = (x, y) => physical.worldPoint(canvas, { x, y }, 0.75);
    const from = at(40, 600); const to = at(280, 760);
    const start = physical.canvasPoint(canvas, from, 0.75); const end = physical.canvasPoint(canvas, to, 0.75);
    const expected = { ...start, width: end.x - start.x, height: end.y - start.y };
    assert.equal(await driver.execute(point => document.elementFromPoint(point.x, point.y)?.dataset.testid === 'canvas', from), true);
    await physical.resetEvents();
    await physical.drag(from, to, ['Shift']);
    await waitFor(async () => (await driver.invoke('get_workspace')).project.pages[0].regions.length === 2, 'The physical drag did not create a second request');
    assertRectClose((await region(1)).rect, expected);
    assertRectClose((await region(1)).rect, { x: 40, y: 600, width: 240, height: 160 }, 1);
    await physical.assertPointerGesture();
    await writeFile(join(evidence, 'pointer-draw-geometry.json'), JSON.stringify({ canvas, from, to,
      physicalFrom: physical.rootPoint(from), physicalTo: physical.rootPoint(to), expected, actual: (await region(1)).rect }, null, 2));
    await writeFile(join(evidence, 'pointer-draw-events.json'), JSON.stringify(await physical.events(), null, 2));
    assert.equal(await driver.execute(() => document.activeElement?.getAttribute('aria-label')), 'Request 2');
    await assertCanvasOnly(driver);
  });

  await step('Real typing and text selection stay inside the request instead of dragging it', async () => {
    const textarea = 'textarea[aria-label="Request 2"]';
    const value = 'Add a contact section with a real form.';
    await physical.click(await physical.center(textarea));
    await physical.resetEvents();
    await physical.raw({ action: 'text', text: value });
    await waitFor(async () => (await region(1)).prompt === value, 'Physical typing did not reach the on-canvas request');
    const keys = (await physical.events()).filter(event => event.type === 'keydown');
    assert.ok(keys.length > 0 && keys.every(event => event.trusted));
    const before = (await driver.invoke('get_workspace')).project;
    const bounds = await physical.rect(textarea);
    const from = { x: bounds.left + 12, y: bounds.top + 12 };
    const to = { x: Math.min(bounds.right - 8, from.x + 45), y: from.y };
    await physical.drag(from, to);
    await driver.executeAsync(() => new Promise(accept => requestAnimationFrame(() => requestAnimationFrame(() => accept(true)))));
    assert.equal(await driver.execute(selector => {
      const input = document.querySelector(selector);
      return input.selectionEnd > input.selectionStart;
    }, textarea), true, 'The real textarea drag did not select any input text');
    const after = (await driver.invoke('get_workspace')).project;
    assert.deepEqual(after, before, 'Dragging within a textarea must select text without editing placement or creating requests');
    await writeFile(join(evidence, 'on-canvas-text-events.json'), JSON.stringify(await physical.events(), null, 2));
  });

  await step('One global Fuzzy or Strict choice applies to every described request', async () => {
    for (const mode of ['fuzzy', 'strict']) {
      await driver.button(mode === 'fuzzy' ? 'Fuzzy' : 'Strict');
      await waitFor(async () => (await driver.invoke('get_workspace')).project.generationMode === mode, 'The global generation setting did not persist');
      const checked = await reviewRequirements(driver, mode, evidence, `two-requests-${mode}`);
      assert.equal(checked.requests.length, 2);
      await assertGenerationModes(driver);

      assert.equal((await driver.invoke('get_workspace')).project.pages[0].regions.every(request => !('mode' in request)), true);
    }
  });

  await step('Off-guide signed requests stay on the free canvas and enter generation requirements', async () => {
    const before = (await driver.invoke('get_workspace')).project;
    const canvas = await physical.rect('[data-testid="canvas"]');
    const guide = await physical.rect('[data-testid="screen-guide"]');
    assert.ok(canvas.worldLeft < 0 && canvas.worldTop < 0, 'The editable canvas must extend above and left of the screen guide');
    assert.ok(canvas.width > guide.width && canvas.height > guide.height, 'The guide must not constrain the editable canvas');
    assert.equal(await driver.execute(() => !!document.querySelector('.ruler-x .ruler-label') && !!document.querySelector('.ruler-y .ruler-label')), true, 'Percentage ruler ticks must be visible');
    const from = physical.worldPoint(canvas, { x: -280, y: 180 }, 0.75);
    const to = physical.worldPoint(canvas, { x: -40, y: 340 }, 0.75);
    assert.equal(await driver.execute(point => document.elementFromPoint(point.x, point.y)?.dataset.testid === 'canvas', from), true);
    await physical.drag(from, to, ['Shift']);
    await waitFor(async () => (await driver.invoke('get_workspace')).project.pages[0].regions.length === 3, 'The free canvas rejected a request to the left of the guide');
    await driver.fill('Request 3', 'Place a companion section to the left of the screen guide.');
    await waitFor(async () => (await region(2)).prompt.includes('companion section'), 'The off-guide request was not committed');
    const request = await region(2);
    assert.ok(request.rect.x < 0 && request.rect.x + request.rect.width < 0);
    const reviewed = await reviewRequirements(driver, before.generationMode, evidence, 'off-guide-requests');
    const frozen = reviewed.requests.find(item => item.regionId === request.id);
    assert.deepEqual(frozen.placementHint, request.rect);
    assert.ok(frozen.placementPercent.x < 0, 'Requirements must express intentional negative percentages');
    await driver.click('.element-toolbar [aria-label="Delete layer"]');
    await driver.click('//section[contains(@class,"workspace-surface")]//button[normalize-space(.)="Remove"]', 'xpath');
    await waitFor(async () => (await driver.invoke('get_workspace')).project.pages[0].regions.length === 2, 'The temporary off-guide request was not removed');
    assert.deepEqual((await driver.invoke('get_workspace')).project.pages, before.pages);
  });

  await step('The configured percentage grid snaps a real off-guide drag and keyboard movement', async () => {
    const before = (await driver.invoke('get_workspace')).project;
    await driver.button('Grid and rulers');
    await driver.click(driver.field('Show grid'), 'xpath');
    await driver.click(driver.field('Snap requests to the grid'), 'xpath');
    await driver.button('Save grid settings');
    await waitFor(async () => { const { project } = await driver.invoke('get_workspace'); return project.showGrid && project.gridSettings.snap; }, 'The configured percentage snapping did not turn on');
    const canvas = await physical.rect('[data-testid="canvas"]');
    const from = physical.worldPoint(canvas, { x: -600, y: 400 }, 0.75);
    const to = physical.worldPoint(canvas, { x: -360, y: 600 }, 0.75);
    assert.equal(await driver.execute(point => document.elementFromPoint(point.x, point.y)?.dataset.testid === 'canvas', from), true);
    await physical.drag(from, to, ['Shift']);
    const snapped = await waitFor(async () => { const { project } = await driver.invoke('get_workspace'); return project.pages[0].regions.length === 3 && project.pages[0].regions[2]; }, 'The real snapped drag did not create a request');
    const steps = { x: before.viewport.width * 0.05, y: before.viewport.height * 0.05 };
    for (const [value, step] of [[snapped.rect.x, steps.x], [snapped.rect.y, steps.y], [snapped.rect.x + snapped.rect.width, steps.x], [snapped.rect.y + snapped.rect.height, steps.y]]) {
      assert.ok(Math.abs(value / step - Math.round(value / step)) < 0.000001, `The placement ${value} did not snap to a 5% grid step of ${step}`);
    }
    await physical.click(await physical.center('button[aria-label="Move request 3"]'));
    await physical.keys('Right');
    const moved = await waitFor(async () => { const value = await region(2); return Math.abs(value.rect.x - snapped.rect.x - steps.x) < 0.000001 && value; }, 'The move grip did not use the configured 5% step');
    await writeFile(join(evidence, 'percentage-grid-gesture.json'), JSON.stringify({ settings: { ...before.gridSettings, snap: true }, steps, snapped: snapped.rect, moved: moved.rect }, null, 2));
    await driver.click('.element-toolbar [aria-label="Delete layer"]');
    await driver.click('//section[contains(@class,"workspace-surface")]//button[normalize-space(.)="Remove"]', 'xpath');
    await waitFor(async () => (await driver.invoke('get_workspace')).project.pages[0].regions.length === 2, 'The temporary snapping request was not removed');
    await driver.button('Grid and rulers');
    await driver.click(driver.field('Show grid'), 'xpath');
    await driver.click(driver.field('Snap requests to the grid'), 'xpath');
    await driver.button('Save grid settings');
    await waitFor(async () => { const { project } = await driver.invoke('get_workspace'); return !project.showGrid && !project.gridSettings.snap; }, 'The exact-gesture grid settings were not restored');
    assert.deepEqual((await driver.invoke('get_workspace')).project.pages, before.pages);
  });

  await step('Single clicks and Shift-click open no prompt; a double-click opens one focused request and Undo removes it', async () => {
    const before = (await driver.invoke('get_workspace')).project;
    const canvas = await physical.rect('[data-testid="canvas"]');
    const point = physical.worldPoint(canvas, { x: 340, y: 600 }, 0.75);
    assert.equal(await driver.execute(at => document.elementFromPoint(at.x, at.y)?.dataset.testid === 'canvas', point), true);
    await physical.click(point);
    await driver.executeAsync(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve(true)))));
    assert.deepEqual((await driver.invoke('get_workspace')).project, before, 'A single click must not open a prompt');
    await physical.raw({ action: 'click', point: physical.rootPoint(point), modifiers: ['Shift'] });
    await driver.executeAsync(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve(true)))));
    assert.deepEqual((await driver.invoke('get_workspace')).project, before, 'A Shift-click must not bypass double-click activation');
    await physical.resetEvents();
    // Start a distinct click pair rather than extending the preceding single-click sequence.
    await physical.doubleClick({ x: point.x + 24, y: point.y });
    await waitFor(async () => (await driver.invoke('get_workspace')).project.pages[0].regions.length === 3, 'Double-clicking the empty page did not open a request');
    assert.ok((await physical.events()).some(event => event.type === 'dblclick' && event.trusted), 'Activation must use a real operating-system double-click');
    assert.equal(await driver.execute(() => document.activeElement?.getAttribute('aria-label')), 'Request 3');
    assert.equal((await region(2)).prompt, '');
    await driver.button('Undo');
    await waitFor(async () => (await driver.invoke('get_workspace')).project.pages[0].regions.length === 2, 'Undo did not remove the clicked request');
    assert.deepEqual((await driver.invoke('get_workspace')).project.pages, before.pages);
  });

  await step('Real pointer move and resize preserve the accepted geometry', async () => {
    await physical.click(await physical.center('textarea[aria-label="Request 2"]'));
    const target = 'button[aria-label="Move request 2"]';
    const beforeMove = (await region(1)).rect;
    let from = await physical.center(target);
    await physical.resetEvents();
    await physical.drag(from, { x: from.x + 32 * 0.75, y: from.y + 24 * 0.75 });
    await waitFor(async () => Math.abs((await region(1)).rect.x - beforeMove.x - 32) < 0.000001, 'The real pointer move did not reach Rust');
    const moved = { ...beforeMove, x: beforeMove.x + 32, y: beforeMove.y + 24 };
    assertRectClose((await region(1)).rect, moved);
    await physical.assertPointerGesture();
    await writeFile(join(evidence, 'pointer-move-events.json'), JSON.stringify(await physical.events(), null, 2));
    const handle = 'button[aria-label="Resize request 2"]';
    const bounds = await physical.rect(handle);
    assert.ok(bounds.width >= 23.5 && bounds.height >= 23.5, 'The zoomed resize target is smaller than 24 screen pixels');
    from = await physical.center(handle);
    await physical.resetEvents();
    await physical.drag(from, { x: from.x + 40 * 0.75, y: from.y + 32 * 0.75 });
    await waitFor(async () => Math.abs((await region(1)).rect.width - 280) < 0.000001, 'The real pointer resize did not reach Rust');
    assertRectClose((await region(1)).rect, { ...moved, width: 280, height: 192 });
    await physical.assertPointerGesture();
    await writeFile(join(evidence, 'pointer-resize-events.json'), JSON.stringify(await physical.events(), null, 2));
    const beforeClick = await driver.invoke('get_workspace');
    await physical.click(await physical.center(target));
    await driver.executeAsync(() => new Promise(accept => requestAnimationFrame(() => requestAnimationFrame(() => accept(true)))));
    assert.deepEqual((await driver.invoke('get_workspace')).project, beforeClick.project, 'A click without a drag must not create a document revision');
    await driver.button('Undo');
    await waitFor(async () => Math.abs((await region(1)).rect.width - 240) < 0.000001, 'Undo did not restore the earlier placement hint');
    await driver.button('Redo');
    await waitFor(async () => Math.abs((await region(1)).rect.width - 280) < 0.000001, 'Redo did not restore the resized placement hint');
  });

  await step('Rapid real arrow keys apply every movement and resize delta', async () => {
    await physical.click(await physical.center('textarea[aria-label="Request 2"]'));
    await physical.keys('Escape');
    assert.equal(await driver.execute(() => document.activeElement?.getAttribute('aria-label')), 'Move request 2',
      'Escape from the request input must focus its keyboard placement grip');
    const before = await region(1);
    await physical.resetEvents();
    await physical.keys('Right', 8);
    await waitFor(async () => Math.abs((await region(1)).rect.x - before.rect.x - 8) < 0.000001, 'The native edit queue lost a rapid arrow movement');
    assertRectClose((await region(1)).rect, { ...before.rect, x: before.rect.x + 8 });
    const moves = (await physical.events()).filter(event => event.type === 'keydown' && event.key === 'ArrowRight');
    assert.equal(moves.length, 8);
    assert.equal(moves.every(event => event.trusted), true);
    await physical.click(await physical.center('textarea[aria-label="Request 2"]'));
    await physical.click(await physical.center('button[aria-label="Resize request 2"]'));
    const beforeResize = await region(1);
    await physical.keys('Right', 5);
    await waitFor(async () => Math.abs((await region(1)).rect.width - beforeResize.rect.width - 5) < 0.000001, 'The native edit queue lost a resize-handle arrow delta');
    assertRectClose((await region(1)).rect, { ...beforeResize.rect, width: beforeResize.rect.width + 5 });
    const focus = await driver.execute(() => {
      const element = document.activeElement;
      const style = getComputedStyle(element);
      return { label: element.getAttribute('aria-label'), visible: element.matches(':focus-visible'), offset: parseFloat(style.outlineOffset), width: parseFloat(style.outlineWidth), color: style.outlineColor, background: style.backgroundColor };
    });
    await writeFile(join(evidence, 'placement-focus.json'), JSON.stringify(focus, null, 2));
    assert.equal(focus.label, 'Resize request 2'); assert.equal(focus.visible, true);
    assert.ok(focus.offset <= -2.9 && focus.width >= 1.9, `The placement focus ring must stay inside its opaque control at every page/editor appearance: ${JSON.stringify(focus)}`);
    await writeFile(join(evidence, 'keyboard-burst-events.json'), JSON.stringify(await physical.events(), null, 2));
  });

  await step('Every canvas zoom keeps on-page text and placement grips readable', async () => {
    const before = (await driver.invoke('get_workspace')).project;
    const measurements = [];
    for (const zoom of [0.25, 0.5, 0.75, 1, 1.5, 2]) {
      await driver.select('Zoom', String(zoom));
      await waitFor(() => driver.execute(value => Math.abs(document.querySelector('[data-testid="screen-guide"]').getBoundingClientRect().width - 390 * value) < 1, zoom), 'The chosen zoom did not render');
      await driver.revealSelection();
      await driver.executeAsync(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve(true)))));
      await physical.click(await physical.center('button[aria-label="Move request 2"]'));
      await physical.keys('Tab');
      const sizes = await driver.execute(() => ({
        text: parseFloat(getComputedStyle(document.querySelector('textarea[aria-label="Request 2"]')).fontSize),
        grip: document.querySelector('button[aria-label="Resize request 2"]').getBoundingClientRect().width,
        focus: { label: document.activeElement?.getAttribute('aria-label'), visible: document.activeElement?.matches(':focus-visible'),
          width: parseFloat(getComputedStyle(document.activeElement).outlineWidth),
          offset: parseFloat(getComputedStyle(document.activeElement).outlineOffset) },
      }));
      assert.ok(Math.abs(sizes.text - 14) < 0.1, `Text became unreadable at ${zoom * 100}%: ${sizes.text}`);
      assert.ok(Math.abs(sizes.grip - 28) < 0.1, `The placement target shrank at ${zoom * 100}%: ${sizes.grip}`);
      assert.equal(sizes.focus.label, 'Resize request 2'); assert.equal(sizes.focus.visible, true);
      assert.ok(sizes.focus.width >= 1.9 && sizes.focus.offset <= -2.9, `Focus became hard to see at ${zoom * 100}%: ${JSON.stringify(sizes.focus)}`);
      measurements.push({ zoom, ...sizes });
    }
    await writeFile(join(evidence, 'zoom-legibility.json'), JSON.stringify(measurements, null, 2));
    await driver.select('Zoom', '0.75');
    await driver.press('[data-testid="canvas"]', '\uE011');
    const centered = await waitFor(() => driver.execute(() => {
      const scroll = document.querySelector('[data-testid="workspace-scroll"]');
      const guide = document.querySelector('[data-testid="screen-guide"]').getBoundingClientRect();
      const workspace = scroll.getBoundingClientRect();
      return Math.abs((guide.left + guide.right) / 2 - workspace.left - scroll.clientWidth / 2) < 2 && Math.abs(guide.top - workspace.top - 64) < 2
        ? { guide: guide.toJSON(), workspace: workspace.toJSON() } : false;
    }), 'Center screen guide did not restore the reference screen in view');
    await writeFile(join(evidence, 'center-screen-guide.json'), JSON.stringify(centered, null, 2));
    assert.deepEqual((await driver.invoke('get_workspace')).project, before);
  });

  await step('Light and dark editor themes preserve the dark mobile document', async () => {
    const before = (await driver.invoke('get_workspace')).project;
    for (const theme of ['light', 'dark']) {
      await driver.select('Editor theme', theme);
      await waitFor(() => driver.execute(value => document.documentElement.dataset.theme === value, theme), `The ${theme} editor theme did not apply`);
      await driver.executeAsync(() => new Promise(accept => requestAnimationFrame(() => requestAnimationFrame(() => accept(true)))));
      let previous = null; let capture = null; let settled = false; let frames = 0;
      for (; frames < 8; frames += 1) {
        await driver.executeAsync(() => new Promise(resolve => setTimeout(resolve, 120)));
        capture = await driver.screenshot();
        if (capture === previous) { settled = true; break; }
        previous = capture;
      }
      await writeFile(join(evidence, `editor-${theme}.png`), Buffer.from(capture, 'base64'));
      await writeFile(join(evidence, `editor-${theme}-paint.json`), JSON.stringify({ theme, settled, captures: frames + 1 }, null, 2));
      assert.equal(settled, true, 'The idle native theme did not finish drawing consistent frames');
      const windowCapture = await physical.captureWindow(join(evidence, `editor-${theme}-window.png`), false);
      await writeFile(join(evidence, `editor-${theme}-window.json`), JSON.stringify(windowCapture, null, 2));
      const screenCapture = await physical.captureWindow(join(evidence, `editor-${theme}-screen.png`), true);
      await writeFile(join(evidence, `editor-${theme}-screen.json`), JSON.stringify(screenCapture, null, 2));
      assert.deepEqual((await driver.invoke('get_workspace')).project, before);
    }
  });

  await step('The 760px editor and modal stay within the window; native Escape closes it', async () => {
    const original = await driver.execute(() => ({ width: window.innerWidth, height: window.innerHeight }));
    const before = (await driver.invoke('get_workspace')).project;
    try {
      await driver.command('POST', '/window/rect', { width: 760, height: 900 });
      await waitFor(() => driver.execute(() => window.innerWidth <= 760 && window.innerWidth >= 740), 'The native editor did not reach its minimum supported width');
      const layout = await driver.execute(() => ({ width: window.innerWidth, document: document.documentElement.scrollWidth,
        body: document.body.scrollWidth, workspace: document.querySelector('.workspace').getBoundingClientRect().toJSON(),
        toolbar: document.querySelector('.app-bar').getBoundingClientRect().toJSON() }));
      assert.ok(layout.document <= layout.width + 1 && layout.body <= layout.width + 1, 'The narrow editor has page-level horizontal overflow');
      assert.ok(layout.workspace.left >= 0 && layout.workspace.right <= layout.width + 1);
      assert.ok(layout.toolbar.left >= 0 && layout.toolbar.right <= layout.width + 1);
      await writeFile(join(evidence, 'narrow-layout.json'), JSON.stringify(layout, null, 2));
      await writeFile(join(evidence, 'editor-760px.png'), Buffer.from(await driver.screenshot(), 'base64'));
      await physical.click(await physical.center('button[aria-label="Editor menu"]'));
      await physical.keys('Right');
      await physical.keys('Down', 5);
      await physical.keys('Return');
      const modal = await physical.rect('.workspace-surface');
      assert.ok(modal.left >= 0 && modal.right <= layout.width + 1);
      assert.ok(modal.top >= 0 && modal.bottom <= 901);
      await physical.keys('Escape');
      await waitFor(() => driver.execute(() => !document.querySelector('.workspace-surface')), 'Native Escape did not close the project dossier dialog');
      assert.equal(await driver.execute(() => document.activeElement?.getAttribute('aria-label')), 'Editor menu', 'Closing the modal did not restore focus to its persistent opening control');
      assert.deepEqual((await driver.invoke('get_workspace')).project, before);
      await assertCanvasOnly(driver);
    } finally {
      await driver.command('POST', '/window/rect', { width: original.width, height: original.height });
      await waitFor(() => driver.execute(width => window.innerWidth >= width - 20, original.width), 'The native window size did not restore');
    }
  });
  await writeFile(join(evidence, 'physical-input-events.json'), JSON.stringify(await physical.events(), null, 2));
  return (await driver.invoke('get_workspace')).project;
}
