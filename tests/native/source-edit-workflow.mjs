import assert from 'node:assert/strict';
import {mkdir, readFile, writeFile, readdir} from 'node:fs/promises';
import {join} from 'node:path';
import {createHash, randomUUID} from 'node:crypto';
import {waitFor} from './webdriver.mjs';
import {chooseFile, chooseFolder, savedState, verifyArchive} from './file-dialog-workflow.mjs';
const digest=value=>createHash('sha256').update(value).digest('hex');
export async function sourceEditWorkflow(driver,physical,step,evidence) {
  const output=join(evidence,'source-edit');await mkdir(output,{recursive:true});
  let project,original,accepted;
  const projectPath=join(output,'Typed source.desegnos');
  const html="<main>\n<h1 id='title' data-desegnos-found='https://example.org/history'>Typed creation 💡</h1>\n<p>Local copy with a <a href='https://example.org/history'>source credit</a>.</p>\n</main>\n";
  const css="main { padding: 24px; }\nh1 { color: #245583; }";
  const javascript="document.querySelector('h1').dataset.ready = 'yes';";
  await step('Native source editing makes one durable transaction with exact source and a resolvable line map',async()=>{
    await driver.button('Blank website');await driver.fill('Project name','Typed source check');await driver.button('Create project');
    original=(await driver.invoke('get_workspace')).project;project=original;
    await physical.initialize(evidence);
    accepted=await driver.invoke('edit_page_source',{projectId:project.id,pageId:project.homePageId,revision:project.revision,html,css,javascript});project=accepted.project;
    assert.equal(project.revision,original.revision+1);assert.equal(accepted.canUndo,true);assert.deepEqual(project.pages[0].content,{html,css,javascript});
    const map=await driver.invoke('get_page_source_map',{projectId:project.id,pageId:project.homePageId});
    assert.equal(map.htmlSha256,digest(html));assert.equal(map.sourceSha256,digest(JSON.stringify({html,css,javascript})));
    const range=map.ranges.find(r=>r.selector==='body > main:nth-of-type(1) > h1:nth-of-type(1)');assert.equal(range.startLine,2);assert.equal(range.endLine,2);
    await writeFile(join(output,'source-map.json'),JSON.stringify(map,null,2));
    await chooseFile(physical,()=>driver.button('Save'),projectPath,output,'typed-first-save');
    await waitFor(()=>driver.execute(()=>document.querySelector('.static-preview iframe')?.contentDocument?.querySelector('#title')?.textContent==='Typed creation 💡'),'Typed source did not render in the actual canvas');
    const matching=await driver.execute(selector=>document.querySelector('.static-preview iframe').contentDocument.querySelectorAll(selector).length,range.selector);assert.equal(matching,1);
    await physical.captureWindow(join(output,'typed-canvas.png'));
  });
  await step('Invalid source and wrong-project or stale edits return diagnostics without changing acknowledged source',async()=>{
    const before=(await driver.invoke('get_workspace')).project;
    for(const params of [{revision:before.revision,javascript:'const valid=1;\nconst invalid = ;'},{revision:before.revision,html:"<main>\n<script src='https://example.org/remote.js'></script></main>"},{revision:0,css:'h1 {color:red}'},{revision:before.revision,projectId:randomUUID(),html:'<p>Wrong project</p>'}]){
      const result=await driver.executeAsync(async args=>{try{await window.__TAURI__.core.invoke('edit_page_source',args);return {accepted:true};}catch(error){return {accepted:false,error};}},{projectId:before.id,pageId:before.homePageId,...params});
      assert.equal(result.accepted,false);assert.ok(result.error.diagnostics.length);assert.ok(result.error.diagnostics.every(d=>d.line>=1&&d.column>=1));
      if(params.javascript)assert.ok(result.error.diagnostics.some(d=>d.language==='javascript'&&d.line===2));
      assert.deepEqual((await driver.invoke('get_workspace')).project,before);
    }
    await driver.invoke('undo_project');assert.deepEqual((await driver.invoke('get_workspace')).project.pages,original.pages);
    await driver.invoke('redo_project');assert.deepEqual((await driver.invoke('get_workspace')).project.pages,before.pages);
    project=(await driver.invoke('get_workspace')).project;
  });
  await step('Save, reopen and actual folder export retain typed code and credits while stripping found metadata',async()=>{
    const path=projectPath;
    await driver.button('Save');
    project=(await savedState(driver,path)).project;await verifyArchive(path,project,await driver.invoke('get_project_notes'));
    await chooseFile(physical,()=>driver.button('Open project'),path,output,'typed-open');project=(await driver.invoke('get_workspace')).project;
    assert.deepEqual(project.pages[0].content,{html,css,javascript});
    const parent=join(output,'export-parent');await mkdir(parent);
    await chooseFolder(physical,()=>driver.button('Export'),parent,output,'typed-export');
    const folders=await waitFor(async()=>{const entries=await readdir(parent,{withFileTypes:true});return entries.filter(e=>e.isDirectory()).length===1&&entries;},'The export did not complete');
    const directory=join(parent,folders[0].name);
    const file=await readFile(join(directory,'index.html'),'utf8');assert.ok(file.includes('Typed creation 💡'));assert.ok(file.includes('source credit'));assert.ok(!file.includes('data-desegnos-found'));
    assert.ok((await readFile(join(directory,'styles.css'),'utf8')).includes('#245583'));
    return project;
  });
  return project;
}
