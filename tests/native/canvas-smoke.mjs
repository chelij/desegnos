// Real-app canvas smoke test. It launches the explicit native-tests build on a private Xvfb
// display and session bus, then drives the editor with WebDriver clicks and keys. The only
// native input is the GTK save dialog, which lives outside the webview.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createReadStream, createWriteStream } from 'node:fs';
import { access, chmod, mkdir, mkdtemp, readFile, readdir, realpath, rm, stat, writeFile } from 'node:fs/promises';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';
import { toolchainEnvironment } from '../../scripts/toolchain.mjs';
import { resolveDbusRunSession, resolveXvfbRun, xvfbEnvironment } from '../../scripts/xvfb.mjs';
import { waitFor, WebDriver, xpathString } from './webdriver.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const helper = join(root, 'tests/native/x11-input.py');
const script = fileURLToPath(import.meta.url);
const args = process.argv.slice(2);

const MODIFIERS = { Control: '\uE009', Shift: '\uE008', Alt: '\uE00A', Meta: '\uE03D' };

function alive(child) { return child.exitCode === null && child.signalCode === null; }
function childExit(child) {
  if (child.exitCode !== null) return Promise.resolve(child.exitCode);
  if (child.signalCode !== null) return Promise.resolve(1);
  return new Promise((accept, reject) => { child.once('error', reject); child.once('exit', code => accept(code ?? 1)); });
}
function signalGroup(pid, signal) {
  try { process.kill(-pid, signal); } catch (error) { if (error.code !== 'ESRCH') throw error; }
}
function groupExists(pid) {
  try { process.kill(-pid, 0); return true; } catch (error) { if (error.code === 'ESRCH') return false; throw error; }
}
async function stopGroup(pid) {
  if (!pid || !groupExists(pid)) return;
  signalGroup(pid, 'SIGTERM');
  const deadline = Date.now() + 2000;
  while (groupExists(pid) && Date.now() < deadline) await delay(40);
  if (groupExists(pid)) signalGroup(pid, 'SIGKILL');
}
async function hashFile(path) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest('hex');
}
async function freePort() {
  const server = createServer();
  await new Promise((accept, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', accept); });
  const port = server.address().port;
  await new Promise((accept, reject) => server.close(error => error ? reject(error) : accept()));
  return port;
}
async function desktopState() {
  const display = process.env.DISPLAY ?? null;
  const number = display?.match(/^:(\d+)(?:\.\d+)?$/)?.[1];
  const files = [];
  for (const path of number ? [`/tmp/.X11-unix/X${number}`, `/tmp/.X${number}-lock`] : []) {
    try { const item = await stat(path); files.push({ path, exists: true, dev: item.dev, ino: item.ino }); }
    catch (error) { if (error.code !== 'ENOENT') throw error; files.push({ path, exists: false }); }
  }
  const probe = display ? spawnSync('xprop', ['-root', '_NET_SUPPORTING_WM_CHECK'], { encoding: 'utf8', timeout: 5000 }) : null;
  return { display, files, connected: probe?.status === 0 };
}
async function verifyDesktop(before) {
  const after = await desktopState();
  assert.deepEqual(after.files, before.files, 'The desktop X11 socket or lock identity changed');
  if (before.connected) assert.equal(after.connected, true, 'The desktop X11 connection failed after cleanup');
  return { before, after, sockets: 'unchanged', connection: before.connected ? 'passed' : 'unavailable before test' };
}

class Pointer {
  constructor(driver) { this.driver = driver; }
  async drag(from, to, duration = 240) {
    await this.driver.command('POST', '/actions', { actions: [{ type: 'pointer', id: 'mouse', parameters: { pointerType: 'mouse' }, actions: [
      { type: 'pointerMove', duration: 0, origin: 'viewport', x: Math.round(from.x), y: Math.round(from.y) },
      { type: 'pointerDown', button: 0 },
      { type: 'pointerMove', duration, origin: 'viewport', x: Math.round(to.x), y: Math.round(to.y) },
      { type: 'pointerUp', button: 0 },
    ] }] });
    await this.driver.command('DELETE', '/actions');
  }
  async click(point, modifiers = []) {
    const keys = ['Control', 'Shift', 'Alt'].filter(key => modifiers.includes(key)).map(key => MODIFIERS[key]);
    const actions = [
      { type: 'pointerMove', duration: 0, origin: 'viewport', x: Math.round(point.x), y: Math.round(point.y) },
      { type: 'pointerDown', button: 0 },
      { type: 'pointerUp', button: 0 },
    ];
    await this.driver.command('POST', '/actions', { actions: [{ type: 'pointer', id: 'mouse', parameters: { pointerType: 'mouse' }, actions }, ...(keys.length ? [{ type: 'key', id: 'keyboard', actions: keys.map(value => ({ type: 'keyDown', value })).concat(keys.slice().reverse().map(value => ({ type: 'keyUp', value }))) }] : [])] });
    await this.driver.command('DELETE', '/actions');
  }
  async move(point) {
    await this.driver.command('POST', '/actions', { actions: [{ type: 'pointer', id: 'mouse', parameters: { pointerType: 'mouse' }, actions: [
      { type: 'pointerMove', duration: 0, origin: 'viewport', x: Math.round(point.x), y: Math.round(point.y) },
    ] }] });
    await this.driver.command('DELETE', '/actions');
  }
  async combo(...keys) {
    const modifiers = [];
    const presses = [];
    for (const key of keys) {
      if (MODIFIERS[key]) modifiers.push(MODIFIERS[key]);
      else presses.push(key);
    }
    const actions = [...modifiers.map(value => ({ type: 'keyDown', value }))];
    for (const key of presses) actions.push({ type: 'keyDown', value: key }, { type: 'keyUp', value: key });
    actions.push(...modifiers.reverse().map(value => ({ type: 'keyUp', value })));
    await this.driver.command('POST', '/actions', { actions: [{ type: 'key', id: 'keyboard', actions }] });
    await this.driver.command('DELETE', '/actions');
  }
  key(key) { return this.combo(key); }
  async focusCanvas() {
    await this.driver.execute(() => {
      const canvas = document.querySelector('#project-canvas');
      if (!(canvas instanceof HTMLElement)) throw new Error('The canvas is missing');
      canvas.focus();
    });
  }
  async press(selector, value, using = 'css selector') {
    await this.driver.click(selector, using);
    await this.combo(value);
  }
}

class PhysicalPointer {
  constructor(driver, pid) { this.driver = driver; this.pid = pid; }
  raw(request) {
    return new Promise((accept, reject) => {
      const child = spawn('/usr/bin/python3', [helper], { stdio: ['pipe', 'pipe', 'pipe'] });
      let stdout = ''; let stderr = '';
      const timer = setTimeout(() => { child.kill('SIGKILL'); reject(new Error('The isolated X11 pointer helper timed out')); }, 10_000);
      child.stdout.on('data', chunk => { stdout += chunk; if (stdout.length > 512_000) child.kill('SIGKILL'); });
      child.stderr.on('data', chunk => { stderr += chunk; if (stderr.length > 32_000) child.kill('SIGKILL'); });
      child.once('error', error => { clearTimeout(timer); reject(error); });
      child.once('close', code => {
        clearTimeout(timer);
        if (code !== 0) { reject(new Error(`Isolated X11 pointer failed: ${stderr || `exit ${code}`}`)); return; }
        try { accept(JSON.parse(stdout)); } catch (error) { reject(error); }
      });
      child.stdin.end(JSON.stringify({ ...request, pid: this.pid }));
    });
  }
  async initialize() {
    const native = await this.raw({ action: 'inspect' });
    const windowRect = await this.driver.command('GET', '/window/rect');
    const viewport = await this.driver.execute(() => ({ width: window.innerWidth, height: window.innerHeight, scale: window.devicePixelRatio }));
    assert.ok(viewport.scale > 0 && Number.isFinite(viewport.scale));
    const rootPoint = { x: native.window.x + Math.floor(native.window.width / 2), y: native.window.y + Math.floor(native.window.height / 2) };
    await this.raw({ action: 'move', point: { x: rootPoint.x - 1, y: rootPoint.y - 1 } });
    await this.raw({ action: 'move', point: rootPoint });
    const observed = await waitFor(() => this.driver.execute(() => window.__smokeEvents.filter(event => event.type === 'pointermove').at(-1)), 'The real X11 pointer did not reach the webview');
    assert.equal(observed.trusted, true, 'The calibration event was not a trusted operating-system pointer event');
    this.origin = { x: rootPoint.x - observed.screenX * viewport.scale, y: rootPoint.y - observed.screenY * viewport.scale };
    this.scale = viewport.scale;
    this.geometry = { native, windowRect, viewport, observed, origin: this.origin };
    assert.ok(this.origin.x >= native.window.x - 1 && this.origin.y >= native.window.y - 1, 'The webview origin lies outside its owned X11 window');
  }
  root(point) {
    assert.ok(this.origin, 'Calibrate the native window before physical pointer input');
    return { x: Math.round(this.origin.x + point.x * this.scale), y: Math.round(this.origin.y + point.y * this.scale) };
  }
  click(point, modifiers = []) { return this.raw({ action: 'click', point: this.root(point), modifiers }); }
  move(point) { return this.raw({ action: 'move', point: this.root(point) }); }
  drag(from, to) { return this.raw({ action: 'drag', from: this.root(from), to: this.root(to) }); }
  /** Drags, holding the button before release, and checks the drag shows `selector` meanwhile. */
  async dragShowing(from, to, selector) {
    let done = false;
    const gesture = this.raw({ action: 'drag', from: this.root(from), to: this.root(to), hold: 800 }).finally(() => { done = true; });
    let shown = false;
    while (!done && !shown) {
      shown = await this.driver.execute(query => {
        const rect = document.querySelector(query)?.getBoundingClientRect();
        return !!rect && rect.width > 10 && rect.height > 10;
      }, selector);
      if (!shown) await new Promise(resolve => setTimeout(resolve, 50));
    }
    await gesture;
    assert.ok(shown, `Nothing visible at ${selector} while dragging`);
  }
}

class NativeDialog {
  constructor(pid) { this.pid = pid; }
  raw(request) {
    return new Promise((accept, reject) => {
      const child = spawn('/usr/bin/python3', [helper], { stdio: ['pipe', 'pipe', 'pipe'] });
      let stdout = ''; let stderr = '';
      const timer = setTimeout(() => { child.kill('SIGKILL'); reject(new Error('The isolated X11 helper timed out')); }, 10_000);
      child.stdout.on('data', chunk => { stdout += chunk; if (stdout.length > 512_000) child.kill('SIGKILL'); });
      child.stderr.on('data', chunk => { stderr += chunk; if (stderr.length > 32_000) child.kill('SIGKILL'); });
      child.once('error', error => { clearTimeout(timer); reject(error); });
      child.once('close', code => {
        clearTimeout(timer);
        if (code !== 0) { reject(new Error(`Isolated X11 input failed: ${stderr || `exit ${code}`}`)); return; }
        try { accept(JSON.parse(stdout)); } catch (error) { reject(error); }
      });
      child.stdin.end(JSON.stringify({ ...request, pid: this.pid }));
    });
  }
  inspect() { return this.raw({ action: 'inspect' }); }
  async waitClosed(windowId, timeout = 10_000) {
    const deadline = Date.now() + timeout;
    do {
      if (!(await this.inspect()).windows.some(window => window.id === windowId)) return true;
      await delay(75);
    } while (Date.now() < deadline);
    return false;
  }
  async chooseFile(trigger, path, label, evidence) {
    const before = await this.inspect();
    const previous = new Set(before.windows.map(window => window.id));
    await trigger();
    const dialog = await waitFor(async () => {
      const snapshot = await this.inspect();
      const added = snapshot.windows.filter(window => !previous.has(window.id));
      assert.ok(added.length <= 1, `Ambiguous native dialog: ${JSON.stringify(added)}`);
      if (!added.length) return false;
      await writeFile(join(evidence, `${label}-dialog.json`), JSON.stringify(snapshot, null, 2));
      return added[0];
    }, `The ${label} native dialog did not appear`, 15_000);
    assert.equal(dialog.pid, before.pid, 'The file dialog belongs to another process');
    await this.raw({ action: 'key', key: 'L', modifiers: ['Control'], windowId: dialog.id });
    await this.raw({ action: 'key', key: 'A', modifiers: ['Control'], windowId: dialog.id });
    await this.raw({ action: 'text', text: path, windowId: dialog.id });
    for (let attempt = 0; attempt < 2; attempt += 1) {
      await this.raw({ action: 'key', key: 'Return', windowId: dialog.id });
      if (await this.waitClosed(dialog.id, 3000)) return;
    }
    await this.raw({ action: 'key', key: 'O', modifiers: ['Alt'], windowId: dialog.id });
    if (await this.waitClosed(dialog.id, 3000)) return;
    throw new Error(`The ${label} dialog did not accept ${path}`);
  }
}

function crashReportNames(entries) { return entries.filter(name => name !== 'running').sort(); }

async function runIsolated() {
  const display = process.env.DISPLAY?.match(/^:(\d+)(?:\.\d+)?$/);
  assert.ok(display && Number(display[1]) >= 90, 'The smoke test requires a private Xvfb display numbered 90 or higher');
  assert.notEqual(process.env.DISPLAY, process.env.DESEGNOS_DESKTOP_DISPLAY, 'Refusing to use the desktop display');
  assert.equal(process.env.GDK_BACKEND, 'x11');
  assert.equal(process.env.WAYLAND_DISPLAY, undefined);
  assert.ok(process.env.DBUS_SESSION_BUS_ADDRESS, 'A private session bus is required');
  const binary = process.env.DESEGNOS_SMOKE_BINARY;
  const evidence = process.env.DESEGNOS_SMOKE_EVIDENCE;
  const data = process.env.DESEGNOS_DATA_DIR;
  assert.ok(binary && evidence && data);
  await mkdir(data, { recursive: true, mode: 0o700 });
  const crashDirectory = join(data, 'crash-reports');
  const report = { passed: false, startedAt: new Date().toISOString(), display: process.env.DISPLAY, binary,
    binarySha256: await hashFile(binary), steps: [], data, crashDirectory,
    input: 'WebDriver clicks and key actions in the real WebKitGTK webview; native X11 keys only for the GTK save dialog.',
    limits: ['Linux WebKitGTK only.', 'No paid AI provider work.'] };
  const port = await freePort();
  const log = createWriteStream(join(evidence, 'app.log'), { flags: 'wx' });
  let app;
  let driver;
  async function stop() {
    if (driver) { try { await driver.stop(); } catch { /* The process may already be gone. */ } driver = null; }
    if (app) {
      const child = app; app = null;
      signalGroup(child.pid, 'SIGTERM');
      await Promise.race([childExit(child), delay(2500)]);
      signalGroup(child.pid, 'SIGKILL');
      await childExit(child);
      await child.streamsClosed;
    }
  }
  for (const signal of ['SIGINT', 'SIGTERM']) process.once(signal, () => { void stop().finally(() => process.exit(1)); });
  try {
    app = spawn(binary, [], { cwd: root, env: { ...process.env, TAURI_WEBDRIVER_PORT: String(port) }, detached: true, stdio: ['ignore', 'pipe', 'pipe'] });
    app.stdout.pipe(log, { end: false }); app.stderr.pipe(log, { end: false });
    const child = app;
    child.streamsClosed = new Promise(accept => child.once('close', accept));
    await new Promise((accept, reject) => { child.once('spawn', accept); child.once('error', reject); });
    const environment = await waitFor(async () => {
      const values = (await readFile(`/proc/${child.pid}/environ`, 'utf8')).split('\0');
      return values.includes(`DISPLAY=${process.env.DISPLAY}`) && values.includes('GDK_BACKEND=x11') && values;
    }, 'The app did not bind to the private X11 display', 5000);
    assert.ok(environment.includes(`DISPLAY=${process.env.DISPLAY}`));
    driver = await new WebDriver(port).start(() => alive(child));
    const pointer = new Pointer(driver);
    const native = new NativeDialog(child.pid);
    await waitFor(async () => {
      const source = await driver.command('GET', '/source');
      return typeof source === 'string' && /class="application(?:\s|")/.test(source);
    }, 'The native document did not finish mounting', 30_000);
    await driver.execute(() => {
      window.__smokeEvents = [];
      for (const type of ['pointerdown', 'pointermove', 'pointerup', 'pointercancel', 'keydown', 'keyup', 'click']) {
        document.addEventListener(type, event => window.__smokeEvents.push({ type, trusted: event.isTrusted, key: event.key, button: event.button, x: event.clientX, y: event.clientY, screenX: event.screenX, screenY: event.screenY, target: event.target?.id || event.target?.className }), true);
      }
      window.__desegnosSmokeErrors = [];
      window.addEventListener('error', event => window.__desegnosSmokeErrors.push({ kind: 'error', message: event.message }));
      window.addEventListener('unhandledrejection', event => window.__desegnosSmokeErrors.push({ kind: 'rejection', message: String(event.reason) }));
      const original = console.error;
      console.error = (...values) => {
        window.__desegnosSmokeErrors.push({ kind: 'console', message: values.map(String).join(' ') });
        original.apply(console, values);
      };
      return true;
    });
    const mouse = new PhysicalPointer(driver, child.pid);
    await mouse.initialize();
    report.pointerGeometry = mouse.geometry;

    const canvas = () => driver.execute(() => {
      const node = document.querySelector('#project-canvas');
      if (!node) return null;
      const rect = node.getBoundingClientRect();
      return { left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom, width: rect.width, height: rect.height };
    });
    const boundaries = () => driver.execute(() => [...document.querySelectorAll('.page-boundary[data-boundary]')].map(node => {
      const rect = node.getBoundingClientRect();
      return { left: rect.left, top: rect.top, width: rect.width, height: rect.height, right: rect.right, bottom: rect.bottom, screen: node.classList.contains('screen') };
    }));
    const boundary = async () => (await boundaries()).find(item => !item.screen) ?? null;
    const stripButton = label => `[aria-label="Canvas tools"] button[aria-label="${label}"]`;
    const activeTool = () => driver.execute(() => document.querySelector('[aria-label="Canvas tools"] [aria-pressed="true"]')?.getAttribute('aria-label') ?? null);
    // Tools are chosen the way a person chooses them: by clicking the strip button.
    const chooseTool = async label => {
      await driver.click(stripButton(label));
      await waitFor(async () => (await activeTool()) === label, `Clicking the ${label} button did not activate the tool`);
    };
    const chooseBoundaryKind = async label => {
      await driver.click('[aria-label="Canvas tools"] [aria-label="Boundary kind"]');
      await driver.click(`//ul[@role="menu" and @aria-label="Boundary kind"]/li/button[@aria-label=${xpathString(label)}]`, 'xpath');
      await waitFor(async () => (await activeTool()) === 'Boundary', `Choosing ${label} in the Boundary flyout did not leave the Boundary tool active`);
      await waitFor(() => driver.execute(() => ![...document.querySelectorAll('ul[role="menu"]')].some(menu => menu.matches(':popover-open'))),
        'The Boundary kind flyout stayed open');
    };
    const chooseRequestShape = async label => {
      await driver.click('[aria-label="Canvas tools"] [aria-label="Request area shape"]');
      await driver.click(`//ul[@role="menu" and @aria-label="Request area shape"]/li/button[@aria-label=${xpathString(label)}]`, 'xpath');
      await waitFor(async () => (await activeTool()) === 'Request', `Choosing ${label} in the Request flyout did not leave the Request tool active`);
      await waitFor(() => driver.execute(() => ![...document.querySelectorAll('ul[role="menu"]')].some(menu => menu.matches(':popover-open'))),
        'The Request area shape flyout stayed open');
    };
    // A corner of the canvas that overlaps no boundary and stays inside the surface.
    const emptyCanvas = async (width, height) => {
      const surface = await canvas();
      const boxes = await boundaries();
      const gap = 20;
      const margin = 16;
      const lefts = [surface.left + margin, surface.right - width - margin];
      const tops = [surface.top + margin, surface.bottom - height - margin];
      const overlaps = (a, b) => a.left < b.right + gap && a.left + a.width > b.left - gap && a.top < b.bottom + gap && a.top + a.height > b.top - gap;
      for (const top of tops) for (const left of lefts) {
        const spot = { left, top, width, height };
        if (left < surface.left || top < surface.top || left + width > surface.right || top + height > surface.bottom) continue;
        if (boxes.some(box => overlaps(spot, box))) continue;
        return { from: { x: spot.left, y: spot.top }, to: { x: spot.left + width, y: spot.top + height } };
      }
      assert.fail(`No free canvas for a ${width}x${height} drag: ${JSON.stringify({ surface, boxes })}`);
    };
    const elements = () => driver.execute(() => [...document.querySelectorAll('[data-element]')].map(node => {
      const rect = node.getBoundingClientRect();
      return { id: node.getAttribute('data-element'), kind: node.getAttribute('data-element-kind'), left: rect.left, top: rect.top, width: rect.width, height: rect.height };
    }));
    const healthy = async label => {
      assert.ok(alive(child), `The app exited during: ${label}`);
      const stopped = await driver.execute(() => document.body.innerText.includes('stopped working'));
      assert.equal(stopped, false, `The editor error boundary appeared during: ${label}`);
      const entries = await readdir(crashDirectory).catch(error => { if (error.code === 'ENOENT') return []; throw error; });
      assert.deepEqual(crashReportNames(entries), [], `A crash report was written during: ${label}`);
    };
    async function step(name, action) {
      const result = { name, passed: false, startedAt: new Date().toISOString() };
      report.steps.push(result);
      try {
        await action();
        await healthy(name);
        result.passed = true;
        console.log(`PASS ${name}`);
      } catch (error) {
        result.error = error.stack;
        try { await healthy(name); } catch (healthError) { result.healthError = healthError.stack; }
        throw error;
      } finally { result.finishedAt = new Date().toISOString(); }
    }
    const home = async () => {
      if (await driver.execute(() => !!document.querySelector('.start-page'))) return;
      await driver.menu(['Home']);
      await waitFor(() => driver.execute(() => !!document.querySelector('.start-page')), 'The start page did not appear');
    };
    const createFromHome = async label => {
      await home();
      await driver.click(`//button[normalize-space(.)=${xpathString(label)}]`, 'xpath');
      await waitFor(() => driver.execute(() => !!document.querySelector('#project-canvas')), `The ${label} template did not open its canvas`);
    };

    await step('Click Blank canvas on the start page', async () => {
      await waitFor(() => driver.execute(() => !!document.querySelector('.start-page')), 'The start page did not load');
      await driver.click(`//button[normalize-space(.)=${xpathString('Blank canvas')}]`, 'xpath');
      await waitFor(() => driver.execute(() => !!document.querySelector('#project-canvas')), 'Blank canvas did not open the editor');
    });

    await step('Create every start-page template and return Home', async () => {
      for (const label of ['Website', 'App screen', 'Slide deck', 'Social image']) {
        await createFromHome(label);
        assert.ok((await driver.execute(() => document.querySelectorAll('[data-boundary]').length)) > 0, `${label} has no boundary`);
      }
      await home();
    });

    await step('Click Blank canvas again for drawing', async () => {
      await driver.click(`//button[normalize-space(.)=${xpathString('Blank canvas')}]`, 'xpath');
      await waitFor(() => driver.execute(() => !!document.querySelector('#project-canvas')), 'Blank canvas did not open the editor');
    });

    await step('The tool strip lists tools in workflow order', async () => {
      const order = await driver.execute(() => [...document.querySelectorAll('[aria-label="Canvas tools"] .tool-strip-group > .tool-strip-slot')]
        .map(slot => slot.querySelector('button')?.getAttribute('aria-label') ?? null));
      assert.deepEqual(order, ['Select', 'Boundary', 'Request', 'Text', 'Rectangle', 'Ellipse', 'Line', 'Image', 'Insert', 'Pointer', 'Note', 'Pan']);
    });

    await step('Draw a Page boundary by clicking the Boundary button and its Page flyout', async () => {
      await waitFor(async () => {
        const rect = await canvas();
        return rect && rect.width > 500 && rect.height > 400 ? rect : false;
      }, 'The canvas did not lay out before drawing');
      await pointer.focusCanvas();
      await chooseBoundaryKind('Page');
      const spot = await emptyCanvas(480, 420);
      await mouse.dragShowing(spot.from, spot.to, '.boundary-draft');
      try {
        await waitFor(async () => !!(await boundary()), 'The Boundary drag did not create a boundary');
      } catch (error) {
        error.message += `\nEvents: ${JSON.stringify(await driver.execute(() => window.__smokeEvents.slice(-20)))}`;
        throw error;
      }
      assert.equal(await driver.execute(() => document.querySelector('[aria-label="Canvas tools"] button[aria-label="Boundary"]')?.getAttribute('aria-pressed')), 'true');
      assert.equal((await boundary()).screen, false, 'The Page boundary rendered as a Screen boundary');
    });

    await step('Draw a Screen boundary from the Boundary flyout', async () => {
      await chooseBoundaryKind('Screen');
      const before = (await boundaries()).length;
      const firstBefore = (await boundaries())[0];
      const spot = await emptyCanvas(280, 140);
      await mouse.drag(spot.from, spot.to);
      await waitFor(async () => (await boundaries()).length === before + 1, 'The Screen boundary drag did not create a boundary');
      assert.ok((await boundaries()).some(item => item.screen), 'The new boundary is not a Screen boundary');
      const firstAfter = (await boundaries())[0];
      assert.ok(Math.abs(firstAfter.left - firstBefore.left) < 2 && Math.abs(firstAfter.top - firstBefore.top) < 2, 'Drawing a boundary moved the view');
    });

    await step('Draw a rectangle by clicking the Rectangle button', async () => {
      const box = await boundary();
      const before = await driver.execute(() => document.querySelectorAll('[data-element-kind="box"]').length);
      await chooseTool('Rectangle');
      await mouse.dragShowing({ x: box.left + 40, y: box.top + 230 }, { x: box.left + Math.min(300, box.width - 40), y: box.top + 300 }, '.element-draft');
      await waitFor(() => driver.execute(previous => document.querySelectorAll('[data-element-kind="box"]').length > previous, before), 'The Rectangle drag did not create a rectangle');
      const drawn = await driver.execute(() => { const nodes = document.querySelectorAll('[data-element-kind="box"]'); const r = nodes[nodes.length - 1].getBoundingClientRect(); return { left: r.left, top: r.top, width: r.width, height: r.height }; });
      const expected = { left: box.left + 40, top: box.top + 230 };
      assert.ok(Math.abs(drawn.left - expected.left) < 4 && Math.abs(drawn.top - expected.top) < 4, `The rectangle landed at ${JSON.stringify(drawn)}, not where it was drawn ${JSON.stringify(expected)}`);
    });

    await step('Ctrl+Z undoes the rectangle and Shift+Ctrl+Z redoes it', async () => {
      const count = () => driver.execute(() => document.querySelectorAll('[data-element-kind="box"]').length);
      const before = await count();
      await pointer.focusCanvas();
      await pointer.combo('Control', 'z');
      await waitFor(async () => (await count()) === before - 1, 'Ctrl+Z did not undo the rectangle');
      await pointer.combo('Control', 'Shift', 'z');
      await waitFor(async () => (await count()) === before, 'Shift+Ctrl+Z did not redo the rectangle');
    });

    await step('Draw an ellipse by clicking the Ellipse button', async () => {
      const box = await boundary();
      await chooseTool('Ellipse');
      await mouse.drag({ x: box.left + 330, y: box.top + 60 }, { x: box.left + Math.min(470, box.width - 30), y: box.top + 160 });
      await waitFor(() => driver.execute(() => !!document.querySelector('[data-element-kind="ellipse"]')), 'The Ellipse drag did not create an ellipse');
    });

    await step('Draw a line by clicking the Line button', async () => {
      const box = await boundary();
      await chooseTool('Line');
      await mouse.drag({ x: box.left + 330, y: box.top + 230 }, { x: box.left + Math.min(470, box.width - 30), y: box.top + 260 });
      await waitFor(() => driver.execute(() => !!document.querySelector('[data-element-kind="line"]')), 'The Line drag did not create a line');
    });

    await step('Type text by clicking the Text button inside the boundary', async () => {
      const box = await boundary();
      await chooseTool('Text');
      await pointer.focusCanvas();
      await mouse.click({ x: box.left + 50, y: box.top + Math.max(340, box.height - 70) });
      await driver.ready('[aria-label="Edit text"]');
      await driver.fill('Edit text', 'Smoke text');
      await driver.press('[aria-label="Edit text"]', '\uE007');
      await waitFor(async () => (await driver.execute(() => document.querySelectorAll('[data-element-kind="text"]').length)) === 1, 'The text layer was not committed');
      await pointer.combo('Control', 'z');
      await waitFor(async () => (await driver.execute(() => document.querySelectorAll('[data-element-kind="text"]').length)) === 0, 'Ctrl+Z after typing did not undo the text');
      await pointer.combo('Control', 'Shift', 'z');
      await waitFor(async () => (await driver.execute(() => document.querySelectorAll('[data-element-kind="text"]').length)) === 1, 'Shift+Ctrl+Z did not bring the text back');
    });

    await step('Request picks two layers, takes typed text and queues a pending request', async () => {
      const layers = await elements();
      const first = layers.find(element => element.kind === 'box');
      const second = layers.find(element => element.kind === 'text');
      assert.ok(first && second, 'Two layers are needed for a request');
      await chooseTool('Request');
      assert.equal(await driver.execute(() => document.querySelector('[aria-label="Canvas tools"] [aria-label="Request"]')?.getAttribute('aria-pressed')), 'true');
      await mouse.click({ x: first.left + first.width / 2, y: first.top + first.height / 2 });
      await driver.ready('.request-box-editor textarea');
      await mouse.click({ x: second.left + second.width / 2, y: second.top + second.height / 2 }, ['Shift']);
      await waitFor(() => driver.execute(() => document.querySelectorAll('.request-target-highlight.draft').length >= 2), 'Shift-clicking a second layer did not add it to the request');
      await driver.fill('Describe the request', 'Make these two layers one card');
      await driver.press(driver.field('Describe the request'), '\uE007', 'xpath');
      await waitFor(() => driver.execute(() => !document.querySelector('.request-box-editor')), 'Enter did not close the request box');
      await driver.ready('[data-request-row]');
      await waitFor(() => driver.execute(() => !!document.querySelector('.request-marker')), 'The request marker was not drawn');
      const rows = await driver.execute(() => document.querySelectorAll('[data-request-row]').length);
      assert.equal(rows, 1, 'The queued request is not listed under Requests');
    });

    await step('A request rectangle drawn over a layer targets it', async () => {
      const ellipse = (await elements()).find(element => element.kind === 'ellipse');
      assert.ok(ellipse, 'The ellipse layer is missing');
      await chooseRequestShape('Rectangle');
      const margin = 12;
      await mouse.dragShowing({ x: ellipse.left - margin, y: ellipse.top - margin }, { x: ellipse.left + ellipse.width + margin, y: ellipse.top + ellipse.height + margin }, '.request-area.draft');
      await driver.ready('.request-box-editor textarea');
      await driver.fill('Describe the request', 'Turn this ellipse into a badge');
      await driver.press(driver.field('Describe the request'), '\uE007', 'xpath');
      await waitFor(() => driver.execute(() => document.querySelectorAll('[data-request-row]').length === 2), 'The drawn request area did not queue a second request');
      await driver.click('(//button[@data-request-row])[2]', 'xpath');
      await waitFor(() => driver.execute(() => document.querySelectorAll('[data-request] .request-target-highlight').length >= 1), 'Selecting the request did not highlight the layer its area covers');
    });

    await step('Select the text layer by clicking the Select button and move it with the pointer', async () => {
      const before = (await elements()).find(element => element.kind === 'text');
      assert.ok(before, 'The text layer is missing');
      await chooseTool('Select');
      await pointer.focusCanvas();
      await mouse.click({ x: before.left + before.width / 2, y: before.top + before.height / 2 });
      await waitFor(() => driver.execute(() => !!document.querySelector('.element-selection-box')), 'Clicking the text did not select it');
      await mouse.drag({ x: before.left + before.width / 2, y: before.top + before.height / 2 }, { x: before.left + before.width / 2 + 60, y: before.top + before.height / 2 + 20 });
      const moved = await waitFor(async () => {
        const current = (await elements()).find(element => element.id === before.id);
        return current && (Math.abs(current.left - before.left) > 20 || Math.abs(current.top - before.top) > 20) && current;
      }, 'The text layer did not move');
      assert.ok(moved);
    });

    await step('Change the selected layer fill in Properties', async () => {
      await driver.fill('Fill', '#ff0000');
      await driver.press(driver.field('Fill'), '\uE007', 'xpath');
      await waitFor(() => driver.execute(() => {
        const node = document.querySelector('[data-element-kind="text"]');
        return !!node && getComputedStyle(node).backgroundColor === 'rgb(255, 0, 0)';
      }), 'The Properties fill change did not reach the text layer');
    });

    await step('A new rectangle starts with the last picked fill', async () => {
      const box = await boundary();
      const before = await driver.execute(() => document.querySelectorAll('[data-element-kind="box"]').length);
      await chooseTool('Rectangle');
      await mouse.drag({ x: box.left + 40, y: box.top + 70 }, { x: box.left + 200, y: box.top + 150 });
      await waitFor(() => driver.execute(previous => document.querySelectorAll('[data-element-kind="box"]').length > previous, before), 'The Rectangle drag did not create a rectangle');
      const colour = await driver.execute(() => { const nodes = document.querySelectorAll('[data-element-kind="box"]'); return getComputedStyle(nodes[nodes.length - 1]).backgroundColor; });
      assert.equal(colour, 'rgb(255, 0, 0)', 'The new rectangle did not keep the picked fill');
      await pointer.focusCanvas();
      await pointer.combo('Control', 'z');
      await waitFor(() => driver.execute(previous => document.querySelectorAll('[data-element-kind="box"]').length === previous, before), 'Ctrl+Z did not remove the new rectangle');
    });

    await step('Select both layers, group and ungroup them', async () => {
      await chooseTool('Select');
      await pointer.focusCanvas();
      await pointer.combo('Control', 'a');
      await waitFor(() => driver.execute(() => document.querySelectorAll('.element-selection-box').length >= 2), 'Ctrl+A did not select both layers');
      await pointer.combo('Control', 'g');
      await waitFor(() => driver.execute(() => !!document.querySelector('[data-element-group="true"]')), 'Ctrl+G did not group the layers');
      await pointer.combo('Control', 'Shift', 'g');
      await waitFor(() => driver.execute(() => !document.querySelector('[data-element-group="true"]')), 'Ctrl+Shift+G did not ungroup the layers');
    });

    await step('Copy and paste through the paste choice', async () => {
      await chooseTool('Select');
      await pointer.focusCanvas();
      await pointer.combo('Control', 'a');
      const count = await driver.execute(() => document.querySelectorAll('[data-element]').length);
      await pointer.combo('Control', 'c');
      const box = await boundary();
      await mouse.move({ x: box.left + box.width / 2, y: box.top + box.height / 2 });
      await pointer.combo('Control', 'Shift', 'v');
      await driver.ready('.paste-choice');
      await driver.click(`//div[@role="group" and @aria-label="Paste linked or unlinked"]//button[normalize-space(.)=${xpathString('Unlinked')}]`, 'xpath');
      await driver.click(`//div[@role="group" and @aria-label="Paste linked or unlinked"]//button[normalize-space(.)=${xpathString('Paste')}]`, 'xpath');
      await waitFor(() => driver.execute(previous => document.querySelectorAll('[data-element]').length > previous, count), 'Paste did not add layers');
    });

    await step('Add a Phone size to the boundary', async () => {
      const box = await boundary();
      await chooseTool('Select');
      await pointer.focusCanvas();
      await mouse.click({ x: box.left + box.width - 24, y: box.top + box.height - 24 });
      await driver.ready('//div[@role="group" and @aria-label="Sizes"]', 'xpath');
      const before = await driver.execute(() => document.querySelectorAll('[data-size-row]').length);
      await driver.click(`//div[@role="group" and @aria-label="Sizes"]//button[@aria-label=${xpathString('Add Phone')}]`, 'xpath');
      await waitFor(() => driver.execute(previous => document.querySelectorAll('[data-size-row]').length > previous, before), 'The Phone size was not added');
    });

    await step('Delete removes a selected layer from Layers', async () => {
      const before = await driver.execute(() => document.querySelectorAll('[data-element]').length);
      const rows = await driver.execute(() => document.querySelectorAll('[data-element-row] .dock-tree-select.element').length);
      assert.ok(rows > 0, 'Layers lists no layer rows');
      await driver.click('[data-element-row] .dock-tree-select.element');
      await waitFor(() => driver.execute(() => !!document.querySelector('.element-selection-box')), 'Clicking a Layers row did not select the layer');
      await pointer.combo('Delete');
      await waitFor(() => driver.execute(previous => document.querySelectorAll('[data-element]').length < previous, before), 'Delete did not remove the selected layer');
      // Deleting asks nothing; the toast's Undo brings the layer back.
      await driver.click('//button[normalize-space(.)="Undo"]', 'xpath');
      await waitFor(() => driver.execute(previous => document.querySelectorAll('[data-element]').length === previous, before), 'Undo in the toast did not restore the layer');
      await driver.click('[data-element-row] .dock-tree-select.element');
      await pointer.combo('Delete');
      await waitFor(() => driver.execute(previous => document.querySelectorAll('[data-element]').length < previous, before), 'Delete did not remove the layer again');
    });

    await step('Delete removes a selected boundary from Layers', async () => {
      const before = (await boundaries()).length;
      const rows = await driver.execute(() => [...document.querySelectorAll('[data-boundary-row]')].map(node => node.getAttribute('data-boundary-row')));
      // The first boundary is no longer a protected home page.
      const screenRow = await driver.execute(() => {
        const row = [...document.querySelectorAll('[data-boundary-row]')].at(0);
        return row?.getAttribute('data-boundary-row') ?? null;
      });
      assert.ok(screenRow, 'Layers lists no boundary rows');
      await driver.click(`//button[@data-boundary-row=${xpathString(screenRow)}]`, 'xpath');
      await driver.click('[aria-label="Near-black background"]');
      await waitFor(() => driver.execute(id => getComputedStyle(document.querySelector(`.page-boundary[data-size="${id}"]`)).backgroundColor === 'rgb(18, 18, 18)', screenRow), 'Picking the near-black background swatch did not darken the first boundary');
      await driver.click(`//button[@data-boundary-row=${xpathString(screenRow)}]`, 'xpath');
      await pointer.combo('Delete');
      await waitFor(async () => (await boundaries()).length === before - 1, 'Delete did not remove the selected boundary');
      assert.ok(rows.length >= 1);
    });

    await step('Open History', async () => {
      await driver.click('[aria-label="History and alternatives"]');
      await driver.ready('.history-panel');
    });

    let savedPath;
    await step('Ctrl+S saves the project into a temporary file', async () => {
      const documents = join(process.env.DESEGNOS_SMOKE_SCRATCH, 'Documents');
      savedPath = join(documents, 'Smoke canvas.desegnos');
      await pointer.combo('Control', 's');
      await driver.ready('[aria-label="Save untitled project"]');
      await driver.fill('Project name', 'Smoke canvas');
      await native.chooseFile(
        () => driver.click(`//form[@aria-label="Save untitled project"]//button[normalize-space(.)=${xpathString('Choose location…')}]`, 'xpath'),
        savedPath, 'save-project', evidence,
      );
      await driver.click(`//form[@aria-label="Save untitled project"]//button[normalize-space(.)=${xpathString('Save project')}]`, 'xpath');
      await waitFor(async () => {
        try { return (await stat(savedPath)).isFile(); } catch (error) { if (error.code === 'ENOENT') return false; throw error; }
      }, 'The Ctrl+S save did not write the temporary project file');
      await waitFor(() => driver.execute(() => !document.querySelector('[aria-label="Save untitled project"]')), 'The save form stayed open');
    });

    await step('Reopen the saved project from Home', async () => {
      await home();
      await driver.click(`//button[contains(@class,"start-recent-open")][.//span[normalize-space(.)=${xpathString('Smoke canvas')}]]`, 'xpath');
      await waitFor(() => driver.execute(() => !!document.querySelector('#project-canvas')), 'The saved project did not reopen');
      const result = await driver.execute(() => ({ boundaries: document.querySelectorAll('[data-boundary]').length, elements: document.querySelectorAll('[data-element]').length, sizes: document.querySelectorAll('[data-size-row]').length, requests: document.querySelectorAll('[data-request-row]').length, markers: document.querySelectorAll('.request-marker').length }));
      assert.ok(result.boundaries > 0 && result.elements >= 3 && result.requests >= 1 && result.markers >= 1, JSON.stringify(result));
    });

    const errors = await driver.execute(() => window.__desegnosSmokeErrors);
    await writeFile(join(evidence, 'browser-errors.json'), JSON.stringify(errors, null, 2));
    report.browserErrors = errors;
    await writeFile(join(evidence, 'final.png'), Buffer.from(await driver.screenshot(), 'base64'));
    await writeFile(join(evidence, 'saved-project.sha256'), `${await hashFile(savedPath)}\n`);
    report.passed = true;
  } catch (error) {
    report.error = error.stack;
    if (driver) {
      try { await writeFile(join(evidence, 'failure.png'), Buffer.from(await driver.screenshot(), 'base64')); } catch {}
      try { await writeFile(join(evidence, 'failure.html'), await driver.command('GET', '/source')); } catch {}
      try { await writeFile(join(evidence, 'failure-steps.json'), JSON.stringify(report.steps, null, 2)); } catch {}
    }
    console.error(error.stack);
  } finally {
    await stop();
    await new Promise(accept => log.end(accept));
    report.finishedAt = new Date().toISOString();
    await writeFile(join(evidence, 'report.json'), JSON.stringify(report, null, 2));
    process.exitCode = report.passed ? 0 : 1;
  }
}

async function main() {
  if (args.includes('--help')) {
    console.log('node tests/native/canvas-smoke.mjs [--skip-build] [--binary /absolute/desegnos]');
    console.log('Builds the explicit native-tests app, then drives the start page, canvas, Properties, Layers, History and save/reopen on a guarded Xvfb display.');
    return;
  }
  if (process.platform !== 'linux') throw new Error('The canvas smoke test requires the guarded Linux Xvfb route.');
  const skipBuild = args.includes('--skip-build');
  const binaryIndex = args.indexOf('--binary');
  if (binaryIndex !== -1 && !args[binaryIndex + 1]) throw new Error('--binary needs an absolute path');
  const target = resolve('target/native-tests');
  const binary = resolve(binaryIndex === -1 ? join(target, 'debug/desegnos') : args[binaryIndex + 1]);
  process.chdir(root);
  if (!skipBuild) {
    const build = spawn(process.execPath, ['scripts/tauri.mjs', 'build', '--debug', '--no-bundle', '--features', 'native-tests,gtk-dialogs', '--config', 'src-tauri/tauri.test.conf.json', '--config', JSON.stringify({ build: { beforeBuildCommand: 'npm run build' } }), '--', '--no-default-features'], {
      cwd: root, env: { ...toolchainEnvironment(), CARGO_TARGET_DIR: target }, stdio: 'inherit',
    });
    if (await childExit(build) !== 0) throw new Error('The native-tests build failed');
  }
  await access(binary);
  const evidence = resolve('test-results/smoke', new Date().toISOString().replace(/[:.]/g, '-') + `-${process.pid}`);
  await mkdir(evidence, { recursive: true });
  const scratch = await mkdtemp(join(tmpdir(), 'desegnos-smoke-'));
  const profile = Object.fromEntries(['data', 'config', 'cache', 'runtime'].map(name => [name, join(scratch, name)]));
  for (const path of Object.values(profile)) { await mkdir(path); await chmod(path, 0o700); }
  const documents = join(scratch, 'Documents');
  await mkdir(documents, { mode: 0o700 });
  await writeFile(join(profile.config, 'user-dirs.dirs'), `XDG_DOCUMENTS_DIR="${documents}"\n`, { mode: 0o600 });
  const before = await desktopState();
  let result = 1;
  try {
    const isolated = spawn(resolveXvfbRun(), ['-d', '-e', join(evidence, 'xvfb.log'), '-f', join(scratch, 'Xauthority'), '-s', '-screen 0 1600x1100x24',
      resolveDbusRunSession(), '--', process.execPath, script, '--isolated'], {
      cwd: root, detached: true, stdio: 'inherit', env: xvfbEnvironment({ ...process.env,
        XDG_DATA_HOME: profile.data, XDG_CONFIG_HOME: profile.config, XDG_CACHE_HOME: profile.cache, XDG_RUNTIME_DIR: profile.runtime,
        GDK_SCALE: '1', GDK_DPI_SCALE: '1', DESEGNOS_DATA_DIR: join(scratch, 'app-data'),
        DESEGNOS_DESKTOP_DISPLAY: process.env.DISPLAY ?? '', DESEGNOS_SMOKE_BINARY: binary,
        DESEGNOS_SMOKE_EVIDENCE: evidence, DESEGNOS_SMOKE_SCRATCH: scratch }),
    });
    const interrupt = () => signalGroup(isolated.pid, 'SIGTERM');
    process.once('SIGINT', interrupt); process.once('SIGTERM', interrupt);
    try { result = await childExit(isolated); }
    finally { process.removeListener('SIGINT', interrupt); process.removeListener('SIGTERM', interrupt); }
  } finally {
    await rm(scratch, { recursive: true, force: true, maxRetries: 20, retryDelay: 100 });
    const cleanup = await verifyDesktop(before);
    await writeFile(join(evidence, 'desktop-cleanup.json'), JSON.stringify(cleanup, null, 2));
  }
  console.log(`Smoke evidence: ${evidence}`);
  process.exitCode = result;
}

try { if (args.length === 1 && args[0] === '--isolated') await runIsolated(); else await main(); }
catch (error) { console.error(error.stack); process.exitCode = 1; }
