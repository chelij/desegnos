import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { chooseFile } from './file-dialog-workflow.mjs';
import { waitFor } from './webdriver.mjs';
import { selectionJumpWorkflow } from './selection-jump-workflow.mjs';

export async function flashWorkflow(driver, physical, step, evidence) {
  const original = '~/Projects/chelib/chelib-redesign/chelib-redesign.desegnos';
  const hash = async path => createHash('sha256').update(await readFile(path)).digest('hex');
  const originalHash = await hash(original);
  const output = resolve('test-results/flash', process.env.DESEGNI_FLASH_LABEL ?? 'capture');
  await mkdir(output, { recursive: true });
  const manifest = JSON.parse(await readFile(join(evidence, 'real-use-manifest.json'), 'utf8'));
  const owner = manifest.projects.find(p => p.name === 'chelib-redesign');
  assert.ok(owner, 'The owner project must be copied by seedRealUse');
  assert.ok(owner.path.startsWith(evidence), 'Never open the original project');
  assert.equal(await hash(owner.path), originalHash, 'The recent-project copy must match the requested owner archive');
  await writeFile(join(output,'source-integrity.json'), JSON.stringify({ original, sha256:originalHash, copiedTo:owner.path },null,2));
  await physical.initialize(evidence);
  for (const kind of (process.env.DESEGNI_SELECTION_ONLY === '1' ? ['owner'] : ['owner', 'simple'])) {
    await step(`${kind}: trusted element drag with 60 Hz window capture and DOM trace`, async () => {
      if (kind === 'owner') {
        // The native picker is the same opening path used by the real-use suite.
        if (await driver.execute(() => !!document.querySelector('.recovery-list'))) await driver.button('Not now');
        await chooseFile(physical, () => driver.button('Open project'), owner.path, evidence, 'flash-owner-picker');
      } else {
        await driver.button('New project'); await driver.button('Blank website');
        await driver.fill('Project name', 'Flash fixture'); await driver.button('Create project');
        const p = (await driver.invoke('get_workspace')).project;
        await driver.invoke('edit_page_source', { projectId:p.id, pageId:p.homePageId, revision:p.revision,
          html:'<main><h1>Stable canvas</h1><button id="flash-target">Move this element</button></main>',
          css:'html,body{margin:0;background:#162329;color:#f4f0e8;font:24px sans-serif}main{padding:40px}button{position:absolute;left:80px;top:180px;width:260px;height:70px;background:#e9bd7c;color:#18232a}',javascript:'' });
        await driver.button('Save');
      }
      await waitFor(() => driver.execute(() => document.querySelector('.static-preview')?.dataset.previewReady === 'true'), 'Editable preview not ready', 30000);
      if (kind === 'owner') {
        await selectionJumpWorkflow(driver, physical, step, evidence);
        if (process.env.DESEGNI_SELECTION_ONLY === '1') return;
      }
      await driver.select('Zoom', '0.75');
      await driver.press('[data-testid="canvas"]', '\uE011');
      await delay(500);
      const point = await driver.execute(simple => {
        const f=document.querySelector('.static-preview iframe'), box=f.getBoundingClientRect(), z=Number(document.querySelector('[data-testid="canvas"]').dataset.zoom);
        for(const n of f.contentDocument.querySelectorAll(simple?'#flash-target':'h1,button,a,h2,p')){
          const r=n.getBoundingClientRect(); if(r.width<25||r.height<15)continue;
          const p={x:box.x+(r.x+Math.min(r.width/2,100))*z,y:box.y+(r.y+r.height/2)*z};
          if(p.x>250&&p.x<innerWidth-180&&p.y>180&&p.y<innerHeight-180&&document.elementFromPoint(p.x,p.y)?.closest('[data-testid="canvas"]')) return p;
        } return null;
      },kind==='simple');
      assert.ok(point, 'No visible generated element to drag');
      await physical.click(point);
      await waitFor(()=>driver.execute(()=>!!document.querySelector('[data-testid="direct-element-selection"]')), 'Element not selected');
      const initial = (await driver.invoke('get_workspace')).project;
      await driver.execute(() => {
        window.__flashEvents=[]; window.__flashFrames=[]; const start=performance.now();
        window.__flashLog=(type,data)=>window.__flashEvents.push({t:performance.now()-start,type,data});
        const ids=new WeakMap(); let serial=0; const id=n=>{if(!ids.has(n))ids.set(n,++serial);return ids.get(n);};
        const describe=n=>({id:id(n),tag:n.tagName,cls:typeof n.className==='string'?n.className:'',text:n.matches?.('.notice-host')?n.textContent:undefined});
        const observer=new MutationObserver(records=>{for(const r of records){
          if(r.type==='childList') for(const [type,list] of [['add',r.addedNodes],['remove',r.removedNodes]])for(const n of list)if(n.nodeType===1&&(n.matches('iframe,.notice-host,.canvas')||n.querySelector('iframe')))window.__flashLog(type,describe(n));
          if(r.type==='attributes'&&r.target.matches('html,body,.application,.notice-host'))window.__flashLog('attribute',{...describe(r.target),name:r.attributeName,value:r.target.getAttribute(r.attributeName)});
          if(r.target.closest?.('.notice-host'))window.__flashLog('NoticeHost mutation',{text:document.querySelector('.notice-host')?.textContent});
        }});observer.observe(document.documentElement,{subtree:true,childList:true,attributes:true});
        const resize=new ResizeObserver(entries=>{for(const e of entries)window.__flashLog('resize',{...describe(e.target),width:e.contentRect.width,height:e.contentRect.height});});
        for(const n of document.querySelectorAll('.application,.canvas,.static-preview,.live-canvas-content,.notice-host'))resize.observe(n);
        for(const type of ['pointerdown','pointerup'])document.addEventListener(type,e=>window.__flashLog(type,{trusted:e.isTrusted,x:e.clientX,y:e.clientY}),true);
        const sample=()=>{window.__flashFrames.push({t:performance.now()-start,frames:[...document.querySelectorAll('.preview-frame')].map(p=>({cls:p.className,ready:p.dataset.previewReady,hash:p.dataset.sourceHash,iframes:[...p.querySelectorAll('iframe')].map(f=>{let d;try{d=f.contentDocument;}catch{}const r=f.getBoundingClientRect();return{id:id(f),visibility:getComputedStyle(f).visibility,url:d?.URL,body:d?.body?.innerHTML.length,target:d?.querySelector(document.querySelector('[data-testid="direct-element-selection"]')?.dataset.selector || '#__missing_selection')?.getBoundingClientRect().toJSON(),width:r.width,height:r.height};})})),selection:document.querySelector('[data-testid="direct-element-selection"]')?.getBoundingClientRect().toJSON()});window.__flashRaf=requestAnimationFrame(sample);};sample();
        window.__flashStop=()=>{cancelAnimationFrame(window.__flashRaf);observer.disconnect();resize.disconnect();};return true;
      });
      const native=await physical.inspect();
      const video=join(output,`${kind}.mkv`);
      const capture=spawn('ffmpeg',['-y','-loglevel','error','-f','x11grab','-framerate','60','-video_size',`${native.window.width}x${native.window.height}`,'-i',`${native.display}+${native.window.x},${native.window.y}`,'-c:v','ffv1',video],{stdio:['ignore','ignore','pipe']});
      let captureErrors='';capture.stderr.on('data',b=>captureErrors+=b);
      const done=new Promise(resolve=>capture.on('close',resolve));
      await delay(400);
      await driver.execute(()=>window.__flashLog('capture drag start',{wall:Date.now()}));
      await physical.drag(point,{x:point.x+36,y:point.y+24},['Alt']);
      await delay(2200);
      capture.kill('SIGINT');assert.ok([0,255].includes(await done),captureErrors);
      const trace=await driver.execute(()=>{window.__flashStop();return{events:window.__flashEvents,frames:window.__flashFrames,errors:window.__desegniTestErrors};});
      const final=(await driver.invoke('get_workspace')).project;
      assert.ok(final.revision>initial.revision,'Drag did not commit a real edit');
      const alignment=await driver.execute(()=>{const f=document.querySelector('.static-preview iframe'),o=document.querySelector('[data-testid="direct-element-selection"]'),z=Number(document.querySelector('[data-testid="canvas"]').dataset.zoom);if(!o)return null;const n=f.contentDocument.querySelector(o.dataset.selector);if(!n)return null;const a=o.getBoundingClientRect(),b=n.getBoundingClientRect(),r=f.getBoundingClientRect();return Math.max(Math.abs(a.x-r.x-b.x*z),Math.abs(a.y-r.y-b.y*z),Math.abs(a.width-b.width*z),Math.abs(a.height-b.height*z));});
      await writeFile(join(output,`${kind}-trace.json`),JSON.stringify({native,point,initialRevision:initial.revision,finalRevision:final.revision,alignment,...trace},null,2));
      assert.ok(alignment!==null&&alignment<1,`Selection alignment: ${alignment}`);
      if(process.env.DESEGNI_FLASH_ASSERT!=='0') {
        const visibleTargets = trace.frames.flatMap(s => s.frames.flatMap(p => p.iframes.filter(f => f.visibility!=='hidden' && f.target).map(f => ({t:s.t,...f.target}))));
        assert.ok(visibleTargets.length > 10);
        const end = visibleTargets.at(-1), start = visibleTargets[0];
        assert.ok(Math.abs(end.x-start.x-48)<1 && Math.abs(end.y-start.y-32)<1,'Drag must move the real document by its world-space offset');
        const drop = trace.events.find(e=>e.type==='pointerup').t;
        assert.ok(visibleTargets.filter(r=>r.t>=drop).every(r=>Math.abs(r.x-end.x)<1 && Math.abs(r.y-end.y)<1),'Element snapped back after drop');
        assert.ok(trace.frames.every(s=>s.frames.filter(p=>p.cls.includes('static-preview')).every(p=>p.iframes.some(f=>f.visibility!=='hidden'&&f.body>0))), 'Canvas lost its visible document');
      }
    });
  }
  if (process.env.DESEGNI_SELECTION_ONLY === '1') {
    assert.equal(await hash(original), originalHash, 'Original archive changed');
    return;
  }
  await step('Cancelled dragging and Undo/Redo retain the editable document and selection', async () => {
    const before = (await driver.invoke('get_workspace')).project;
    const from = await driver.execute(()=>{const r=document.querySelector('[data-testid="direct-element-selection"]').getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2};});
    assert.ok(await driver.execute(p=>!!document.elementFromPoint(p.x,p.y)?.closest('[data-testid="canvas"]'),from));
    await physical.raw({ action:'drag', from:physical.rootPoint(from), to:physical.rootPoint({x:from.x+40,y:from.y+30}), cancelDrag:true });
    assert.deepEqual((await driver.invoke('get_workspace')).project.pages, before.pages, 'Escape must cancel movement');
    const x = () => driver.execute(()=>document.querySelector('.static-preview iframe')?.contentDocument?.querySelector('#flash-target')?.getBoundingClientRect().x);
    const moved = await x();
    await driver.button('Undo');
    await waitFor(async()=>Math.abs(await x()-moved+48)<1 && await driver.execute(()=>document.querySelector('.static-preview')?.dataset.previewReady==='true'),'Undo did not restore original position');
    await driver.button('Redo');
    await waitFor(async()=>Math.abs(await x()-moved)<1 && await driver.execute(()=>document.querySelector('.static-preview')?.dataset.previewReady==='true'),'Redo did not restore moved position');
  });
  await step('Live replacement keeps a visible frame and reports startup script errors after swapping', async () => {
    const p=(await driver.invoke('get_workspace')).project;
    await driver.invoke('edit_page_source',{projectId:p.id,pageId:p.homePageId,revision:p.revision,javascript:'throw new Error("flash-regression-startup")'});
    await driver.button('Save'); await driver.button('Live');
    const live = () => driver.execute(()=>document.querySelector('.live-canvas-content iframe:not([aria-hidden])')?.src);
    await waitFor(live,'Live document did not become visible');
    await waitFor(()=>driver.execute(()=>document.querySelector('.notice-error')?.textContent.includes('stopped working')),'Startup script error was lost during staging');
    await driver.execute(()=>{window.__liveSamples=[];const tick=()=>{window.__liveSamples.push({t:performance.now(),visible:[...document.querySelectorAll('.live-canvas-content iframe')].filter(f=>getComputedStyle(f).visibility!=='hidden').length});window.__liveRaf=requestAnimationFrame(tick);};tick();});
    let prior=await live(); await driver.button('Undo');
    await waitFor(async()=>{const next=await live();return next&&next!==prior;},'Live Undo did not swap');
    prior=await live(); await driver.button('Redo');
    await waitFor(async()=>{const next=await live();return next&&next!==prior;},'Live Redo did not swap');
    await delay(2200);
    await waitFor(()=>driver.execute(()=>document.querySelector('.notice-error')?.textContent.includes('stopped working')),'Reloaded startup error was lost');
    const samples=await driver.execute(()=>{cancelAnimationFrame(window.__liveRaf);return window.__liveSamples;});
    await writeFile(join(output,'live-reload-samples.json'),JSON.stringify(samples,null,2));
    assert.ok(samples.length>10 && samples.every(s=>s.visible===1),'Live reload blanked its visible document');
    assert.deepEqual(await driver.execute(()=>window.__desegniTestErrors),[]);
  });
  assert.equal(await hash(original), originalHash, 'The original archive changed during the native test');
}
