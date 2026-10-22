import assert from 'node:assert/strict';
import test from 'node:test';
import { createHash } from 'node:crypto';
import { lstat, mkdir, mkdtemp, readFile, readlink, rm, symlink, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { completeLaunchPaths, prepareLaunchPaths, restoreLaunchPaths } from '../scripts/launch-paths.mjs';

const windows = process.platform === 'win32';
const symlinkOnly = { skip: windows ? 'Windows symlinks require Developer Mode or elevation; copy-based cases remain enabled.' : false };
const names = windows ? { old: 'desegni.exe', canonical: 'desegnos.exe' } : { old: 'desegni', canonical: 'desegnos' };
const hash = text => createHash('sha256').update(text).digest('hex');

async function assertAlias(plan, contents) {
  assert.equal((await lstat(plan.canonical)).isSymbolicLink(), false);
  assert.equal((await lstat(plan.legacy)).isSymbolicLink(), !plan.windows);
  if (!plan.windows) assert.equal(await readlink(plan.legacy), names.canonical);
  assert.equal(await readFile(plan.legacy, 'utf8'), contents);
}

const parent = fileURLToPath(new URL('../test-results/', import.meta.url));
await mkdir(parent, { recursive: true });
async function fixture(run) {
  const directory = await mkdtemp(join(parent, 'launch-paths-'));
  try { await run(directory); } finally { await rm(directory, { recursive: true, force: true }); }
}

test('migration removes the old reverse link before build and preserves a byte-exact rollback', symlinkOnly, () => fixture(async directory => {
  const old = join(directory, names.old), canonical = join(directory, names.canonical);
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

test('a failed build restores the original product launch link', symlinkOnly, () => fixture(async directory => {
  await writeFile(join(directory, names.old), 'previous executable'); await symlink('desegni', join(directory, names.canonical));
  await restoreLaunchPaths(await prepareLaunchPaths(directory));
  assert.equal(await readlink(join(directory, names.canonical)), 'desegni');
  assert.equal(await readFile(join(directory, names.canonical), 'utf8'), 'previous executable');
}));

test('an interrupted migration resumes from its verified ownership receipt', symlinkOnly, () => fixture(async directory => {
  await writeFile(join(directory, names.old), 'previous executable'); await symlink('desegni', join(directory, names.canonical));
  await prepareLaunchPaths(directory); // Simulate process exit before Cargo starts.
  const resumed = await prepareLaunchPaths(directory);
  await writeFile(resumed.canonical, 'resumed build'); await completeLaunchPaths(resumed);
  assert.equal(await readFile(resumed.legacy, 'utf8'), 'resumed build'); assert.equal(await readlink(resumed.legacy), 'desegnos');
}));

test('clean and repeated builds converge on the platform compatibility alias', () => fixture(async directory => {
  for (const version of ['first', 'second']) {
    const plan = await prepareLaunchPaths(directory); await writeFile(plan.canonical, version); await completeLaunchPaths(plan);
    await assertAlias(plan, version);
  }
}));

test('unrelated binaries are rejected without modifying them', () => fixture(async directory => {
  await writeFile(join(directory, names.old), 'unrelated file');
  await assert.rejects(prepareLaunchPaths(directory), /unrecognized executable/);
  assert.equal(await readFile(join(directory, names.old), 'utf8'), 'unrelated file');
}));

test('unrelated links are rejected without modifying them', symlinkOnly, () => fixture(async directory => {
  await writeFile(join(directory, 'another-app'), 'another application'); await symlink('another-app', join(directory, names.canonical));
  await assert.rejects(prepareLaunchPaths(directory), /unrelated launch link/);
  assert.equal(await readlink(join(directory, names.canonical)), 'another-app'); assert.equal(await readFile(join(directory, 'another-app'), 'utf8'), 'another application');
}));

test('a compatibility executable changed during build is preserved', () => fixture(async directory => {
  await writeFile(join(directory, names.old), 'owned old build');
  await writeFile(`${join(directory, names.old)}.sha256`, hash('owned old build'));
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

for (const useWindows of new Set([windows, true])) {
  const label = useWindows ? 'Windows copies' : 'Unix links';
  test(`${label}: receipts resume interrupted migration and restore after failure without an initial symlink`, () => fixture(async directory => {
    const old = join(directory, useWindows ? 'desegni.exe' : 'desegni');
    await writeFile(old, 'owned executable'); await writeFile(`${old}.sha256`, hash('owned executable'));
    await prepareLaunchPaths(directory, useWindows);
    const plan = await prepareLaunchPaths(directory, useWindows);
    await restoreLaunchPaths(plan);
    assert.equal(await readFile(plan.canonical, 'utf8'), 'owned executable');
    assert.equal((await lstat(plan.canonical)).isSymbolicLink(), !useWindows);
    const resumed = await prepareLaunchPaths(directory, useWindows);
    await writeFile(resumed.canonical, 'resumed build'); await completeLaunchPaths(resumed);
    assert.equal(await readFile(resumed.legacy, 'utf8'), 'resumed build');
  }));

  test(`${label}: unrelated executables and changed rollback files are preserved`, () => fixture(async directory => {
    const old = join(directory, useWindows ? 'desegni.exe' : 'desegni');
    await writeFile(old, 'unrelated executable');
    await assert.rejects(prepareLaunchPaths(directory, useWindows), /unrecognized executable/);
    assert.equal(await readFile(old, 'utf8'), 'unrelated executable');
    await writeFile(`${old}.sha256`, hash('unrelated executable'));
    const plan = await prepareLaunchPaths(directory, useWindows);
    await writeFile(plan.canonical, 'new build');
    const backup = `${old}.before-desegnos-${hash('unrelated executable').slice(0, 16)}`;
    await writeFile(backup, 'unrelated rollback');
    await assert.rejects(completeLaunchPaths(plan), /unrelated rollback file/);
    assert.equal(await readFile(old, 'utf8'), 'unrelated executable');
    assert.equal(await readFile(backup, 'utf8'), 'unrelated rollback');
  }));
}

test('Windows launch links accept absolute case-insensitive targets', symlinkOnly, () => fixture(async directory => {
  const old = join(directory, 'desegni.exe'), canonical = join(directory, 'desegnos.exe');
  await writeFile(old, 'old build');
  // Simulate the spelling returned by Windows readlink without needing Windows privileges.
  await symlink(old.toUpperCase(), canonical);
  const plan = await prepareLaunchPaths(directory, true);
  await assert.rejects(lstat(canonical), { code: 'ENOENT' });
  await writeFile(canonical, 'new build'); await completeLaunchPaths(plan);
  await rm(old); await symlink(canonical.toUpperCase(), old);
  const repeated = await prepareLaunchPaths(directory, true);
  await completeLaunchPaths(repeated);
}));

test('Windows executable lookup accepts existing mixed-case filenames', { skip: !windows && 'Requires a case-insensitive Windows filesystem.' }, () => fixture(async directory => {
  await writeFile(join(directory, 'DeSeGnI.ExE'), 'owned build');
  await writeFile(join(directory, 'DeSeGnI.ExE.sha256'), hash('owned build'));
  const plan = await prepareLaunchPaths(directory);
  await writeFile(join(directory, 'DeSeGnOs.ExE'), 'new build');
  await completeLaunchPaths(plan);
  await assertAlias(plan, 'new build');
}));
