import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFile, mkdir, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chooseFile, savedState, verifyArchive } from './file-dialog-workflow.mjs';
import { assertCanvasOnly } from './workflow.mjs';
import { waitFor, xpathString } from './webdriver.mjs';

const fixture = fileURLToPath(new URL('../../crates/desegni-storage/tests/fixtures/schema1-mixed-generation.desegni', import.meta.url));
const fixtureHash = 'f6ab68d745746f287d13f55f0835c8135f09513c73a62ccb737c17a6f914e050';

export async function migrationWorkflow(driver, physical, step, evidence) {
  const original = (await driver.invoke('get_workspace')).project;
  const directory = join(evidence, 'migration-files');
  await mkdir(directory, { recursive: true, mode: 0o700 });
  const destination = join(directory, 'schema1 legacy site.desegni');
  await copyFile(fixture, destination);
  const bytes = await readFile(destination);
  assert.equal(createHash('sha256').update(bytes).digest('hex'), fixtureHash);
  const extracted = spawnSync('/usr/bin/python3', ['-c', 'import json,sys,zipfile; z=zipfile.ZipFile(sys.argv[1]); assert z.testzip() is None; print(z.read("project.json").decode("utf8"))', fixture], { encoding: 'utf8', timeout: 10_000 });
  if (extracted.error) throw extracted.error;
  assert.equal(extracted.status, 0, extracted.stderr);
  const legacy = JSON.parse(extracted.stdout);
  const modes = legacy.pages.flatMap(page => page.regions.map(region => ({ pageId: page.id, regionId: region.id, mode: region.mode })));
  assert.equal(legacy.schemaVersion, 1);
  assert.equal(new Set(modes.map(item => item.mode)).size, 2, 'The migration fixture must exercise conflicting old per-request modes');
  const expected = { ...legacy, schemaVersion: 6, gridSettings: { spacing: 8, unit: 'pixels', snap: true, rulers: true }, latestGeneration: null, appliedJobIds: [], generationMode: 'fuzzy',
    pages: legacy.pages.map(page => ({ ...page, content: null, regions: page.regions.map(({ mode, ...region }) => region) })),
    extensions: { ...legacy.extensions, 'io.desegni.legacy-generation': {
      schemaVersion: 1, data: { sourceSchemaVersion: 1, perAreaModes: modes },
    } },
  };

  await step('Native Open migrates a real schema1 archive without changing its original bytes', async () => {
    await chooseFile(physical, async () => {
      await driver.button('Open project');
    }, destination, evidence, 'migration-01-open');
    const state = await waitFor(async () => {
      const view = await driver.invoke('get_workspace');
      return view.project?.id === legacy.id && view;
    }, 'The schema1 archive did not open');
    assert.deepEqual(state.project, expected);
    assert.equal(state.dirty, true);
    assert.ok(state.migrationNotice?.includes('one generation setting'));
    assert.equal(await driver.execute(notice => document.body.textContent.includes(notice), state.migrationNotice), true);
    assert.deepEqual(await readFile(destination), bytes);
    await assertCanvasOnly(driver);
    await writeFile(join(evidence, 'migration-open.png'), Buffer.from(await driver.screenshot(), 'base64'));
    await driver.click('button[aria-label="Dismiss migration notice"]');
    assert.equal(await driver.execute(() => document.querySelector('.migration-notice') === null), true);
    assert.deepEqual((await driver.invoke('get_workspace')).project, expected);
  });

  await step('Saving the migrated project preserves the exact schema1 file as its backup', async () => {
    await driver.button('Save');
    const state = await savedState(driver, destination);
    assert.deepEqual(state.project, expected);
    assert.equal(state.migrationNotice, null);
    assert.deepEqual(await readFile(`${destination}.previous`), bytes);
    const report = await verifyArchive(destination, expected, await driver.invoke('get_project_notes'));
    await writeFile(join(evidence, 'migration-verification.json'), JSON.stringify({ originalSha256: fixtureHash, preservedModes: modes, saved: report }, null, 2));
  });

  await step('The earlier canvas project remains recoverable after opening and saving a legacy project', async () => {
    await driver.menu(['Recover unsaved projects']);
    const action = `//section[contains(@class,"workspace-surface")]//section[@aria-label=${xpathString(`Recover ${original.name}`)}]//button[normalize-space(.)="Recover project"]`;
    assert.equal((await driver.all(action, 'xpath')).length, 1);
    await driver.click(action, 'xpath');
    const state = await waitFor(async () => {
      const view = await driver.invoke('get_workspace');
      return view.project?.id === original.id && view;
    }, 'The earlier canvas project was not recovered');
    assert.deepEqual(state.project, original);
    assert.equal(state.migrationNotice, null);
    await assertCanvasOnly(driver);
  });
  return (await driver.invoke('get_workspace')).project;
}
