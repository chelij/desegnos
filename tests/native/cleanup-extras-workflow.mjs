import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {mkdir, writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {waitFor, xpathString} from './webdriver.mjs';
import {chooseFile, savedState, verifyArchive} from './file-dialog-workflow.mjs';
import {PhysicalInput} from './physical-workflow.mjs';
import {selectionSaveWorkflow} from './selection-save-workflow.mjs';
import {assertCanvasOnly} from './workflow.mjs';

const png = 'iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAEElEQVR4nGOwaQoAIgYIBQAdRgQ5DejjuQAAAABJRU5ErkJggg==';
const pngOther = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNQCi3/DwADeQHuOPZfDAAAAABJRU5ErkJggg==';
const workspace = driver => driver.invoke('get_workspace');
async function closed(driver) { await waitFor(()=>driver.execute(()=>!document.querySelector('dialog[open]')),'Dialog did not close'); }
async function closeDialog(driver) { await driver.click('(//dialog[@open])[last()]//button[starts-with(@aria-label,"Close ")]','xpath'); await closed(driver); }
async function pointFor(driver, selector) {
  return driver.execute(query=>{
    const frame=document.querySelector('.static-preview iframe'),node=frame?.contentDocument?.querySelector(query);
    if(!node)throw new Error(`Missing canvas item ${query}`);
    const r=node.getBoundingClientRect(),f=frame.getBoundingClientRect(),z=Number(document.querySelector('[data-testid="canvas"]').dataset.zoom);
    return {x:f.x+(r.x+r.width/2)*z,y:f.y+(r.y+r.height/2)*z};
  },selector);
}
async function canvasReady(driver, heading) {
  await waitFor(()=>driver.execute(text=>document.querySelector('.static-preview')?.dataset.previewReady==='true'&&document.querySelector('.static-preview iframe')?.contentDocument?.querySelector('h1')?.textContent===text,heading),'The current page did not finish rendering');
}
async function historyCurrent(driver, expectedRow) {
  await driver.button('Change history');
  const rows=await driver.execute(()=>[...document.querySelectorAll('ul[aria-label="Change history"] > li > button')].map(node=>({label:node.getAttribute('aria-label'),checked:node.getAttribute('aria-checked')})));
  assert.equal(rows.findIndex(row=>row.checked==='true'),expectedRow,JSON.stringify(rows));
  await driver.escape(); return rows;
}
async function historyJump(driver, row, expectedPosition) {
  await driver.button('Change history');
  await driver.click(`ul[aria-label="Change history"] > li:nth-child(${row+1}) > button`);
  await waitFor(async()=>(await workspace(driver)).history.position===expectedPosition,'History did not reach the chosen native undo position');
}

export async function cleanupExtrasWorkflow(driver,physical,step,evidence,provider) {
  const output=join(evidence,'cleanup-extras');await mkdir(output);
  const projectPath=join(output,'Workflow website.desegnos');
  const picture=join(output,'picture.png');await writeFile(picture,Buffer.from(png,'base64'));
  let project,home,about;
  await step('First-run tips advance, dismiss and persist while the canvas stays free of sidebars',async()=>{
    await driver.button('Blank website');await driver.fill('Project name','History A');await driver.button('Create project');
    await driver.ready('.first-run-tips');assert.match(await driver.execute(()=>document.querySelector('.first-run-tips').textContent),/Place a box/);
    await driver.button('Next');assert.match(await driver.execute(()=>document.querySelector('.first-run-tips').textContent),/Describe it/);
    await driver.button('Next');assert.match(await driver.execute(()=>document.querySelector('.first-run-tips').textContent),/Press Generate/);
    await driver.button('Got it');assert.equal(await driver.execute(()=>localStorage.getItem('desegnos.first-run-tips')),'done');
    assert.equal(await driver.execute(()=>document.querySelector('.first-run-tips')),null);
    await assertCanvasOnly(driver);await physical.initialize(evidence);
    project=(await workspace(driver)).project;home=project.homePageId;
  });
  await step('History distinguishes repeated content, ordinary Undo/Redo, branching and settings',async()=>{
    for(const name of ['History B','History A']){
      await driver.click('.project-title');await driver.fill('Project name',name);await driver.button('Rename project');await closed(driver);
      await waitFor(async()=>(await workspace(driver)).project.name===name,'Rename did not commit');
    }
    assert.equal((await historyCurrent(driver,0)).length,3);
    await driver.button('Undo last change');await driver.button('Undo last change');await historyCurrent(driver,2);
    await driver.button('Redo last change');await driver.button('Redo last change');await historyCurrent(driver,0);
    await historyJump(driver,2,0);assert.equal((await workspace(driver)).project.name,'History A');
    await historyJump(driver,0,2);await driver.button('Undo last change');await driver.toggleGrid();
    assert.equal((await workspace(driver)).canRedo,false);const rows=await historyCurrent(driver,0);const end=(await workspace(driver)).history.position;
    await historyJump(driver,rows.length-1,0);assert.equal((await workspace(driver)).project.showGrid,true);
    await historyJump(driver,0,end);assert.equal((await workspace(driver)).project.showGrid,false);
    await physical.captureWindow(join(output,'history.png'));
  });
  await step('Install two real editable pages for checks, preview and picture workflows',async()=>{
    await chooseFile(physical,()=>driver.button('Save'),projectPath,output,'initial-save');
    project=(await workspace(driver)).project;
    project=(await driver.invoke('edit_project',{revision:project.revision,command:{type:'add_page',name:'About',path:'/about'}})).project;about=project.pages.at(-1).id;
    for(const [pageId,name] of [[home,'Home'],[about,'About']]){
      project=(await workspace(driver)).project;
      const html=`<main>\n<h1>${name}</h1>\n<p id="unfinished">${name==='About'?'Sample text':'Welcome to the studio.'}</p>\n<img id="picture" width="240" height="120" alt="${name==='Home'?'Image placeholder: Studio photo':'Studio photo'}" ${name==='Home'?'data-desegnos-placeholder="image"':''} src="data:image/png;base64,${pngOther}">\n<button id="action">Try action</button>\n</main>\n`;
      await driver.invoke('edit_page_source',{projectId:project.id,pageId,revision:project.revision,html,css:'body { margin: 0; font: 16px sans-serif; }\nmain { padding: 24px; }\nimg { display: block; object-fit: cover; }\nbutton { margin-top: 16px; }',javascript:"const report=()=>parent.postMessage({type:'native-device-check',width:innerWidth},'*');\nreport();addEventListener('resize',report);\ndocument.getElementById('action').addEventListener('click',()=>parent.postMessage({type:'native-device-action'},'*'));"});
    }
    await driver.button('Save');await driver.page(home);await canvasReady(driver,'Home');
    await physical.captureWindow(join(output,'fixture-pages.png'));
  });
  await step('Page check switches pages before selecting and reuses a targeted fix request without generation',async()=>{
    await driver.click('button[aria-label="Open page check"]');
    const row='//section[@aria-label="Page check"]//li[.//strong[normalize-space(.)="Placeholder text"] and .//span[contains(.,"About")]]';
    await driver.click(`${row}//button[normalize-space(.)="Show"]`,'xpath');await canvasReady(driver,'About');
    const selected=await waitFor(()=>driver.execute(()=>document.querySelector('[data-testid="direct-element-selection"]')?.dataset.selector),'Page check did not select the item');
    assert.match(selected,/p:nth-of-type\(1\)/);
    await driver.click(`${row}//button[normalize-space(.)="Ask AI to fix"]`,'xpath');
    await waitFor(async()=>(await workspace(driver)).project.pages.find(page=>page.id===about).regions.some(region=>region.prompt.includes('Replace this placeholder')),'Fix request was not created on the correct page');
    let page=(await workspace(driver)).project.pages.find(page=>page.id===about);assert.equal(page.regions.length,1);assert.ok(page.regions[0].element||page.regions[0].elements?.length);
    await driver.click(`${row}//button[normalize-space(.)="Ask AI to fix"]`,'xpath');page=(await workspace(driver)).project.pages.find(page=>page.id===about);assert.equal(page.regions.length,1);
    assert.equal(provider.generationRequests().length,0);
    await driver.button('Close page check');await driver.page(home);await canvasReady(driver,'Home');
    await physical.captureWindow(join(output,'page-check.png'));
  });
  await step('Browse replaces an actual image placeholder without resizing it and Undo restores the slot',async()=>{
    await driver.ready('.image-drop-target');
    await chooseFile(physical,()=>driver.click('.image-drop-target'),picture,output,'browse-picture');
    await waitFor(()=>driver.execute(()=>document.querySelector('.static-preview iframe')?.contentDocument?.querySelector('#picture')?.getAttribute('src')?.endsWith('DejjuQAAAABJRU5ErkJggg==')),'Chosen picture did not render');
    const dimensions=await driver.execute(()=>{const node=document.querySelector('.static-preview iframe').contentDocument.querySelector('#picture');return {width:node.width,height:node.height,alt:node.alt,placeholder:node.hasAttribute('data-desegnos-placeholder')};});
    assert.deepEqual(dimensions,{width:240,height:120,alt:'Studio photo',placeholder:false});
    await driver.button('Undo last change');await driver.ready('.image-drop-target');await driver.button('Redo last change');
    await waitFor(()=>driver.execute(()=>!document.querySelector('.image-drop-target')),'Redo did not restore the chosen picture');
    await physical.captureWindow(join(output,'picture-browse.png'));
  });
  await step('A real GTK file drag reaches the canvas as trusted drop input and changes only the picture',async()=>{
    await driver.page(about);await canvasReady(driver,'About');
    const before=(await workspace(driver)).project.pages.find(page=>page.id===about).content;
    await driver.executeAsync(async()=>{window.__nativeFileDrops=[];await window.__TAURI__.event.listen('desegnos://image-drag',event=>{const p=event.payload;window.__nativeFileDrops.push({type:`native-${p.phase}`,files:p.picture?1:0,position:p.position,message:p.message});});for(const name of ['dragenter','dragover','drop'])document.addEventListener(name,event=>window.__nativeFileDrops.push({type:event.type,trusted:event.isTrusted,files:event.dataTransfer.files.length,types:[...event.dataTransfer.types],x:event.clientX,y:event.clientY,target:event.target?.className}),true);});
    const source=spawn('/usr/bin/python3',[fileURLToPath(new URL('./file-drag-source.py',import.meta.url)),picture],{env:process.env,stdio:['ignore','pipe','pipe']});
    let log='',errors='';source.stdout.on('data',chunk=>log+=chunk);source.stderr.on('data',chunk=>errors+=chunk);
    try{
      await waitFor(()=>{if(source.exitCode!==null)throw new Error(errors);return log.includes('"ready": true');},'GTK file drag source did not open');
      const snapshot=await new PhysicalInput(driver,source.pid).inspect();const window=snapshot.window;
      await physical.raw({action:'drag',sourcePid:source.pid,from:{x:window.x+window.width/2,y:window.y+window.height/2},to:physical.rootPoint(await pointFor(driver,'#picture'))});
      await waitFor(async()=>{const content=(await workspace(driver)).project.pages.find(page=>page.id===about).content;return content.html!==before.html;},'A native file drop did not change the picture');
      const events=await driver.execute(()=>window.__nativeFileDrops);assert.ok(events.some(event=>event.type==='native-drop'&&event.files===1),JSON.stringify(events));
      const after=(await workspace(driver)).project.pages.find(page=>page.id===about).content;assert.equal(after.css,before.css);assert.equal(after.javascript,before.javascript);
      await writeFile(join(output,'native-file-drop.json'),JSON.stringify({events,log},null,2));
    }finally{await writeFile(join(output,'native-file-drop.json'),JSON.stringify({events:await driver.execute(()=>window.__nativeFileDrops),log,errors,elementError:await driver.execute(()=>document.querySelector('.element-edit-error')?.textContent)},null,2));source.kill('SIGTERM');}
    await physical.captureWindow(join(output,'picture-drop.png'));
  });
  await step('Device preview uses real layout widths, runs enabled interactions and leaves authored source unchanged',async()=>{
    const before=(await workspace(driver)).project;
    await driver.button('Preview');await driver.ready('.device-stage iframe');
    await driver.execute(()=>{window.__deviceMessages=[];window.addEventListener('message',event=>{if(event.source===document.querySelector('.device-stage iframe')?.contentWindow&&event.data?.type?.startsWith('native-device-'))window.__deviceMessages.push(event.data);});});
    await driver.click('//label[contains(.,"Try buttons and links")]/input','xpath');
    for(const [name,width] of [['Desktop',1440],['Tablet',768],['Phone',390]]){
      await driver.click(`//div[@aria-label="Preview device"]/button[normalize-space(.)=${xpathString(name)}]`,'xpath');
      await waitFor(()=>driver.execute(value=>window.__deviceMessages.some(message=>message.type==='native-device-check'&&message.width===value),width),`${name} did not lay out at ${width}px`);
    }
    const at=await driver.execute(()=>{const frame=document.querySelector('.device-stage iframe'),rect=frame.getBoundingClientRect();const scale=rect.width/frame.offsetWidth;return{x:rect.left+80*scale,y:rect.top+260*scale};});
    // Use WebDriver's frame context for the native preview button, which runs in an opaque sandbox.
    const frame=await driver.find('.device-stage iframe');await driver.command('POST','/frame',{id:{'element-6066-11e4-a52e-4f735466cecf':frame}});await driver.click('#action');await driver.command('POST','/frame',{id:null});
    await waitFor(()=>driver.execute(()=>window.__deviceMessages.some(message=>message.type==='native-device-action')),'Enabled preview button did not run');
    assert.ok(Number.isFinite(at.x));await physical.captureWindow(join(output,'device-preview.png'));await closeDialog(driver);
    assert.deepEqual((await workspace(driver)).project,before);
  });
  await step('Split and Code views search, select, validate and save through the real source editor',async()=>{
    await driver.click('//div[@aria-label="Editor view"]/button[normalize-space(.)="Split"]','xpath');await driver.ready('[contenteditable="true"][aria-label="HTML code"]');
    await driver.fill('Find in code','About');assert.ok(await driver.execute(()=>document.querySelectorAll('.code-line.match').length)>0);
    await driver.fill('Find in code','');
    await physical.click(await pointFor(driver,'h1'));
    await waitFor(()=>driver.execute(()=>[...document.querySelectorAll('.code-line.selected')].some(node=>node.textContent.includes('About'))),'Canvas selection did not find source');
    const before=(await workspace(driver)).project.pages.find(page=>page.id===about).content;
    await driver.click('.code-tabs button:nth-child(3)');await driver.fill('JavaScript code','const okay=1;\nconst bad = ;');
    await waitFor(()=>driver.execute(()=>!!document.querySelector('.code-pane-note.status-invalid')),'Invalid code did not show line diagnostics');
    assert.equal((await workspace(driver)).project.pages.find(page=>page.id===about).content.javascript,before.javascript);
    assert.match(await driver.execute(()=>document.querySelector('[contenteditable="true"][aria-label="JavaScript code"]').textContent),/const bad = ;/);
    await driver.fill('JavaScript code',before.javascript+'\n// Verified local edit');
    await waitFor(async()=>(await workspace(driver)).project.pages.find(page=>page.id===about).content.javascript.endsWith('// Verified local edit'),'Valid code did not save');
    await driver.click('//div[@aria-label="Editor view"]/button[normalize-space(.)="Code"]','xpath');assert.equal(await driver.execute(()=>document.querySelector('.design-pane').getClientRects().length),0);
    await driver.click('//div[@aria-label="Editor view"]/button[normalize-space(.)="Design"]','xpath');await canvasReady(driver,'About');
    assert.equal(provider.generationRequests().length,0);await physical.captureWindow(join(output,'code-editing.png'));
  });
  await step('Picture and code changes survive a real archive save/reopen and History starts a fresh session',async()=>{
    await driver.button('Save');project=(await savedState(driver,projectPath)).project;await verifyArchive(projectPath,project,await driver.invoke('get_project_notes'));
    const previousSession=(await workspace(driver)).history.sessionId;
    await chooseFile(physical,()=>driver.button('Open project'),projectPath,output,'reopen-extras');
    await waitFor(async()=>(await workspace(driver)).history.sessionId!==previousSession,'Reopen did not create a new undo session');
    assert.deepEqual((await workspace(driver)).project,project);await driver.button('Change history');
    assert.equal(await driver.execute(()=>document.querySelector('ul[aria-label="Change history"]').textContent.includes('No changes yet')),true);await driver.escape();
    assert.equal(await driver.execute(()=>!!document.querySelector('.first-run-tips')),false);
  });
  return selectionSaveWorkflow(driver,physical,step,output,home,about,projectPath,provider);
}
