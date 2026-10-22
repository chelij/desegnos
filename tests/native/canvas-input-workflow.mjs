import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { waitFor } from './webdriver.mjs';
import { chooseFile, savedState, verifyArchive } from './file-dialog-workflow.mjs';

const workspace = driver => driver.invoke('get_workspace');
const settled = driver => driver.executeAsync(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve(true)))));
const measurementClear = (driver, number) => driver.execute(number => {
  const label=document.querySelector('.workspace-measurement'), text=document.querySelector(`textarea[aria-label="Request ${number}"]`);
  if (!label || !text) return true;
  const a=label.getBoundingClientRect(),b=text.getBoundingClientRect();
  return a.right<=b.left || a.left>=b.right || a.bottom<=b.top || a.top>=b.bottom;
},number);

async function sitePoint(driver) {
  return driver.execute(() => {
    const frame = document.querySelector('.static-preview iframe'), element = frame?.contentDocument?.querySelector('#fixture-action');
    if (!frame || !element) throw new Error('The real generated action is missing');
    const outer = frame.getBoundingClientRect(), child = element.getBoundingClientRect(), canvas = document.querySelector('[data-testid="canvas"]'), stage = canvas.getBoundingClientRect(), zoom = Number(canvas.dataset.zoom);
    const point = { x: outer.x + (child.x + child.width / 2) * zoom, y: outer.y + (child.y + child.height / 2) * zoom };
    const mapped = { x: (point.x - stage.left) / zoom + Number(canvas.dataset.worldLeft), y: (point.y - stage.top) / zoom + Number(canvas.dataset.worldTop) };
    return { point, zoom, outer: { x:outer.x,y:outer.y,width:outer.width,height:outer.height }, child: { x:child.x,y:child.y,width:child.width,height:child.height }, mapped, iframeScroll: { x:frame.contentWindow.scrollX,y:frame.contentWindow.scrollY }, hit:frame.contentDocument.elementFromPoint(mapped.x,mapped.y)?.id, visible: !!document.elementFromPoint(point.x,point.y)?.closest('[data-testid="canvas"]') };
  });
}

