import assert from 'node:assert/strict';
import test from 'node:test';
import { createHash } from 'node:crypto';
import { lstat, mkdir, mkdtemp, readFile, readlink, rm, symlink, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { completeLaunchPaths, prepareLaunchPaths, restoreLaunchPaths } from '../scripts/launch-paths.mjs';

const parent = fileURLToPath(new URL('../test-results/', import.meta.url));
await mkdir(parent, { recursive: true });
async function fixture(run) {
  const directory = await mkdtemp(join(parent, 'launch-paths-'));
  try { await run(directory); } finally { await rm(directory, { recursive: true, force: true }); }
}

test('migration removes the old reverse link before build and preserves a byte-exact rollback', () => fixture(async directory => {
  const old = join(directory, 'desegni'), canonical = join(directory, 'desegnos');
  await writeFile(old, 'previous owned executable'); await symlink('desegni', canonical);
  const plan = await prepareLaunchPaths(directory);
  await assert.rejects(lstat(canonical), { code: 'ENOENT' });
  assert.equal(await readFile(old, 'utf8'), 'previous owned executable');
  await writeFile(canonical, 'new canonical executable');
  const result = await completeLaunchPaths(plan);
  assert.equal((await lstat(canonical)).isSymbolicLink(), false);
  assert.equal(await readlink(old), 'desegnos');
  assert.equal(await readFile(old, 'utf8'), 'new canonical executable');
  assert.equal(await readFile(result.backup, 'utf8'), 'previous owned executable');
}));

test('a failed build restores the original product launch link', () => fixture(async directory => {
  await writeFile(join(directory, 'desegni'), 'previous executable'); await symlink('desegni', join(directory, 'desegnos'));
  await restoreLaunchPaths(await prepareLaunchPaths(directory));
  assert.equal(await readlink(join(directory, 'desegnos')), 'desegni');
  assert.equal(await readFile(join(directory, 'desegnos'), 'utf8'), 'previous executable');
}));

test('an interrupted migration resumes from its verified ownership receipt', () => fixture(async directory => {
  await writeFile(join(directory, 'desegni'), 'previous executable'); await symlink('desegni', join(directory, 'desegnos'));
  await prepareLaunchPaths(directory); // Simulate process exit before Cargo starts.
  const resumed = await prepareLaunchPaths(directory);
  await writeFile(resumed.canonical, 'resumed build'); await completeLaunchPaths(resumed);
  assert.equal(await readFile(resumed.legacy, 'utf8'), 'resumed build'); assert.equal(await readlink(resumed.legacy), 'desegnos');
}));

test('clean and repeated builds use a regular canonical executable and converge on one legacy link', () => fixture(async directory => {
  for (const version of ['first', 'second']) {
    const plan = await prepareLaunchPaths(directory); await writeFile(plan.canonical, version); await completeLaunchPaths(plan);
    assert.equal((await lstat(plan.canonical)).isSymbolicLink(), false);
    assert.equal(await readlink(plan.legacy), 'desegnos'); assert.equal(await readFile(plan.legacy, 'utf8'), version);
  }
}));

test('unrelated binaries and links are rejected without modifying them', () => fixture(async directory => {
  await writeFile(join(directory, 'desegni'), 'unrelated file');
  await assert.rejects(prepareLaunchPaths(directory), /unrecognized executable/);
  assert.equal(await readFile(join(directory, 'desegni'), 'utf8'), 'unrelated file');
  await writeFile(join(directory, 'another-app'), 'another application'); await symlink('another-app', join(directory, 'desegnos'));
  await assert.rejects(prepareLaunchPaths(directory), /unrelated launch link/);
  assert.equal(await readlink(join(directory, 'desegnos')), 'another-app'); assert.equal(await readFile(join(directory, 'another-app'), 'utf8'), 'another application');
}));

test('a compatibility executable changed during build is preserved', () => fixture(async directory => {
  await writeFile(join(directory, 'desegni'), 'owned old build'); await symlink('desegni', join(directory, 'desegnos'));
  const plan = await prepareLaunchPaths(directory); await writeFile(plan.canonical, 'new build'); await writeFile(plan.legacy, 'a concurrent replacement');
  await assert.rejects(completeLaunchPaths(plan), /changed during the build/);
  assert.equal(await readFile(plan.legacy, 'utf8'), 'a concurrent replacement');
}));

test('Windows generated-copy receipts permit migration and safe subsequent refresh', () => fixture(async directory => {
  const old = join(directory, 'desegni.exe'), canonical = join(directory, 'desegnos.exe');
  await writeFile(old, 'previous executable'); await writeFile(canonical, 'previous executable');
  await writeFile(`${canonical}.sha256`, createHash('sha256').update('previous executable').digest('hex'));
  const plan = await prepareLaunchPaths(directory, true); await writeFile(canonical, 'new Windows build');
  const result = await completeLaunchPaths(plan); assert.equal(await readFile(result.backup, 'utf8'), 'previous executable');
  assert.equal((await lstat(old)).isSymbolicLink(), false); assert.equal(await readFile(old, 'utf8'), 'new Windows build');
  const next = await prepareLaunchPaths(directory, true); await writeFile(canonical, 'next Windows build'); await completeLaunchPaths(next);
  assert.equal(await readFile(old, 'utf8'), 'next Windows build');
}));
