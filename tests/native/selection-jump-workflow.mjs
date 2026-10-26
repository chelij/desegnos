import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdir, writeFile, copyFile, readFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { waitFor } from './webdriver.mjs';
import { seedRealUse } from './real-use-fixtures.mjs';

// Called after flashWorkflow opens the real-use COPY and calibrates trusted X11 input.
export async function selectionJumpWorkflow(driver, physical, step, evidence) {
  const output = resolve('test-results/selection-jump', process.env.DESEGNOS_FLASH_LABEL ?? 'capture');
  await mkdir(output, { recursive: true });
  await copyFile(resolve('test-results/flash', process.env.DESEGNOS_FLASH_LABEL ?? 'capture', 'source-integrity.json'), join(output, 'source-integrity.json'));
  if (process.env.DESEGNOS_SELECTION_RECOVERY === '1') {
    const copies = join(evidence, 'recovery-fixtures');
    const profile = join(evidence, 'recovery-profile');
    const manifest = JSON.parse(await readFile(join(evidence, 'real-use-manifest.json'), 'utf8'));
    await seedRealUse(profile, copies, { source: manifest.source });
    const project = (await driver.invoke('get_workspace')).project;
    const appData = join(profile, 'io.github.chelij.desegnos');
    const record = JSON.parse(await readFile(join(appData, `${project.id}.recovery.json`), 'utf8'));
    const journal = JSON.parse((await readFile(join(appData, record.journalFile), 'utf8')).trim().split('\n').at(-1));
    const page = journal.project.pages.find(page => page.id === project.homePageId);
    await driver.invoke('edit_page_source', { projectId: project.id, pageId: page.id, revision: project.revision, ...page.content });
    const content = (await driver.invoke('get_workspace')).project.pages.find(item => item.id === page.id).content;
    const sourceHash = createHash('sha256').update(JSON.stringify({ html:content.html, css:content.css, javascript:content.javascript })).digest('hex');
    await driver.button('Save');
    await waitFor(() => driver.execute(hash => { const preview=document.querySelector('.static-preview'); return preview?.dataset.previewReady === 'true' && preview.dataset.sourceHash === hash; }, sourceHash), 'Recovery source did not mount');
    await writeFile(join(output, 'recovery-source.json'), JSON.stringify({ journalRevision: journal.project.revision, savedRevision: record.savedRevision, copiedJournal: join(appData, record.journalFile) }, null, 2));
  }
  const reports = [];
  const sample = expected => driver.execute(expected => {
    const frame = [...document.querySelectorAll('.static-preview iframe')].find(f => getComputedStyle(f).visibility !== 'hidden');
    const doc = frame?.contentDocument, outline = document.querySelector('[data-testid="direct-element-selection"]');
    const describe = n => n ? { id:n.getAttribute('data-desegnos-id'), tag:n.localName, text:n.textContent.trim().replace(/\s+/g,' ').slice(0,240), rect:n.getBoundingClientRect().toJSON(), style:n.getAttribute('style') } : null;
    const id = outline?.dataset.elementId;
    const selected = id ? doc?.querySelector(`[data-desegnos-id="${CSS.escape(id)}"]`) : outline ? doc?.querySelector(outline.dataset.selector) : null;
    const actual = [...(doc?.querySelectorAll('a.book') ?? [])].find(n => n.textContent.trim().replace(/\s+/g,' ') === expected);
    const o=outline?.getBoundingClientRect(), r=actual?.getBoundingClientRect(), f=frame?.getBoundingClientRect(), z=Number(document.querySelector('[data-testid="canvas"]').dataset.zoom);
    return { selected:describe(selected), expected:describe(actual), selector:outline?.dataset.selector, outline:o?.toJSON(), frame:f?.toJSON(), zoom:z,
      alignment:o&&r&&f ? Math.max(Math.abs(o.x-f.x-r.x*z),Math.abs(o.y-f.y-r.y*z),Math.abs(o.width-r.width*z),Math.abs(o.height-r.height*z)) : null,
      frames:[...document.querySelectorAll('.static-preview iframe')].map(f=>({visible:getComputedStyle(f).visibility, ids:[...f.contentDocument.querySelectorAll('[data-desegnos-id]')].map(n=>n.getAttribute('data-desegnos-id'))})) };
  }, expected);
  const capture = async name => {
    const path=join(evidence,`selection-${name}.png`); await physical.captureWindow(path); await copyFile(path,join(output,`${name}.png`));
  };
  await driver.select('Zoom',process.env.DESEGNOS_SELECTION_ZOOM ?? '0.75');
  await driver.press('[data-testid="canvas"]','\uE011');
  await delay(400);
  const names=await driver.execute(()=>{const f=[...document.querySelectorAll('.static-preview iframe')].find(f=>getComputedStyle(f).visibility!=='hidden');return [...f.contentDocument.querySelectorAll('a.book')].map(n=>n.textContent.trim().replace(/\s+/g,' '));});
  assert.ok(names.some(n=>/Wheely Lucky/i.test(n)), 'Exact owner card must exist');
  const targets=[names.find(n=>/Wheely Lucky/i.test(n)),...names.filter(n=>!/Wheely Lucky/i.test(n)).slice(0,2)];
  for (const [index,name] of targets.entries()) await step(`Selection identity and geometry: ${name}`,async()=>{
    const point=await driver.execute(name=>{const f=[...document.querySelectorAll('.static-preview iframe')].find(f=>getComputedStyle(f).visibility!=='hidden'),n=[...f.contentDocument.querySelectorAll('a.book')].find(n=>n.textContent.trim().replace(/\s+/g,' ')===name),r=n.getBoundingClientRect(),b=f.getBoundingClientRect(),z=Number(document.querySelector('[data-testid="canvas"]').dataset.zoom);return{x:b.x+(r.x+r.width/2)*z,y:b.y+(r.y+r.height/2)*z};},name);
    assert.ok(await driver.execute(p=>!!document.elementFromPoint(p.x,p.y)?.closest('[data-testid="canvas"]'),point),'Card reachable');
    await physical.click(point); await delay(300);
    const initial=await sample(name);
    assert.equal(initial.selected?.text, name, 'Trusted click must select the requested card');
    assert.ok(initial.selected?.id, 'Card must have a stable editor ID');
    reports.push({name,phase:'selected',...initial}); await capture(`${index}-selected`);
    for(const [phase,action,dx,dy] of [
      ['right-once',()=>physical.keys('Right'),1,0],['down-once',()=>physical.keys('Down'),1,1],
      ['right-several',()=>physical.keys('Right',4),5,1],['down-several',()=>physical.keys('Down',4),5,5],
      ['drag',async()=>{const s=await sample(name),r=s.expected.rect,f=s.frame,z=s.zoom,p={x:f.x+(r.x+r.width/2)*z,y:f.y+(r.y+r.height/2)*z};await physical.drag(p,{x:p.x+24,y:p.y+18},['Alt']);},5+24/Number(process.env.DESEGNOS_SELECTION_ZOOM ?? 0.75),5+18/Number(process.env.DESEGNOS_SELECTION_ZOOM ?? 0.75)],
    ]) {
      const revision=(await driver.invoke('get_workspace')).project.revision;
      await driver.execute(() => {
        window.__selectionFrames=[];
        const tick=()=>{
          const o=document.querySelector('[data-testid="direct-element-selection"]');
          const frames=[...document.querySelectorAll('.static-preview iframe')];
          window.__selectionFrames.push({time:performance.now(),id:o?.dataset.elementId,selector:o?.dataset.selector,outline:o?.getBoundingClientRect().toJSON(),frames:frames.map(f=>{
            const d=f.contentDocument,id=o?.dataset.elementId,n=id?d?.querySelector(`[data-desegnos-id="${CSS.escape(id)}"]`):null;
            return {visible:getComputedStyle(f).visibility,frame:f.getBoundingClientRect().toJSON(),id:n?.getAttribute('data-desegnos-id'),text:n?.textContent,rect:n?.getBoundingClientRect().toJSON()};
          })});window.__selectionRaf=requestAnimationFrame(tick);
        };tick();
      });
      await action();
      await waitFor(async()=> (await driver.invoke('get_workspace')).project.revision>revision && await driver.execute(()=>document.querySelector('.static-preview')?.dataset.previewReady==='true' && !!document.querySelector('.element-resize-corner:not(:disabled)')),'Move did not settle',15000);
      await delay(800);
      await waitFor(() => driver.execute(() => document.querySelector('.static-preview')?.dataset.previewReady==='true' && !!document.querySelector('.element-resize-corner:not(:disabled)')), 'Queued moves did not finish');
      const trace=await driver.execute(()=>{cancelAnimationFrame(window.__selectionRaf);return window.__selectionFrames;});
      const s=await sample(name); reports.push({name,phase,...s,trace,delta:{x:s.expected.rect.x-initial.expected.rect.x,y:s.expected.rect.y-initial.expected.rect.y}});
      await capture(`${index}-${phase}`);
      await writeFile(join(output,'identity-and-geometry.json'),JSON.stringify({native:await physical.inspect(),reports,events:await physical.events()},null,2));
      if(process.env.DESEGNOS_SELECTION_ASSERT!=='0') {
        assert.ok(trace.every(frame=>frame.id===initial.selected?.id), `${phase}: selection changed during swap`);
        if (phase !== 'drag') for (const item of trace) for (const frame of item.frames.filter(frame => frame.visible !== 'hidden')) {
          assert.equal(frame.text?.trim(), name, `${phase}: visible frame target changed`);
          const o=item.outline, r=frame.rect, f=frame.frame, z=s.zoom;
          assert.ok(o && r && Math.max(Math.abs(o.x-f.x-r.x*z), Math.abs(o.y-f.y-r.y*z), Math.abs(o.width-r.width*z), Math.abs(o.height-r.height*z)) < 1, `${phase}: outline drifted during swap`);
        }
        assert.equal(s.selected?.id,initial.selected?.id,`${phase}: selected ID changed`);
        assert.equal(s.selected?.tag,'a',`${phase}: selected tag changed`);
        assert.equal(s.selected?.text,name,`${phase}: selected content changed`);
        assert.ok(s.alignment!==null&&s.alignment<1,`${phase}: outline mismatch ${s.alignment}`);
        assert.ok(Math.abs(s.expected.rect.x-initial.expected.rect.x-dx)<1&&Math.abs(s.expected.rect.y-initial.expected.rect.y-dy)<1,`${phase}: wrong movement`);
      }
    }
  });
  const events=await physical.events(); assert.ok(events.filter(e=>e.type==='keydown'&&e.key.startsWith('Arrow')).every(e=>e.trusted));
}
