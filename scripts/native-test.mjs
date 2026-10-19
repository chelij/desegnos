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
import { authorProject, recoverProject } from '../tests/native/workflow.mjs';
import { interactionWorkflow } from '../tests/native/interaction-workflow.mjs';
import { PhysicalInput, physicalAuthoring } from '../tests/native/physical-workflow.mjs';
import { chooseFile, savedState, fileDialogWorkflow } from '../tests/native/file-dialog-workflow.mjs';
import { editorActions } from '../tests/native/editor-actions.mjs';
import { migrationWorkflow } from '../tests/native/migration-workflow.mjs';
import { schema2MigrationWorkflow } from '../tests/native/schema2-migration-workflow.mjs';
import { startProviderServer, assertNoSyntheticKeys } from '../tests/native/provider-server.mjs';
import { generationWorkflow } from '../tests/native/generation-workflow.mjs';
import { chatgptWorkflow } from '../tests/native/chatgpt-workflow.mjs';
import { identityWorkflow } from '../tests/native/identity-workflow.mjs';

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
  const provider = await startProviderServer();
  process.env.DESEGNI_NATIVE_TEST_OPENAI_ENDPOINT = provider.endpoint;
  report.generationProvider = 'Controlled loopback HTTP server using real native transport; synthetic credentials, no paid OpenAI calls.';
  let app;
  let driver;
  let launch = 0;
  const logs = [];
  async function stop(signal = 'SIGTERM') {
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
    const environment = (await readFile(`/proc/${child.pid}/environ`, 'utf8')).split('\0');
    assert.ok(environment.includes(`DISPLAY=${process.env.DISPLAY}`), 'The test app is connected to a different display');
    assert.ok(environment.includes('GDK_BACKEND=x11'), 'The test app has not selected X11');
    driver = await new WebDriver(port).start(() => child.exitCode === null && child.signalCode === null);
    assert.equal(driver.capabilities.platformName, 'linux');
    assert.equal(driver.capabilities.browserName, 'WebKitGTK');
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
    if (['generation', 'identity'].includes(process.env.DESEGNI_NATIVE_TEST_SCOPE)) {
      report.scope = 'Focused generation workflow; authoring has separate full-workflow evidence.';
      await step('Create and save a real native project for focused generation verification', async () => {
        await driver.button('New blank page');
        await driver.fill('Project name', 'Native generation verification');
        await driver.button('Mobile');
        await driver.button('Create project');
        await waitFor(() => driver.execute(() => !!document.querySelector('[data-testid="canvas"]')), 'The focused canvas did not mount');
        assert.equal((await driver.invoke('get_workspace')).project.pages[0].appearance, 'light');
        await driver.select('Page appearance', 'dark');
        await waitFor(async () => (await driver.invoke('get_workspace')).project.pages[0].appearance === 'dark', 'The focused page did not switch to Dark');
        await driver.press('[data-testid="canvas"]', '\uE007');
        await driver.fill('Request 1', 'Create a native interactive button and explain its behavior.');
        await waitFor(async () => (await driver.invoke('get_workspace')).project.pages[0].regions[0].prompt, 'The focused request did not persist');
        await physical.initialize(evidence);
        const destination = join(evidence, 'focused site.desegni');
        await chooseFile(physical, () => driver.button('Save'), destination, evidence, 'focused-save');
        await savedState(driver, destination);
        await driver.fill('Request 1', 'Create a native interactive button and explain its behavior. Preserve the requested appearance.');
        project = (await driver.invoke('get_workspace')).project;
      });
    } else {
      project = await authorProject(driver, step, evidence);
      try { project = await physicalAuthoring(driver, physical, step, evidence); }
      finally { await physical.cleanup(); }
      project = await interactionWorkflow(driver, physical, step, evidence);
      project = await editorActions(driver, physical, step);
      project = await migrationWorkflow(driver, physical, step, evidence);
      project = await schema2MigrationWorkflow(driver, physical, step, evidence);
      project = await fileDialogWorkflow(driver, physical, step, evidence);
    }
    if (process.env.DESEGNI_NATIVE_TEST_SCOPE === 'identity') {
      report.scope = 'Focused identity workflow using a real native source generation and private session key.';
      await step('Prepare a real generated source project for focused identity import', async () => {
        await driver.invoke('set_ai_key', { key: provider.key(), storage: 'session' });
        provider.setMode('complete'); await driver.button('Generate');
        await waitFor(async () => (await driver.invoke('get_workspace')).project.pages[0].content, 'The source website did not generate', 25000);
        await waitFor(() => driver.execute(() => !document.querySelector('dialog[open]')), 'The source generation progress window did not close');
      });
    } else {
      project = await generationWorkflow(driver, physical, step, evidence, provider);
      project = await chatgptWorkflow(driver, physical, step, evidence, provider);
    }
    project = await identityWorkflow(driver, physical, step, evidence, provider);
    await step('The native editor workflow reports no browser errors or unhandled rejections', async () => {
      const errors = await driver.execute(() => window.__desegniTestErrors);
      await writeFile(join(evidence, 'browser-errors.json'), JSON.stringify(errors, null, 2));
      assert.deepEqual(errors, []);
    });
    await writeFile(join(evidence, 'authored.png'), Buffer.from(await driver.screenshot(), 'base64'));
    await writeFile(join(evidence, 'authored-project.json'), JSON.stringify(project, null, 2));
    await step('Crash the native app with SIGKILL before any session teardown', () => stop('SIGKILL'));
    const submittedCount = provider.generationRequests().length;
    const identitySubmittedCount = provider.identityRequests().length;
    await recoverProject(await start(), project, step);
    await step('Restart keeps imported identity without retaining the session key or repeating analysis or generation', async () => {
      const ai = await driver.invoke('get_ai_settings');
      assert.equal(ai.credential.configured, false);
      assert.equal(ai.credential.storage, 'none');
      assert.equal(ai.authentication, process.env.DESEGNI_NATIVE_TEST_SCOPE === 'identity' ? 'apiKey' : 'chatgpt');
      assert.ok(ai.chatgpt.accounts.length >= (process.env.DESEGNI_NATIVE_TEST_SCOPE === 'identity' ? 0 : 2));
      assert.equal(ai.chatgpt.accounts.some(account => account.connected || account.planEnabled), false);
      assert.equal(ai.chatgpt.accounts.some(account => account.storage !== 'none'), false);
      if (process.env.DESEGNI_NATIVE_TEST_SCOPE !== 'identity') assert.deepEqual(ai.catalogue, []);
      const job = await driver.invoke('get_generation', { projectId: project.id });
      assert.equal(job, null, 'The blank imported project must not inherit another project\'s generation');
      const identity = await driver.invoke('get_identity_analysis');
      assert.equal(identity.status, 'complete');
      assert.equal(identity.source.sha256, project.importedIdentity.sourceSha256);
      assert.equal(provider.identityRequests().length, identitySubmittedCount);
      assert.equal(provider.generationRequests().length, submittedCount);
    });
    const recoveryErrors = await driver.execute(() => window.__desegniTestErrors);
    await writeFile(join(evidence, 'recovery-browser-errors.json'), JSON.stringify(recoveryErrors, null, 2));
    assert.deepEqual(recoveryErrors, []);
    await writeFile(join(evidence, 'recovered.png'), Buffer.from(await driver.screenshot(), 'base64'));
    report.passed = true;
  } catch (error) {
    report.error = error.stack;
    if (driver) {
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
  }
  process.exitCode = report.passed ? 0 : 1;
}

