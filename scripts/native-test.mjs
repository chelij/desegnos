import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createReadStream, createWriteStream } from 'node:fs';
import { access, chmod, mkdir, mkdtemp, readFile, rm, stat, writeFile } from 'node:fs/promises';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';
import { toolchainEnvironment } from './toolchain.mjs';
import { waitFor, WebDriver } from '../tests/native/webdriver.mjs';
import { recoverProject } from '../tests/native/workflow.mjs';
import { PhysicalInput } from '../tests/native/physical-workflow.mjs';
import { chooseFile, savedState } from '../tests/native/file-dialog-workflow.mjs';
import { startProviderServer, assertNoSyntheticKeys } from '../tests/native/provider-server.mjs';
import { identityWorkflow } from '../tests/native/identity-workflow.mjs';
import { uxAuditWorkflow } from '../tests/native/ux-audit-workflow.mjs';
import { canvasInputWorkflow } from '../tests/native/canvas-input-workflow.mjs';
import { logoScopeWorkflow } from '../tests/native/logo-scope-workflow.mjs';
import { layersWorkflow } from '../tests/native/layers-workflow.mjs';
import { imageResizeWorkflow } from '../tests/native/image-resize-workflow.mjs';
import { cleanupExtrasWorkflow } from '../tests/native/cleanup-extras-workflow.mjs';
import { codeMetaWorkflow } from '../tests/native/code-meta-workflow.mjs';
import { sourceEditWorkflow } from '../tests/native/source-edit-workflow.mjs';
import { realUseWorkflow } from '../tests/native/real-use-workflow.mjs';
import { seedRealUse } from '../tests/native/real-use-fixtures.mjs';
import { websiteImportWorkflow } from '../tests/native/website-import-workflow.mjs';
import { uiTour } from '../tests/native/ui-tour.mjs';
import { flashWorkflow } from '../tests/native/flash-workflow.mjs';
import { projectFoldersWorkflow } from '../tests/native/project-folders-workflow.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const guardedXvfb = '~/.local/bin/xvfb-run';
const args = process.argv.slice(2);

async function hashFile(path) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest('hex');
}

async function childExit(child) {
  if (child.exitCode !== null) return child.exitCode;
  if (child.signalCode !== null) return 1;
  return new Promise((accept, reject) => {
    child.once('error', reject);
    child.once('exit', (code, signal) => accept(code ?? (signal ? 1 : 0)));
  });
}

function terminateGroup(child, signal) {
  if (!child.pid) return;
  try { process.kill(-child.pid, signal); }
  catch (error) { if (error.code !== 'ESRCH') throw error; }
}

async function desktopState() {
  const number = process.env.DISPLAY?.match(/^:(\d+)(?:\.\d+)?$/)?.[1];
  const paths = number ? [`/tmp/.X11-unix/X${number}`, `/tmp/.X${number}-lock`] : [];
  const files = [];
  for (const path of paths) {
    try { const item = await stat(path); files.push({ path, dev: item.dev, ino: item.ino }); }
    catch (error) { if (error.code !== 'ENOENT') throw error; }
  }
  const probe = process.env.DISPLAY ? spawnSync('xprop', ['-root', '_NET_SUPPORTING_WM_CHECK'], { timeout: 5000, encoding: 'utf8' }) : null;
  return { display: process.env.DISPLAY ?? null, files, connected: probe?.status === 0 };
}

async function verifyDesktop(original) {
  for (const previous of original.files) {
    const current = await stat(previous.path);
    assert.equal(current.dev, previous.dev, `Desktop display resource changed: ${previous.path}`);
    assert.equal(current.ino, previous.ino, `Desktop display resource was replaced: ${previous.path}`);
  }
  if (original.connected) {
    const probe = spawnSync('xprop', ['-root', '_NET_SUPPORTING_WM_CHECK'], { timeout: 5000, encoding: 'utf8' });
    assert.equal(probe.status, 0, 'The original desktop X11 connection failed after test cleanup');
  }
  return { resourceCheck: 'passed', connectionCheck: original.connected ? 'passed' : 'unavailable before test' };
}

