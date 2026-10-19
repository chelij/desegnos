import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { waitFor, xpathString } from './webdriver.mjs';

const reviewAction = '[role="menuitem"][aria-label="Review requirements"]';
const prompt = 'Create a mobile header for Saka with the accepted brand color.\nKeep this literal text: ```md <script>never execute this</script> ```';
const choice = 'Use deep blue #16324F and the name Saka on every page.';
const exceptionReason = 'This campaign header deliberately uses a warm orange accent for the seasonal promotion.';

export function dataBlocks(markdown) {
  return [...markdown.matchAll(/^(`{3,})json\n([\s\S]*?)^\1\s*$/gm)].map(match => JSON.parse(match[2]));
}

export async function assertCanvasOnly(driver) {
  assert.equal(await driver.execute(() => document.querySelectorAll('aside:not(.first-run-tips), .project-sidebar, .inspector').length), 0,
    'The corrected editor must not contain a sidebar or inspector');
  assert.equal(await driver.execute(() => document.querySelectorAll('input[data-coordinate], [data-testid="canvas"] input[type="number"]').length), 0,
    'Request placement must not expose a numeric geometry inspector');
}

export async function assertGenerationModes(driver) {
  await driver.menu(['AI style']);
  const choices=await driver.execute(()=>[...document.querySelectorAll('[role=menu][aria-label="AI style"] button')].map(b=>({label:b.getAttribute('aria-label'),checked:b.getAttribute('aria-checked')})));
  assert.deepEqual(choices.map(c=>c.label),['Flexible — small adjustments only','Precise — follow placement closely']);
  const mode=(await driver.invoke('get_workspace')).project.generationMode;
  assert.equal(choices[mode==='strict'?1:0].checked,'true');
  await driver.escape();
}

export async function assertReviewCount(driver, expected) {
  assert.equal((await driver.all(reviewAction)).length, 0, 'Technical requirements must stay inside the closed menu');
  const { project } = await driver.invoke('get_workspace');
  const count = project.pages.flatMap(page => page.regions).filter(region => region.prompt.trim()).length;
  assert.equal(count, expected, 'The native target count differs from the described canvas requests');
  const generate=await driver.execute(()=>[...document.querySelectorAll('.shell-primary-actions button')].find(b=>b.textContent.trim()==='Generate')?.disabled);
  assert.equal(generate,expected===0,'One Generate button reflects whether described pending requests exist');
  await assertGenerationModes(driver);

}

export async function reviewRequirements(driver, mode, evidence, name = 'REQUIREMENTS') {
  const { project } = await driver.invoke('get_workspace');
  assert.equal(project.generationMode, mode);
  const targets = project.pages.flatMap(page => page.regions.filter(region => !region.completed && region.prompt.trim()).map(region => ({ page, region })));
  await assertReviewCount(driver, targets.length);
  const prepared = await driver.invoke('prepare_requirements');
  // The owner removed pre-send technical dialogs. Check the real native frozen
  // document without restoring developer UI to the normal creation flow.
  const markdown=prepared.markdown;
  assert.equal(markdown.replace(/^Job: .+$/m, 'Job: JOB'), prepared.markdown.replace(/^Job: .+$/m, 'Job: JOB'));
  assert.equal(prepared.sha256, createHash('sha256').update(prepared.markdown).digest('hex'));
  assert.equal(prepared.regionCount, targets.length);
  assert.equal(prepared.pageCount, new Set(targets.map(target => target.page.id)).size);
  assert.match(markdown, /^Requirements format: 10$/m);
  assert.match(markdown, /^## Generation setting$/m);
  assert.equal([...markdown.matchAll(/"generationMode"\s*:/g)].length, 1);
  assert.doesNotMatch(markdown, /within 1 CSS pixel/);
  const records = dataBlocks(markdown);
  const settings = records.filter(record => record && !Array.isArray(record) && 'generationMode' in record);
  assert.equal(settings.length, 1, 'Requirements need one global generation-mode setting');
  assert.equal(settings[0].generationMode, mode);
  const requests = records.filter(record => record.regionId);
  assert.equal(requests.length, targets.length);
  for (const { page, region } of targets) {
    const request = requests.find(record => record.regionId === region.id);
    assert.ok(request);
    assert.equal(request.pageId, page.id);
    assert.equal(request.prompt, region.prompt);
    assert.deepEqual(request.placementHint, region.rect);
    assert.deepEqual(request.viewport, page.viewport);
    assert.equal(request.appearance, page.appearance);
    assert.deepEqual(request.placementPercent, { x: region.rect.x * 100 / page.viewport.width, y: region.rect.y * 100 / page.viewport.height, width: region.rect.width * 100 / page.viewport.width, height: region.rect.height * 100 / page.viewport.height });
    assert.equal('mode' in request, false, 'Per-request mode must not override the global mode');
    assert.equal('rect' in request, false, 'Requirements must describe placement intent instead of fixed site geometry');
  }
  const context = records.find(record => record.brief);
  assert.deepEqual(context.brief, project.brief);
  assert.deepEqual(context.decisions, project.decisions);
  assert.deepEqual(context.exceptions, project.exceptions);
  await writeFile(join(evidence, `${name}.md`), markdown);
  await writeFile(join(evidence, `${name}-snapshot.json`), JSON.stringify(prepared, null, 2));
  return { markdown, settings: settings[0], requests };
}

export async function authorProject(driver, step, evidence) {
  await step('A fresh native app has no inherited project or recovery data', async () => {
    await driver.ready('//button[@aria-label="Blank website"]', 'xpath');
    await driver.ready('//button[@aria-label="Use my existing brand"]', 'xpath');
    const state = await driver.invoke('get_workspace');
    assert.equal(state.project, null);
    assert.equal(state.recovery, null);
    assert.deepEqual(state.recoveries, []);
    assert.equal(state.recoveryError, null);
    await assertCanvasOnly(driver);
  });

  await step('Blank website starts Mobile / Light, then the page dropdown switches to Dark', async () => {
    assert.equal(await driver.execute(() => !!document.querySelector('input[name="projectName"]')), false);
    await driver.button('Blank website');
    await driver.fill('Project name', 'Native Saka website');
    await driver.button('Mobile');
    await driver.button('Create project');
    await waitFor(() => driver.execute(() => !!document.querySelector('[data-testid="canvas"]')), 'The canvas did not appear');
    const { project } = await driver.invoke('get_workspace');
    assert.equal(project.schemaVersion, 6);
    assert.equal(project.showGrid, true);
    assert.deepEqual(project.gridSettings, { spacing: 8, unit: 'pixels', snap: true, rulers: true });
    assert.equal(project.latestGeneration, null);
    assert.deepEqual(project.viewport, { width: 390, height: 844 });
    assert.equal(project.appearance, 'light');
    assert.equal(project.pages[0].appearance, 'light');
    assert.equal(await driver.execute(() => document.querySelector('[data-testid="canvas"]').classList.contains('dark')), false);
    await driver.select('Page appearance', 'dark');
    await waitFor(async () => (await driver.invoke('get_workspace')).project.pages[0].appearance === 'dark', 'The page appearance dropdown did not switch to Dark');
    assert.equal(project.generationMode, 'fuzzy');
    assert.equal(project.pages.length, 1);
    assert.deepEqual(project.pages[0].regions, []);
    assert.equal(await driver.execute(() => document.querySelectorAll('[data-testid="canvas"] textarea').length), 0);
    assert.equal(await driver.execute(() => document.querySelector('[data-testid="canvas"]').classList.contains('dark')), true);
    await assertCanvasOnly(driver);
    await assertReviewCount(driver, 0);
  });

  await step('Canvas Enter creates and focuses an actual on-page request textarea', async () => {
    await driver.press('[data-testid="canvas"]', '\uE007');
    await waitFor(async () => (await driver.invoke('get_workspace')).project.pages[0].regions.length === 1, 'Canvas Enter did not create a request');
    assert.equal(await driver.execute(() => document.activeElement?.getAttribute('aria-label')), 'Request 1');
    assert.equal(await driver.execute(() => document.activeElement?.closest('[data-testid="canvas"]') !== null), true);
    assert.equal((await driver.invoke('get_workspace')).project.pages[0].regions[0].prompt, '');
    await driver.fill('Request 1', '   ');
    await waitFor(async () => (await driver.invoke('get_workspace')).project.pages[0].regions[0].prompt === '   ', 'Whitespace input did not reach the model');
    await assertReviewCount(driver, 0);
    await assertCanvasOnly(driver);
  });

  await step('The on-canvas prompt preserves literal input and one global Strict choice', async () => {
    await driver.fill('Request 1', prompt);
    await waitFor(async () => (await driver.all('//button[normalize-space(.)="Strict"]', 'xpath')).length === 1, 'The global generation setting did not appear with the first committed request');
    await driver.button('Strict');
    await waitFor(async () => {
      const { project } = await driver.invoke('get_workspace');
      return project.pages[0].regions[0].prompt === prompt && project.generationMode === 'strict';
    }, 'The canvas prompt or global generation mode was lost');
    const { project } = await driver.invoke('get_workspace');
    assert.equal('mode' in project.pages[0].regions[0], false);
    await assertGenerationModes(driver);

    await assertReviewCount(driver, 1);
    await driver.button('Undo');
    await waitFor(async () => (await driver.invoke('get_workspace')).project.generationMode === 'fuzzy', 'Undo did not restore the global mode');
    await driver.button('Redo');
    await waitFor(async () => (await driver.invoke('get_workspace')).project.generationMode === 'strict', 'Redo did not restore the global mode');
  });

  await step('Grid visibility, percentage spacing, rulers and zoom preserve placement intent', async () => {
    const before = (await driver.invoke('get_workspace')).project.pages[0].regions[0].rect;
    await driver.toggleGrid();
    await waitFor(async () => !(await driver.invoke('get_workspace')).project.showGrid, 'The grid did not turn off');
    await driver.toggleGrid();
    await waitFor(async () => (await driver.invoke('get_workspace')).project.showGrid, 'The grid did not turn back on');
    await driver.button('Grid and rulers');
    await driver.select('Unit', 'percent');
    await driver.fill('Grid spacing', 5);
    await driver.button('Save grid settings');
    await waitFor(async () => {
      const settings = (await driver.invoke('get_workspace')).project.gridSettings;
      return settings.spacing === 5 && settings.unit === 'percent' && settings.snap && settings.rulers;
    }, 'The percentage grid settings were not persisted');
    await driver.button('Grid and rulers');
    await driver.click(driver.field('Show percentage rulers'), 'xpath');
    await driver.button('Save grid settings');
    await waitFor(async () => !(await driver.invoke('get_workspace')).project.gridSettings.rulers, 'Ruler visibility was not persisted');
    await waitFor(() => driver.execute(() => document.querySelectorAll('.workspace-ruler').length === 0), 'Turning rulers off did not remove them from the workspace');
    await driver.button('Grid and rulers');
    await driver.click(driver.field('Show percentage rulers'), 'xpath');
    await driver.button('Save grid settings');
    await waitFor(() => driver.execute(() => document.querySelectorAll('.workspace-ruler').length === 2), 'Turning rulers on did not restore both percentage scales');
    await driver.menu(['100%'], 'Canvas zoom');
    await waitFor(() => driver.execute(() => Math.abs(document.querySelector('[data-testid="screen-guide"]').getBoundingClientRect().width - 390) < 1), 'The footer zoom menu did not control the actual screen guide');
    await driver.select('Zoom', '0.75');
    await waitFor(() => driver.execute(() => Math.abs(document.querySelector('[data-testid="screen-guide"]').getBoundingClientRect().width - 390 * 0.75) < 1), 'The screen guide did not render at 75% zoom');
    assert.deepEqual((await driver.invoke('get_workspace')).project.pages[0].regions[0].rect, before);
  });

  await step('Accepted identity and scoped creative exceptions populate PROJECT.md', async () => {
    await driver.button('Project dossier');
    await driver.button('Brief');
    await driver.fill('Purpose', 'A clear mobile home for the Saka studio.');
    await driver.fill('Brand name', 'Saka');
    await waitFor(async () => (await driver.invoke('get_workspace')).project.brief.brandName === 'Saka', 'The brief did not persist');
    await driver.button('Rules');
    await driver.fill('Topic', 'Brand identity');
    await driver.fill('Rule', choice);
    await driver.click(driver.field('Never change this rule'), 'xpath');
    await driver.button('Add choice');
    await waitFor(async () => (await driver.invoke('get_workspace')).project.decisions.length === 1, 'The identity choice was not recorded');
    await driver.button('Exceptions');
    await driver.fill('Which rule?', 'Seasonal accent color');
    await driver.fill('Why make an exception?', exceptionReason);
    const { project } = await driver.invoke('get_workspace');
    await driver.select('Applies to', `region:${project.pages[0].regions[0].id}`);
    await driver.button('Add exception');
    await waitFor(async () => (await driver.invoke('get_workspace')).project.exceptions.length === 1, 'The creative exception was not recorded');
    await driver.menu(['PROJECT.md'], 'Dossier options');
    const markdown = await waitFor(() => driver.source('PROJECT.md source'), 'PROJECT.md was not displayed');
    assert.equal(markdown, await driver.invoke('get_project_notes'));
    assert.match(markdown, /Use deep blue #16324F/);
    assert.match(markdown, /seasonal promotion/);
    await writeFile(join(evidence, 'PROJECT.md'), markdown);
    await driver.click('button[aria-label="Close Project brief"]');
  });

  await step('Requirements carry exact on-canvas input, one global mode and placement hints', async () => {
    await reviewRequirements(driver, 'strict', evidence);
    await assertCanvasOnly(driver);
  });
  return (await driver.invoke('get_workspace')).project;
}

export async function recoverProject(driver, expected, step) {
  await step('A fresh process restores the exact canvas project and preserves other recoveries', async () => {
    const action = `//section[@aria-label=${xpathString(`Recover ${expected.name}`)}]//button[normalize-space(.)="Recover project"]`;
    await waitFor(async () => (await driver.all(action, 'xpath')).length === 1, 'The expected named recovery did not appear');
    const before = await driver.invoke('get_workspace');
    assert.equal(before.project, null);
    assert.equal(before.recovery.projectId, expected.id);
    assert.equal(before.recovery.revision, expected.revision);
    const remaining = before.recoveries.filter(item => item.projectId !== expected.id);
    await driver.click(action, 'xpath');
    await waitFor(() => driver.execute(() => !!document.querySelector('[data-testid="canvas"]')), 'Recovery did not restore the canvas');
    const after = await driver.invoke('get_workspace');
    assert.deepEqual(after.project, expected);
    assert.deepEqual([...after.recoveries].sort((a, b) => a.projectId.localeCompare(b.projectId)),
      remaining.sort((a, b) => a.projectId.localeCompare(b.projectId)));
    assert.equal(after.recoveryError, null);
    assert.equal(after.dirty, true);
    await assertCanvasOnly(driver);
    assert.equal(await driver.execute(() => document.querySelectorAll('[data-testid="canvas"] textarea').length), expected.pages.find(page => page.id === expected.homePageId).regions.filter(region => !region.completed).length);
  });
}