async function main() {
  if (args.includes('--help')) {
    console.log('node scripts/native-test.mjs [--skip-build] [--generation-only|--identity-only] [--binary /absolute/test-build/desegnos]\nBuilds the explicit native-tests feature, then runs real native UI and crash recovery checks on a guarded isolated Xvfb display. --generation-only covers generation, direct editing, accounts, identity import and recovery. --identity-only covers a generated source project, identity import and recovery.');
    return;
  }
  if (process.platform !== 'linux') throw new Error('This runner requires the guarded Linux Xvfb route. Native Windows/macOS isolation must be configured on those runners.');
  const skipBuild = args.includes('--skip-build');
  const binaryIndex = args.indexOf('--binary');
  if (binaryIndex !== -1 && !args[binaryIndex + 1]) throw new Error('--binary needs an absolute file path');
  const known = new Set(['--skip-build', '--generation-only', '--identity-only', '--binary', ...(binaryIndex === -1 ? [] : [args[binaryIndex + 1]])]);
  for (const argument of args) if (!known.has(argument)) throw new Error(`Unknown argument ${argument}`);
  if (args.includes('--identity-only')) process.env.DESEGNI_NATIVE_TEST_SCOPE = 'identity';
  if (args.includes('--generation-only')) process.env.DESEGNI_NATIVE_TEST_SCOPE = 'generation';
  process.chdir(root);
  const target = resolve('target/native-tests');
  const binary = resolve(binaryIndex === -1 ? join(target, 'debug/desegni') : args[binaryIndex + 1]);
  if (!skipBuild) {
    const build = spawn(process.execPath, ['scripts/tauri.mjs', 'build', '--debug', '--no-bundle', '--features', 'native-tests', '--config', 'src-tauri/tauri.test.conf.json'],
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
  const original = await desktopState();
  let result = 1;
  let desktop;
  try {
    const isolated = spawn(guardedXvfb, ['-d', '-e', join(evidence, 'xvfb.log'), '-f', join(scratch, 'Xauthority'), '-s', '-screen 0 1600x1100x24',
      'dbus-run-session', '--', process.execPath, fileURLToPath(import.meta.url), '--isolated'], {
      cwd: root, detached: true, stdio: 'inherit', env: { ...process.env,
        XDG_DATA_HOME: profile.data, XDG_CONFIG_HOME: profile.config, XDG_CACHE_HOME: profile.cache, XDG_RUNTIME_DIR: profile.runtime,
        GDK_SCALE: '1', GDK_DPI_SCALE: '1',
        DESEGNI_DESKTOP_DISPLAY: process.env.DISPLAY ?? '', DESEGNI_NATIVE_TEST_BINARY: binary, DESEGNI_NATIVE_TEST_EVIDENCE: evidence },
    });
    const interrupt = () => terminateGroup(isolated, 'SIGTERM');
    process.once('SIGINT', interrupt); process.once('SIGTERM', interrupt);
    try { result = await childExit(isolated); }
    finally { process.removeListener('SIGINT', interrupt); process.removeListener('SIGTERM', interrupt); }
  } finally {
    await rm(scratch, { recursive: true, force: true });
    desktop = await verifyDesktop(original);
    await writeFile(join(evidence, 'desktop-cleanup.json'), JSON.stringify(desktop, null, 2));
  }
  console.log(`Native evidence: ${evidence}`);
  process.exitCode = result;
}

try { if (args.length === 1 && args[0] === '--isolated') await runIsolated(); else await main(); }
catch (error) { console.error(error.stack); process.exitCode = 1; }
