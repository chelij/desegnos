// Resolves the Linux virtual-display tooling (xvfb-run, dbus-run-session) from PATH or an env override.
import { spawn } from 'node:child_process';
import { accessSync, constants, statSync } from 'node:fs';
import { delimiter, join } from 'node:path';

function executableFile(path) {
  try { accessSync(path, constants.X_OK); return statSync(path).isFile(); }
  catch { return false; }
}

function onPath(name) {
  for (const directory of (process.env.PATH ?? '').split(delimiter)) {
    if (!directory) continue;
    const candidate = join(directory, name);
    if (executableFile(candidate)) return candidate;
  }
  return null;
}

function resolve(name, variable, install) {
  const configured = process.env[variable];
  if (configured) {
    if (!executableFile(configured)) throw new Error(`${variable} is not an executable file: ${configured}`);
    return configured;
  }
  const found = onPath(name);
  if (!found) throw new Error(`Cannot find ${name} on PATH. ${install}, or set ${variable} to its path.`);
  return found;
}

export function resolveXvfbRun() {
  return resolve('xvfb-run', 'XVFB_RUN', 'Install xvfb (package xorg-server-xvfb on Arch, xvfb on Debian/Ubuntu)');
}

export function resolveDbusRunSession() {
  return resolve('dbus-run-session', 'DBUS_RUN_SESSION', 'Install dbus (package dbus on Arch, dbus-daemon on Debian/Ubuntu)');
}

// Drops Wayland and forces the X11 backends so GTK and Qt use the virtual display.
export function xvfbEnvironment(environment = process.env) {
  const result = { ...environment };
  delete result.WAYLAND_DISPLAY;
  delete result.WAYLAND_SOCKET;
  return Object.assign(result, { XDG_SESSION_TYPE: 'x11', GDK_BACKEND: 'x11', QT_QPA_PLATFORM: 'xcb' });
}

// Runs `node <script> --isolated` on a private Xvfb display and session bus, in its own process
// group so the caller can stop everything the check started.
export function spawnXvfbSession({ script, evidence, scratch, env, cwd }) {
  return spawn(resolveXvfbRun(), ['-d', '-e', join(evidence, 'xvfb.log'), '-f', join(scratch, 'Xauthority'), '-s', '-screen 0 1600x1100x24',
    resolveDbusRunSession(), '--', process.execPath, script, '--isolated'], { cwd, env: xvfbEnvironment(env), detached: true, stdio: ['ignore', 'inherit', 'inherit'] });
}
