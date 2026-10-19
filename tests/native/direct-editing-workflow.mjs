import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { writeFile } from 'node:fs/promises';
import { chooseFile } from './file-dialog-workflow.mjs';
import { join } from 'node:path';
import { waitFor } from './webdriver.mjs';

export async function directEditingWorkflow(driver, physical, step, output, provider) {
  const workspace = () => driver.invoke('get_workspace');
  const initial = (await workspace()).project;
  const requests = provider.generationRequests().length;
  const setGrid = async settings => {
    await driver.button('Grid and rulers');
    await driver.select('Unit', settings.unit);
    await driver.fill('Grid spacing', settings.spacing);
    const current = (await workspace()).project.gridSettings;
    if (current.snap !== settings.snap) await driver.click(driver.field('Snap requests to the grid'), 'xpath');
    await driver.button('Save grid settings');
    await waitFor(async () => {
      const actual = (await workspace()).project.gridSettings;
      return Object.entries(settings).every(([key, value]) => actual[key] === value);
    }, 'The explicit grid settings did not persist');
  };
  const layout = () => driver.execute(() => {
    const frame = document.querySelector('.static-preview iframe'); const node = frame?.contentDocument?.querySelector('#fixture-action');
    if (!node) return null;
    const r = node.getBoundingClientRect(), f = frame.getBoundingClientRect(); const zoom = Number(document.querySelector('[data-testid="canvas"]').dataset.zoom);
    return { x: r.x, y: r.y, width: r.width, height: r.height, text: node.textContent, tag: node.localName,
      point: { x: f.x + (r.x + r.width / 2) * zoom, y: f.y + (r.y + r.height / 2) * zoom } };
  });
  const selected = () => driver.execute(() => !!document.querySelector('[data-testid="direct-element-selection"]'));
  const ready = async () => { await waitFor(async () => {
    const content=(await workspace()).project.pages[0].content;
    const digest=createHash('sha256').update(JSON.stringify({html:content.html,css:content.css,javascript:content.javascript})).digest('hex');
    return driver.execute(value => {
      const preview=document.querySelector('.static-preview'),frame=preview?.querySelector('iframe'),outline=document.querySelector('[data-testid="direct-element-selection"]');
      const node=frame?.contentDocument?.querySelector('#fixture-action');
      if(!node||!outline||preview.dataset.previewReady!=='true'||preview.dataset.sourceHash!==value||document.querySelector('.static-preview .preview-error,.canvas.element-transforming,.element-toolbar > button:disabled'))return false;
      const r=node.getBoundingClientRect(), f=frame.getBoundingClientRect(), b=outline.getBoundingClientRect(), z=Number(document.querySelector('[data-testid="canvas"]').dataset.zoom);
      return Math.max(Math.abs(b.x-f.x-r.x*z),Math.abs(b.y-f.y-r.y*z),Math.abs(b.width-r.width*z),Math.abs(b.height-r.height*z))<0.7;
    },digest);
  }, 'The committed source did not render with its matching outline'); };
  const clickElement = async () => {
    const content=(await workspace()).project.pages[0].content;
    const digest=createHash('sha256').update(JSON.stringify({html:content.html,css:content.css,javascript:content.javascript})).digest('hex');
    await waitFor(()=>driver.execute(value=>{const frame=document.querySelector('.static-preview');return frame?.dataset.previewReady==='true'&&frame.dataset.sourceHash===value;},digest),'The current editable preview did not become ready');
    const item=await layout(); const point={x:item.point.x+item.width*0.75/2-6,y:item.point.y};
    await physical.click(point);
    try { await waitFor(selected, 'Single-click did not select the item'); }
    catch (error) {
      await writeFile(join(output,'failed-element-click.json'),JSON.stringify({point,item,events:await physical.events(),native:await physical.inspect(),view:await driver.execute(at=>{
        const f=document.querySelector('.static-preview iframe'),r=f.getBoundingClientRect(),z=Number(document.querySelector('[data-testid="canvas"]').dataset.zoom);
        return{hit:document.elementFromPoint(at.x,at.y)?.outerHTML.slice(0,900),siteHit:f.contentDocument.elementFromPoint((at.x-r.x)/z,(at.y-r.y)/z)?.outerHTML,focus:document.activeElement?.outerHTML.slice(0,500),errors:window.__desegniTestErrors};
      },point)},null,2));
      throw error;
    }
  };
  const buttonPoint = text => driver.execute(label => {
    const node = [...document.querySelectorAll('.element-toolbar button')].find(node => node.getAttribute('aria-label') === label || node.textContent.trim() === label); const r = node.getBoundingClientRect(); return {x:r.x+r.width/2,y:r.y+r.height/2};
  }, text);
  await step('Single-click outlines the actual element and direct text/style edits preserve nested markup without provider calls', async () => {
    await driver.select('Zoom', '0.75'); await driver.press('[data-testid="canvas"]', '\uE011');
    await clickElement();
    assert.deepEqual((await workspace()).project, initial);
    assert.equal(await driver.execute(() => document.querySelectorAll('.annotation textarea').length), initial.pages[0].regions.filter(region => !region.completed).length);
    const alignment = await driver.execute(() => {
      const frame = document.querySelector('.static-preview iframe'), box = document.querySelector('[data-testid="direct-element-selection"]').getBoundingClientRect();
      const r = frame.contentDocument.querySelector('#fixture-action').getBoundingClientRect(), f=frame.getBoundingClientRect(), z=Number(document.querySelector('[data-testid="canvas"]').dataset.zoom);
      return { error:Math.max(Math.abs(box.x-f.x-r.x*z),Math.abs(box.y-f.y-r.y*z),Math.abs(box.width-r.width*z),Math.abs(box.height-r.height*z)) };
    });
    assert.ok(alignment.error < 1, JSON.stringify(alignment));
    await physical.captureWindow(join(output, 'direct-selection-window.png'));
    await driver.button('Edit'); await driver.fill('Text', 'Edited locally <safe> & clear');
    await driver.click('.element-appearance summary'); await driver.fill('Width', '280px'); await driver.button('Save changes');
    await waitFor(async () => (await layout())?.text === 'Edited locally <safe> & clear', 'The local text edit did not appear'); await ready();
    const content = (await workspace()).project.pages[0].content;
    assert.match(content.html, /<span>Edited locally &lt;safe&gt; &amp; clear<\/span>/);
    assert.equal(content.css, initial.pages[0].content.css); assert.equal(content.javascript, initial.pages[0].content.javascript);
    assert.equal((await layout()).width, 280);
    await physical.captureWindow(join(output, 'direct-edit-result-window.png'));
    assert.equal(provider.generationRequests().length, requests);
  });
  await step('Position offsets, rapid arrow keys and drag movement update the real element without AI', async () => {
    const start = await layout(); await physical.click(await buttonPoint('Move'));
    await driver.fill('Move horizontally (px)', 17); await driver.fill('Move vertically (px)', 13); await driver.button('Move element');
    await waitFor(async () => Math.abs((await layout())?.x-start.x-17)<0.5, 'Position offsets did not apply'); await ready();
    await driver.execute(() => document.getElementById('page-canvas').focus()); await physical.keys('Right', 8);
    await waitFor(async () => Math.abs((await layout())?.x-start.x-25)<0.5, 'Rapid arrow keys lost movement'); await ready();
    const prior = await layout(); const handle = prior.point;
    await physical.drag(handle, {x:handle.x+30,y:handle.y+20}, ['Alt']);
    await waitFor(async () => Math.abs((await layout())?.x-prior.x-40)<1 && Math.abs((await layout())?.y-prior.y-20/0.75)<1, 'Dragging the selected element did not apply its world offset'); await ready();
    assert.equal(provider.generationRequests().length, requests);
    await physical.captureWindow(join(output, 'direct-move-result-window.png'));
  });
  await step('Actual-size element drags snap to the grid, corners resize and the icon bar follows canvas panning', async () => {
    const before = (await workspace()).project;
    const configured = { ...before.gridSettings, spacing: 8, unit: 'pixels', snap: true };
    const changedGrid = Object.entries(configured).some(([key, value]) => before.gridSettings[key] !== value);
    if (changedGrid) await setGrid(configured);
    await driver.select('Zoom', '1'); await driver.press('[data-testid="canvas"]', '\uE011'); await clickElement(); await ready();
    const prior = await layout();
    await physical.drag(prior.point, {x:prior.point.x+11,y:prior.point.y+13});
    await waitFor(async()=>{const item=await layout();return item&&Math.abs(item.x-Math.round((prior.x+11)/8)*8)<.6&&Math.abs(item.y-Math.round((prior.y+13)/8)*8)<.6;},'Actual-size movement did not snap to grid'); await ready();
    await driver.button('Undo'); await clickElement(); await ready();
    const original = await layout(); const handle=await driver.execute(()=>{const b=document.querySelector('[data-corner="se"]').getBoundingClientRect();return{x:b.x+b.width/2,y:b.y+b.height/2};});
    await physical.drag(handle,{x:handle.x+11,y:handle.y+13});
    await waitFor(async()=>{const item=await layout();return item&&Math.abs(item.x+item.width-Math.round((original.x+original.width+11)/8)*8)<.6&&Math.abs(item.y+item.height-Math.round((original.y+original.height+13)/8)*8)<.6;},'Actual-size resizing did not snap its moving edges'); await ready();
    await driver.button('Undo'); await clickElement(); await ready();
    const pan=await driver.execute(()=>{const b=document.querySelector('[data-testid="workspace-scroll"]').getBoundingClientRect();return{x:b.right-80,y:b.bottom-90};});
    await physical.drag(pan,{x:pan.x-30,y:pan.y-20}); await ready();
    const bar=await waitFor(()=>driver.execute(()=>{const b=document.querySelector('.element-toolbar').getBoundingClientRect(),o=document.querySelector('[data-testid="direct-element-selection"]').getBoundingClientRect();const value={left:b.x-o.x,gap:o.y-b.bottom};return Math.abs(value.left)<.7&&Math.abs(value.gap-8)<.7&&value;}),'The toolbar did not follow the final pan position');
    assert.ok(Math.abs(bar.left)<.7&&Math.abs(bar.gap-8)<.7,JSON.stringify(bar));
    assert.deepEqual((await workspace()).project.pages,before.pages);
    if (changedGrid) await setGrid(before.gridSettings);
    await driver.select('Zoom','0.75'); await driver.press('[data-testid="canvas"]', '\uE011'); await clickElement(); await ready();
    assert.equal(provider.generationRequests().length,requests);
  });
  await step('Four real corner drags resize the actual element, anchor the opposite corner and keep the icon bar above its top-left', async () => {
    const beforeCorners=(await workspace()).project;
    for (const [corner,dx,dy] of [['nw',-15,-9],['ne',18,-6],['sw',-12,6],['se',15,9]]) {
      const before=await layout(); const handle=await driver.execute(value=>{const b=document.querySelector(`[data-corner="${value}"]`).getBoundingClientRect();return{x:b.x+b.width/2,y:b.y+b.height/2};},corner);
      await physical.drag(handle,{x:handle.x+dx,y:handle.y+dy},['Alt']);
      const west=corner.includes('w'), north=corner.includes('n');
      await waitFor(async()=>{const after=await layout();return after&&Math.abs(after.width-before.width-(west?-dx:dx)/0.75)<0.7&&Math.abs(after.height-before.height-(north?-dy:dy)/0.75)<0.7;},`The ${corner} corner did not resize the real element`);
      await ready(); const after=await layout();
      assert.ok(Math.abs((west?after.x+after.width:after.x)-(west?before.x+before.width:before.x))<0.7);
      assert.ok(Math.abs((north?after.y+after.height:after.y)-(north?before.y+before.height:before.y))<0.7);
      const bar=await driver.execute(()=>{const b=document.querySelector('.element-toolbar').getBoundingClientRect(),o=document.querySelector('[data-testid="direct-element-selection"]').getBoundingClientRect();return{left:b.x-o.x,gap:o.y-b.bottom,labels:[...document.querySelectorAll('.element-toolbar button')].map(b=>({name:b.getAttribute('aria-label'),text:b.textContent.trim(),title:b.title}))};});
      assert.ok(Math.abs(bar.left)<0.7&&Math.abs(bar.gap-8)<0.7,JSON.stringify(bar));
      assert.ok(bar.labels.every(b=>b.name&&b.title&&!b.text));
    }
    await physical.captureWindow(join(output,'corner-resize-icon-toolbar-window.png'));
    const before=(await workspace()).project;
    const handle=await driver.execute(()=>{const b=document.querySelector('[data-corner="se"]').getBoundingClientRect();return{x:b.x+b.width/2,y:b.y+b.height/2};});
    await physical.raw({action:'drag',from:physical.rootPoint(handle),to:physical.rootPoint({x:handle.x+30,y:handle.y+18}),cancelDrag:true});
    assert.deepEqual((await workspace()).project,before,'Escape committed a cancelled resize'); await ready();
    for(let index=0;index<4;index++)await driver.button('Undo');
    assert.deepEqual((await workspace()).project.pages,beforeCorners.pages,'Each corner gesture must undo in one step');
    await clickElement(); await ready();
    assert.equal(provider.generationRequests().length,requests);
  });
  await step('A focused resize corner opens local editing with trusted Space and preserves the site', async () => {
    const before = (await workspace()).project;
    await driver.execute(() => document.querySelector('[data-corner="se"]').focus());
    await physical.resetEvents(); await physical.keys('space');
    await waitFor(() => driver.execute(() => !!document.querySelector('dialog[aria-label="Edit element"][open]')), 'Space on the resize corner did not open local editing');
    assert.ok((await physical.events()).some(event => event.type === 'keydown' && event.trusted && event.key === ' '));
    assert.deepEqual((await workspace()).project, before);
    await driver.button('Cancel');
    await ready();
    assert.equal(provider.generationRequests().length, requests);
  });
  await step('A near-45-degree element resizes from a real corner without unstable dimensions or an anchor jump', async () => {
    const before = (await workspace()).project;
    const rotated = await driver.execute(() => {
      const clone = document.querySelector('.static-preview iframe').contentDocument.querySelector('#fixture-action').cloneNode(true);
      clone.style.transform = 'rotate(44.999deg)'; return clone.outerHTML;
    });
    await driver.button('Replace'); await driver.select('Replace with', 'html');
    await driver.fill('Replacement HTML', rotated); await driver.button('Replace element');
    await waitFor(() => driver.execute(() => document.querySelector('.static-preview iframe')?.contentDocument?.querySelector('#fixture-action')?.style.transform === 'rotate(44.999deg)'), 'The near-45-degree fixture did not render');
    await ready(); const rotatedBefore = await layout();
    const handle = await driver.execute(() => { const rect = document.querySelector('[data-corner="se"]').getBoundingClientRect(); return { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 }; });
    await physical.drag(handle, { x: handle.x + 10, y: handle.y + 10 }, ['Alt']);
    await waitFor(async () => { const after = await layout(); return after && after.width > rotatedBefore.width + 3 && after.height > rotatedBefore.height + 3; }, 'The rotated element did not resize');
    await ready(); const after = await layout();
    assert.ok(after.width < rotatedBefore.width + 100 && after.height < rotatedBefore.height + 100, 'Near-singular transform produced runaway dimensions');
    assert.ok(Math.abs(after.x - rotatedBefore.x) < 1 && Math.abs(after.y - rotatedBefore.y) < 1, 'The opposite corner moved during rotated resizing');
    await physical.captureWindow(join(output, 'rotated-corner-window.png'));
    await driver.button('Undo'); await driver.button('Undo');
    assert.deepEqual((await workspace()).project.pages, before.pages);
    await clickElement(); await ready();
    assert.equal(provider.generationRequests().length, requests);
  });
  await step('Right-click selects its element and offers local Edit or an on-page Generate prompt without submitting AI',async()=>{
    const link=()=>driver.execute(()=>{const f=document.querySelector('.static-preview iframe'),b=f.contentDocument.querySelector('#fixture-retained').getBoundingClientRect(),r=f.getBoundingClientRect(),z=Number(document.querySelector('[data-testid="canvas"]').dataset.zoom);return{x:r.x+(b.x+b.width-8)*z,y:r.y+(b.y+b.height/2)*z};});
    const before=(await workspace()).project;
    const inputPoint=await driver.execute(()=>{const f=document.querySelector('.static-preview iframe'),b=f.contentDocument.querySelector('#fixture-form input').getBoundingClientRect(),r=f.getBoundingClientRect(),z=Number(document.querySelector('[data-testid="canvas"]').dataset.zoom);return{x:r.x+(b.x+6)*z,y:r.y+(b.y+b.height/2)*z};});
    await physical.rightClick(inputPoint);
    await waitFor(()=>driver.execute(()=>!!document.querySelector('[role="menu"][aria-label="Element actions"]')),'Generated input did not offer editor actions');
    assert.ok(await driver.execute(()=>document.querySelector('[data-testid="direct-element-selection"]').dataset.selector.endsWith('input:nth-of-type(1)')));
    await physical.captureWindow(join(output,'generated-input-context-menu-window.png'));
    await physical.keys('Escape');
    await physical.resetEvents(); await physical.rightClick(await link());
    await driver.executeAsync(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(()=>resolve(true)))));
    await writeFile(join(output,'right-click-events.json'),JSON.stringify({events:await physical.events(),view:await driver.execute(()=>({selection:document.querySelector('[data-testid="direct-element-selection"]')?.dataset.selector,menus:[...document.querySelectorAll('[role="menu"]')].map(m=>m.outerHTML),errors:window.__desegniTestErrors}))},null,2));
    await waitFor(()=>driver.execute(()=>!!document.querySelector('[role="menu"][aria-label="Element actions"]')),'Right-click did not open element actions');
    assert.equal(await driver.execute(()=>document.querySelector('[data-testid="direct-element-selection"]').dataset.selector.endsWith('a:nth-of-type(1)')),true);
    assert.deepEqual((await workspace()).project,before);
    await physical.captureWindow(join(output,'right-click-element-actions-window.png'));
    await driver.click('[role="menu"][aria-label="Element actions"] button[aria-label="Edit"]');
    await driver.fill('Text','Local link edit'); await driver.fill('Link address','#local-link'); await driver.button('Save changes');
    await waitFor(()=>driver.execute(()=>document.querySelector('.static-preview iframe')?.contentDocument?.querySelector('#fixture-retained')?.getAttribute('href')==='#local-link'),'Context Edit did not change the link');
    await driver.button('Undo');
    await waitFor(()=>driver.execute(()=>document.querySelector('.static-preview iframe')?.contentDocument?.querySelector('#fixture-retained')?.textContent==='Open retained preview'),'Undo did not restore the link');
    await physical.rightClick(await link()); await physical.keys('Escape');
    assert.equal(await driver.execute(()=>!!document.querySelector('[role="menu"][aria-label="Element actions"]')),false);
    const button=await layout(); await physical.rightClick({x:button.point.x+button.width*0.75/2-6,y:button.point.y});
    await driver.click('[role="menu"][aria-label="Element actions"] button[aria-label="Generate"]');
    await waitFor(async()=> (await workspace()).project.pages[0].regions.some(r=>!r.completed&&r.element?.tag==='button'),'Context Generate did not open an element prompt');
    assert.equal(await driver.execute(()=>document.activeElement?.tagName==='TEXTAREA'),true);
    const promptPoint=await physical.center('.annotation textarea'); await physical.rightClick(promptPoint);
    await physical.captureWindow(join(output,'prompt-native-text-context-menu-window.png'));
    await physical.keys('Escape');
    assert.equal(provider.generationRequests().length,requests);
    await driver.execute(()=>document.getElementById('page-canvas').focus()); await driver.button('Undo');
    assert.deepEqual((await workspace()).project.pages,before.pages);
    await clickElement();
  });
  await step('Local replacement, reordering and deletion have working Undo and Redo', async () => {
    await driver.button('Replace'); await driver.select('Replace with','h2'); await driver.fill('Text', 'Replacement heading'); await driver.button('Replace element');
    await waitFor(async () => (await layout())?.tag === 'h2', 'Replacement was not applied'); await ready();
    assert.equal((await layout()).text, 'Replacement heading');
    await driver.menu(['Move later'], 'Element options'); await ready();
    assert.equal(await driver.execute(() => document.querySelector('.static-preview iframe').contentDocument.querySelector('#fixture-action').previousElementSibling.localName), 'a');
    const imagePath = join(output, 'local-direct-image.svg');
    await writeFile(imagePath, '<svg xmlns="http://www.w3.org/2000/svg" width="240" height="42"><rect width="240" height="42" fill="#007acc"/></svg>');
    await driver.button('Replace'); await driver.select('Replace with', 'img');
    await chooseFile(physical, () => driver.click('dialog[open] input[type=file]'), imagePath, output, 'direct-image-picker');
    await driver.fill('Image description','A locally chosen blue rectangle'); await driver.button('Replace element');
    await waitFor(async () => (await layout())?.tag === 'img', 'The chosen image was not inserted'); await ready();
    assert.equal(await driver.execute(() => { const image = document.querySelector('.static-preview iframe').contentDocument.querySelector('#fixture-action'); return image.naturalWidth; }),240);
    await driver.button('Undo'); await waitFor(async () => (await layout())?.tag === 'h2', 'Undo image replacement did not restore the heading');
    await clickElement();
    await driver.menu(['Delete element'], 'Element options'); await driver.button('Cancel'); assert.equal((await layout()).tag,'h2');
    await driver.menu(['Delete element'], 'Element options'); await driver.button('Delete element');
    await waitFor(async () => !(await layout()), 'Delete did not remove the item');
    await physical.keys('Z',1,['Control']); await waitFor(async () => (await layout())?.tag === 'h2', 'Undo did not restore the element');
    await physical.keys('Z',1,['Control','Shift']); await waitFor(async () => !(await layout()), 'Redo did not delete the element');
    assert.equal(provider.generationRequests().length, requests);
    // Restore the generated source through ordinary user Undo, retaining all original fixtures for generation checks.
    for (let count=0; count<30 && JSON.stringify((await workspace()).project.pages[0].content)!==JSON.stringify(initial.pages[0].content); count++) await driver.button('Undo');
    assert.deepEqual((await workspace()).project.pages, initial.pages);
    await waitFor(async () => (await layout())?.text === 'Try generated interaction', 'Undo did not restore the original generated page');
    await driver.execute(() => document.getElementById('page-canvas').focus()); await physical.keys('Escape');
  });
  await step('Written requests follow local element movement and invalid geometry never blocks Save or Undo', async () => {
    await physical.doubleClick((await layout()).point);
    const area = await waitFor(async () => (await workspace()).project.pages[0].regions.find(region => !region.completed && region.element), 'The element prompt did not open');
    const index = (await workspace()).project.pages[0].regions.findIndex(region => region.id === area.id);
    await driver.fill(`Request ${index+1}`, 'Keep this request attached to this button');
    await waitFor(async () => (await workspace()).project.pages[0].regions.find(region => region.id === area.id).prompt.trim(), 'The written prompt did not persist');
    await clickElement(); const before = await layout();
    await physical.click(await buttonPoint('Move')); await driver.fill('Move horizontally (px)',21); await driver.fill('Move vertically (px)',11); await driver.button('Move element');
    await waitFor(async () => { const r=(await workspace()).project.pages[0].regions.find(region => region.id === area.id); return Math.abs(r.rect.x-before.x-21)<0.5 && Math.abs(r.rect.y-before.y-11)<0.5; }, 'The written request retained stale geometry');
    const moved = (await workspace()).project.pages[0].regions.find(region => region.id === area.id);
    assert.equal(moved.prompt, 'Keep this request attached to this button');
    const details = await driver.invoke('inspect_element',{ projectId:initial.id, pageId:initial.homePageId, target:moved.element }); assert.equal(details.target.sourceSha256,moved.element.sourceSha256);
    const requirements = await driver.invoke('prepare_requirements'); assert.ok(requirements.markdown.includes(moved.element.sourceSha256));
    await ready(); await physical.click(await buttonPoint('Move')); await driver.fill('Move horizontally (px)',100000); await driver.button('Move element');
    await waitFor(async () => (await layout())?.x > 100000, 'The edge case did not move outside selectable bounds');
    await driver.button('Save'); await waitFor(async () => !(await workspace()).dirty, 'A measurement failure blocked Save');
    await driver.button('Undo'); await waitFor(async () => Math.abs((await layout())?.x-moved.rect.x)<0.5, 'A measurement failure blocked Undo');
    assert.equal(provider.generationRequests().length,requests);
    for (let count=0;count<30 && JSON.stringify((await workspace()).project.pages)!==JSON.stringify(initial.pages);count++) await driver.button('Undo');
    assert.deepEqual((await workspace()).project.pages,initial.pages);
    await driver.execute(() => document.getElementById('page-canvas').focus()); await physical.keys('Escape');
  });

}