export async function canvasInputWorkflow(driver, physical, step, evidence, provider) {
  const output = join(evidence, 'canvas-input'); await mkdir(output, { recursive:true });
  let id;
  await step('Create a real canvas and generated action for pointer accuracy checks', async () => {
    await driver.button('Blank website'); await driver.fill('Project name', 'Canvas pointer and size'); await driver.button('Create project');
    id = await waitFor(async () => (await workspace(driver)).project?.id, 'The canvas project did not open');
    await physical.initialize(evidence); await driver.click('.canvas-empty-hint button'); await driver.fill('Request 1', 'Make a clear page with an interactive action.');
    await driver.invoke('set_ai_key', { key:provider.key(), storage:'session' }); provider.setMode('complete', { editorAudit:true }); await driver.button('Generate');
    await waitFor(async () => (await driver.invoke('get_generation', { projectId:id }))?.status === 'applied', 'The controlled result did not apply',25000);
    await waitFor(() => driver.execute(() => document.querySelector('.static-preview')?.dataset.previewReady === 'true'), 'The accepted canvas did not render');
    await driver.button('Keep changes');
    await driver.button('Fit'); await settled(driver);
  });
  await step('Trusted site clicks remain under the pointer after combined zoom and panning', async () => {
    const observations=[];
    for (const direction of ['up','up','down','up','down','down']) {
      const before = await sitePoint(driver); assert.equal(before.visible,true,JSON.stringify(before));
      await physical.wheel(before.point,direction,['Control'],2); await settled(driver);
      const view = await physical.rect('.workspace-scroll'); const start={x:view.right-100,y:view.top+85};
      await physical.drag(start,{x:start.x+32,y:start.y+20}); await settled(driver);
      const at=await sitePoint(driver); observations.push(at); await writeFile(join(output,'pointer-observations.json'),JSON.stringify(observations,null,2));
      assert.equal(at.visible,true,JSON.stringify(at)); await physical.click(at.point);
      const selected=await waitFor(() => driver.execute(() => document.querySelector('[data-testid="direct-element-selection"]')?.dataset.selector),'Clicking the visible action selected nothing');
      assert.match(selected,/button:nth-of-type\(1\)$/);
      const mismatch=await driver.execute(() => { const frame=document.querySelector('.static-preview iframe'), child=frame.contentDocument.querySelector('#fixture-action').getBoundingClientRect(), outer=frame.getBoundingClientRect(), selected=document.querySelector('[data-testid="direct-element-selection"]').getBoundingClientRect(), zoom=Number(document.querySelector('[data-testid="canvas"]').dataset.zoom);return {x:selected.x-(outer.x+child.x*zoom),y:selected.y-(outer.y+child.y*zoom)}; });
      assert.ok(Math.abs(mismatch.x)<1 && Math.abs(mismatch.y)<1,JSON.stringify({at,mismatch}));
    }
    await physical.captureWindow(join(output,'pointer-after-pan-zoom.png'));
  });
  await step('Long pans rebase the browser window without drifting the pointer or growing scroll geometry', async () => {
    await driver.click('[aria-label="Canvas tools"] button[title^="Pan"]');
    const observations=[];
    for (const delta of [{x:-700,y:0},{x:-700,y:0},{x:0,y:-470},{x:0,y:-470},{x:700,y:0},{x:700,y:0},{x:0,y:470},{x:0,y:470}]) {
      const view=await physical.rect('.workspace-scroll'),from={x:delta.x>0?view.left+160:delta.x<0?view.right-160:view.left+view.width/2,y:delta.y>0?view.top+100:delta.y<0?view.bottom-140:view.top+view.height/2};
      const camera=()=>driver.execute(()=>{const stage=document.querySelector('[data-testid="canvas"]'),view=document.querySelector('.workspace-scroll'),zoom=Number(stage.dataset.zoom);return {x:Number(stage.dataset.worldLeft)+view.scrollLeft/zoom,y:Number(stage.dataset.worldTop)+view.scrollTop/zoom,width:view.scrollWidth,height:view.scrollHeight,zoom};});
      const before=await camera(); await physical.drag(from,{x:from.x+delta.x,y:from.y+delta.y}); await settled(driver); const after=await camera();
      observations.push({before,after,delta});
      assert.ok(Math.abs(after.x-before.x+delta.x/before.zoom)<3/before.zoom && Math.abs(after.y-before.y+delta.y/before.zoom)<3/before.zoom,JSON.stringify(observations.at(-1)));
      assert.ok(after.width<7000 && after.height<7000,JSON.stringify(after));
    }
    await writeFile(join(output,'rebasing-pans.json'),JSON.stringify(observations,null,2));
    await driver.click('[aria-label="Canvas tools"] button[title^="Select"]'); await driver.button('Fit'); await settled(driver);
  });
  await step('A request placed near the bottom-right edge stays anchored at the cursor when focused', async () => {
    await driver.click('[aria-label="Canvas tools"] button[title^="Add request"]');
    const view=await physical.rect('.workspace-scroll'),point={x:view.right-45,y:view.bottom-105};
    await physical.click(point);
    const region=await waitFor(async () => (await workspace(driver)).project.pages[0].regions.find(region=>!region.completed),'The edge request did not appear');
    await driver.fill('Request 2','Keep this box where I clicked.'); await settled(driver);
    const actual=await driver.execute(id=>{const box=document.querySelector(`[data-region-id="${id}"]`).getBoundingClientRect();return {x:box.x,y:box.y};},region.id);
    await writeFile(join(output,'edge-placement.json'),JSON.stringify({point,actual},null,2));
    assert.ok(Math.abs(actual.x-point.x)<8 && Math.abs(actual.y-point.y)<8,JSON.stringify({point,actual}));
    await physical.keys('Escape'); await driver.click('.element-toolbar [aria-label="Delete layer"]'); await driver.button('Remove');
  });
  await step('A drawn request keeps the full height the user dragged instead of stopping at half the view', async () => {
    await driver.button('Fit'); await driver.button('Grid and rulers'); await driver.click(driver.field('Snap requests to the grid'),'xpath'); await driver.button('Save grid settings');
    await driver.click('[aria-label="Canvas tools"] button[title^="Add request"]');
    const view=await physical.rect('.workspace-scroll'), from={x:view.left+80,y:view.top+45},to={x:view.left+410,y:view.bottom-90};
    await physical.drag(from,to); await driver.fill('Request 2','Preserve the full area I marked.'); await settled(driver);
    const region=await waitFor(async () => (await workspace(driver)).project.pages[0].regions.find(region=>!region.completed),'The drawn request did not persist');
    const drawn=await driver.execute(id=>{ const node=document.querySelector(`[data-region-id="${id}"]`),rect=node.getBoundingClientRect();return {height:rect.height,zoom:Number(document.querySelector('[data-testid="canvas"]').dataset.zoom)}; },region.id);
    await writeFile(join(output,'drawn-height.json'),JSON.stringify({region,drawn,from,to},null,2));
    assert.ok(Math.abs(drawn.height-region.rect.height*drawn.zoom)<2,JSON.stringify({region,drawn}));
  });
  await step('Large request boxes pass the former coordinate and dimension limits through real drawing and local history', async () => {
    for (let index=0; index<4; index++) await driver.button('Zoom out');
    await waitFor(() => driver.execute(() => Number(document.querySelector('[data-testid="canvas"]').dataset.zoom)===.001),'The canvas did not zoom out for large-area drawing');
    const view=await physical.rect('.workspace-scroll'),from={x:view.left+130,y:view.top+145},to={x:view.left+730,y:view.top+495};
    await driver.click('[aria-label="Canvas tools"] button[title^="Add request"]'); await physical.drag(from,to);
    await driver.fill('Request 3','A large area that must keep its complete placement.'); await settled(driver);
    const before=(await workspace(driver)).project,region=before.pages[0].regions.at(-1);
    assert.ok(region.rect.width>500000 && region.rect.height>300000,JSON.stringify(region));
    const drawn=await physical.rect(`[data-region-id="${region.id}"]`); assert.ok(Math.abs(drawn.width-region.rect.width*.001)<2 && Math.abs(drawn.height-region.rect.height*.001)<2);
    // Move a real grip, without changing its intended dimensions.
    const grip=await physical.rect('[aria-label="Move request 3"]'),start={x:grip.left+grip.width/2,y:grip.top+grip.height/2}; await physical.drag(start,{x:start.x+45,y:start.y+30}); await settled(driver);
    const moved=await waitFor(async()=>{const project=(await workspace(driver)).project; return project.revision>before.revision && project;},'Moving the large request was not saved');
    assert.equal(moved.pages[0].regions.at(-1).rect.width,region.rect.width); assert.equal(moved.pages[0].regions.at(-1).rect.height,region.rect.height);
    assert.ok(Math.abs(moved.pages[0].regions.at(-1).rect.x-region.rect.x-45000)<2);
    await driver.button('Undo'); await waitFor(async()=>JSON.stringify((await workspace(driver)).project.pages[0].regions.at(-1).rect)===JSON.stringify(region.rect),'Undo did not restore the large rectangle');
    await driver.button('Redo'); await waitFor(async()=>JSON.stringify((await workspace(driver)).project.pages[0].regions.at(-1).rect)===JSON.stringify(moved.pages[0].regions.at(-1).rect),'Redo did not restore the large movement');
    const resize=await physical.rect('[aria-label="Resize request 3"]'),corner={x:resize.left+resize.width/2,y:resize.top+resize.height/2};
    await physical.drag(corner,{x:corner.x+80,y:corner.y+60}); await settled(driver);
    const resized=await waitFor(async()=>{const project=(await workspace(driver)).project;return project.revision>moved.revision && project;},'The oversized resize did not persist');
    const size=resized.pages[0].regions.at(-1).rect;
    assert.ok(Math.abs(size.width-region.rect.width-80000)<2 && Math.abs(size.height-region.rect.height-60000)<2,JSON.stringify(size));
    await driver.button('Undo'); await waitFor(async()=>JSON.stringify((await workspace(driver)).project.pages[0].regions.at(-1).rect)===JSON.stringify(moved.pages[0].regions.at(-1).rect),'Undo did not restore the oversized resize');
    await driver.button('Redo'); await waitFor(async()=>JSON.stringify((await workspace(driver)).project.pages[0].regions.at(-1).rect)===JSON.stringify(size),'Redo did not restore the oversized resize');
    await writeFile(join(output,'large-geometry.json'),JSON.stringify({drawn:region.rect,moved:moved.pages[0].regions.at(-1).rect,resized:size},null,2));
  });
  await step('A Shift-drawn circle has a round outline and actual-shape hit testing', async () => {
    // Park the enormous existing request away from the guide before drawing there.
    // Its textarea otherwise covers the entire guide at ordinary zoom.
    const largeGrip=await physical.rect('[aria-label="Move request 3"]'), largeFrom={x:largeGrip.left+largeGrip.width/2,y:largeGrip.top+largeGrip.height/2};
    await physical.drag(largeFrom,{x:largeFrom.x+650,y:largeFrom.y}); await settled(driver);
    await driver.button('Fit'); await driver.menu(['Circle'], 'Request shape'); await settled(driver);
    const view = await physical.rect('.workspace-scroll'), from = { x:view.left+view.width*.55, y:view.top+90 };
    await physical.drag(from, { x:from.x+120, y:from.y+75 }, ['Shift']);
    await driver.fill('Request 4', 'Edit inside the circle, preserving the corners.');
    const region = await waitFor(async () => (await workspace(driver)).project.pages[0].regions.find(region => region.shape?.kind==='ellipse'), 'The circle did not persist');
    assert.ok(Math.abs(region.rect.width-region.rect.height)<.01, JSON.stringify(region));
    assert.equal(await driver.execute(id => !!document.querySelector(`[data-testid="area-outline"][data-region-id="${id}"] ellipse`),region.id), true);
    await physical.keys('Escape'); await driver.click('[aria-label="Canvas tools"] button[title^="Select"]');
    const outline = await physical.rect(`[data-testid="area-outline"][data-region-id="${region.id}"]`);
    await physical.click({x:outline.left+4,y:outline.top+4});
    await waitFor(() => driver.execute(id => !document.querySelector(`[data-testid="area-outline"][data-region-id="${id}"]`).classList.contains('selected'),region.id),'Clicking a circle corner incorrectly selected the area');
    await physical.click({x:outline.left+outline.width/2,y:outline.top+outline.height/2});
    await waitFor(() => driver.execute(id => document.querySelector(`[data-testid="area-outline"][data-region-id="${id}"]`).classList.contains('selected'),region.id),'Clicking inside the circle did not select it');
    await physical.keys('V'); await physical.keys('R');
    assert.equal(await driver.execute(() => localStorage.getItem('desegnos.request-shape')), 'ellipse');
    assert.equal(await driver.execute(() => document.querySelector('[aria-label="Request shape"]').textContent.includes('Circle')),true);
    assert.equal(await measurementClear(driver,4),true,'The size label covers the circle prompt');
    await physical.captureWindow(join(output,'circle-outline.png'));
  });
  await step('A native lasso closes, keeps relative points through move/resize, and supports Undo/Redo', async () => {
    await physical.keys('L');
    const view=await physical.rect('.workspace-scroll'), start={x:view.left+view.width*.78,y:view.top+view.height*.5};
    const points=[[0,0],[70,-10],[105,35],[90,85],[35,100],[-20,65],[-30,15],[-5,5]].map(([x,y])=>({x:start.x+x,y:start.y+y}));
    await physical.path(points); await driver.fill('Request 5','Only edit the closed freehand area.');
    const region=await waitFor(async()=> (await workspace(driver)).project.pages[0].regions.find(region=>region.shape?.kind==='freehand'),'The freehand region did not persist');
    assert.ok(region.shape.points.length>=3 && region.shape.points.length<=200);
    assert.ok(region.shape.points.every(point=>point.length===2 && point.every(n=>Number.isFinite(n)&&n>=0&&n<=1)));
    assert.equal(await driver.execute(id=>document.querySelector(`[data-testid="area-outline"][data-region-id="${id}"] path`)?.getAttribute('d').endsWith('Z'),region.id),true);
    await physical.keys('Escape');
    const grip=await physical.rect('[aria-label="Move request 5"]'), from={x:grip.left+grip.width/2,y:grip.top+grip.height/2};
    await physical.drag(from,{x:from.x-25,y:from.y+10});
    const moved=await waitFor(async()=>{const r=(await workspace(driver)).project.pages[0].regions.find(r=>r.id===region.id);return r.rect.x!==region.rect.x&&r;},'The lasso move did not persist');
    assert.deepEqual(moved.shape,region.shape);
    const resize=await physical.rect('[aria-label="Resize request 5"]'), corner={x:resize.left+resize.width/2,y:resize.top+resize.height/2};
    await physical.drag(corner,{x:corner.x+30,y:corner.y+20});
    const resized=await waitFor(async()=>{const r=(await workspace(driver)).project.pages[0].regions.find(r=>r.id===region.id);return r.rect.width!==moved.rect.width&&r;},'The lasso resize did not persist');
    assert.deepEqual(resized.shape,region.shape);
    await driver.button('Undo'); await waitFor(async()=>JSON.stringify((await workspace(driver)).project.pages[0].regions.find(r=>r.id===region.id).rect)===JSON.stringify(moved.rect),'Undo did not restore lasso bounds');
    await driver.button('Redo'); await waitFor(async()=>JSON.stringify((await workspace(driver)).project.pages[0].regions.find(r=>r.id===region.id).rect)===JSON.stringify(resized.rect),'Redo did not restore lasso bounds');
    assert.equal(await measurementClear(driver,5),true,'The size label covers the freehand prompt');
    await writeFile(join(output,'shape-geometry.json'),JSON.stringify({circle:(await workspace(driver)).project.pages[0].regions.find(r=>r.shape?.kind==='ellipse'),drawn:region,moved,resized},null,2));
    await physical.captureWindow(join(output,'freehand-outline.png'));
  });
  await step('Escape cancels a lasso without leaving a partial area', async () => {
    await driver.click('[aria-label="Canvas tools"] button[title^="Add request"]');
    const before=(await workspace(driver)).project.pages[0].regions.length;
    const view=await physical.rect('.workspace-scroll'),from={x:view.right-75,y:view.top+75};
    await physical.path([from,{x:from.x-50,y:from.y+40},{x:from.x-20,y:from.y+70},{x:from.x+15,y:from.y+30}],true); await settled(driver);
    assert.equal((await workspace(driver)).project.pages[0].regions.length,before);
    assert.equal(await driver.execute(()=>!!document.querySelector('[data-testid="area-draft"]')),false);
  });
  await step('Rectangles, circles and lassos save and reopen byte-exactly as a Desegnos project', async () => {
    const destination=(await driver.invoke('get_workspace')).path;
    await driver.button('Save');
    const saved=await savedState(driver,destination); await verifyArchive(destination,saved.project,await driver.invoke('get_project_notes'));
    await driver.menu(['New project']); await driver.button('Blank website'); await driver.fill('Project name','Temporary reopen check'); await driver.button('Create project');
    await waitFor(async()=>(await workspace(driver)).project.id!==saved.project.id,'The temporary project did not replace the saved one');
    const unsaved=(await workspace(driver)).dirty;
    await chooseFile(physical,async()=>{await driver.menu(['Open project']);},destination,evidence,'canvas-open');
    await waitFor(async()=>(await workspace(driver)).project?.id===saved.project.id && (await workspace(driver)).path===destination,'The Desegnos project did not reopen');
    assert.deepEqual((await workspace(driver)).project,saved.project);
    // Leave an acknowledged unsaved edit for the runner's actual crash/recovery check.
    await driver.button('Fit'); await driver.button('Layers'); await driver.click('section[aria-label="Layers"] .layer-row[data-region-id="'+saved.project.pages[0].regions.at(-1).id+'"] button.layer-select'); await driver.button('Close Layers'); await settled(driver);
    const anchor=await physical.rect('[data-region-id="'+saved.project.pages[0].regions.at(-1).id+'"]'),view=await physical.rect('.workspace-scroll');
    assert.ok(anchor.left>=view.left && anchor.left<view.right && anchor.top>=view.top && anchor.top<view.bottom,JSON.stringify({anchor,view}));
    await driver.fill(`Request ${saved.project.pages[0].regions.length}`,'The shaped area is preserved after reopening, with an unsaved revision.');
    await waitFor(async()=>(await workspace(driver)).dirty,'The recovery revision did not persist');
  });
  return (await workspace(driver)).project;
}
