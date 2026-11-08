import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { createWriteStream } from 'node:fs';
import { access, chmod, mkdir, mkdtemp, readdir, readFile, readlink, realpath, rm, stat, writeFile } from 'node:fs/promises';
import { createConnection } from 'node:net';
import { isAbsolute, join, resolve } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';
import { alive, assertPrivateXdg, checkDesktop, createPrivateXdg, desktopState, exit, groupExists, hashFile, signalGroup, stopGroup, unusedPort } from './native-guard.mjs';
import { resolveDbusRunSession, resolveXvfbRun, spawnXvfbSession } from './xvfb.mjs';

const root = fileURLToPath(new URL('../', import.meta.url));
const script = fileURLToPath(import.meta.url);
const helper = join(root, 'tests/native/x11-input.py');

async function command(program, args, { input, timeout = 5000 } = {}) {
  const child = spawn(program, args, { stdio: ['pipe', 'pipe', 'pipe'] });
  let stdout = '', stderr = '';
  child.stdout.on('data', chunk => { stdout += chunk; if (stdout.length > 512_000) child.kill('SIGKILL'); });
  child.stderr.on('data', chunk => { stderr += chunk; if (stderr.length > 32_000) child.kill('SIGKILL'); });
  child.stdin.end(input);
  const timer = setTimeout(() => child.kill('SIGKILL'), timeout);
  try { assert.equal(await exit(child), 0, `${program} failed: ${stderr}`); return stdout; }
  finally { clearTimeout(timer); }
}

async function socketOwner(path) {
  const inodes = new Set((await readFile('/proc/net/unix', 'utf8')).split('\n').slice(1)
    .map(line => line.trim().split(/\s+/)).filter(fields => fields[7] === path || fields[7] === `@${path}`).map(fields => `socket:[${fields[6]}]`));
  for (const pid of (await readdir('/proc')).filter(name => /^\d+$/.test(name))) {
    let fds;
    try { fds = await readdir(`/proc/${pid}/fd`); } catch { continue; }
    for (const fd of fds) {
      try { if (inodes.has(await readlink(`/proc/${pid}/fd/${fd}`))) return Number(pid); } catch { /* the descriptor closed */ }
    }
  }
  return null;
}

async function guardLockOwners(information) {
  const device = BigInt(information.dev);
  const major = ((device >> 8n) & 0xfffn) | ((device >> 32n) & 0xfffff000n);
  const minor = (device & 0xffn) | ((device >> 12n) & 0xffffff00n);
  return [...new Set((await readFile('/proc/locks', 'utf8')).split('\n').flatMap(line => {
    const match = /^\d+:\s+FLOCK\s+ADVISORY\s+WRITE\s+(\d+)\s+([\da-f]+):([\da-f]+):(\d+)\s+0\s+EOF$/i.exec(line);
    return match && BigInt(`0x${match[2]}`) === major && BigInt(`0x${match[3]}`) === minor && BigInt(match[4]) === BigInt(information.ino) ? [Number(match[1])] : [];
  }))];
}

function tcpState(host, port) {
  return new Promise(accept => {
    const socket = createConnection({ host, port });
    let completed = false;
    const finish = result => { if (completed) return; completed = true; socket.destroy(); accept(result); };
    socket.once('connect', () => finish('open'));
    socket.once('error', error => finish(error.code === 'ECONNREFUSED' ? 'refused' : error.code));
    socket.setTimeout(300, () => finish('timeout'));
  });
}

