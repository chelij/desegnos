import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { mkdir, readFile, readdir, stat, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { waitFor } from './webdriver.mjs';
import { assertCanvasOnly } from './workflow.mjs';

const inspectArchive = String.raw`
import hashlib, json, sys, zipfile
expected = json.load(sys.stdin)
with zipfile.ZipFile(sys.argv[1]) as archive:
    names = archive.namelist()
    assert len(names) == len(set(names)) == 3, names
    assert set(names) == {"manifest.json", "project.json", "PROJECT.md"}, names
    assert archive.testzip() is None, "An archive entry failed its CRC check"
    files = {name: archive.read(name) for name in names}
manifest = json.loads(files["manifest.json"])
project = json.loads(files["project.json"])
assert manifest["format"] == "desegni", manifest
assert manifest["schemaVersion"] == expected["project"]["schemaVersion"], manifest
assert manifest["projectId"] == expected["project"]["id"], manifest
assert manifest["revision"] == expected["project"]["revision"], manifest
assert project == expected["project"], "Saved project data disagrees with acknowledged Rust state"
assert files["PROJECT.md"] == expected["notes"].encode("utf-8"), "PROJECT.md bytes disagree with Rust notes"
assert set(manifest["files"]) == {"project.json", "PROJECT.md"}, manifest
hashes = {name: hashlib.sha256(files[name]).hexdigest() for name in manifest["files"]}
assert hashes == manifest["files"], "Manifest integrity hashes disagree with saved bytes"
print(json.dumps({"path": sys.argv[1], "projectId": project["id"], "revision": project["revision"],
                  "entries": names, "hashes": hashes, "notesBytes": len(files["PROJECT.md"])}))
`;

export async function verifyArchive(path, project, notes) {
  const checked = spawnSync('python3', ['-c', inspectArchive, path], {
    input: JSON.stringify({ project, notes }), encoding: 'utf8', timeout: 15_000, maxBuffer: 1024 * 1024,
  });
  if (checked.error) throw checked.error;
  assert.equal(checked.status, 0, `Actual archive verification failed: ${checked.stderr || checked.stdout}`);
  assert.equal((await stat(path)).mode & 0o077, 0, 'The saved private project file permits access by other users');
  return JSON.parse(checked.stdout);
}

function windows(snapshot) {
  assert.ok(Array.isArray(snapshot.windows), 'Physical input must enumerate fresh PID-owned native windows');
  return snapshot.windows;
}

async function waitUntilClosed(physical, windowId, timeout = 10_000) {
  const deadline = Date.now() + timeout;
  do {
    if (!windows(await physical.inspect()).some(window => window.id === windowId)) return true;
    await delay(75);
  } while (Date.now() < deadline);
  return false;
}

export async function chooseFile(physical, trigger, path, evidence, label, cancel = false, folder = false) {
  const before = await physical.inspect();
  const previous = new Set(windows(before).map(window => window.id));
  await trigger();
  const dialog = await waitFor(async () => {
    const snapshot = await physical.inspect();
    const added = windows(snapshot).filter(window => !previous.has(window.id));
    assert.ok(added.length <= 1, `Ambiguous native file dialog: ${JSON.stringify(added)}`);
    if (!added.length) return false;
    const window = added[0];
    assert.equal(window.pid, snapshot.pid, 'The file dialog belongs to a different process');
    assert.equal(window.mapped, true, 'The native file dialog is not mapped');
    await writeFile(join(evidence, `${label}-window.json`), JSON.stringify(snapshot, null, 2));
    return window;
  }, `The ${label} native file dialog did not appear`, 15_000);
  if (cancel) {
    await physical.raw({ action: 'key', key: 'Escape', windowId: dialog.id });
    assert.ok(await waitUntilClosed(physical, dialog.id), 'Escape did not cancel the native file dialog');
    return;
  }
  assert.equal(resolve(path), path, 'The file picker needs an absolute test-owned path');
  if (folder) {
    // The observed GTK Select Folder dialog opens in Recent, where Open stays
    // disabled for a typed location. Its Home row enters filesystem browsing.
    assert.equal(dialog.title, 'Select Folder');
    assert.ok(dialog.width >= 600 && dialog.height >= 300);
    await physical.raw({ action: 'click', point: { x: dialog.x + 60, y: dialog.y + 60 }, windowId: dialog.id });
  }
  await physical.raw({ action: 'key', key: 'L', modifiers: ['Control'], windowId: dialog.id });
  await physical.raw({ action: 'key', key: 'A', modifiers: ['Control'], windowId: dialog.id });
  await physical.raw({ action: 'text', text: folder ? `${path}/` : path, windowId: dialog.id });
  if (folder) {
    await delay(250);
    // GTK selects an inline child-name completion after the trailing slash.
    // Backspace removes that suggestion (or the slash if there is none),
    // leaving the requested directory instead of accepting its first child.
    await physical.raw({ action: 'key', key: 'BackSpace', windowId: dialog.id });
  }
  if (folder) await physical.captureWindow(join(evidence, `${label}-typed.png`));
  for (let attempt = 0; attempt < 2; attempt += 1) {
    await physical.raw({ action: 'key', key: 'Return', windowId: dialog.id });
    if (await waitUntilClosed(physical, dialog.id, 3000)) return;
    if (folder) await physical.captureWindow(join(evidence, `${label}-return-${attempt}.png`));
  }
  // Return can navigate before accepting; Open is GTK's visible action.
  if (folder) {
    await physical.raw({ action: 'key', key: 'O', modifiers: ['Alt'], windowId: dialog.id });
    if (await waitUntilClosed(physical, dialog.id, 3000)) return;
  }
  await physical.captureWindow(join(evidence, `${label}-failure.png`));
  throw new Error(`The ${label} chooser did not accept its typed path: ${JSON.stringify(await physical.inspect())}`);
}

export function chooseFolder(physical, trigger, path, evidence, label) {
  return chooseFile(physical, trigger, path, evidence, label, false, true);
}

export async function savedState(driver, path) {
  return waitFor(async () => {
    const state = await driver.invoke('get_workspace');
    return state.path === path && state.dirty === false && state;
  }, 'The actual native save did not acknowledge its chosen path and revision', 15_000);
}

async function toggleGrid(driver) {
  const before = await driver.invoke('get_workspace');
  await driver.menu(['View', 'Show grid']);
  return waitFor(async () => {
    const state = await driver.invoke('get_workspace');
    return state.project.showGrid !== before.project.showGrid && state.project.revision > before.project.revision && state;
  }, 'The grid edit was not acknowledged by Rust');
}

export async function fileDialogWorkflow(driver, physical, step, evidence) {
  const directory = join(resolve(evidence), 'picker-files');
  await mkdir(directory, { recursive: true, mode: 0o700 });
  const suffix = randomUUID();
  const firstPath = join(directory, `saved site ${suffix}.desegni`);
  const secondPath = join(directory, `saved copy ${suffix}.desegni`);
  const requirementsPath = join(directory, `requirements ${suffix}.md`);
  const initial = (await driver.invoke('get_workspace')).project;
  assert.ok(initial && initial.pages.some(page => page.regions.some(region => region.prompt.trim())), 'Author a described area before exercising native file dialogs');
  const initialNotes = await driver.invoke('get_project_notes');
  const archiveReports = [];
  let savedProject;
  let savedNotes;

  await step('Native Save picker writes a private archive with actual project and PROJECT.md hashes', async () => {
    await chooseFile(physical, () => driver.button('Save'), firstPath, evidence, 'picker-01-save');
    const state = await savedState(driver, firstPath);
    assert.deepEqual(state.project, initial);
    archiveReports.push(await verifyArchive(firstPath, initial, initialNotes));
  });

  await step('Cancelling the native Save As picker preserves the project, destination and files', async () => {
    const before = await driver.invoke('get_workspace');
    const files = (await readdir(directory)).sort();
    await chooseFile(physical, () => driver.button('Save as'), null, evidence, 'picker-02-cancel-save-as', true);
    const after = await driver.invoke('get_workspace');
    assert.deepEqual(after.project, before.project);
    assert.equal(after.path, before.path);
    assert.equal(after.dirty, before.dirty);
    assert.deepEqual((await readdir(directory)).sort(), files);
  });

  await step('Save updates the selected file and retains the preceding valid archive', async () => {
    const edited = await toggleGrid(driver);
    await driver.button('Save');
    const state = await savedState(driver, firstPath);
    assert.deepEqual(state.project, edited.project);
    savedProject = state.project;
    savedNotes = await driver.invoke('get_project_notes');
    archiveReports.push(await verifyArchive(firstPath, savedProject, savedNotes));
    archiveReports.push(await verifyArchive(`${firstPath}.previous`, initial, initialNotes));
  });

  await step('Native Save As picker changes the destination without modifying the first saved file', async () => {
    const firstBytes = await readFile(firstPath);
    await chooseFile(physical, () => driver.button('Save as'), secondPath, evidence, 'picker-03-save-as');
    const state = await savedState(driver, secondPath);
    assert.deepEqual(state.project, savedProject);
    assert.deepEqual(await readFile(firstPath), firstBytes);
    archiveReports.push(await verifyArchive(secondPath, savedProject, savedNotes));
    const changed = await toggleGrid(driver);
    await driver.button('Save');
    const updated = await savedState(driver, secondPath);
    assert.deepEqual(updated.project, changed.project);
    archiveReports.push(await verifyArchive(secondPath, updated.project, await driver.invoke('get_project_notes')));
    archiveReports.push(await verifyArchive(`${secondPath}.previous`, savedProject, savedNotes));
  });

  await step('Native Open picker restores the exact independently saved project', async () => {
    await chooseFile(physical, () => driver.button('Open project'), firstPath, evidence, 'picker-04-open');
    const state = await savedState(driver, firstPath);
    assert.deepEqual(state.project, savedProject);
    assert.equal(await driver.invoke('get_project_notes'), savedNotes);
  });

  await step('Cancelling the native Open picker leaves the current project intact', async () => {
    const before = await driver.invoke('get_workspace');
    await chooseFile(physical, () => driver.button('Open project'), null, evidence, 'picker-05-cancel-open', true);
    const after = await driver.invoke('get_workspace');
    assert.deepEqual(after.project, before.project);
    assert.equal(after.path, before.path);
    assert.equal(after.dirty, before.dirty);
  });

  await step('Native Save requirements picker writes the exact displayed frozen Markdown bytes', async () => {
    await driver.menu(['Technical', 'Review requirements']);
    const markdown = await waitFor(() => driver.source('REQUIREMENTS.md source'), 'The requirements document was not displayed');
    await chooseFile(physical, () => driver.button('Save requirements'), requirementsPath, evidence, 'picker-06-save-requirements');
    const actual = await waitFor(async () => {
      try { return await readFile(requirementsPath); }
      catch (error) { if (error.code === 'ENOENT') return false; throw error; }
    }, 'The requirements picker did not produce its actual Markdown file');
    assert.deepEqual(actual, Buffer.from(markdown, 'utf8'));
    assert.equal((await stat(requirementsPath)).mode & 0o077, 0, 'The private requirements document permits access by other users');
    await writeFile(join(evidence, 'picker-requirements-verification.json'), JSON.stringify({
      path: requirementsPath, bytes: actual.length, sha256: createHash('sha256').update(actual).digest('hex'),
      jobId: markdown.match(/^Job: (.+)$/m)?.[1],
    }, null, 2));
    await driver.click('button[aria-label="Close Review requirements"]');
  });

  let dirty;
  await step('A final acknowledged edit remains unsaved for the crash recovery check', async () => {
    const edited = await toggleGrid(driver);
    const firstArea = edited.project.pages[0].regions[0];
    const prompt = `${firstArea.prompt}\nUnsaved native recovery marker: ${suffix}`;
    await driver.fill('Request 1', prompt);
    dirty = await waitFor(async () => {
      const state = await driver.invoke('get_workspace');
      return state.project.pages[0].regions[0].prompt === prompt && state;
    }, 'The unique unsaved recovery input was not acknowledged');
    assert.equal(dirty.path, firstPath);
    assert.equal(dirty.dirty, true);
    assert.notDeepEqual(dirty.project, savedProject);
    archiveReports.push(await verifyArchive(firstPath, savedProject, savedNotes));
  });
  await writeFile(join(evidence, 'picker-archive-verification.json'), JSON.stringify(archiveReports, null, 2));
  await assertCanvasOnly(driver);
  return dirty.project;
}
