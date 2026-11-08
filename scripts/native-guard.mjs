// Shared guards for the native test scripts: owned process groups, private XDG storage and
// proof that the developer's own desktop X11 session was left alone.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { mkdir, realpath, stat } from 'node:fs/promises';
import { createServer } from 'node:net';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';

export const XDG_DIRECTORIES = { XDG_DATA_HOME: 'data', XDG_CONFIG_HOME: 'config', XDG_CACHE_HOME: 'cache', XDG_RUNTIME_DIR: 'runtime' };

export function alive(child) { return child.exitCode === null && child.signalCode === null; }

export function exit(child) {
  if (!alive(child)) return Promise.resolve(child.exitCode ?? 1);
  return new Promise((accept, reject) => { child.once('error', reject); child.once('exit', code => accept(code ?? 1)); });
}

export function signalGroup(pid, signal) {
  try { process.kill(-pid, signal); } catch (error) { if (error.code !== 'ESRCH') throw error; }
}

export function groupExists(pid) {
  try { process.kill(-pid, 0); return true; } catch (error) { if (error.code === 'ESRCH') return false; throw error; }
}

export async function stopGroup(pid) {
  if (!pid || !groupExists(pid)) return;
  signalGroup(pid, 'SIGTERM');
  const deadline = Date.now() + 2000;
  while (groupExists(pid) && Date.now() < deadline) await delay(40);
  if (groupExists(pid)) {
    signalGroup(pid, 'SIGKILL');
    const killed = Date.now() + 2000;
    while (groupExists(pid) && Date.now() < killed) await delay(40);
    assert.equal(groupExists(pid), false, `Owned process group ${pid} remains after cleanup`);
  }
}

export async function hashFile(path) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest('hex');
}

export async function unusedPort() {
  const server = createServer();
  await new Promise((accept, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', accept); });
  const port = server.address().port;
  await new Promise((accept, reject) => server.close(error => error ? reject(error) : accept()));
  return port;
}

// Creates the private XDG directories under `scratch` and points `environment` at them.
export async function createPrivateXdg(scratch, environment) {
  for (const [variable, directory] of Object.entries(XDG_DIRECTORIES)) {
    environment[variable] = join(scratch, directory);
    await mkdir(environment[variable], { mode: 0o700 });
  }
}

// Every XDG variable must resolve to its private directory under `scratch`, owned by this
// user and closed to others. `processEnvironment` (NAME=value lines) also proves that a
// spawned helper process was started inside the same private session.
export async function assertPrivateXdg(scratch, processEnvironment) {
  for (const [variable, directory] of Object.entries(XDG_DIRECTORIES)) {
    const expected = join(scratch, directory);
    assert.equal(await realpath(process.env[variable]), expected, `${variable} is not the private directory`);
    if (processEnvironment) assert.ok(processEnvironment.includes(`${variable}=${expected}`), `A helper process does not belong to the private ${variable}`);
    const item = await stat(expected);
    assert.ok(item.uid === process.getuid() && (item.mode & 0o077) === 0, `Unsafe private ${variable} storage`);
  }
}

export async function desktopState() {
  const display = process.env.DISPLAY ?? null;
  const number = display?.match(/^:(\d+)(?:\.\d+)?$/)?.[1];
  const files = [];
  for (const path of number ? [`/tmp/.X11-unix/X${number}`, `/tmp/.X${number}-lock`] : []) {
    try { const item = await stat(path); files.push({ path, exists: true, dev: item.dev, ino: item.ino }); }
    catch (error) { if (error.code !== 'ENOENT') throw error; files.push({ path, exists: false }); }
  }
  const connection = display ? spawnSync('xprop', ['-root', '_NET_SUPPORTING_WM_CHECK'], { encoding: 'utf8', timeout: 5000 }) : null;
  return { display, files, connected: connection?.status === 0 };
}

export async function checkDesktop(before) {
  const after = await desktopState();
  assert.deepEqual(after.files, before.files, 'The original desktop X socket or lock identity changed');
  if (before.connected) assert.equal(after.connected, true, 'The original desktop X11 connection failed after cleanup');
  return { before, after, resourceIdentity: 'passed', connection: before.connected ? 'passed' : 'unavailable before test' };
}
