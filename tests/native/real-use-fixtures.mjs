// Copies the owner's real projects and unsaved-work recovery into the isolated test profile,
// so the real-use suite runs against actual data instead of made-up fixtures.
// Originals are only read. Recovery records are rewritten to point at the copies, so nothing
// the test app saves can reach the real project files.
import { copyFile, mkdir, readFile, readdir, stat, writeFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { basename, join } from 'node:path';
import { inflateRawSync } from 'node:zlib';

const APP = 'io.github.chelij.desegni';

// One entry of a project archive, read through the zip central directory.
async function archiveEntry(path, wanted) {
  const bytes = await readFile(path);
  const end = bytes.lastIndexOf(Buffer.from([0x50, 0x4b, 0x05, 0x06]));
  if (end < 0) return null;
  for (let at = bytes.readUInt32LE(end + 16), left = bytes.readUInt16LE(end + 10); left > 0; left--) {
    const method = bytes.readUInt16LE(at + 10), size = bytes.readUInt32LE(at + 20);
    const nameLength = bytes.readUInt16LE(at + 28), extra = bytes.readUInt16LE(at + 30), comment = bytes.readUInt16LE(at + 32);
    const name = bytes.toString('utf8', at + 46, at + 46 + nameLength), local = bytes.readUInt32LE(at + 42);
    if (name === wanted) {
      const start = local + 30 + bytes.readUInt16LE(local + 26) + bytes.readUInt16LE(local + 28);
      const data = bytes.subarray(start, start + size);
      return method === 8 ? inflateRawSync(data) : data;
    }
    at += 46 + nameLength + extra + comment;
  }
  return null;
}

// Mirrors the app (src-tauri workspace.rs read_recovery): every edit raises the revision, so a
// journal at the saved revision is the saved file itself, unless opening upgraded it with a
// notice that still needs saving (schema 6 and older). Schema 7 is upgraded silently.
async function holdsUnsavedWork(record, projectPath, journal) {
  if (!projectPath || journal.project?.revision !== record.savedRevision) return true;
  if (journal.sha256 === record.savedSha256 || journal.sha256 === record.upgradedSha256) return false;
  if (record.upgradedSha256 || record.unsavedMigration) return true;
  try {
    const manifest = JSON.parse(await archiveEntry(projectPath, 'manifest.json'));
    return !(manifest.projectId === journal.project.id && manifest.revision === record.savedRevision
      && manifest.files?.['project.json'] === record.savedSha256 && manifest.schemaVersion === 7);
  } catch { return true; }
}

export async function seedRealUse(dataHome, evidence) {
  const source = process.env.DESEGNI_REAL_USE_SOURCE ?? join(process.env.XDG_DATA_HOME ?? join(homedir(), '.local/share'), APP);
  const target = join(dataHome, APP), copies = join(evidence, 'real-projects');
  await mkdir(target, { recursive: true, mode: 0o700 }); await mkdir(copies, { recursive: true });
  const manifest = { source, projects: [], recoveries: [] };
  const copied = new Map();
  async function copyProject(path) {
    if (!path) return null;
    if (copied.has(path)) return copied.get(path);
    try { await stat(path); } catch { copied.set(path, null); return null; }
    const destination = join(copies, `${copied.size + 1}-${basename(path)}`);
    await copyFile(path, destination); copied.set(path, destination);
    return destination;
  }
  let recent = [];
  try { recent = JSON.parse(await readFile(join(source, 'recent-projects.json'), 'utf8')); } catch { /* no recent projects */ }
  for (const entry of recent) {
    const copy = await copyProject(entry.path);
    if (copy) manifest.projects.push({ name: entry.name, path: copy, bytes: (await stat(copy)).size });
  }
  for (const name of (await readdir(source).catch(() => [])).filter(name => name.endsWith('.recovery.json'))) {
    let record;
    try { record = JSON.parse(await readFile(join(source, name), 'utf8')); } catch { continue; }
    try { await copyFile(join(source, record.journalFile), join(target, record.journalFile)); } catch { continue; }
    const projectPath = await copyProject(record.projectPath);
    // A saved revision only matches its own file; without the file the work is offered as never saved.
    const rewritten = projectPath ? { ...record, projectPath } : { ...record, projectPath: null, savedRevision: null, savedSha256: null };
    await writeFile(join(target, name), JSON.stringify(rewritten), { mode: 0o600 });
    const journal = JSON.parse((await readFile(join(target, record.journalFile), 'utf8')).trimEnd().split('\n').at(-1));
    // A journal that is just its saved file holds no unsaved work, and the app rightly skips it.
    const unsaved = await holdsUnsavedWork(record, projectPath, journal);
    if (unsaved) manifest.recoveries.push({ projectId: name.replace('.recovery.json', ''), name: journal.project?.name ?? null, hasFile: !!projectPath, pages: journal.project?.pages?.length ?? 0 });
    else manifest.alreadySaved = (manifest.alreadySaved ?? 0) + 1;
  }
  await writeFile(join(evidence, 'real-use-manifest.json'), JSON.stringify(manifest, null, 2));
  return manifest;
}
