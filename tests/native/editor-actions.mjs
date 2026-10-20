import assert from 'node:assert/strict';
import { waitFor, xpathString } from './webdriver.mjs';
import { assertCanvasOnly } from './workflow.mjs';

async function project(driver) { return (await driver.invoke('get_workspace')).project; }
async function modalButton(driver, text) {
  await driver.click(`//dialog[@open]//button[normalize-space(.)=${xpathString(text)}]`, 'xpath');
}
async function closed(driver) {
  await waitFor(() => driver.execute(() => !document.querySelector('dialog[open]')), 'The editor dialog did not close');
}
async function undo(driver, matches) {
  await driver.button('Undo');
  return waitFor(async () => { const value = await project(driver); return matches(value) && value; }, 'Undo did not restore the edited record');
}

export async function editorActions(driver, physical, step) {
  const original = await project(driver);
  const homeId = original.homePageId;
  async function physicalClick(selector) {
    const rect = await physical.rect(selector);
    await physical.click({ x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 });
  }

  await step('Project rename commits through its dialog and undo restores the name', async () => {
    await driver.click('.project-title');
    await driver.fill('Project name', `${original.name} renamed`);
    await modalButton(driver, 'Rename project');
    await closed(driver);
    await waitFor(async () => (await project(driver)).name === `${original.name} renamed`, 'Rename was not accepted');
    await undo(driver, value => value.name === original.name);
  });

  let addedPage;
  await step('Add page and page settings preserve routes, appearance, dimensions and draft status', async () => {
    await driver.button('Add page');
    await driver.fill('Page name', 'About the studio');
    await driver.click('dialog[open] .page-address summary');
    await driver.fill('Page path', '/about');
    await modalButton(driver, 'Add page');
    await closed(driver);
    const added = await waitFor(async () => {
      const value = await project(driver);
      return value.pages.length === original.pages.length + 1 && value.pages.find(page => page.path === '/about');
    }, 'The added page was not accepted');
    addedPage = added.id;
    assert.deepEqual(added.viewport, original.viewport);
    assert.equal(added.appearance, original.appearance);
    await driver.button('Page settings');
    await driver.fill('Page name', 'Studio');
    await driver.click('dialog[open] .page-address summary');
    await driver.fill('Page path', '/studio');
    await driver.click('dialog[open] .custom-screen summary');
    await driver.fill('Screen width', 640);
    await driver.fill('Screen height', 720);
    await driver.select('Website appearance', 'light');
    await driver.click(driver.field('Include this page in the website'), 'xpath');
    await modalButton(driver, 'Save page settings');
    await closed(driver);
    const changed = (await project(driver)).pages.find(page => page.id === addedPage);
    assert.equal(changed.name, 'Studio');
    assert.equal(changed.path, '/studio');
    assert.deepEqual(changed.viewport, { width: 640, height: 720 });
    assert.equal(changed.appearance, 'light');
    assert.equal(changed.included, false);
  });

  await step('Nested menus and browser-style page tabs support native keyboard navigation', async () => {
    const before = await project(driver);
    await driver.page(homeId);
    for (const [key, modifiers, target] of [['Page_Down',['Control'],addedPage],['Page_Up',['Control'],homeId],['Tab',['Control'],addedPage],['Tab',['Control','Shift'],homeId]]) {
      await physical.keys(key, 1, modifiers);
      await waitFor(() => driver.execute(id => document.getElementById(`page-tab-${id}`)?.getAttribute('aria-selected') === 'true', target), `${key} ${modifiers.join("+")} did not switch page tabs`);
    }
    await physical.keys('Right');
    await waitFor(() => driver.execute(id => document.getElementById(`page-tab-${id}`)?.getAttribute('aria-selected') === 'true', addedPage), 'Right arrow did not activate the next page tab');
    await physical.keys('Home');
    await waitFor(() => driver.execute(id => document.getElementById(`page-tab-${id}`)?.getAttribute('aria-selected') === 'true', homeId), 'Home did not activate the first page tab');
    await physical.keys('End');
    await waitFor(() => driver.execute(id => document.getElementById(`page-tab-${id}`)?.getAttribute('aria-selected') === 'true', addedPage), 'End did not activate the last page tab');
    await physicalClick('button[aria-label="Editor menu"]');
    await waitFor(() => driver.execute(() => document.activeElement?.getAttribute('aria-label') === 'Project'), 'The menu did not focus its first enabled item');
    await physical.keys('End');
    await physical.keys('Right');
    await waitFor(() => driver.execute(() => document.querySelectorAll('[role="menu"]').length === 2), 'Right arrow did not open the Technical submenu');
    await physical.keys('Escape');
    assert.equal(await driver.execute(() => document.activeElement?.getAttribute('aria-label')), 'Technical');
    await physical.keys('Escape');
    assert.equal(await driver.execute(() => document.querySelectorAll('[role="menu"]').length), 0);
    assert.equal(await driver.execute(() => document.activeElement?.getAttribute('aria-label')), 'Editor menu');
    await physicalClick('button[aria-label="Editor menu"]');
    await physicalClick(`[id="page-tab-${addedPage}"]`);
    assert.equal(await driver.execute(() => document.querySelectorAll('[role="menu"]').length), 0, 'Outside interaction did not dismiss the menu');
    assert.deepEqual(await project(driver), before, 'Menu and tab navigation must not change saved project data');
  });

  await step('A duplicate route reports its error inside the dialog without mutating the project', async () => {
    const before = await project(driver);
    await driver.button('Add page');
    await driver.fill('Page name', 'Duplicate route');
    await driver.click('dialog[open] .page-address summary');
    await driver.fill('Page path', '/STUDIO');
    await modalButton(driver, 'Add page');
    await waitFor(() => driver.execute(() => document.querySelector('dialog[open] [role="alert"]')?.textContent.includes('same public path')), 'Route validation was not visible in the active dialog');
    assert.deepEqual(await project(driver), before);
    await driver.click('dialog[open] button[aria-label="Dismiss dialog error"]');
    await driver.click('button[aria-label="Close Add page"]');
    await closed(driver);
  });

  await step('Page removal requires its explicit choice and remains undoable', async () => {
    await driver.button('Remove page');
    await modalButton(driver, 'Cancel');
    await closed(driver);
    assert.ok((await project(driver)).pages.some(page => page.id === addedPage));
    await driver.button('Remove page');
    await modalButton(driver, 'Remove');
    await closed(driver);
    await waitFor(async () => !(await project(driver)).pages.some(page => page.id === addedPage), 'The page was not removed');
    await undo(driver, value => value.pages.some(page => page.id === addedPage));
    await driver.button('Redo');
    await waitFor(async () => !(await project(driver)).pages.some(page => page.id === addedPage), 'Redo did not remove the page again');
  });

  await step('Topbar page navigation returns to the on-canvas requests without a sidebar', async () => {
    const before = await project(driver);
    await driver.page(homeId);
    await waitFor(() => driver.execute(() => !!document.querySelector('textarea[aria-label="Request 1"]')), 'The topbar page selector did not return to Home');
    assert.deepEqual(await project(driver), before);
    await assertCanvasOnly(driver);
    assert.equal(before.pages.flatMap(page => page.regions).every(request => !('mode' in request)), true);
  });

  await step('The topbar Remove request action supports Cancel, Remove and Undo', async () => {
    await driver.click('textarea[aria-label="Request 2"]');
    const before = await project(driver);
    const count = before.pages.find(page => page.id === homeId).regions.length;
    await driver.button('Remove request');
    await modalButton(driver, 'Cancel');
    await closed(driver);
    assert.deepEqual(await project(driver), before);
    await driver.button('Remove request');
    await modalButton(driver, 'Remove');
    await closed(driver);
    await waitFor(async () => (await project(driver)).pages.find(page => page.id === homeId).regions.length === count - 1, 'The unreferenced area was not removed');
    const restored = await undo(driver, value => value.pages.find(page => page.id === homeId).regions.length === count);
    assert.deepEqual(restored.pages, before.pages);
  });

  await step('Brief fields and identity editing preserve accepted records and allow deliberate removal', async () => {
    await driver.button('Project dossier');
    await driver.button('Brief');
    await driver.fill('Audience', 'Visitors looking for the studio and its work.');
    await driver.fill('Voice and tone', 'Direct and welcoming.');
    await waitFor(async () => (await project(driver)).brief.tone === 'Direct and welcoming.', 'The remaining brief fields were not accepted');
    const before = await project(driver);
    await driver.button('Rules');
    await driver.button('Edit rule');
    await driver.fill('Rule', 'A draft that will be cancelled.');
    await driver.button('Cancel editing');
    assert.deepEqual((await project(driver)).decisions, before.decisions);
    await driver.button('Edit rule');
    await driver.fill('Rule', `${before.decisions[0].value} Keep navigation labels plain.`);
    await driver.button('Save choice');
    await waitFor(async () => (await project(driver)).decisions[0].value.endsWith('Keep navigation labels plain.'), 'The edited identity choice was not accepted');
    assert.equal((await project(driver)).decisions[0].id, before.decisions[0].id);
    await driver.click('button[aria-label="Close Project brief"]');
    await closed(driver);
    await undo(driver, value => value.decisions[0].value === before.decisions[0].value);
    await driver.button('Project dossier');
    await driver.button('Rules');
    await driver.button('Remove rule');
    await waitFor(async () => (await project(driver)).decisions.length === 0, 'The deliberate decision removal was not accepted');
    await driver.click('button[aria-label="Close Project brief"]');
    await closed(driver);
    await undo(driver, value => value.decisions.length === before.decisions.length);
    assert.deepEqual((await project(driver)).decisions, before.decisions);
  });

  await step('Creative exception removal is deliberate and undo restores its exact scope and reason', async () => {
    const before = await project(driver);
    await driver.button('Project dossier');
    await driver.button('Exceptions');
    await driver.button('Remove exception');
    await waitFor(async () => (await project(driver)).exceptions.length === 0, 'The creative exception was not removed');
    await driver.click('button[aria-label="Close Project brief"]');
    await closed(driver);
    await undo(driver, value => value.exceptions.length === before.exceptions.length);
    assert.deepEqual((await project(driver)).exceptions, before.exceptions);
  });

  await step('Nested replacement dialogs return focus to their parent and then the New command', async () => {
    const before = await project(driver);
    await physicalClick('button[aria-label="Editor menu"]');
    await waitFor(() => driver.execute(() => document.activeElement?.getAttribute('aria-label') === 'Project'), 'The menu did not focus its first item');
    await physical.keys('Right');
    await waitFor(() => driver.execute(() => document.activeElement?.getAttribute('aria-label') === 'New project'), 'The Project submenu did not focus New project');
    await physical.keys('Return');
    await waitFor(() => driver.execute(() => !!document.querySelector('dialog[aria-label="New project"][open]')), 'Keyboard activation did not open New project');
    await driver.button('New blank page');
    await driver.fill('Project name', 'Cancelled nested project');
    await physicalClick('dialog[open] .new-project .form-actions .primary');
    await physical.keys('Escape');
    await waitFor(() => driver.execute(() => document.querySelectorAll('dialog[open]').length === 1
      && document.activeElement?.closest('dialog')?.getAttribute('aria-label') === 'New project'), 'Escape did not restore focus within the remaining parent dialog');
    await physical.keys('Escape');
    await closed(driver);
    assert.equal(await driver.execute(() => document.activeElement?.getAttribute('aria-label')), 'Editor menu');
    assert.deepEqual(await project(driver), before);
  });

  await step('New project uses the explicit discard choice and keeps the previous project recoverable', async () => {
    const before = await project(driver);
    await driver.menu(['New project']);
    await driver.button('New blank page');
    await driver.fill('Project name', 'Native custom recovery project');
    await driver.button('Web');
    await driver.select('Resolution', 'custom');
    await driver.fill('Width', 800);
    await driver.fill('Height', 600);
    // A new project opens in its own tab group; the unsaved project stays open, unchanged.
    await modalButton(driver, 'Create project');
    await closed(driver);
    const next = await waitFor(async () => { const value = await project(driver); return value.id !== before.id && value; }, 'The new project was not created');
    assert.deepEqual(next.viewport, { width: 800, height: 600 });
    assert.equal(next.appearance, 'light');
    assert.equal(next.generationMode, 'fuzzy');
    assert.deepEqual(next.pages[0].regions, []);
    const tabs = (await driver.invoke('get_workspace')).openProjects;
    assert.deepEqual(tabs.filter(open => [before.id, next.id].includes(open.id)).map(open => [open.id, open.dirty]), [[before.id, true], [next.id, false]]);
    await driver.click(`.project-tab-group[data-project-id="${before.id}"] .parked-tab`, 'css selector');
    const restored = await waitFor(async () => { const value = await project(driver); return value.id === before.id && value; }, 'Switching back to the earlier project did not work');
    assert.deepEqual(restored, before);
    await assertCanvasOnly(driver);
  });
  return project(driver);
}
