import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

// Browser integration without a new dependency or a native editor window.
const temporary = await mkdtemp(join(tmpdir(), 'desegnos-frozen-'));
const compiler = new URL('bin/tsc', import.meta.resolve('typescript/package.json'));
const compiled = spawnSync(process.execPath, [compiler.pathname, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022', '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/frozenPreview.ts'], { encoding: 'utf8' });
assert.equal(compiled.status, 0, compiled.stdout + compiled.stderr);
const bridge = await readFile('src-tauri/src/preview_bridge.js', 'utf8');
const source = '<button data-desegnos-id="button01" style="color: red">Source</button><p data-desegnos-id="text0001">Editable</p><aside>Idless source</aside><div data-desegnos-id="animated" class="animated">Animation</div><iframe data-desegnos-id="frame001" src="/inner"></iframe>';
const provenance = html => html.replace('<body>', '<body data-desegnos-preview-key="body-key" data-desegnos-preview-source="selector:body">').replace('<aside>Idless source</aside>', '<aside data-desegnos-preview-key="idless-key" data-desegnos-preview-source="selector:body > aside:nth-of-type(1)">Idless source</aside>').replace(/data-desegnos-id="([^"]+)"/g, (_, id) => `data-desegnos-id="${id}" data-desegnos-preview-key="key-${id}" data-desegnos-preview-source="id:${id}"`);
const bridgeTag = '<script src="/bridge.js" data-session="session" data-page="page" data-routes="{}" data-file-mode="true"></script>';
const fixture = '<!doctype html><html><head>' + bridgeTag + '<style>.active{background:rgb(1, 2, 3)}@keyframes pulse{from{opacity:1}to{opacity:.2}}.animated{animation:pulse .2s infinite alternate}</style></head><body>' + source + `<script>
try{parent.document.body;document.body.dataset.editorAccess='allowed'}catch{document.body.dataset.editorAccess='blocked'}
setTimeout(()=>{const frame=document.createElement('iframe');frame.src='/inner';frame.id='late-frame';document.body.append(frame)},100);
window.ticks=0;setInterval(()=>{window.ticks++;document.body.dataset.tick=window.ticks},20);
const button=document.querySelector('button');button.classList.add('active');button.style.opacity='.7';
button.insertAdjacentHTML('beforebegin','<span id="inserted">Runtime only</span>');
const clone=button.cloneNode(true);clone.id='clone';button.after(clone);
document.querySelector('p').textContent='Changed by script';
document.querySelector('aside').insertAdjacentHTML('beforebegin','<aside id="runtime-aside">Runtime aside</aside>');
addEventListener('mousemove',()=>button.style.color='blue');
</script></body></html>`;
const inner = `<!doctype html><html><head>${bridgeTag}
<script>
  if (window.self !== window.top) document.documentElement.classList.add('embedded-project');
  if (window.name === 'project-view') {
    const context = { framed: self !== top, parentIsTop: parent === top, name: window.name, query: location.search, referrer: document.referrer };
    document.documentElement.dataset.frameContext = JSON.stringify(context);
    top.postMessage({ type: 'test-framing-context', context }, '*');
  }
</script>
<style>html.embedded-project .site-nav{display:none!important}</style>
</head><body><header class="site-nav" data-desegnos-id="header01">Header</header><a data-desegnos-id="back0001">Back to projects</a><button data-desegnos-id="inner001">Inner</button>
<script>if(self!==top){document.querySelector('a').remove();document.querySelector('button').classList.add('active')}</script>
</body></html>`;
const server = createServer(async (req, res) => {
  res.setHeader('Access-Control-Allow-Origin', '*');
  if (req.url === '/editor') { res.setHeader('Content-Type', 'text/html'); res.end('<div id="host" style="width:900px;height:900px"></div>'); }
  else if (req.url === '/large') { res.setHeader('Content-Type', 'text/html'); res.end(provenance('<!doctype html><html><head>'+bridgeTag+'</head><body>'+Array.from({length:10000},(_,i)=>`<p data-desegnos-id="large${String(i).padStart(5,'0')}">Element ${i}<span data-desegnos-id="span${String(i).padStart(5,'0')}"> content</span></p>`).join('')+'</body></html>')); }
  else if (req.url === '/page' || req.url === '/inner') { res.setHeader('Content-Type', 'text/html'); res.end(provenance(req.url === '/page' ? fixture : inner)); }
  else if (req.url === '/startup-frame') {
    res.setHeader('Content-Type', 'text/html');
    res.end('<!doctype html><html><head>'+bridgeTag+'</head><body><iframe title="Empty frame"></iframe><script>addEventListener("load",()=>setTimeout(()=>{const frame=document.createElement("iframe");frame.src="/slow-framed-page?embed=1";frame.name="project-view";document.body.append(frame)},50))</script></body></html>');
  }
  else if (req.url === '/slow-framed-page?embed=1') {
    // The request begins in an about:blank window. Its bridge and framed head
    // script arrive after the old per-child timeout, during the outer capture.
    setTimeout(()=>{res.setHeader('Content-Type','text/html');res.end(provenance(inner));},2200);
  }
  else if (req.url === '/bridge.js') { res.setHeader('Content-Type', 'text/javascript'); res.end(bridge); }
  else if (['/frozenPreview.js', '/previewDocument.js'].includes(req.url)) { res.setHeader('Content-Type', 'text/javascript'); res.end(await readFile(join(temporary, req.url.slice(1)))); }
  else res.writeHead(404).end();
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const origin = `http://127.0.0.1:${server.address().port}`;
const chrome = spawn(process.env.CHROMIUM ?? 'chromium', ['--headless', '--no-sandbox', '--disable-gpu', '--remote-debugging-port=0', `--user-data-dir=${join(temporary, 'profile')}`, 'about:blank'], { stdio: ['ignore', 'ignore', 'pipe'] });
let socket;
let closeBrowser;
try {
  const endpoint = await new Promise((resolve, reject) => { let output=''; chrome.stderr.on('data', data => { output+=data; const match=output.match(/DevTools listening on (ws:\/\/\S+)/);if(match)resolve(match[1]); }); chrome.on('error',reject); });
  socket = new WebSocket(endpoint);
  await new Promise(resolve => socket.addEventListener('open', resolve, { once: true }));
  let sequence=0; const requests=new Map();
  socket.addEventListener('message', event => { const message=JSON.parse(event.data);const request=requests.get(message.id);if(request){requests.delete(message.id);message.error?request.reject(message.error):request.resolve(message.result);} });
  const send = (method, params={}, sessionId) => new Promise((resolve,reject) => { const id=++sequence; requests.set(id,{resolve,reject});socket.send(JSON.stringify({id,method,params,sessionId})); });
  closeBrowser = () => send('Browser.close').catch(() => {});
  const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await send('Target.attachToTarget', { targetId, flatten: true });
  const evaluate = async expression => { const value=await send('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true},sessionId);if(value.exceptionDetails)throw new Error(JSON.stringify(value.exceptionDetails));return value.result.value; };
  await send('Page.enable', {}, sessionId);
  const loaded = new Promise(resolve => {
    const listener = event => { const value=JSON.parse(event.data);if(value.method==='Page.loadEventFired' && value.sessionId===sessionId){socket.removeEventListener('message',listener);resolve();} };
    socket.addEventListener('message',listener);
  });
  await send('Page.navigate', { url: origin + '/editor' }, sessionId);
  await loaded;
  await evaluate(`(async()=>{window.module=await import('/frozenPreview.js');window.mapping=new module.SourceMapping()})()`);
  const result = await evaluate(`(async()=>{
    const start=performance.now();
    window.snapshot=await module.capturePreview(${JSON.stringify(origin+'/page')},'session',document.querySelector('#host'));
    const captureMs=performance.now()-start;
    window.frozen=document.createElement('iframe');frozen.sandbox='allow-same-origin';frozen.width='900';frozen.height='900';
    const loaded=new Promise(resolve=>frozen.onload=resolve);frozen.srcdoc=module.freezeSnapshot(snapshot);document.body.append(frozen);await loaded;
    window.doc=frozen.contentDocument;mapping.bind(doc,${JSON.stringify(source)},${JSON.stringify(origin+'/page')});
    window.innerDoc=doc.querySelector('iframe').contentDocument;mapping.bind(innerDoc,${JSON.stringify('<header data-desegnos-id="header01">Header</header><a data-desegnos-id="back0001">Back to projects</a><button data-desegnos-id="inner001">Inner</button>')},${JSON.stringify(origin+'/inner')});
    return {captureMs,runnerRemoved:document.querySelector('#host iframe')===null,editorAccess:doc.body.dataset.editorAccess,active:doc.querySelector('button').className,background:doc.defaultView.getComputedStyle(doc.querySelector('button')).backgroundColor,innerHidden:innerDoc.defaultView.getComputedStyle(innerDoc.querySelector('header')).display,backRemoved:!innerDoc.querySelector('a'),innerActive:innerDoc.querySelector('button').className};
  })()`);
  await test('captures load styling and framed-page behavior, then removes the script runner', () => {
    assert.equal(result.editorAccess,'blocked');assert.equal(result.runnerRemoved,true);assert.equal(result.active,'active');assert.equal(result.background,'rgb(1, 2, 3)');assert.equal(result.innerHidden,'none');assert.equal(result.backRemoved,true);assert.equal(result.innerActive,'active');
    console.log(`Capture fixture: ${Math.round(result.captureMs)} ms`);
  });
  await test('nested head scripts preserve framed root classes and hide navigation in the frozen canvas', async () => {
    assert.equal(await evaluate(`innerDoc.documentElement.classList.contains('embedded-project')`),true);
    assert.equal(await evaluate(`innerDoc.defaultView.getComputedStyle(innerDoc.querySelector('.site-nav')).display`),'none');
    assert.equal(await evaluate(`snapshot.frames[0].snapshot.html.includes('embedded-project')`),true);
  });
  await test('a nested page loading during capture keeps its framed script state after freezing', async () => {
    const value = await evaluate(`(async()=>{
      // The Live baseline uses the same sandbox/referrer policy as PreviewFrame.
      const live=document.createElement('iframe');live.sandbox='allow-scripts allow-forms';live.referrerPolicy='no-referrer';
      const liveContext=new Promise(resolve=>{
        const receive=event=>{
          if(event.data?.type!=='test-framing-context'||event.source!==live.contentWindow.frames[1])return;
          window.removeEventListener('message',receive);resolve(event.data.context);
        };
        window.addEventListener('message',receive);
      });
      live.src=${JSON.stringify(origin+'/startup-frame')};document.body.append(live);
      const context=await liveContext;live.remove();
      const snapshot=await module.capturePreview(${JSON.stringify(origin+'/startup-frame')},'session',document.querySelector('#host'));
      if(snapshot.frames.length!==1)return {captured:snapshot.frames.length};
      const captured=new DOMParser().parseFromString(snapshot.frames[0].snapshot.html,'text/html');
      const node=document.createElement('iframe');node.sandbox='allow-same-origin';
      const loaded=new Promise(resolve=>node.onload=resolve);node.srcdoc=module.freezeSnapshot(snapshot);document.body.append(node);await loaded;
      const child=node.contentDocument.querySelector('iframe[srcdoc]').contentDocument;
      const result={contextMatchesLive:JSON.stringify(context)===child.documentElement.dataset.frameContext,captured:1,snapshotClass:captured.documentElement.classList.contains('embedded-project'),frozenClass:child.documentElement.classList.contains('embedded-project'),display:child.defaultView.getComputedStyle(child.querySelector('.site-nav')).display,url:snapshot.frames[0].snapshot.url};
      node.remove();return result;
    })()`);
    assert.deepEqual(value,{contextMatchesLive:true,captured:1,snapshotClass:true,frozenClass:true,display:'none',url:origin+'/slow-framed-page?embed=1'});
  });
  await test('frames inserted during settling are frozen but never gain source identity', async () => {
    assert.deepEqual(await evaluate(`(()=>{const frame=doc.querySelector('#late-frame');return {mapped:!!mapping.source(frame),header:frame.contentDocument.defaultView.getComputedStyle(frame.contentDocument.querySelector('header')).display}})()`),{mapped:false,header:'none'});
  });
  await test('CSS animation freezes at the captured value', async () => {
    const before=await evaluate(`doc.defaultView.getComputedStyle(doc.querySelector('.animated')).opacity`);
    await evaluate(`new Promise(resolve=>setTimeout(resolve,250))`);
    assert.equal(await evaluate(`doc.defaultView.getComputedStyle(doc.querySelector('.animated')).opacity`),before);
    assert.equal(await evaluate(`doc.getAnimations().length`),0);
  });
  await test('runtime nodes and cloned IDs have no editable source identity', async () => {
    assert.deepEqual(await evaluate(`({inserted:!!mapping.source(doc.querySelector('#inserted')),clone:!!mapping.source(doc.querySelector('#clone')),original: mapping.source(doc.querySelector('button')).getAttribute('data-desegnos-id')})`), {inserted:false,clone:false,original:'button01'});
  });
  await test('style saves merge only the editor delta into source style', async () => {
    const style=await evaluate(`(()=>{const node=doc.querySelector('button');const before=node.getAttribute('style');const next=doc.createElement('div').style;next.cssText=before;next.setProperty('translate','10px 20px','important');return mapping.style(node,before,next.cssText)})()`);
    assert.match(style,/color: red/);assert.match(style,/translate: 10px 20px !important/);assert.doesNotMatch(style,/opacity|background|active/);
    assert.equal(await evaluate(`mapping.source(doc.querySelector('button')).outerHTML`),'<button data-desegnos-id="button01" style="color: red">Source</button>');
  });
  await test('script-changed text cannot send a rendered text index to source', async () => {
    assert.equal(await evaluate(`mapping.textEditable(doc.querySelector('p'))`),false);
    assert.equal(await evaluate(`mapping.textEditable(doc.querySelector('button'))`),true);
  });
  await test('timers and pointer handlers cannot change the frozen document', async () => {
    const before=await evaluate(`doc.body.outerHTML`);
    await evaluate(`doc.querySelector('button').dispatchEvent(new doc.defaultView.MouseEvent('mousemove',{bubbles:true}));new Promise(resolve=>setTimeout(resolve,450))`);
    assert.equal(await evaluate(`doc.body.outerHTML`),before);
    assert.equal(await evaluate(`frozen.contentWindow.ticks===undefined`),true);
    assert.equal(await evaluate(`doc.querySelectorAll('script,[onload],[onmousemove]').length`),0);
  });
  await test('ID-less source elements retain their source selector after inserted siblings', async () => {
    assert.deepEqual(await evaluate(`(()=>{const node=mapping.find(doc,'selector:body > aside:nth-of-type(1)');return {text:node.textContent,source:mapping.source(node).outerHTML,runtime:!!mapping.source(doc.querySelector('#runtime-aside'))}})()`), {text:'Idless source',source:'<aside>Idless source</aside>',runtime:false});
  });
  await test('cancelled captures remove the isolated runner immediately', async () => {
    assert.equal(await evaluate(`(async()=>{const abort=new AbortController();const capture=module.capturePreview(${JSON.stringify(origin+'/page')},'session',document.querySelector('#host'),abort.signal);abort.abort();try{await capture;return false}catch{return document.querySelector('#host iframe')===null}})()`),true);
  });
  await test('large-page capture maps all 20,000 source elements and reports timing', async () => {
    const samples=[];
    for (let run=0;run<3;run++) {
      samples.push(await evaluate(`(async()=>{const source=(await fetch('/large').then(response=>response.text())).replace(/ data-desegnos-preview-(?:key|source)="[^"]*"/g,'');const start=performance.now();const value=await module.capturePreview(${JSON.stringify(origin+'/large')},'session',document.querySelector('#host'));const captured=performance.now();const html=module.freezeSnapshot(value);const sanitized=performance.now();const node=document.createElement('iframe');node.sandbox='allow-same-origin';const loaded=new Promise(resolve=>node.onload=resolve);node.srcdoc=html;document.body.append(node);await loaded;const painted=performance.now();const mapping=new module.SourceMapping();mapping.bind(node.contentDocument,source,${JSON.stringify(origin+'/large')});const mapped=Array.from(node.contentDocument.querySelectorAll('[data-desegnos-id]')).filter(item=>mapping.source(item)).length;const finished=performance.now();node.remove();return {mapped,total:finished-start,capture:captured-start,sanitize:sanitized-captured,load:painted-sanitized,map:finished-painted}})()`));
    }
    const median=samples.map(sample=>sample.total).sort((a,b)=>a-b)[1];
    console.log(`20,000-element capture + frozen load + source mapping: ${JSON.stringify(samples.map(sample=>Object.fromEntries(Object.entries(sample).map(([key,value])=>[key,Math.round(value)]))))} ms`);
    samples.forEach(sample=>assert.equal(sample.mapped,20000));
    const budget=Number(process.env.DESEGNOS_PREVIEW_MAX_MS ?? 0);
    if (budget>0) assert.ok(median<budget,`large-page median took ${Math.round(median)} ms (budget ${budget} ms)`);
  });
  await test('a redirected document cannot borrow source identities from the requested page', async () => {
    assert.deepEqual(await evaluate(`(()=>{const other=new module.SourceMapping();other.bind(doc,${JSON.stringify(source)},${JSON.stringify(origin+'/another-page')});const redirected=other.source(doc.querySelector('button'))===null;const saved=doc.documentElement.getAttribute('data-desegnos-snapshot-url');doc.documentElement.setAttribute('data-desegnos-snapshot-url',${JSON.stringify(origin+'/projects/index.html')});other.bind(doc,${JSON.stringify(source)},${JSON.stringify(origin+'/projects/')});const directoryAlias=!!other.source(doc.querySelector('button'));doc.documentElement.setAttribute('data-desegnos-snapshot-url',saved);return {redirected,directoryAlias}})()`),{redirected:true,directoryAlias:true});
  });
  await test('snapshot validation rejects excessive nesting', async () => {
    assert.equal(await evaluate(`(()=>{try{module.freezeSnapshot({html:'',base:'',frames:[]},17);return false}catch{return true}})()`),true);
  });
} finally {
  const exited = new Promise(resolve => { if (chrome.exitCode !== null) resolve(); else chrome.once('exit', resolve); });
  await closeBrowser?.();
  socket?.close(); if (chrome.exitCode === null) chrome.kill();
  await exited;
  server.closeAllConnections(); await new Promise(resolve=>server.close(resolve));
  await rm(temporary,{recursive:true,force:true,maxRetries:10,retryDelay:100});
}