async function isolated() {
  const scratch = process.env.DESEGNOS_STARTUP_SCRATCH;
  const evidence = process.env.DESEGNOS_STARTUP_EVIDENCE;
  const binary = process.env.DESEGNOS_STARTUP_BINARY;
  assert.ok(scratch && evidence && binary && process.env.DESEGNOS_STARTUP_TOKEN, 'Missing private startup guard');
  assert.match(scratch, /^\/tmp\/desegnos-startup-[^/]+$/);
  assert.equal(await realpath(scratch), scratch);
  const information = await stat(scratch);
  assert.ok(information.uid === process.getuid() && (information.mode & 0o077) === 0, 'Unsafe private startup storage');
  await assertPrivateXdg(scratch);
  const display = process.env.DISPLAY?.match(/^:(\d+)(?:\.\d+)?$/);
  assert.ok(display && Number(display[1]) >= 90 && process.env.DISPLAY !== process.env.DESEGNOS_DESKTOP_DISPLAY, 'Refusing a desktop display');
  assert.equal(process.env.GDK_BACKEND, 'x11');
  for (const variable of ['WAYLAND_DISPLAY', 'WAYLAND_SOCKET']) assert.equal(process.env[variable], undefined);
  assert.ok(process.env.DBUS_SESSION_BUS_ADDRESS && process.env.DBUS_SESSION_BUS_ADDRESS !== process.env.DESEGNOS_STARTUP_PREVIOUS_BUS, 'Missing isolated session bus');
  assert.ok((await readlink(`/proc/${process.ppid}/exe`)).endsWith('/dbus-run-session'), 'Startup must run inside its private dbus-run-session');
  const socketPath = `/tmp/.X11-unix/X${display[1]}`;
  const lockPath = `/tmp/.X${display[1]}-lock`;
  // A display wrapper that reserves per-display lock files can name their directory inside the
  // private runtime directory here; a plain xvfb-run has none.
  const guardDirectory = process.env.DESEGNOS_XVFB_GUARD_DIR;
  const guardLockPath = guardDirectory ? join(process.env.XDG_RUNTIME_DIR, guardDirectory, display[1]) : null;
  const displayResources = {};
  for (const path of [socketPath, lockPath, guardLockPath].filter(Boolean)) {
    try { const item = await stat(path); assert.equal(item.uid, process.getuid()); displayResources[path] = { exists: true, dev: item.dev, ino: item.ino }; }
    catch (error) { if (error.code !== 'ENOENT') throw error; displayResources[path] = { exists: false }; }
  }
  const ownership = { display: process.env.DISPLAY, serverPid: null, socketPath, lockPath, guardLockPath, displayResources, ownershipVerified: false };
  await writeFile(join(scratch, 'display.json'), JSON.stringify(ownership));
  assert.ok(displayResources[socketPath].exists, 'The owned X socket is missing');
  // The server is whichever process holds the display's listening socket; lock files are optional
  // (a guarded Xvfb may run with -nolock), but any that exist must name the same process.
  const serverPid = await socketOwner(socketPath);
  assert.ok(serverPid, 'No process of this user owns the isolated X socket');
  const guarded = Boolean(guardLockPath && displayResources[guardLockPath].exists);
  ownership.guarded = guarded;
  if (guarded) assert.deepEqual(await guardLockOwners(displayResources[guardLockPath]), [serverPid], 'The private Xvfb guard lock is not held by the X server');
  if (displayResources[lockPath].exists) assert.equal(Number((await readFile(lockPath, 'utf8')).trim()), serverPid, 'The X lock file names another process');
  assert.equal((await stat(`/proc/${serverPid}`)).uid, process.getuid(), 'Xvfb belongs to another user');
  const serverEnvironment = (await readFile(`/proc/${serverPid}/environ`, 'utf8')).split('\0');
  const serverArguments = (await readFile(`/proc/${serverPid}/cmdline`, 'utf8')).split('\0');
  for (const variable of ['XDG_RUNTIME_DIR', 'XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME']) assert.ok(serverEnvironment.includes(`${variable}=${process.env[variable]}`), 'Xvfb does not belong to the private XDG session');
  assert.ok(serverArguments.includes(`:${display[1]}`) && /\/Xvfb(?:\.real)?$/.test(await readlink(`/proc/${serverPid}/exe`)), 'The isolated display is not owned Xvfb');
  await writeFile(join(scratch, 'display.json'), JSON.stringify({ ...ownership, serverPid, ownershipVerified: true }));
  const connectionDeadline = Date.now() + 10_000;
  let connected = false;
  while (Date.now() < connectionDeadline) {
    try { await command('xprop', ['-root', '_NET_SUPPORTING_WM_CHECK']); connected = true; break; }
    catch { await delay(100); }
  }
  assert.ok(connected, 'The owned Xvfb display did not accept an actual client connection');
  const config = JSON.parse(await readFile(join(root, 'src-tauri/tauri.conf.json'), 'utf8')).app.windows.find(window => window.label === 'editor');
  assert.ok(config, 'The normal editor configuration is missing');
  const report = { passed: false, binary, binarySha256: await hashFile(binary), display: process.env.DISPLAY, serverPid, displayResources,
    startup: 'Fresh normal binary with private XDG storage and no automation client or invoke calls.',
    screenshot: 'startup.png', visualReview: 'Required: inspect the real screenshot for a blank untitled canvas with the tool strip and dock, with no start screen. No DOM assertions are claimed.',
    webdriver: { port: await unusedPort(), samples: [] }, startedAt: new Date().toISOString() };
  const log = createWriteStream(join(evidence, 'app.log'), { flags: 'wx' });
  let app;
  let monitor;
  let probeFailure;
  let probing = false;
  async function rejectDriver() {
    if (probing) return;
    probing = true;
    try {
      const results = await Promise.all(['127.0.0.1', '::1'].map(async host => ({ host, state: await tcpState(host, report.webdriver.port) })));
      report.webdriver.samples.push({ at: new Date().toISOString(), results });
      for (const item of results) assert.equal(item.state, 'refused', `The normal binary WebDriver port is ${item.state} on ${item.host}`);
    } catch (error) { probeFailure = error; }
    finally { probing = false; }
  }
  const interrupted = () => { if (app?.pid) signalGroup(app.pid, 'SIGTERM'); };
  process.once('SIGINT', interrupted); process.once('SIGTERM', interrupted);
  try {
    await rejectDriver(); if (probeFailure) throw probeFailure;
    app = spawn(binary, [], { cwd: root, env: { ...process.env, TAURI_WEBDRIVER_PORT: String(report.webdriver.port) }, detached: true, stdio: ['ignore', 'pipe', 'pipe'] });
    const streamsClosed = new Promise(accept => app.once('close', accept));
    app.streamsClosed = streamsClosed;
    app.stdout.pipe(log, { end: false }); app.stderr.pipe(log, { end: false });
    await new Promise((accept, reject) => { app.once('spawn', accept); app.once('error', reject); });
    await writeFile(join(scratch, 'app.json'), JSON.stringify({ pid: app.pid, token: process.env.DESEGNOS_STARTUP_TOKEN }));
    const environment = (await readFile(`/proc/${app.pid}/environ`, 'utf8')).split('\0');
    assert.ok(environment.includes(`DISPLAY=${process.env.DISPLAY}`) && environment.includes('GDK_BACKEND=x11'), 'The app is not on the owned Xvfb display');
    assert.equal(await hashFile(`/proc/${app.pid}/exe`), report.binarySha256, 'The running binary differs from the requested binary');
    report.appPid = app.pid;
    monitor = setInterval(() => { void rejectDriver(); }, 75);
    const deadline = Date.now() + 30_000;
    let window;
    while (Date.now() < deadline) {
      assert.ok(alive(app), 'The normal native binary exited before its window appeared');
      if (probeFailure) throw probeFailure;
      try { window = JSON.parse(await command('python3', [helper], { input: JSON.stringify({ action: 'inspect', pid: app.pid }) })); break; }
      catch { await delay(150); }
    }
    assert.ok(window, 'No owned native editor window appeared');
    assert.equal(window.display, process.env.DISPLAY);
    assert.equal(window.window.pid, app.pid);
    assert.equal(window.window.title, config.title);
    assert.equal(window.window.width, config.width);
    assert.equal(window.window.height, config.height);
    report.window = window;
    const stableUntil = Date.now() + 5000;
    while (Date.now() < stableUntil) { assert.ok(alive(app), 'Normal native startup did not stay alive'); if (probeFailure) throw probeFailure; await delay(100); }
    const owned = JSON.parse(await command('python3', [helper], { input: JSON.stringify({ action: 'inspect', pid: app.pid, windowId: window.window.id }) }));
    assert.deepEqual(owned.window, window.window, 'The startup window changed unexpectedly during the liveness check');
    await command('import', ['-display', process.env.DISPLAY, '-window', String(window.window.id), join(evidence, 'startup.png')], { timeout: 10_000 });
    const png = await readFile(join(evidence, 'startup.png'));
    assert.ok(png.subarray(0, 8).equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10])), 'The owned-window screenshot is not PNG');
    report.image = { width: png.readUInt32BE(16), height: png.readUInt32BE(20), sha256: await hashFile(join(evidence, 'startup.png')) };
    assert.equal(report.image.width, window.window.width); assert.equal(report.image.height, window.window.height);
    await rejectDriver(); if (probeFailure) throw probeFailure;
    assert.ok(alive(app)); report.passed = true;
  } catch (error) { report.error = error.stack; }
  finally {
    clearInterval(monitor);
    while (probing) await delay(20);
    if (probeFailure) { report.passed = false; report.error = probeFailure.stack; }
    if (app?.pid) {
      try { await stopGroup(app.pid); await exit(app); await app.streamsClosed; report.appStopped = !alive(app); }
      catch (error) { report.passed = false; report.error ??= error.stack; }
      app.stdout.unpipe(log); app.stderr.unpipe(log);
    }
    await new Promise(accept => log.end(accept));
    process.removeListener('SIGINT', interrupted); process.removeListener('SIGTERM', interrupted);
    report.finishedAt = new Date().toISOString();
    await writeFile(join(evidence, 'report.json'), JSON.stringify(report, null, 2) + '\n');
  }
  process.exitCode = report.passed ? 0 : 1;
}

