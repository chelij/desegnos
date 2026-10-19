import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFile, writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {waitFor} from './webdriver.mjs';
import {chooseFile, savedState, verifyArchive} from './file-dialog-workflow.mjs';
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const workspace=driver=>driver.invoke('get_workspace');
async function ready(driver){await waitFor(()=>driver.execute(()=>document.querySelector('.static-preview')?.dataset.previewReady==='true'),'Canvas did not finish rendering');}
async function point(driver,selector){
 await driver.executeAsync(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(()=>resolve(true)))));
 let previous;
 return waitFor(async()=>{const value=await driver.execute(query=>{const f=document.querySelector('.static-preview iframe'),r=f.contentDocument.querySelector(query).getBoundingClientRect(),b=f.getBoundingClientRect(),z=Number(document.querySelector('[data-testid="canvas"]').dataset.zoom);return{x:b.x+(r.x+r.width/2)*z,y:b.y+(r.y+r.height/2)*z};},selector);const stable=previous&&Math.abs(previous.x-value.x)<0.1&&Math.abs(previous.y-value.y)<0.1;previous=value;return stable&&value;},'Canvas did not settle before trusted selection');
}
async function select(driver,physical,selector,extra=false){await physical.raw({action:'click',point:physical.rootPoint(await point(driver,selector)),modifiers:extra?['Shift']:[]});}
function externalArchive(path,indent,destination=path,different=false){
  const result=spawnSync('/usr/bin/python3',['-c',`import json,sys,uuid,zipfile,hashlib,os
p,indent,dest,different=sys.argv[1],int(sys.argv[2]),sys.argv[3],sys.argv[4]=='true'
with zipfile.ZipFile(p) as z: files={n:z.read(n) for n in z.namelist()}
m=json.loads(files['manifest.json'])
if different:
 data=json.loads(files['project.json']);old=data['id'];new=str(uuid.uuid4());data['id']=new;m['projectId']=new
 files['project.json']=json.dumps(data,ensure_ascii=False,separators=(',',':')).encode()
 files['PROJECT.md']=files['PROJECT.md'].replace(old.encode(),new.encode())
 m['files']={n:hashlib.sha256(files[n]).hexdigest() for n in ['project.json','PROJECT.md']}
files['manifest.json']=json.dumps(m,indent=indent).encode()
with zipfile.ZipFile(dest,'w',zipfile.ZIP_DEFLATED) as z:
 for n,v in files.items():z.writestr(n,v)
os.chmod(dest,0o600)
`,path,String(indent),destination,String(different)],{encoding:'utf8',timeout:10000});assert.equal(result.status,0,result.stderr);
}
async function conflict(driver,defaultLabel,canReplace=true){
 await driver.ready('dialog[open] .save-conflict');
 await waitFor(()=>driver.execute(label=>document.activeElement?.textContent===label,defaultLabel),'Save conflict did not focus its safe default');
 assert.equal(await driver.execute(()=>[...document.querySelectorAll('.save-conflict button')].some(button=>button.textContent==='Replace it')),canReplace);
}
export async function selectionSaveWorkflow(driver,physical,step,output,home,about,projectPath,provider){
 let multiId,stableIds;
 await step('Shift selection creates one request around both items with stable target IDs',async()=>{
  await driver.page(about);await ready(driver);
  await select(driver,physical,'h1');await select(driver,physical,'#unfinished',true);
  await waitFor(()=>driver.execute(()=>document.querySelectorAll('[data-testid="multi-element-selection"]').length===1),'Shift click did not outline the second item');
  assert.deepEqual(await driver.execute(()=>[...document.querySelectorAll('.element-toolbar button')].map(b=>b.getAttribute('aria-label')||b.textContent)),['Ask AI','Delete selected items']);
  const before=(await workspace(driver)).project.pages.find(p=>p.id===about).regions.map(r=>r.id);
  await driver.button('Ask AI');
  const region=await waitFor(async()=>{const p=(await workspace(driver)).project.pages.find(p=>p.id===about);return p.regions.find(r=>!before.includes(r.id));},'Multi-selection did not create one request');
  multiId=region.id;assert.equal(region.elements.length,2);stableIds=region.elements.map(t=>t.id);assert.ok(stableIds.every(Boolean));
  const rects=await driver.execute(()=>{const doc=document.querySelector('.static-preview iframe').contentDocument;return ['h1','#unfinished'].map(s=>{const r=doc.querySelector(s).getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height};});});
  const x=Math.min(...rects.map(r=>r.x)),y=Math.min(...rects.map(r=>r.y));
  assert.deepEqual(region.rect,{x,y,width:Math.max(...rects.map(r=>r.x+r.width))-x,height:Math.max(...rects.map(r=>r.y+r.height))-y});
  const index=(await workspace(driver)).project.pages.find(p=>p.id===about).regions.findIndex(r=>r.id===multiId)+1;
  await driver.fill(`Request ${index}`,'Give these two items matching typography.');
  await waitFor(async()=>(await workspace(driver)).project.pages.find(p=>p.id===about).regions.find(r=>r.id===multiId).prompt.includes('matching typography'),'Multi request did not persist');
 });
 await step('Local text edits preserve pending target identity and preparation succeeds without another selection',async()=>{
  // The written request covers the heading on the canvas. Edit the same element
  // through the real source pane, without hiding the request or rebinding its IDs.
  await driver.click('//div[@aria-label="Editor view"]/button[normalize-space(.)="Split"]','xpath');
  await driver.ready('[contenteditable="true"][aria-label="HTML code"]');
  await driver.click('[contenteditable="true"][aria-label="HTML code"]');
  await physical.keys('Home',1,['Control']);await physical.keys('Down');await physical.keys('Home');await physical.keys('Right',4);await physical.keys('Right',5,['Shift']);
  await physical.raw({action:'text',text:'About our studio'});
  await waitFor(()=>driver.execute(()=>document.querySelector('.static-preview iframe')?.contentDocument?.querySelector('h1')?.textContent==='About our studio'),'Local heading edit did not apply');
  await driver.click('//div[@aria-label="Editor view"]/button[normalize-space(.)="Design"]','xpath');await ready(driver);
  const project=(await workspace(driver)).project,region=project.pages.find(p=>p.id===about).regions.find(r=>r.id===multiId);assert.deepEqual(region.elements.map(t=>t.id),stableIds);
  await driver.invoke('set_ai_key',{key:provider.key(),storage:'session'});
  const job=await driver.invoke('prepare_generation');assert.match(job.requirements.markdown,/format/i);for(const id of stableIds)assert.ok(job.requirements.markdown.includes(id));
  await driver.invoke('discard_generation',{jobId:job.jobId});assert.equal(provider.generationRequests().length,0);
 });
 await step('Multiple local deletions make one undo step and restore both exact identities',async()=>{
  await driver.page(home);await ready(driver);
  await select(driver,physical,'#picture');await select(driver,physical,'#action',true);
  const before=await workspace(driver);await driver.button('Delete selected items');
  const dialog=await waitFor(async()=>(await physical.inspect()).windows.find(w=>w.title==='Delete items'),'The delete confirmation did not appear');
  await physical.captureWindow(join(output,'delete-confirmation.png'));
  await physical.raw({action:'key',key:'Return',windowId:dialog.id});
  await waitFor(()=>driver.execute(()=>!document.querySelector('.static-preview iframe')?.contentDocument?.querySelector('#action')),'Selected items were not deleted');
  assert.equal((await workspace(driver)).history.position,before.history.position+1);
  await driver.button('Undo last change');await ready(driver);
  assert.deepEqual((await workspace(driver)).project.pages,before.project.pages);
  await physical.captureWindow(join(output,'multi-selection.png'));
  await driver.page(about);await ready(driver);
 });
 await step('A removed target offers Pick again and Remove request without losing the written prompt',async()=>{
  const before=await workspace(driver),request=before.project.pages.find(p=>p.id===about).regions.find(r=>r.id===multiId);
  await driver.click('//div[@aria-label="Editor view"]/button[normalize-space(.)="Split"]','xpath');
  await driver.ready('[contenteditable="true"][aria-label="HTML code"]');await driver.click('[contenteditable="true"][aria-label="HTML code"]');
  await physical.keys('Home',1,['Control']);await physical.keys('Down');await physical.keys('Home');await physical.keys('Down',1,['Shift']);await physical.keys('BackSpace');
  await waitFor(()=>driver.execute(()=>!document.querySelector('.static-preview iframe')?.contentDocument?.querySelector('h1')),'Deleting a source line did not remove its element');
  await driver.click('//div[@aria-label="Editor view"]/button[normalize-space(.)="Design"]','xpath');await ready(driver);
  const stale=`[data-region-id="${multiId}"] .request-stale`;
  await driver.ready(stale);assert.match(await driver.execute(q=>document.querySelector(q).textContent,stale),/This element was removed/);
  await driver.click(`${stale} button:first-of-type`);await select(driver,physical,'#action');
  await waitFor(async()=>{const next=(await workspace(driver)).project.pages.find(p=>p.id===about).regions.find(r=>r.id===multiId);return next.elements.length===1&&next.elements[0].tag==='button';},'Pick again did not retarget the original request');
  assert.equal((await workspace(driver)).project.pages.find(p=>p.id===about).regions.find(r=>r.id===multiId).prompt,request.prompt);
  await driver.button('Undo last change');await driver.ready(stale);await driver.click(`${stale} button:last-of-type`);await driver.button('Remove');
  await waitFor(async()=>!(await workspace(driver)).project.pages.find(p=>p.id===about).regions.some(r=>r.id===multiId),'Remove request did not remove only the stale request');
  await driver.button('Undo last change');await driver.button('Undo last change');await ready(driver);
  assert.deepEqual((await workspace(driver)).project.pages,before.project.pages);
 });
 await step('Save conflict Cancel and a cancelled copy picker preserve external bytes and the current draft',async()=>{
  await driver.button('Save');const before=(await workspace(driver)).project;
  await driver.click('.project-title');await driver.fill('Project name','Safe draft');await driver.button('Rename project');
  externalArchive(projectPath,1);const changed=await readFile(projectPath);
  await driver.button('Save');await conflict(driver,'Save as copy');await driver.button('Cancel');
  assert.equal(hash(await readFile(projectPath)),hash(changed));assert.equal((await workspace(driver)).dirty,true);
  await driver.button('Save');await conflict(driver,'Save as copy');
  await chooseFile(physical,()=>driver.button('Save as copy'),null,output,'cancel-conflict-copy',true);
  assert.equal(hash(await readFile(projectPath)),hash(changed));assert.equal((await workspace(driver)).project.name,'Safe draft');
  assert.equal((await workspace(driver)).project.id,before.id);assert.equal((await workspace(driver)).dirty,true);
 });
 const copyPath=join(output,'Safe copy.desegnos');
 await step('Save as copy writes the current project while retaining the changed original',async()=>{
  const original=await readFile(projectPath);await driver.button('Save');await conflict(driver,'Save as copy');
  await chooseFile(physical,()=>driver.button('Save as copy'),copyPath,output,'conflict-copy');
  const saved=await savedState(driver,copyPath);await verifyArchive(copyPath,saved.project,await driver.invoke('get_project_notes'));
  assert.equal(hash(await readFile(projectPath)),hash(original));
 });
 await step('Explicit Replace keeps the exact external archive as its backup',async()=>{
  externalArchive(copyPath,2);const observed=await readFile(copyPath);await driver.button('Save');await conflict(driver,'Save as copy');
  await driver.button('Replace it');await savedState(driver,copyPath);
  assert.equal(hash(await readFile(copyPath+'.previous')),hash(observed));
 });
 await step('Another disk change invalidates a pending Replace and requires a fresh choice',async()=>{
  externalArchive(copyPath,3);await driver.button('Save');await conflict(driver,'Save as copy');
  externalArchive(copyPath,4);const newer=await readFile(copyPath),backup=await readFile(copyPath+'.previous');await driver.button('Replace it');
  await conflict(driver,'Save as copy');assert.equal(hash(await readFile(copyPath)),hash(newer));assert.equal(hash(await readFile(copyPath+'.previous')),hash(backup));await driver.button('Cancel');
  // Reopen the disk version to establish its actual baseline, then restore the saved draft normally.
  await driver.button('Save');await conflict(driver,'Save as copy');await driver.button('Replace it');await savedState(driver,copyPath);
 });
 const otherPath=join(output,'Other project.desegnos');
 await step('Save as another project needs an in-app confirmation with Cancel as default',async()=>{
  externalArchive(copyPath,2,otherPath,true);const other=await readFile(otherPath);
  await chooseFile(physical,()=>driver.button('Save as'),otherPath,output,'different-project-save-as');
  await conflict(driver,'Cancel');assert.match(await driver.execute(()=>document.querySelector('dialog[open] h2').textContent),/different project/);
  await driver.button('Cancel');assert.equal(hash(await readFile(otherPath)),hash(other));assert.equal((await workspace(driver)).path,copyPath);
  await physical.captureWindow(join(output,'safe-save.png'));
 });
 await step('Unreadable destinations offer a new name and never expose Replace',async()=>{
  const nonProject=join(output,'Notes.desegnos');await writeFile(nonProject,'Private notes, not a project.');
  await chooseFile(physical,()=>driver.button('Save as'),nonProject,output,'nonproject-save-as');await conflict(driver,'Save as copy',false);
  await driver.button('Cancel');assert.equal(await readFile(nonProject,'utf8'),'Private notes, not a project.');
 });
 await driver.page(home);await driver.button('Save');
 return (await workspace(driver)).project;
}