async function freePort() {
  const server = createServer();
  await new Promise((accept, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', accept); });
  const port = server.address().port;
  await new Promise((accept, reject) => server.close(error => error ? reject(error) : accept()));
  return port;
}

async function runIsolated() {
  const display = process.env.DISPLAY?.match(/^:(\d+)(?:\.\d+)?$/);
  assert.ok(display && Number(display[1]) >= 90, 'Native tests require an isolated Xvfb display numbered 90 or higher');
  assert.notEqual(process.env.DISPLAY, process.env.DESEGNI_DESKTOP_DISPLAY, 'Refusing to interact with the original desktop display');
  assert.equal(process.env.GDK_BACKEND, 'x11');
  assert.equal(process.env.WAYLAND_DISPLAY, undefined);
  assert.equal(process.env.WAYLAND_SOCKET, undefined);
  assert.ok(process.env.DBUS_SESSION_BUS_ADDRESS, 'An isolated session bus is required');
  const displayProbe = spawnSync('xprop', ['-root', '_NET_SUPPORTING_WM_CHECK'], { encoding: 'utf8', timeout: 5000 });
  assert.equal(displayProbe.status, 0, 'Cannot connect to the isolated Xvfb display');
  const binary = process.env.DESEGNI_NATIVE_TEST_BINARY;
  const evidence = process.env.DESEGNI_NATIVE_TEST_EVIDENCE;
  assert.ok(binary && evidence);
  const report = { passed: false, startedAt: new Date().toISOString(), display: process.env.DISPLAY,
    platform: process.platform, node: process.version, binary, binarySha256: await hashFile(binary),
    input: 'Embedded WebDriver controls plus trusted OS pointer/key input through system X11/XTEST on the guarded virtual display.',
    nativeCommands: 'Real Rust commands; no invoke mocks or replacement project state.', steps: [],
    limits: ['Paid AI provider behavior is not covered.', 'This run covers Linux WebKitGTK; Windows and macOS require their own native runs.'] };
  if (process.env.DESEGNI_NATIVE_TEST_ACCESSIBILITY === '1') process.env.ATSPI_DBUS_IMPLEMENTATION = 'dbus-daemon';
  const provider = await startProviderServer();
  process.env.DESEGNI_NATIVE_TEST_OPENAI_ENDPOINT = provider.endpoint;
  process.env.DESEGNI_NATIVE_TEST_WEBSITE_ORIGIN = provider.endpoint.replace(/\/v1$/, '');
  report.generationProvider = 'Controlled loopback HTTP server using real native transport; synthetic credentials, no paid OpenAI calls.';
  let app;
  let driver;
  let accessibility;
  let launch = 0;
  const logs = [];
  async function stop(signal = 'SIGTERM') {
    accessibility?.kill('SIGTERM'); accessibility = null;
    if (signal === 'SIGKILL') {
      assert.ok(app && app.exitCode === null && app.signalCode === null, 'The crash test requires a running native app');
      const child = app;
      terminateGroup(child, 'SIGKILL');
      app = null; driver = null;
      await childExit(child);
      await child.streamsClosed;
      assert.equal(child.signalCode, 'SIGKILL', 'The native app did not terminate from the simulated crash');
      return;
    }
    if (driver) { try { await driver.stop(); } catch { /* The crash recovery run may already have killed its session. */ } driver = null; }
    if (app) {
      const child = app; app = null;
      terminateGroup(child, signal);
      await Promise.race([childExit(child), delay(2000)]);
      terminateGroup(child, 'SIGKILL');
      await childExit(child);
      await child.streamsClosed;
    }
  }
  async function start() {
    const port = await freePort();
    const log = createWriteStream(join(evidence, `app-${++launch}.log`), { flags: 'wx' });
    logs.push(log);
    app = spawn(binary, [], { cwd: root, env: { ...process.env, TAURI_WEBDRIVER_PORT: String(port) },
      detached: true, stdio: ['ignore', 'pipe', 'pipe'] });
    app.stdout.pipe(log, { end: false }); app.stderr.pipe(log, { end: false });
    const child = app;
    child.streamsClosed = new Promise(accept => child.once('close', accept));
    await new Promise((accept, reject) => { child.once('spawn', accept); child.once('error', reject); });
    // A spawned child's /proc environment can still show the pre-exec process.
    // Verify the exact isolated binding before connecting or delivering input.
    const environment = await waitFor(async () => {
      const values = (await readFile(`/proc/${child.pid}/environ`, 'utf8')).split('\0');
      return values.includes(`DISPLAY=${process.env.DISPLAY}`) && values.includes('GDK_BACKEND=x11') && values;
    }, 'The test app did not bind to the isolated X11 display', 5000);
    assert.ok(environment.includes(`DISPLAY=${process.env.DISPLAY}`), 'The test app is connected to a different display');
    assert.ok(environment.includes('GDK_BACKEND=x11'), 'The test app has not selected X11');
    driver = await new WebDriver(port).start(() => child.exitCode === null && child.signalCode === null);
    assert.equal(driver.capabilities.platformName, 'linux');
    assert.equal(driver.capabilities.browserName, 'WebKitGTK');
    if (process.env.DESEGNI_NATIVE_TEST_ACCESSIBILITY === '1') {
      accessibility = spawn('python', ['tests/native/accessibility-listener.py', String(child.pid)], { cwd: root, env: process.env, stdio: ['ignore', 'pipe', 'pipe'] });
      let ready = false;
      accessibility.stdout.on('data', bytes => { if (String(bytes).includes('Accessibility active')) ready = true; });
      accessibility.stderr.pipe(log, { end: false });
      await waitFor(() => ready, 'The isolated accessibility listener did not activate', 15000);
    }
    await waitFor(async () => {
      const source = await driver.command('GET', '/source');
      return typeof source === 'string' && /class="application(?:\s|")/.test(source);
    }, 'The native document did not finish mounting before interaction', 30_000);
    await driver.execute(() => {
      window.__desegniTestErrors = [];
      window.addEventListener('error', event => window.__desegniTestErrors.push({ kind: 'error', message: event.message }));
      window.addEventListener('unhandledrejection', event => window.__desegniTestErrors.push({ kind: 'rejection', message: String(event.reason) }));
      const original = console.error;
      console.error = (...values) => {
        window.__desegniTestErrors.push({ kind: 'console', message: values.map(String).join(' ') });
        original.apply(console, values);
      };
      return true;
    });
    report.capabilities = driver.capabilities;
    return driver;
  }
  async function step(name, action) {
    const result = { name, passed: false, startedAt: new Date().toISOString() };
    report.steps.push(result);
    try { await action(); result.passed = true; console.log(`PASS ${name}`); }
    catch (error) { result.error = error.stack; throw error; }
    finally { result.finishedAt = new Date().toISOString(); }
  }
  for (const signal of ['SIGINT', 'SIGTERM']) process.once(signal, () => { void stop().finally(() => process.exit(1)); });
  try {
    await start();
    let project;
    const physical = new PhysicalInput(driver, app.pid);
    if (process.env.DESEGNI_NATIVE_TEST_SCOPE === 'flash') {
      await flashWorkflow(driver, physical, step, evidence);
      report.passed = true; return;
    } else if (process.env.DESEGNI_NATIVE_TEST_SCOPE === 'ui-tour') {
      report.scope = 'Screenshot tour of workspace surfaces in both themes and widths.';
      report.tour = await uiTour(driver, physical, step, evidence, provider, async () => {
        await stop('SIGKILL');
        await start();
        return { driver, physical: new PhysicalInput(driver, app.pid) };
      });
      report.passed = report.tour.failures.length === 0;
      return;
    } else if (process.env.DESEGNI_NATIVE_TEST_SCOPE === 'code-meta') {
      report.scope = 'Focused mounted HTML metadata toggle with six stored pictures.';
      project = await codeMetaWorkflow(driver, physical, step, evidence);
      const errors = await driver.execute(() => window.__desegniTestErrors);
      await writeFile(join(evidence, 'browser-errors.json'), JSON.stringify(errors, null, 2));
      assert.deepEqual(errors, []);
      report.passed = true;
      return;
    } else if (process.env.DESEGNI_NATIVE_TEST_SCOPE === 'website-import') {
      report.scope = 'As-is website import, native heading editing and read-only chelib Astro import.';
      project = await websiteImportWorkflow(driver, physical, step, evidence);
    } else if (process.env.DESEGNI_NATIVE_TEST_SCOPE === 'project-folders') {
      report.scope = 'Project folders, automatic saves, default/per-project locations, safe live-site sync, import, copies and legacy migration.';
      project = await projectFoldersWorkflow(driver, physical, step, evidence);
    } else if (process.env.DESEGNI_NATIVE_TEST_SCOPE === 'real-use') {
      report.scope = 'Real-use flows on copies of the owner\'s projects and unsaved work (scripts/usage-report.mjs).';
      project = await realUseWorkflow(driver, physical, step, evidence, provider);
    } else if (process.env.DESEGNI_NATIVE_TEST_SCOPE === 'image-resize') {
      report.scope = 'Large original picture preview followed by Generate-time resizing.';
      project = await imageResizeWorkflow(driver, physical, step, evidence, provider);
    } else if (process.env.DESEGNI_NATIVE_TEST_SCOPE === 'cleanup-extras') {
      report.scope = 'Cleanup extra workflows: exact History, tips, page checks, real file drop, device previews and code editing.';
      project = await cleanupExtrasWorkflow(driver, physical, step, evidence, provider);
    } else if (process.env.DESEGNI_NATIVE_TEST_SCOPE === 'source-edit') {
      report.scope = 'Source editing, validation diagnostics, source map, history, archive and real export.';
      project = await sourceEditWorkflow(driver, physical, step, evidence);
    } else if (process.env.DESEGNI_NATIVE_TEST_SCOPE === 'layers') {
      report.scope = 'Focused native Layers, real system clipboard, visual stacking, local history, export and recovery.';
      project = await layersWorkflow(driver, physical, step, evidence, provider);
    } else if (process.env.DESEGNI_NATIVE_TEST_SCOPE === 'canvas-input') {
      report.scope = 'Focused canvas input, zoom, pan and unlimited box sizing.';
      project = await canvasInputWorkflow(driver, physical, step, evidence, provider);
    } else if (['identity', 'logo-scope'].includes(process.env.DESEGNI_NATIVE_TEST_SCOPE)) {
      report.scope = 'Beginner brand import: folders, Astro ranking and automatic fitting, manual limits/reset, website addresses, ZIPs, review choices and recovery.';
      await step('Create a real source website for brand import', async () => {
        await driver.button('Blank website');
        await driver.fill('Project name', 'Native brand source');
        await driver.button('Mobile'); await driver.button('Create project');
        await waitFor(() => driver.execute(() => !!document.querySelector('[data-testid="canvas"]')), 'The focused canvas did not mount');
        await physical.initialize(evidence);
        await driver.press('[data-testid="canvas"]', '\uE007');
        await driver.fill('Request 1', 'Create a native interactive button and explain its behavior.');
        await waitFor(async () => (await driver.invoke('get_workspace')).project.pages[0].regions[0]?.prompt, 'The request did not persist');
        await driver.invoke('set_ai_key', { key: provider.key(), storage: 'session' });
        provider.setMode('complete'); await driver.button('Generate');
        await waitFor(async () => (await driver.invoke('get_workspace')).project.pages[0].content, 'The source website did not generate', 25000);
        const destination = (await driver.invoke('get_workspace')).path;
        await driver.button('Save');
        await savedState(driver, destination);
      });
    } else {
      report.scope = 'Current UX audit core loop with real export, local edits, generation and recovery.';
      project = await uxAuditWorkflow(driver, physical, step, evidence, provider);
    }
    if (!process.env.DESEGNI_NATIVE_TEST_SCOPE || ['identity', 'logo-scope'].includes(process.env.DESEGNI_NATIVE_TEST_SCOPE)) {
      project = await identityWorkflow(driver, physical, step, evidence, provider);
      if (process.env.DESEGNI_NATIVE_TEST_SCOPE === 'logo-scope') project = await logoScopeWorkflow(driver, physical, step, evidence, provider);
    }
    // A saved-only workflow correctly has no unsaved journal to recover.
    // Make one real acknowledged edit so this runner tests crash recovery.
    if (!(await driver.invoke('get_workspace')).dirty) {
      await step('Leave an acknowledged unsaved canvas edit for crash recovery', async () => {
        const before = (await driver.invoke('get_workspace')).project;
        await driver.invoke('edit_project', {revision:before.revision,command:{type:'set_grid',enabled:!before.showGrid}});
        project = await waitFor(async () => {
          const state = await driver.invoke('get_workspace');
          return state.dirty && state.project.revision > before.revision && state.project.showGrid !== before.showGrid && state.project;
        }, 'The unsaved grid edit was not journaled');
      });
    }
    await step('The native editor workflow reports no browser errors or unhandled rejections', async () => {
      const errors = await driver.execute(() => window.__desegniTestErrors);
      await writeFile(join(evidence, 'browser-errors.json'), JSON.stringify(errors, null, 2));
      assert.deepEqual(errors, []);
    });
    await writeFile(join(evidence, 'authored.png'), Buffer.from(await driver.screenshot(), 'base64'));
    await writeFile(join(evidence, 'authored-project.json'), JSON.stringify(project, null, 2));
    const generationBeforeRestart = await driver.invoke('get_generation', { projectId: project.id });
    await step('Crash the native app with SIGKILL before any session teardown', () => stop('SIGKILL'));
    const submittedCount = provider.generationRequests().length;
    const identitySubmittedCount = provider.identityRequests().length;
    const restarted = await start();
    if (process.env.DESEGNI_NATIVE_TEST_SCOPE === 'website-import') { restarted.requestTimeout = 120000; await restarted.command('POST', '/timeouts', { script: 120000, implicit: 0, pageLoad: 120000 }); }
    await recoverProject(restarted, project, step);
    await step('Restart preserves project data and recent paths, drops the session key and never replays paid work', async () => {
      const ai = await driver.invoke('get_ai_settings');
      assert.equal(ai.credential.configured, false);
      assert.equal(ai.credential.storage, 'none');
      assert.equal(ai.chatgpt.accounts.some(account => account.connected || account.planEnabled || account.storage !== 'none'), false);
      const job = await driver.invoke('get_generation', { projectId: project.id });
      assert.equal(job?.jobId ?? null, generationBeforeRestart?.jobId ?? null);
      if (project.importedIdentity) {
        const identity = await driver.invoke('get_identity_analysis');
        assert.equal(identity.status, 'complete');
        assert.equal(identity.source.sha256, project.importedIdentity.sourceSha256);
      }
      const recent = await driver.invoke('get_recent_projects');
      if (process.env.DESEGNI_NATIVE_TEST_SCOPE !== 'image-resize') assert.ok(recent.projects.length > 0);
      assert.equal(provider.identityRequests().length, identitySubmittedCount);
      assert.equal(provider.generationRequests().length, submittedCount);
    });
    if (process.env.DESEGNI_NATIVE_TEST_SCOPE === 'cleanup-extras') {
      await step('Dismissed first-run tips stay dismissed after a fresh native process', async () => {
        assert.equal(await driver.execute(() => localStorage.getItem('desegnos.first-run-tips')), 'done');
        assert.equal(await driver.execute(() => !!document.querySelector('.first-run-tips')), false);
      });
    }
    const recoveryErrors = await driver.execute(() => window.__desegniTestErrors);
    await writeFile(join(evidence, 'recovery-browser-errors.json'), JSON.stringify(recoveryErrors, null, 2));
    assert.deepEqual(recoveryErrors, []);
    await writeFile(join(evidence, 'recovered.png'), Buffer.from(await driver.screenshot(), 'base64'));
    report.passed = true;
  } catch (error) {
    report.error = error.stack;
    if (driver) {
      try { await writeFile(join(evidence, 'failure-input-events.json'), JSON.stringify(await driver.execute(() => ({ events: window.__desegniNativeEvents, focus: document.activeElement?.outerHTML.slice(0, 1000) })), null, 2)); } catch {}
      try { await writeFile(join(evidence, 'failure-workspace.json'), JSON.stringify(await driver.invoke('get_workspace'), null, 2)); } catch {}
      try { await writeFile(join(evidence, 'failure.png'), Buffer.from(await driver.screenshot(), 'base64')); } catch {}
      try { await writeFile(join(evidence, 'failure.html'), await driver.command('GET', '/source')); } catch {}
    }
    console.error(error.stack);
  } finally {
    await stop();
    for (const log of logs) await new Promise(accept => log.end(accept));
    try { await assertNoSyntheticKeys([evidence, process.env.XDG_DATA_HOME], join(evidence, 'credential-nondisclosure.json')); }
    catch (error) { report.passed = false; report.error = error.stack; }
    await provider.close();
    report.oauth = provider.oauth.records();
    report.finishedAt = new Date().toISOString();
    await writeFile(join(evidence, 'report.json'), JSON.stringify(report, null, 2));
    process.exitCode = report.passed ? 0 : 1;
  }
}

async function main() {
  if (args.includes('--help')) {
    console.log('node scripts/native-test.mjs [--skip-build] [--flash-only|--ui-tour-only|--code-meta-only|--project-folders-only|--ux-audit-only|--identity-only|--canvas-input-only|--layers-only|--source-edit-only|--cleanup-extras-only|--image-resize-only|--real-use-only] [--binary /absolute/test-build/desegnos]\nBuilds the explicit native-tests feature, then runs real native UI and crash recovery checks on a guarded isolated Xvfb display. The default covers the current UX core loop and brand import. --ux-audit-only excludes the separately verified brand-import flow. --identity-only covers brand import and recovery. --layers-only covers request and website stacking, the real system clipboard, history, export and recovery.');
    return;
  }
  if (process.platform !== 'linux') throw new Error('This runner requires the guarded Linux Xvfb route. Native Windows/macOS isolation must be configured on those runners.');
  const skipBuild = args.includes('--skip-build');
  const binaryIndex = args.indexOf('--binary');
  if (binaryIndex !== -1 && !args[binaryIndex + 1]) throw new Error('--binary needs an absolute file path');
  const known = new Set(['--flash-only', '--ui-tour-only', '--code-meta-only', '--website-import-only', '--project-folders-only', '--image-resize-only', '--real-use-only', '--skip-build', '--generation-only', '--ux-audit-only', '--identity-only', '--canvas-input-only', '--layers-only', '--source-edit-only', '--cleanup-extras-only', '--logo-scope-only', '--binary', ...(binaryIndex === -1 ? [] : [args[binaryIndex + 1]])]);
  for (const argument of args) if (!known.has(argument)) throw new Error(`Unknown argument ${argument}`);
  if (args.includes('--flash-only')) process.env.DESEGNI_NATIVE_TEST_SCOPE = 'flash';
  if (args.includes('--ui-tour-only')) process.env.DESEGNI_NATIVE_TEST_SCOPE = 'ui-tour';
  if (args.includes('--code-meta-only')) process.env.DESEGNI_NATIVE_TEST_SCOPE = 'code-meta';
  if (args.includes('--website-import-only')) process.env.DESEGNI_NATIVE_TEST_SCOPE = 'website-import';
  if (args.includes('--project-folders-only')) process.env.DESEGNI_NATIVE_TEST_SCOPE = 'project-folders';
  if (args.includes('--image-resize-only')) process.env.DESEGNI_NATIVE_TEST_SCOPE = 'image-resize';
  if (args.includes('--real-use-only')) process.env.DESEGNI_NATIVE_TEST_SCOPE = 'real-use';
  if (args.includes('--canvas-input-only')) process.env.DESEGNI_NATIVE_TEST_SCOPE = 'canvas-input';
  if (args.includes('--identity-only')) process.env.DESEGNI_NATIVE_TEST_SCOPE = 'identity';
  if (args.includes('--generation-only') || args.includes('--ux-audit-only')) process.env.DESEGNI_NATIVE_TEST_SCOPE = 'audit';
  if (args.includes('--logo-scope-only')) process.env.DESEGNI_NATIVE_TEST_SCOPE = 'logo-scope';
  if (args.includes('--layers-only')) process.env.DESEGNI_NATIVE_TEST_SCOPE = 'layers';
  if (args.includes('--cleanup-extras-only')) process.env.DESEGNI_NATIVE_TEST_SCOPE = 'cleanup-extras';
  if (args.includes('--source-edit-only')) process.env.DESEGNI_NATIVE_TEST_SCOPE = 'source-edit';
  process.chdir(root);
  const target = resolve('target/native-tests');
  const binary = resolve(binaryIndex === -1 ? join(target, 'debug/desegnos') : args[binaryIndex + 1]);
  if (!skipBuild) {
    const build = spawn(process.execPath, ['scripts/tauri.mjs', 'build', '--debug', '--no-bundle', '--features', 'native-tests,gtk-dialogs', '--config', 'src-tauri/tauri.test.conf.json', '--config', JSON.stringify({ build: { beforeBuildCommand: 'npm run build' } }), '--', '--no-default-features'],
      { cwd: root, env: { ...toolchainEnvironment(), CARGO_TARGET_DIR: target }, stdio: 'inherit' });
    const status = await childExit(build);
    if (status !== 0) throw new Error(`The explicit native-tests build failed (${status})`);
  }
  await access(binary);
  await access(guardedXvfb);
  const evidence = resolve('test-results/native', new Date().toISOString().replace(/[:.]/g, '-') + `-${process.pid}`);
  await mkdir(evidence, { recursive: true });
  const scratch = await mkdtemp(join(tmpdir(), 'dsgn-native-'));
  const profile = Object.fromEntries(['data', 'config', 'cache', 'runtime'].map(name => [name, join(scratch, name)]));
  for (const path of Object.values(profile)) { await mkdir(path); await chmod(path, 0o700); }
  // Automatic project creation must never write to the owner's real Documents folder.
  const documents = join(scratch, 'Documents');
  await mkdir(documents, { mode: 0o700 });
  await writeFile(join(profile.config, 'user-dirs.dirs'), `XDG_DOCUMENTS_DIR="${documents}"\n`, { mode: 0o600 });
  if (['real-use', 'flash'].includes(process.env.DESEGNI_NATIVE_TEST_SCOPE)) {
    const seeded = await seedRealUse(profile.data, evidence, { recoveries: process.env.DESEGNI_NATIVE_TEST_SCOPE !== 'flash' });
    console.log(`Real-use data: ${seeded.projects.length} projects, ${seeded.recoveries.length} recoveries from ${seeded.source}`);
  }
  const original = await desktopState();
  let result = 1;
  let desktop;
  try {
    const isolated = spawn(guardedXvfb, ['-d', '-e', join(evidence, 'xvfb.log'), '-f', join(scratch, 'Xauthority'), '-s', '-screen 0 1600x1100x24',
      'dbus-run-session', '--', process.execPath, fileURLToPath(import.meta.url), '--isolated'], {
      cwd: root, detached: true, stdio: 'inherit', env: { ...process.env,
        XDG_DATA_HOME: profile.data, XDG_CONFIG_HOME: profile.config, XDG_CACHE_HOME: profile.cache, XDG_RUNTIME_DIR: profile.runtime,
        GDK_SCALE: '1', GDK_DPI_SCALE: '1',
        ...(process.env.DESEGNI_NATIVE_TEST_ACCESSIBILITY === '1' ? { ATSPI_DBUS_IMPLEMENTATION: 'dbus-daemon' } : {}),
        DESEGNI_DESKTOP_DISPLAY: process.env.DISPLAY ?? '', DESEGNI_NATIVE_TEST_BINARY: binary, DESEGNI_NATIVE_TEST_EVIDENCE: evidence },
    });
    const interrupt = () => terminateGroup(isolated, 'SIGTERM');
    process.once('SIGINT', interrupt); process.once('SIGTERM', interrupt);
    try { result = await childExit(isolated); }
    finally { process.removeListener('SIGINT', interrupt); process.removeListener('SIGTERM', interrupt); }
  } finally {
    let cleanupError;
    try { await rm(scratch, { recursive: true, force: true, maxRetries: 20, retryDelay: 100 }); }
    catch (error) { cleanupError = error; }
    desktop = await verifyDesktop(original);
    await writeFile(join(evidence, 'desktop-cleanup.json'), JSON.stringify({ ...desktop, scratchCleanup: cleanupError ? { path: scratch, error: cleanupError.message } : 'passed' }, null, 2));
    if (cleanupError) throw cleanupError;
  }
  console.log(`Native evidence: ${evidence}`);
  process.exitCode = result;
}

try { if (args.length === 1 && args[0] === '--isolated') await runIsolated(); else await main(); }
catch (error) { console.error(error.stack); process.exitCode = 1; }