async function main() {
  const args = process.argv.slice(2);
  if (args.length === 1 && args[0] === '--help') { console.log('node scripts/startup-test.mjs --binary /absolute/path/to/normal/desegnos\nLaunches only on guarded Xvfb; saves the real startup window and rejects an enabled WebDriver port.'); return; }
  assert.equal(process.platform, 'linux', 'Normal startup verification requires the guarded Linux Xvfb route');
  assert.ok(args.length === 2 && args[0] === '--binary' && isAbsolute(args[1]), 'Provide --binary with an absolute normal-build executable path');
  const binary = await realpath(args[1]);
  resolveXvfbRun(); resolveDbusRunSession();
  await access(binary); await access(helper);
  const evidence = resolve(root, 'test-results/startup', `${new Date().toISOString().replace(/[:.]/g, '-')}-${process.pid}`);
  await mkdir(evidence, { recursive: true });
  const scratch = await mkdtemp('/tmp/desegnos-startup-'); await chmod(scratch, 0o700);
  const environment = { ...process.env };
  for (const variable of Object.keys(environment)) if (variable.startsWith('DBUS_') || variable.startsWith('GNOME_KEYRING_') || variable.startsWith('DESEGNOS_NATIVE_TEST_') || variable.startsWith('DESEGNOS_TEST_') || variable.startsWith('DESEGNOS_STARTUP_') || variable.startsWith('TAURI_WEBDRIVER_')) delete environment[variable];
  for (const variable of ['DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'XAUTHORITY', 'SSH_AUTH_SOCK', 'OPENAI_API_KEY']) delete environment[variable];
  await createPrivateXdg(scratch, environment);
  Object.assign(environment, { GDK_SCALE: '1', GDK_DPI_SCALE: '1',
    DESEGNOS_DESKTOP_DISPLAY: process.env.DISPLAY ?? '', DESEGNOS_STARTUP_PREVIOUS_BUS: process.env.DBUS_SESSION_BUS_ADDRESS ?? '', DESEGNOS_STARTUP_BINARY: binary,
    DESEGNOS_STARTUP_EVIDENCE: evidence, DESEGNOS_STARTUP_SCRATCH: scratch, DESEGNOS_STARTUP_TOKEN: randomBytes(24).toString('hex') });
  const desktop = await desktopState();
  let session;
  let result = 1;
  let timedOut = false;
  let failure;
  const interrupted = () => { if (session?.pid) signalGroup(session.pid, 'SIGTERM'); };
  process.once('SIGINT', interrupted); process.once('SIGTERM', interrupted);
  try {
    session = spawnXvfbSession({ script, evidence, scratch, env: environment, cwd: root });
    let forceTimer;
    const timer = setTimeout(() => { timedOut = true; signalGroup(session.pid, 'SIGTERM'); forceTimer = setTimeout(() => signalGroup(session.pid, 'SIGKILL'), 2000); }, 70_000);
    try { result = await exit(session); } finally { clearTimeout(timer); clearTimeout(forceTimer); }
    assert.equal(timedOut, false, 'The isolated normal-startup check timed out');
  } catch (error) { failure = error.stack; }
  finally {
    const cleanup = { scratchRemoved: false, virtualDisplayStopped: false, app: 'not started', appStopped: null, sessionStopped: false };
    try {
      const owned = JSON.parse(await readFile(join(scratch, 'app.json'), 'utf8'));
      assert.equal(owned.token, environment.DESEGNOS_STARTUP_TOKEN);
      await stopGroup(owned.pid); cleanup.appStopped = !groupExists(owned.pid); cleanup.app = cleanup.appStopped ? 'stopped' : 'still running';
    } catch (error) { if (error.code !== 'ENOENT') failure ??= error.stack; }
    if (session?.pid) { try { await stopGroup(session.pid); cleanup.sessionStopped = !groupExists(session.pid); } catch (error) { failure ??= error.stack; } }
    try {
      const display = JSON.parse(await readFile(join(scratch, 'display.json'), 'utf8'));
      cleanup.display = display;
      for (const path of [display.socketPath, display.lockPath]) {
        try { await stat(path); throw new Error(`Owned virtual display resource remains: ${path}`); }
        catch (error) { if (error.code !== 'ENOENT') throw error; }
      }
      const guard = display.guardLockPath && display.displayResources[display.guardLockPath];
      if (guard?.exists) {
        try { const current = await stat(display.guardLockPath); assert.equal(current.dev, guard.dev); assert.equal(current.ino, guard.ino); }
        catch (error) { if (error.code !== 'ENOENT') throw error; }
        assert.deepEqual(await guardLockOwners(guard), [], 'The private Xvfb guard lock remains held after cleanup');
        cleanup.guardLockReleased = true;
      } else cleanup.guardLockReleased = 'not created';
      cleanup.virtualDisplayStopped = true;
    } catch (error) { if (error.code !== 'ENOENT') failure ??= error.stack; }
    try { await rm(scratch, { recursive: true, force: true }); cleanup.scratchRemoved = true; } catch (error) { failure ??= error.stack; }
    try { cleanup.desktop = await checkDesktop(desktop); } catch (error) { failure ??= error.stack; }
    cleanup.passed = !failure && cleanup.scratchRemoved && cleanup.virtualDisplayStopped && cleanup.appStopped !== false && cleanup.sessionStopped;
    if (failure) cleanup.error = failure;
    await writeFile(join(evidence, 'cleanup.json'), JSON.stringify(cleanup, null, 2) + '\n');
    process.removeListener('SIGINT', interrupted); process.removeListener('SIGTERM', interrupted);
    if (!cleanup.passed) result = 1;
  }
  console.log(`Normal startup evidence: ${evidence}`);
  if (failure) console.error(failure);
  process.exitCode = result;
}

try { if (process.argv.length === 3 && process.argv[2] === '--isolated') await isolated(); else await main(); }
catch (error) { console.error(error.stack); process.exitCode = 1; }
