import assert from 'node:assert/strict';
import test from 'node:test';
import { join } from 'node:path';
import { buildRelease, localProfile } from '../scripts/build-release.mjs';

// Exercise orchestration without invoking Cargo, building Rust, or launching an app.
for (const [platform, target, executable, startup] of [
  ['win32', undefined, 'desegnos.exe', false],
  ['darwin', undefined, 'desegnos', false],
  ['linux', undefined, 'desegnos', true],
  ['linux', 'x86_64-pc-windows-msvc', 'desegnos.exe', false],
]) {
  test(`local release on ${platform}, target ${target ?? 'host'}`, async () => {
    const calls = [], inspected = [];
    const env = target ? { CARGO_BUILD_TARGET: target } : {};
    const run = async (program, args, options) => {
      calls.push({ program, args, options });
      return JSON.stringify({ target_directory: join('custom target', 'directory') });
    };
    await buildRelease({ platform, env, run, inspect: async path => {
      inspected.push(path); return { size: 123, mtime: new Date(0) };
    } });
    assert.equal(calls[0].program, process.execPath);
    assert.match(calls[0].args[0], /run\.mjs$/);
    assert.deepEqual(calls[0].args.slice(1), ['tauri', 'build', '--no-bundle', '--features', 'custom-protocol']);
    assert.equal(calls[1].program, 'cargo');
    assert.deepEqual(calls[1].args, ['metadata', '--offline', '--no-deps', '--format-version', '1']);
    const binary = join('custom target', 'directory', ...(target ? [target] : []), 'release', executable);
    assert.deepEqual(inspected, [binary]);
    assert.equal(calls.length, startup ? 3 : 2);
    if (startup) assert.deepEqual(calls[2].args.slice(1), ['--binary', binary]);
    assert.deepEqual(calls[0].options.env, { ...env, ...localProfile });
    for (const call of calls.slice(1)) assert.equal(call.options.env, env);
  });
}

test('a distribution build keeps the full release profile', async () => {
  const calls = [], env = {};
  await buildRelease({ platform: 'darwin', env, dist: true, inspect: async () => ({ size: 1, mtime: new Date(0) }),
    run: async (program, args, options) => { calls.push(options.env); return JSON.stringify({ target_directory: 'target' }); } });
  for (const used of calls) assert.equal(used, env);
});

test('a failed release build stops before inspecting or launching an executable', async () => {
  let calls = 0;
  await assert.rejects(buildRelease({ env: {}, run: async () => {
    calls++; throw new Error('build failed');
  }, inspect: async () => assert.fail('must not inspect a stale binary') }), /build failed/);
  assert.equal(calls, 1);
});
