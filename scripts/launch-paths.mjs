import { constants } from 'node:fs';
import { copyFile, lstat, readFile, readlink, rename, symlink, unlink, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { basename, join } from 'node:path';

const fingerprint = async path => createHash('sha256').update(await readFile(path)).digest('hex');
const information = path => lstat(path).catch(error => { if (error.code !== 'ENOENT') throw error; return null; });
const receipt = path => readFile(`${path}.sha256`, 'utf8').catch(error => { if (error.code !== 'ENOENT') throw error; return null; });

// Remove the old product-name alias before Cargo can follow it into the old
// executable. A recognized reverse link or matching generated-copy receipt
// identifies compatibility files owned by this launcher.
export async function prepareLaunchPaths(directory, windows = process.platform === 'win32') {
  const canonical = join(directory, windows ? 'desegnos.exe' : 'desegnos');
  const legacy = join(directory, windows ? 'desegni.exe' : 'desegni');
  const plan = { canonical, legacy, windows, removedOldLink: false, restoreLegacyLink: false, legacySha256: null };
  const current = await information(canonical), old = await information(legacy);
  if (current?.isSymbolicLink()) {
    if (await readlink(canonical) !== basename(legacy) || !old?.isFile() || old.isSymbolicLink()) throw new Error(`Refusing to replace an unrelated launch link at ${canonical}.`);
    plan.legacySha256 = await fingerprint(legacy);
  }
  if (old?.isSymbolicLink()) {
    if (await readlink(legacy) !== basename(canonical)) throw new Error(`Refusing to replace an unrelated launch link at ${legacy}.`);
  } else if (old) {
    if (!old.isFile()) throw new Error(`Refusing to replace an unrelated path at ${legacy}.`);
    const hash = await fingerprint(legacy);
    const previousCopy = current?.isFile() && !current.isSymbolicLink() && await receipt(canonical) === await fingerprint(canonical) && hash === await fingerprint(canonical);
    if (hash !== plan.legacySha256 && await receipt(legacy) !== hash && !previousCopy) throw new Error(`Refusing to replace an unrecognized executable at ${legacy}. Keep it elsewhere before building.`);
    plan.legacySha256 = hash;
    plan.restoreLegacyLink = !current && await receipt(legacy) === hash;
  }
  if (current?.isSymbolicLink()) {
    const previous = await receipt(legacy);
    if (previous !== null && previous !== plan.legacySha256) throw new Error(`Refusing to replace an unrelated ownership receipt at ${legacy}.sha256.`);
    if (previous === null) await writeFile(`${legacy}.sha256`, plan.legacySha256, { flag: 'wx', mode: 0o600 });
    await unlink(canonical); plan.removedOldLink = true;
  }
  return plan;
}

export async function restoreLaunchPaths(plan) {
  if ((plan?.removedOldLink || plan?.restoreLegacyLink) && !await information(plan.canonical) && await fingerprint(plan.legacy) === plan.legacySha256) await symlink(basename(plan.legacy), plan.canonical);
}

export async function completeLaunchPaths(plan) {
  const current = await information(plan.canonical);
  if (!current?.isFile() || current.isSymbolicLink()) throw new Error(`The build did not produce the canonical executable ${plan.canonical}.`);
  const old = await information(plan.legacy);
  if (old?.isSymbolicLink()) {
    if (await readlink(plan.legacy) !== basename(plan.canonical)) throw new Error(`The compatibility link changed during the build: ${plan.legacy}.`);
    return { path: plan.canonical, backup: null };
  }
  let backup = null;
  if (old) {
    if (!old.isFile() || !plan.legacySha256 || await fingerprint(plan.legacy) !== plan.legacySha256) throw new Error(`The compatibility executable changed during the build: ${plan.legacy}.`);
    backup = `${plan.legacy}.before-desegnos-${plan.legacySha256.slice(0, 16)}`;
    const previous = await information(backup);
    if (previous) {
      if (!previous.isFile() || previous.isSymbolicLink() || await fingerprint(backup) !== plan.legacySha256) throw new Error(`An unrelated rollback file exists at ${backup}.`);
      await unlink(plan.legacy);
    } else await rename(plan.legacy, backup);
  }
  if (plan.windows) {
    await copyFile(plan.canonical, plan.legacy, constants.COPYFILE_EXCL);
    await writeFile(`${plan.legacy}.sha256`, await fingerprint(plan.legacy));
  } else await symlink(basename(plan.canonical), plan.legacy);
  return { path: plan.canonical, backup };
}
