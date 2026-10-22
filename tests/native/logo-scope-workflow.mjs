import assert from 'node:assert/strict';
import { writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { waitFor } from './webdriver.mjs';

export async function logoScopeWorkflow(driver, physical, step, evidence, provider) {
  const workspace = () => driver.invoke('get_workspace');
  let project = (await workspace()).project;
  const asset = project.importedIdentity.assets[0];
  let generated;
  await step('Generate resolves the approved logo reference to its exact original image without page commentary', async () => {
    provider.setMode('complete', { logoScope:true });
    await driver.fill('Request 1', 'Show Northstar Studio, its approved logo and an image to edit locally. Leave one short image placeholder label.');
    await driver.button('Generate');
    generated = await waitFor(async () => {
      const view = await driver.invoke('get_generation',{projectId:project.id});
      if (['failed','unknown','blocked'].includes(view?.status)) throw new Error(JSON.stringify(view));
      const current = (await workspace()).project;
      return current.pages[0].content?.html.includes('project-logo') && current;
    }, 'The approved project logo did not apply', 25000);
    const record = provider.generationRequests().at(-1);
    assert.match(record.markdown, /Requirements format: 14/);
    assert.ok(record.markdown.includes(`desegnos-asset:${asset.id}`));
    assert.ok(!record.markdown.includes(asset.dataUri));
    assert.match(record.markdown, /never in page HTML/);
    assert.match(record.markdown, /small local nudges only/);
    assert.ok(generated.pages[0].content.html.includes(asset.dataUri));
    await waitFor(() => driver.execute(() => {
      const doc = document.querySelector('.static-preview iframe')?.contentDocument;
      const image = doc?.getElementById('project-logo');
      return image?.complete && image.naturalWidth === 128 && doc.body.textContent.trim() === 'Northstar StudioImage placeholder';
    }), 'The exact project logo did not render or the page contains extra notes');
    await physical.captureWindow(join(evidence, 'logo-generated.png'));
  });
  await step('The local image picker uses the approved project asset and changes only that image', async () => {
    await driver.button('Layers');
    await driver.ready('section[aria-label="Layers"] .layer-select[data-layer-selector$="img:nth-of-type(2)"]');
    await driver.click('section[aria-label="Layers"] .layer-select[data-layer-selector$="img:nth-of-type(2)"]');
    await driver.button('Close Layers');
    await driver.execute(()=>document.getElementById('page-canvas').focus({preventScroll:true}));await physical.keys('F2');await driver.ready('.workspace-surface[aria-label="Edit element"]');
    await driver.select('Project image', asset.id);
    await physical.captureWindow(join(evidence, 'project-image-picker.png'));
    await driver.button('Save changes');
    project = await waitFor(async () => {
      const current = (await workspace()).project;
      return current.revision > generated.revision && current;
    }, 'The local logo choice was not saved');
    assert.equal(project.pages[0].content.css, generated.pages[0].content.css);
    assert.equal(project.pages[0].content.javascript, generated.pages[0].content.javascript);
    await waitFor(() => driver.execute(uri => {
      const doc = document.querySelector('.static-preview iframe')?.contentDocument, image = doc?.getElementById('editable-image');
      return image?.getAttribute('src') === uri && image.complete && image.naturalWidth === 128;
    },asset.dataUri), 'The chosen original project image did not render');
    await physical.captureWindow(join(evidence, 'logo-locally-replaced.png'));
  });
  await step('Undo and Redo preserve the exact generated and locally edited sources without another provider call', async () => {
    const count = provider.generationRequests().length;
    await physical.keys('Z',1,['Control']);
    await waitFor(async () => JSON.stringify((await workspace()).project.pages[0].content) === JSON.stringify(generated.pages[0].content),'Undo did not restore the original image');
    await physical.keys('Z',1,['Control','Shift']);
    await waitFor(async () => JSON.stringify((await workspace()).project.pages[0].content) === JSON.stringify(project.pages[0].content),'Redo did not restore the exact project image');
    assert.equal(provider.generationRequests().length,count);
    project = (await workspace()).project;
    await writeFile(join(evidence,'logo-scope-project.json'),JSON.stringify(project,null,2));
  });
  return project;
}
