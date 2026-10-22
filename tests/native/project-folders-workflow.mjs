import assert from 'node:assert/strict';
import { mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { basename, dirname, join } from 'node:path';
import { chooseFile, chooseFolder, chooseProjectCopy, projectCopyPath, savedState, verifyArchive } from './file-dialog-workflow.mjs';
import { waitFor } from './webdriver.mjs';

export async function projectFoldersWorkflow(driver, physical, step, evidence) {
  const output = join(evidence, 'project-folders');
  const parent = join(output, 'chosen-parent');
  const preference = join(output, 'default-parent');
  await mkdir(parent, { recursive: true }); await mkdir(preference);
  await physical.initialize(evidence);
  const workspace = () => driver.invoke('get_workspace');
  let project, path, originalPath;

  const recentFixtures = [];
  await step('Start screen removes a recent project with the keyboard without opening or changing its files', async () => {
    for (const name of ['Remove this recent', 'Missing recent file', 'Missing recent folder']) {
      await driver.button('Blank website'); await driver.fill('Project name', name); await driver.button('Create project');
      const state = await waitFor(async () => { const state = await workspace(); return state.project?.name === name && state; }, 'The recent fixture was not created');
      recentFixtures.push({ name, path: state.path });
      await driver.menu(['Close project']);
      await waitFor(async () => !(await workspace()).project, 'Closing the fixture did not return to the start screen');
    }
    const fixture = recentFixtures[0];
    const archive = await readFile(fixture.path), website = await readFile(join(dirname(fixture.path), 'index.html'));
    const files = await readdir(dirname(fixture.path));
    const selector = `button[aria-label="Remove ${fixture.name} from recent projects"]`;
    await physical.raw({ action: 'move', point: physical.rootPoint({ x: 20, y: 20 }) });
    assert.equal(await driver.execute(query => getComputedStyle(document.querySelector(query)).opacity, selector), '0');
    const point = await driver.execute(query => { const rect = document.querySelector(query).getBoundingClientRect(); return { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 }; }, selector);
    await physical.raw({ action: 'move', point: physical.rootPoint(point) });
    assert.equal(await driver.execute(query => getComputedStyle(document.querySelector(query)).opacity, selector), '1');
    await physical.raw({ action: 'move', point: physical.rootPoint({ x: 20, y: 20 }) });
    await driver.execute(query => document.querySelector(query).focus(), selector);
    assert.equal(await driver.execute(query => getComputedStyle(document.querySelector(query)).opacity, selector), '1');
    assert.equal(await driver.execute(query => document.querySelector(query).title, selector), 'Remove from recent');
    await physical.captureWindow(join(output, 'recent-remove-focus.png'));
    await physical.keys('Return');
    await waitFor(() => driver.execute(query => !document.querySelector(query), selector), 'Removing the recent did not update the start screen');
    assert.equal((await workspace()).project, null);
    assert.deepEqual((await driver.invoke('get_recent_projects')).projects.map(item => item.path), recentFixtures.slice(1).reverse().map(item => item.path));
    assert.deepEqual(await readFile(fixture.path), archive);
    assert.deepEqual(await readFile(join(dirname(fixture.path), 'index.html')), website);
    assert.deepEqual(await readdir(dirname(fixture.path)), files);
    await driver.menu(['New project']);
    assert.equal(await driver.execute(query => !!document.querySelector(query), selector), false, 'The New project dialog kept a stale recent entry');
    await physical.keys('Escape');
  });

  for (const [index, kind] of [[1, 'file'], [2, 'folder']]) {
    await step(`A missing recent ${kind} offers removal in its error and never recreates the project`, async () => {
      const fixture = recentFixtures[index];
      const missingPath = kind === 'file' ? fixture.path : dirname(fixture.path);
      await rm(missingPath, { recursive: true });
      if (kind === 'folder') await driver.menu(['New project']);
      const selector = `${kind === 'folder' ? '.workspace-surface ' : ''}.recent-projects button[title=${JSON.stringify(fixture.path)}]`;
      await driver.click(selector);
      await driver.ready('.new-project [role="alert"]');
      assert.match(await driver.execute(() => document.querySelector('.new-project [role="alert"]').textContent), /Could not open this recent project/);
      assert.equal((await workspace()).project, null);
      await physical.captureWindow(join(output, `recent-missing-${kind}.png`));
      await driver.click('//form[contains(@class,"new-project")]//*[@role="alert"]//button[normalize-space(.)="Remove from recent"]', 'xpath');
      await waitFor(() => driver.execute(query => !document.querySelector(query) && !document.querySelector('.new-project [role="alert"]'), selector), 'The error action did not remove the missing recent');
      assert.equal((await driver.invoke('get_recent_projects')).projects.some(item => item.path === fixture.path), false);
      await assert.rejects(readFile(fixture.path), { code: 'ENOENT' });
      if (kind === 'folder') await physical.keys('Escape');
    });
  }

  await step('New project shows the isolated OS Documents default and a per-project parent picker', async () => {
    const location = await driver.invoke('get_project_location');
    assert.match(location.path, /\/dsgn-native-[^/]+\/Documents\/Desegnos$/);
    assert.equal(location.customized, false);
    await driver.button('Blank website'); await driver.fill('Project name', 'Studio: draft');
    assert.ok(await driver.execute(value => document.querySelector('.new-project').textContent.includes(value), location.path));
    await chooseFolder(physical, () => driver.button('Choose folder…'), parent, output, 'choose-parent');
    assert.equal((await driver.invoke('get_project_location')).path, location.path);
    await physical.captureWindow(join(output, 'new-location.png'));
    await driver.button('Create project');
    const state = await waitFor(async () => { const state = await workspace(); return state.project?.name === 'Studio: draft' && state; }, 'The folder-backed project was not created');
    path = join(parent, 'Studio_ draft', 'Studio_ draft.desegnos'); originalPath = path;
    assert.equal(state.path, path); assert.equal(state.projectFolder, true); assert.equal(state.dirty, false);
    assert.deepEqual(state.siteWarnings, []);
    project = state.project;
    await verifyArchive(path, project, await driver.invoke('get_project_notes'));
    assert.match(await readFile(join(dirname(path), 'index.html'), 'utf8'), /<html/);
  });

  await step('Save has no picker and keeps the archive, styles, scripts and nested pages in sync', async () => {
    project = (await driver.invoke('edit_project', { revision: project.revision, command: { type: 'add_page', name: 'About', path: '/about' } })).project;
    for (const page of project.pages) {
      project = (await driver.invoke('edit_page_source', { projectId: project.id, pageId: page.id, revision: project.revision,
        html: `<main><h1>${page.name}</h1><p>Folder workflow</p></main>`, css: 'h1 { color: #245583; }', javascript: "document.documentElement.dataset.saved = 'yes';" })).project;
    }
    await driver.button('Save'); await savedState(driver, path);
    await verifyArchive(path, project, await driver.invoke('get_project_notes'));
    assert.match(await readFile(join(dirname(path), 'about/index.html'), 'utf8'), /About/);
    assert.match(await readFile(join(dirname(path), 'styles.css'), 'utf8'), /245583/);
    const manifest = JSON.parse(await readFile(join(dirname(path), '.desegnos-site.json'), 'utf8'));
    assert.ok(Object.keys(manifest.files).some(p => p.endsWith('.js')));
    assert.equal((await physical.inspect()).windows.length, 1);
  });

  await step('Removing a page removes only its generated file and preserves foreign data', async () => {
    const root = dirname(path);
    await writeFile(join(root, 'about/keep.txt'), 'Foreign nested file');
    await writeFile(join(root, 'owner-notes.txt'), 'Foreign notes');
    const about = project.pages.find(page => page.path === '/about');
    project = (await driver.invoke('edit_project', { revision: project.revision, command: { type: 'delete_page', page_id: about.id } })).project;
    await driver.button('Save'); await savedState(driver, path);
    assert.equal((await readdir(join(root, 'about'))).includes('index.html'), false);
    assert.equal(await readFile(join(root, 'about/keep.txt'), 'utf8'), 'Foreign nested file');
    assert.equal(await readFile(join(root, 'owner-notes.txt'), 'utf8'), 'Foreign notes');
  });

  await step('An external HTML edit produces a visible warning while the full project still saves', async () => {
    const index = join(dirname(path), 'index.html');
    await writeFile(index, 'Owner-edited HTML');
    project = (await driver.invoke('edit_project', { revision: project.revision, command: { type: 'set_grid', enabled: false } })).project;
    await driver.button('Save'); const state = await savedState(driver, path);
    assert.ok(state.siteWarnings.some(s => s.includes('index.html')));
    await waitFor(() => driver.execute(() => [...document.querySelectorAll('.message.warning')].some(node => node.textContent.includes('index.html'))), 'The site warning was not visible');
    await verifyArchive(path, project, await driver.invoke('get_project_notes'));
    assert.equal(await readFile(index, 'utf8'), 'Owner-edited HTML');
    await physical.captureWindow(join(output, 'site-warning.png'));
    await driver.button('Dismiss website warning');
    await driver.button('Save');
    await waitFor(() => driver.execute(() => [...document.querySelectorAll('.message.warning')].some(node => node.textContent.includes('index.html'))), 'A repeated sync failure did not show its warning again');
    await rm(index); await driver.button('Save');
    await waitFor(async () => (await workspace()).siteWarnings.length === 0, 'Retry did not finish syncing');
  });

  await step('Save As cancellation is inert and a saved copy gets its own live project folder', async () => {
    const original = await readFile(path);
    await chooseFile(physical, () => driver.menu(['Save as']), null, output, 'cancel-copy', true);
    assert.equal((await workspace()).path, path);
    const copy = projectCopyPath(join(output, 'Saved copy.desegnos'));
    await chooseProjectCopy(physical, () => driver.menu(['Save as']), copy, output, 'save-copy');
    await savedState(driver, copy); path = copy;
    assert.deepEqual(await readFile(originalPath), original);
    assert.match(await readFile(join(dirname(copy), 'index.html'), 'utf8'), /Folder workflow/);
    await verifyArchive(copy, project, await driver.invoke('get_project_notes'));
  });

  await step('Default location change persists and repeated names reserve numbered folders', async () => {
    await driver.menu(['Default project location']);
    await driver.click('.workspace-surface summary');
    await chooseFolder(physical, () => driver.button('Change…'), preference, output, 'default-location');
    await waitFor(async () => (await driver.invoke('get_project_location')).path === preference, 'The default preference was not saved');
    await physical.captureWindow(join(output, 'default-setting.png'));
    await driver.button('Close Project location');
    for (const number of [1, 2]) {
      await driver.menu(['New project']); await driver.button('Blank website'); await driver.fill('Project name', 'Repeated');
      await driver.button('Create project');
      const name = number === 1 ? 'Repeated' : 'Repeated 2';
      const state = await savedState(driver, join(preference, name, `${name}.desegnos`));
      assert.equal(state.projectFolder, true);
    }
  });

  await step('Standalone legacy archives save in place and Move changes only their active destination', async () => {
    await driver.menu(['Close project']);
    // Close the other open groups so the copied identity is opened as a separate saved session.
    while ((await workspace()).project) await driver.menu(['Close project']);
    const legacy = join(output, 'Legacy.desegni');
    await writeFile(legacy, await readFile(path), { mode: 0o600 });
    await chooseFile(physical, () => driver.menu(['Open project']), legacy, output, 'open-legacy');
    const opened = await workspace(); project = opened.project;
    assert.equal(opened.projectFolder, false);
    await driver.button('Save'); await savedState(driver, legacy);
    const prior = await readFile(legacy);
    await driver.menu(['Move into project folder']);
    const moved = await waitFor(async () => { const value = await workspace(); return value.projectFolder && value.path !== legacy && value; }, 'The legacy destination was not moved');
    assert.equal(dirname(dirname(moved.path)), preference);
    assert.deepEqual(await readFile(legacy), prior);
    assert.ok(!(await driver.invoke('get_recent_projects')).projects.some(item => item.path === legacy));
    await verifyArchive(moved.path, project, await driver.invoke('get_project_notes'));
  });

  const site = join(output, 'Existing website');
  const originals = new Map([
    ['index.html', '<!doctype html><html><head><title>Existing home</title><link rel="stylesheet" href="site.css"><script src="main.js" defer></script></head><body><h1>Original home</h1><a href="about.html">About</a></body></html>'],
    ['about.html', '<html><head><title>Existing about</title></head><body><h1>About the site</h1><a href="index.html">Home</a></body></html>'],
    ['site.css', 'h1 { color: #425b91; }'], ['main.js', "document.documentElement.dataset.imported = 'yes';"],
  ]);
  await mkdir(site); for (const [name, source] of originals) await writeFile(join(site, name), source);

  await step('Open folder imports editable pages and preserves original HTML, CSS and JavaScript once', async () => {
    await chooseFolder(physical, () => driver.menu(['Open project folder…']), site, output, 'import-website');
    await driver.button('Open website for editing');
    const state = await savedState(driver, join(site, `${basename(site)}.desegnos`));
    path = state.path; project = state.project;
    assert.equal(project.pages.length, 2); assert.equal(state.projectFolder, true);
    assert.ok(project.pages.every(page => page.content.html.includes('data-desegnos-id')));
    for (const [name, source] of originals) {
      assert.equal(await readFile(join(site, name), 'utf8'), source);
      assert.equal(await readFile(join(site, '.desegnos-original', name), 'utf8'), source);
    }
    const home = project.pages.find(page => page.id === project.homePageId);
    project = (await driver.invoke('edit_page_source', { projectId: project.id, revision: project.revision, pageId: home.id, html: home.content.html.replace('Original home', 'Edited imported home') })).project;
    await driver.button('Save'); await savedState(driver, path);
    assert.deepEqual((await workspace()).siteWarnings, []);
    assert.match(await readFile(join(site, 'index.html'), 'utf8'), /Edited imported home/);
    assert.match(await readFile(join(site, 'about.html'), 'utf8'), /About the site/);
    for (const [name, source] of originals) assert.equal(await readFile(join(site, '.desegnos-original', name), 'utf8'), source);
    await verifyArchive(path, project, await driver.invoke('get_project_notes'));
    await physical.captureWindow(join(output, 'imported-website.png'));
  });

  await step('Open folder reopens its saved archive with full context and fresh history', async () => {
    await driver.menu(['Close project']);
    await chooseFolder(physical, () => driver.menu(['Open project folder…']), site, output, 'open-project-folder');
    const state = await savedState(driver, path);
    assert.deepEqual(state.project, project); assert.equal(state.canUndo, false);
    const recent = await driver.invoke('get_recent_projects');
    assert.equal(recent.projects[0].path, path);
    await driver.menu(['Default project location']); await driver.click('.workspace-surface summary'); await driver.button('Reset');
    const location = await waitFor(async () => { const value = await driver.invoke('get_project_location'); return !value.customized && value; }, 'Reset did not restore Documents/Desegnos');
    assert.equal(location.path, location.systemDefault); await driver.button('Close Project location');
  });
  return project;
}
