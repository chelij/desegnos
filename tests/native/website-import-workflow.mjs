import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { cp, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { chooseFolder, savedState } from './file-dialog-workflow.mjs';
import { waitFor } from './webdriver.mjs';

async function snapshot(root) {
  const files = {};
  for (const entry of await readdir(root, { withFileTypes: true })) {
    if (entry.isSymbolicLink()) continue;
    if (entry.isDirectory()) {
      if (['.git', 'node_modules'].includes(entry.name)) continue;
      for (const [path, hash] of Object.entries(await snapshot(join(root, entry.name)))) files[`${entry.name}/${path}`] = hash;
    } else files[entry.name] = createHash('sha256').update(await readFile(join(root, entry.name))).digest('hex');
  }
  return files;
}
export async function websiteImportWorkflow(driver, physical, step, evidence) {
  driver.requestTimeout = 120000;
  await driver.command('POST', '/timeouts', { script: 120000, implicit: 0, pageLoad: 120000 });
  const output = join(evidence, 'website-import'); await mkdir(output);
  const plain = join(output, 'Plain website'); await mkdir(plain);
  await writeFile(join(plain, 'index.html'), '<!doctype html><html><head><title>Plain home</title><link rel="stylesheet" href="site.css"></head><body><h1>Original heading</h1><a href="about.html">About</a><script type="module" src="main.js"></script><img src="pixel.svg"></body></html>');
  await writeFile(join(plain, 'about.html'), '<!doctype html><html><head><title>About</title><link rel="stylesheet" href="site.css"></head><body><h1>About us</h1><a href="index.html">Home</a><script type="module" src="main.js"></script></body></html>');
  await writeFile(join(plain, 'site.css'), 'h1{color:rgb(24,55,83);font-size:48px}');
  await writeFile(join(plain, 'pixel.svg'), '<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><rect width="20" height="20" fill="red"/></svg>');
  await writeFile(join(plain, 'main.js'), "import './module.js';");
  await writeFile(join(plain, 'module.js'), "document.body.dataset.scriptRan='yes';parent.postMessage({type:'website-test-page',title:document.title,colour:getComputedStyle(document.querySelector('h1')).color,link:document.querySelector('a').getBoundingClientRect().toJSON()},'*');");
  await physical.initialize(evidence);
  let project;
  await step('New project import offers website editing by default without AI', async () => {
    await driver.button('Use my existing brand');
    await chooseFolder(physical, () => driver.button('Choose folder'), plain, output, 'plain-folder');
    assert.equal(await driver.execute(() => [...document.querySelectorAll('button')].find(n => n.textContent === 'Edit this website')?.getAttribute('aria-pressed')), 'true');
    await driver.button('Open website for editing');
    project = (await savedState(driver, join(plain, 'Plain website.desegnos'))).project;
    assert.equal(project.pages.length, 2);
    await waitFor(() => driver.execute(() => document.querySelector('.static-preview[data-preview-ready="true"] iframe')?.contentDocument?.querySelector('h1')?.textContent === 'Original heading'), 'The imported heading did not render');
    assert.equal(await driver.execute(() => getComputedStyle(document.querySelector('.static-preview iframe').contentDocument.querySelector('h1')).color), 'rgb(24, 55, 83)');
  });
  await step('Imported heading is edited through the normal native canvas text editor and saved in place', async () => {
    await driver.button('Fit');
    const point = await driver.execute(() => {
      const frame = document.querySelector('.static-preview iframe'); const node = frame.contentDocument.querySelector('h1').firstChild;
      const range = frame.contentDocument.createRange(); range.setStart(node, 0); range.setEnd(node, 8);
      const rect = range.getBoundingClientRect(), outer = frame.getBoundingClientRect();
      const zoom = Number(document.querySelector('[data-testid="canvas"]').dataset.zoom);
      return { x: outer.x + (rect.x + 16) * zoom, y: outer.y + (rect.y + rect.height / 2) * zoom };
    });
    await physical.doubleClick(point); await driver.ready('.inline-text-editor[role="textbox"]');
    await physical.keys('A', 1, ['Control']); await physical.raw({ action: 'text', text: 'Edited website heading' }); await physical.keys('Return');
    await waitFor(async () => { const state = await driver.invoke('get_workspace'); return state.project.pages.some(p => p.content.html.includes('Edited website heading')); }, 'The heading edit did not reach the document');
    await driver.button('Save'); const state = await savedState(driver, join(plain, 'Plain website.desegnos')); project = state.project;
    assert.match(await readFile(join(plain, 'index.html'), 'utf8'), /Edited website heading/);
    assert.match(await readFile(join(plain, '.desegnos-original/index.html'), 'utf8'), /Original heading/);
    assert.equal(await readFile(join(plain, 'site.css'), 'utf8'), 'h1{color:rgb(24,55,83);font-size:48px}');
    await physical.captureWindow(join(output, 'edited-heading.png'));
  });
  await step('Canvas live mode executes original modules, shared CSS and relative page links while edit mode keeps scripts inert', async () => {
    assert.equal(await driver.execute(() => document.querySelector('.static-preview iframe').contentDocument.body.dataset.scriptRan), null);
    await driver.execute(() => { window.__websiteTestMessages = []; addEventListener('message', event => { if (event.data?.type === 'website-test-page') window.__websiteTestMessages.push(event.data); }); });
    await driver.button('Live');
    await waitFor(() => driver.execute(() => window.__websiteTestMessages.some(m => m.title === 'Plain home' && m.colour === 'rgb(24, 55, 83)')), 'The original module or root stylesheet did not run in live mode');
    const point = await driver.execute(() => {
      const frame = document.querySelector('.live-canvas-page iframe');
      const rect = window.__websiteTestMessages.at(-1).link, outer = frame.getBoundingClientRect();
      const zoom = Number(document.querySelector('[data-testid="canvas"]').dataset.zoom);
      return { x: outer.x + (rect.x + rect.width / 2) * zoom, y: outer.y + (rect.y + rect.height / 2) * zoom };
    });
    await physical.click(point);
    await waitFor(() => driver.execute(() => window.__websiteTestMessages.some(m => m.title === 'About')), 'The relative link did not open the other imported page');
    await driver.click('.live-toggle');
  });
  await step('Build site exposes progress and Stop without installing dependencies', async () => {
    const build = join(output, 'Stopped Astro'); await mkdir(join(build, 'src/pages'), { recursive: true }); await mkdir(join(build, 'node_modules'));
    await writeFile(join(build, 'package.json'), JSON.stringify({ dependencies: { astro: '*' }, scripts: { build: `node -e "console.log('Compiling fixture');setTimeout(()=>{},60000)"` } }));
    await writeFile(join(build, 'src/pages/index.astro'), '<h1>Source</h1>');
    await chooseFolder(physical, () => driver.menu(['Open project folder…']), build, output, 'stopped-build');
    await driver.button('Build site'); await driver.button('Stop');
    await waitFor(() => driver.execute(() => document.querySelector('.website-choice')?.textContent.includes('Build stopped.')), 'Stopping did not finish the build');
    await driver.button('Cancel');
  });
  await step('chelib Astro built pages load into a separate project without changing any source files', async () => {
    const source = '~/Projects/chelib'; const before = await snapshot(source);
    await chooseFolder(physical, () => driver.menu(['Open project folder…']), source, output, 'chelib-folder');
    await waitFor(() => driver.execute(() => [...document.querySelectorAll('button')].some(n => n.textContent === 'Build site')), 'The original chelib folder did not offer Build site');
    await driver.button('Cancel');
    const copy = join(output, 'chelib copy');
    await cp(source, copy, { recursive: true, verbatimSymlinks: true, filter: src => !src.split('/').some(p => ['.git', 'chelib-redesign'].includes(p)) });
    await chooseFolder(physical, () => driver.menu(['Open project folder…']), copy, output, 'chelib-copy-folder');
    await driver.button('Build site');
    await waitFor(() => driver.execute(() => [...document.querySelectorAll('button')].some(n => n.textContent === 'Open website for editing')), 'The private chelib build did not finish', 300000);
    await driver.button('Open website for editing');
    await waitFor(() => driver.execute(() => document.querySelector('.project-title')?.textContent.startsWith('chelib copy (Desegnos)')), 'chelib did not open', 120000);
    const state = await driver.invoke('get_workspace');
    project = state.project; assert.ok(!state.path.startsWith(source + '/'));
    assert.equal(project.website.framework, 'Astro'); assert.ok(project.pages.length > 0);
    await waitFor(() => driver.execute(() => !!document.querySelector('.static-preview[data-preview-ready="true"] iframe')), 'chelib canvas did not load');
    assert.deepEqual(await snapshot(source), before);
    await rm(join(copy, 'node_modules'), { recursive: true, force: true });
    await writeFile(join(output, 'chelib-pages.json'), JSON.stringify({ path: state.path, pages: project.pages.map(p => ({ name: p.name, path: p.path })) }, null, 2));
    await physical.captureWindow(join(output, 'chelib.png'));
    console.log(`chelib: ${project.pages.length} pages; project in ${dirname(state.path)}`);
  });
  return project;
}
