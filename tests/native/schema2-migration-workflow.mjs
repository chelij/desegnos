import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { access, chmod, copyFile, mkdir, readFile, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chooseFile, savedState, verifyArchive } from './file-dialog-workflow.mjs';
import { assertCanvasOnly } from './workflow.mjs';
import { waitFor, xpathString } from './webdriver.mjs';

const fixture = fileURLToPath(new URL('../../crates/desegni-storage/tests/fixtures/schema2-global-generation.desegni', import.meta.url));
const fixtureJson = fileURLToPath(new URL('../../crates/desegni-storage/tests/fixtures/schema2-global-generation.project.json', import.meta.url));
const fixtureHash = 'f828598377bebee41c6c8a00f884d8888cdfc3f76c49c8a95b7228b8004834b2';

export async function schema2MigrationWorkflow(driver, physical, step, evidence) {
  const original = await driver.invoke('get_workspace');
  assert.ok(original.project, 'Open an authored project before testing schema2 migration');
  assert.equal(original.dirty, true, 'The earlier project must have acknowledged unsaved changes to recover');
  const directory = join(resolve(evidence), 'schema2-migration-files');
  await mkdir(directory, { recursive: true, mode: 0o700 });
  const destination = join(directory, 'schema2 global mode site.desegni');
  await copyFile(fixture, destination);
  await chmod(destination, 0o600);
  const bytes = await readFile(destination);
  assert.equal(createHash('sha256').update(bytes).digest('hex'), fixtureHash);
  const sourceJson = await readFile(fixtureJson);
  const legacy = JSON.parse(sourceJson.toString('utf8'));
  assert.equal(legacy.schemaVersion, 2);
  assert.equal(legacy.generationMode, 'strict', 'The real fixture must preserve a nondefault generation setting');
  assert.notEqual(legacy.id, original.project.id);
  const extracted = spawnSync('python3', ['-c', String.raw`
import sys, zipfile
with zipfile.ZipFile(sys.argv[1]) as archive:
    assert archive.testzip() is None, "The canonical fixture failed its CRC check"
    assert archive.read("project.json") == sys.stdin.buffer.read(), "The adjacent JSON differs from the real archive"
    sys.stdout.write(archive.read("PROJECT.md").decode("utf-8"))
`, fixture], { input: sourceJson, encoding: 'utf8', timeout: 10_000, maxBuffer: 1024 * 1024 });
  if (extracted.error) throw extracted.error;
  assert.equal(extracted.status, 0, extracted.stderr);
  const originalNotes = extracted.stdout;
  await verifyArchive(destination, legacy, originalNotes);
  const expected = {
    ...legacy,
    schemaVersion: 6, gridSettings: { spacing: 8, unit: 'pixels', snap: true, rulers: true }, latestGeneration: null,
    appliedJobIds: [],
    pages: legacy.pages.map(page => ({ ...page, content: null })),
  };
  let savedBytes;

  await step('Native Open upgrades a real schema2 archive while preserving its global mode and original bytes', async () => {
    await chooseFile(physical, async () => {
      await driver.button('Open project');
      await waitFor(() => driver.execute(() => !!document.querySelector('dialog[aria-label="Save your changes?"][open]')), 'The dirty project replacement choice did not appear');
      await driver.click('//dialog[@open]//button[normalize-space(.)="Discard and continue"]', 'xpath');
    }, destination, evidence, 'schema2-migration-01-open');
    const state = await waitFor(async () => {
      const view = await driver.invoke('get_workspace');
      return view.project?.id === legacy.id && view;
    }, 'The real schema2 archive did not open');
    assert.deepEqual(state.project, expected);
    assert.equal(state.path, destination);
    assert.equal(state.dirty, true, 'Migration must remain unsaved until the user explicitly saves');
    assert.ok(state.migrationNotice?.includes('predates saved generated page source'));
    assert.equal(state.migrationNotice.includes('per-area'), false, 'A schema2 project must not be described as using per-area modes');
    assert.equal(await driver.execute(notice => document.body.textContent.includes(notice), state.migrationNotice), true);
    assert.equal(await driver.invoke('get_project_notes'), originalNotes);
    assert.deepEqual(await readFile(destination), bytes);
    await assert.rejects(access(`${destination}.previous`), { code: 'ENOENT' });
    await assertCanvasOnly(driver);
    await writeFile(join(evidence, 'schema2-migration-open.png'), Buffer.from(await driver.screenshot(), 'base64'));
  });

  await step('Explicit Save writes schema6 and keeps the exact schema2 archive as its previous version', async () => {
    await driver.button('Save');
    const state = await savedState(driver, destination);
    assert.deepEqual(state.project, expected);
    assert.equal(state.migrationNotice, null);
    assert.deepEqual(await readFile(`${destination}.previous`), bytes);
    assert.deepEqual(await readFile(fixture), bytes, 'Saving a test-owned copy must not modify the canonical fixture');
    const notes = await driver.invoke('get_project_notes');
    assert.equal(notes, originalNotes);
    const report = await verifyArchive(destination, expected, notes);
    savedBytes = await readFile(destination);
    await writeFile(join(evidence, 'schema2-migration-verification.json'), JSON.stringify({
      originalSha256: fixtureHash,
      sourceProjectSha256: createHash('sha256').update(sourceJson).digest('hex'),
      preservedGenerationMode: legacy.generationMode,
      preservedExtensions: legacy.extensions,
      saved: report,
    }, null, 2));
  });

  await step('The earlier dirty canvas project remains recoverable after the schema2 upgrade and save', async () => {
    await driver.menu(['Project', 'Recover projects']);
    const action = `//dialog[@open]//section[@aria-label=${xpathString(`Recover ${original.project.name}`)}]//button[normalize-space(.)="Recover project"]`;
    assert.equal((await driver.all(action, 'xpath')).length, 1);
    await driver.click(action, 'xpath');
    const state = await waitFor(async () => {
      const view = await driver.invoke('get_workspace');
      return view.project?.id === original.project.id && view;
    }, 'The earlier dirty canvas project was not recovered');
    assert.deepEqual(state.project, original.project);
    assert.equal(state.path, original.path);
    assert.equal(state.dirty, true);
    assert.equal(state.migrationNotice, null);
    assert.deepEqual(await readFile(destination), savedBytes);
    assert.deepEqual(await readFile(`${destination}.previous`), bytes);
    await assertCanvasOnly(driver);
  });
  return (await driver.invoke('get_workspace')).project;
}
