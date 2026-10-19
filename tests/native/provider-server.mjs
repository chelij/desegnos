import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import { createServer } from 'node:http';
import { lstat, readdir, readFile, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { oauthFixture } from './oauth-server.mjs';

const syntheticKeys = new Set();
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const clone = value => structuredClone(value);
const usage = { input_tokens: 211, output_tokens: 89, total_tokens: 300 };

function jsonBlocks(markdown) {
  return [...markdown.matchAll(/^(`{3,})json\n([\s\S]*?)\n\1\n/gm)].map(match => JSON.parse(match[2]));
}

function sourceFor(record, endpoint, options) {
  if (options.logoScope) {
    const asset = jsonBlocks(record.markdown).find(value => Array.isArray(value?.assets))?.assets[0];
    assert.ok(asset?.url?.startsWith('desegnos-asset:'));
    return { html: `<main id="logo-test"><h1>Northstar Studio</h1><img id="project-logo" src="${asset.url}" alt="Northstar Studio" width="128" height="128"><img id="editable-image" src="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAEElEQVR4nGOwaQoAIgYIBQAdRgQ5DejjuQAAAABJRU5ErkJggg==" alt="Image to edit" width="128" height="128"><p>Image placeholder</p></main>`, css:'body{margin:0;background:white;color:#222;font:16px system-ui}main{padding:24px}img{display:block}', javascript:'' };
  }
  if (options.draftAudit) {
    const origin = endpoint.replace(/\/v1$/, '');
    return { html: `<main id="image-draft"><h1>Sample website</h1><p>Placeholder: add your introduction.</p><button disabled>Add your GitHub link</button><figure><img id="downloaded-photo" data-desegnos-download src="${origin}/images/photo.png" alt="Sample green picture"><figcaption>Sample image · Native fixture</figcaption></figure><figure><img id="missing-photo" data-desegnos-download src="${origin}/images/missing.png" alt="Sample missing picture"><figcaption>Placeholder: replace with your product screenshot.</figcaption></figure></main>`,
      css: 'body{margin:0;background:white;color:#222;font:16px system-ui}main{padding:40px}figure{display:inline-block;vertical-align:top;margin:24px 24px 0 0}img{display:block;width:240px;height:135px;object-fit:contain}figcaption{max-width:240px;margin-top:8px}button{padding:12px}', javascript: '' };
  }
  if (options.layerAudit) return {
    html: '<main id="layer-stage"><div id="layer-backdrop" aria-label="Background decoration"></div><div id="blue-card" aria-label="Blue card"><h2>Blue card</h2><label for="card-input">Name</label><input id="card-input" aria-label="Card name" value="A real copied input"><a href="#card-input">Jump to input</a><svg width="24" height="24" viewBox="0 0 24 24" aria-label="Copied graphic"><defs><clipPath id="card-clip"><circle cx="12" cy="12" r="10"/></clipPath></defs><rect width="24" height="24" fill="white" clip-path="url(#card-clip)"/></svg><ul><li>Detail</li></ul></div><div id="orange-card" aria-label="Orange card"><h2>Orange card</h2><p>Front card</p></div><div id="nested-group" aria-label="Nested group"><div id="green-card"><h2>Green nested card</h2></div><div id="purple-card"><h2>Purple nested card</h2></div></div><div id="zero-wrapper"><div id="zero-child">Visible zero-wrapper child</div></div><div id="contents-wrapper"><div id="contents-child">Visible contents child</div></div></main>',
    css: 'html,body{margin:0;background:#f2f2f2;color:#222;font:16px system-ui}#layer-stage{position:relative;width:800px;height:680px;background:white}#layer-backdrop{position:absolute;inset:0;background:red;z-index:-1;pointer-events:none}#zero-wrapper{position:absolute;left:600px;top:430px;width:0;height:0}#zero-child{position:absolute;width:140px;height:80px;padding:8px;background:#ccc}#contents-wrapper{display:contents}#contents-child{position:absolute;left:600px;top:540px;width:140px;height:80px;padding:8px;background:#ccc}#blue-card,#orange-card,#nested-group{position:absolute;box-sizing:border-box;border-radius:8px;padding:16px}#blue-card{left:80px;top:60px;width:280px;height:230px;background:rgb(30,100,190);color:white;z-index:1}#orange-card{left:180px;top:150px;width:280px;height:230px;background:rgb(230,130,45);z-index:2}h2{font-size:19px;margin:0 0 14px}#blue-card label,#blue-card input{display:block}#blue-card input{width:180px;margin:4px 0 8px}#blue-card::before{content:"Copied badge";position:absolute;right:8px;top:8px;background:rgb(250,220,80);color:#222;padding:2px 5px;font-size:10px}#blue-card::after{content:"";position:absolute;right:12px;bottom:12px;width:14px;height:14px;background:rgb(250,220,80);border-radius:50%}#blue-card ul{position:absolute;right:12px;bottom:32px;margin:0;padding-left:20px;font-size:11px}#blue-card li::marker{color:rgb(250,220,80);font-size:19px}#blue-card a{color:white}#blue-card svg{display:block;margin-top:8px}#nested-group{left:80px;top:420px;width:480px;height:210px;background:#ddd;z-index:3;opacity:.9;transform:translateX(0)}#green-card,#purple-card{position:absolute;box-sizing:border-box;width:260px;height:130px;padding:16px}#green-card{left:10px;top:15px;background:rgb(40,170,100);z-index:1}#purple-card{left:120px;top:60px;background:rgb(155,90,185);z-index:2}',
    javascript: '',
  };
  const marker = options.marker ?? `fixture-${record.ordinal}`;
  const probe = `${endpoint.replace(/\/v1$/, '')}/probe/${marker}`;
  const retained = options.retainedPreviewUrl ?? '#retained-unavailable';
  const html = `<main><h1>Generated native preview</h1><p id="fixture-status">Scripts are paused until enabled.</p><button id="fixture-action" type="button"><span>${options.buttonLabel ?? 'Try generated interaction'}</span></button><a id="fixture-retained" href="${retained.replaceAll('&', '&amp;').replaceAll('"', '&quot;')}">Open retained preview</a><form id="fixture-form" action="${probe}/form" method="post"><input name="probe" value="blocked"></form><img alt="" src="${probe}/image"><script>parent.postMessage({type:'desegni-native-inline-script-ran'},'*')</script><script src="${probe}/external-script"></script></main>`;
  const css = 'html,body{margin:0;background:#162329;color:#f4f0e8;font:16px system-ui}main{padding:20px}h1{font-size:22px;margin:0 0 12px}#fixture-action,#fixture-retained{position:fixed;left:20px;box-sizing:border-box;width:240px;height:42px;padding:10px;background:#e9bd7c;color:#18232a;border:1px solid #e9bd7c;border-radius:6px;text-align:center;font:15px system-ui}#fixture-action{top:116px}#fixture-retained{top:174px}img{display:none}';
  const common = `const marker=${JSON.stringify(marker)};const send=(event,detail={})=>parent.postMessage({type:'desegni-native-fixture',marker,event,detail},'*');const rect=id=>{const r=document.getElementById(id).getBoundingClientRect();return {x:r.x,y:r.y,width:r.width,height:r.height}};`;
  const javascript = options.editorAudit
    ? "document.getElementById('fixture-action')?.addEventListener('click',()=>{document.getElementById('fixture-status').textContent='The local action worked.'});"
    : options.hang
    ? `${common}send('hang-armed',{delayMs:1500});setTimeout(()=>{send('hang-entering');console.log('DESEGNI_NATIVE_HANG_ENTERING:'+marker);while(true){}},1500);`
    : `${common}
const report={parentDenied:false,tauriAbsent:!window.__TAURI__,internalsAbsent:!window.__TAURI_INTERNALS__,ipcAbsent:!window.ipc,inlineScriptRan:false,violations:[]};
try{void parent.document.body}catch(error){report.parentDenied=error.name==='SecurityError'}
addEventListener('securitypolicyviolation',event=>report.violations.push(event.effectiveDirective));
document.getElementById('fixture-status').textContent='Generated JavaScript is running.';
document.getElementById('fixture-action').addEventListener('click',event=>send('interaction',{trusted:event.isTrusted,count:1}));
addEventListener('click',event=>{if(event.target.closest?.('#fixture-retained')){event.preventDefault();event.stopPropagation();location.assign(${JSON.stringify(retained)})}},true);
const probe=${JSON.stringify(probe)};
fetch(probe+'/fetch').catch(()=>{});
try{const xhr=new XMLHttpRequest();xhr.open('GET',probe+'/xhr');xhr.send()}catch{}
try{new WebSocket(probe.replace('http:','ws:')+'/socket')}catch{}
try{navigator.sendBeacon(probe+'/beacon','blocked')}catch{}
try{const frame=document.createElement('iframe');frame.src=probe+'/frame';document.body.append(frame)}catch{}
try{document.getElementById('fixture-form').submit()}catch{}
try{fetch('ipc://localhost/edit_project',{method:'POST',body:JSON.stringify({command:{type:'rename_project',name:'PREVIEW IPC ESCAPED'}})}).catch(()=>{})}catch{}
try{window.__TAURI__?.core.invoke('edit_project',{revision:0,command:{type:'rename_project',name:'PREVIEW IPC ESCAPED'}}).catch(()=>{})}catch{}
try{const raw=window.webkit?.messageHandlers?.ipc;report.rawIpcAvailable=!!raw;if(raw){raw.postMessage(JSON.stringify({cmd:'edit_project',callback:0,error:1,payload:{revision:0,command:{type:'rename_project',name:'PREVIEW RAW IPC ESCAPED'}},__TAURI_INVOKE_KEY__:'invalid-native-fixture-key'}));report.rawIpcAttempted=true}}catch(error){report.rawIpcError=String(error)}
parent.postMessage({cmd:'edit_project',command:{type:'rename_project',name:'PREVIEW POSTMESSAGE ESCAPED'}},'*');
setTimeout(()=>send('ready',{...report,action:rect('fixture-action'),retained:rect('fixture-retained')}),180);`;
  return { html, css: options.extraCss ? `${css}\n${options.extraCss}` : options.largeStream ? `${css}\n/*${'Synthetic stream transport padding. '.repeat(2400)}*/` : css, javascript };
}

function candidateFor(record, endpoint, options) {
  const requests = jsonBlocks(record.markdown).filter(value => value && typeof value === 'object' && value.regionId && value.pageId);
  assert.ok(requests.length > 0, 'The attached requirements contain no requested areas');
  const pageIds = [...new Set(requests.map(value => value.pageId))];
  const conflict = options.conflict === true;
  return {
    jobId: record.metadata.jobId, requirementsSha256: record.metadata.requirementsSha256,
    summary: 'Native fixture candidate with complete source and explicit review choices',
    regions: requests.map(value => ({ regionId: value.regionId, status: conflict ? 'conflict' : 'implemented', explanation: conflict ? 'The requested remote service is unavailable in this dependency gate.' : 'Implemented with native HTML, CSS and browser DOM behavior.' })),
    pages: conflict ? [] : pageIds.map(pageId => {
      const content = sourceFor(record, endpoint, options);
      if (/^Requirements format: 8$/m.test(record.markdown)) {
        const areas = requests.filter(value => value.pageId === pageId);
        content.html = content.html.replace('<main', `<main data-desegnos-region="${areas[0].regionId}"`);
        content.html += areas.slice(1).map(area => `<section data-desegnos-region="${area.regionId}"><p>Requested supporting content</p></section>`).join('');
      }
      return {pageId, content};
    }),
    componentRequests: [{ regionId: requests[0].regionId, capability: 'Interactive button', resolution: conflict ? 'unavailable' : 'native', reason: conflict ? 'No external service is available.' : 'A native button and DOM listener provide the interaction without a package.' }],
    decisionProposals: conflict ? [] : [
      { topic: `Native fixture navigation ${record.metadata.jobId}`, value: 'Use a clear native action button.', scope: { kind: 'project' }, rationale: 'The primary action should remain recognizable across the site.' },
      { topic: `Optional fixture accent ${record.metadata.jobId}`, value: 'Use warm accent panels.', scope: { kind: 'project' }, rationale: 'An optional visual direction that requires explicit acceptance.' },
    ],
    criticism: [{ regionId: requests[0].regionId, message: 'Dense page content may compete with the primary action.', suggestion: 'Keep the primary action visually distinct while preserving your chosen style.' }],
  };
}

function identityCandidateFor(record) {
  const files = jsonBlocks(record.markdown).filter(value => value?.fileId);
  assert.ok(files.length > 0, 'Identity analysis must freeze at least one selected source');
  const notes = files.find(file => /identity\.md$/i.test(file.name)) ?? files[0];
  const palette = files.find(file => /tokens\.css$/i.test(file.name)) ?? notes;
  const images = files.filter(file => file.asset);
  return {
    jobId: record.metadata.jobId, requirementsSha256: record.metadata.requirementsSha256,
    summary: 'Northstar Studio pairs a calm blue palette with clear, welcoming language and its existing mark.',
    brandName: 'Northstar Studio', tone: 'Clear, warm and confident.',
    findings: [
      { topic: 'Brand name', value: 'Northstar Studio', evidence: [notes.fileId], confidence: 'high' },
      { topic: 'Brand palette', value: 'Use Northstar blue #225577, warm white #faf8f2, and ink #18232a.', evidence: [palette.fileId], confidence: 'high' },
      { topic: 'Tone', value: 'Clear, warm and confident.', evidence: [notes.fileId], confidence: 'high' },
      { topic: 'Writing voice', value: 'Use clear, warm and confident language, with concise helpful labels.', evidence: [notes.fileId], confidence: 'high' },
      { topic: 'Possible audience', value: 'The source may appeal to independent creative studios; verify this before treating it as a requirement.', evidence: [notes.fileId], confidence: 'low' },
    ],
    logoAssetIds: images.map(file => file.asset.id), warnings: ['Confirm the intended audience before adding it to the brief.'],
  };
}

function readRequest(bytes, ordinal, mode, authentication) {
  const body = JSON.parse(bytes);
  assert.equal(body.stream, true); assert.equal(body.store, false);
  assert.equal(body.model, 'gpt-6.1-sol');
  if (authentication === 'chatgpt') {
    for (const field of ['max_output_tokens', 'service_tier', 'background', 'conversation', 'previous_response_id', 'prompt', 'truncation']) assert.equal(field in body, false, `Unsupported plan field ${field}`);
  } else {
    assert.equal(body.service_tier, 'default');
    assert.ok(body.max_output_tokens > 0 && body.max_output_tokens <= 32768);
  }
  assert.ok(['low', 'medium', 'high'].includes(body.reasoning?.effort));
  assert.equal(body.text?.format?.type, 'json_schema'); assert.equal(body.text.format.strict, true);
  assert.equal(body.text.format.schema?.additionalProperties, false);
  assert.equal(body.input.length, 1); assert.equal(body.input[0].role, 'user');
  const content = body.input[0].content;
  const file = content.find(value => value.type === 'input_file');
  const text = content.find(value => value.type === 'input_text');
  const metadata = JSON.parse(text.text);
  const purpose = metadata.purpose === 'identityAnalysis' ? 'identityAnalysis' : 'generation';
  assert.equal(content.filter(value => value.type === 'input_file').length, 1);
  assert.equal(file?.filename, purpose === 'identityAnalysis' ? 'IDENTITY-SOURCES.md' : 'REQUIREMENTS.md');
  const encoded = file.file_data.match(/^data:text\/markdown;base64,([A-Za-z0-9+/]*={0,2})$/);
  assert.ok(encoded, 'Requirements must be an inline Base64 Markdown file');
  const decoded = Buffer.from(encoded[1], 'base64');
  assert.equal(decoded.toString('base64'), encoded[1], 'Non-canonical Base64 file');
  const markdown = decoded.toString('utf8');
  const webSearch = purpose === 'generation' && /^Requirements format: 8$/m.test(markdown);
  assert.deepEqual(body.tools, webSearch ? [{type:'web_search'}] : []); assert.equal(body.tool_choice, webSearch ? 'auto' : 'none');
  assert.deepEqual(Buffer.from(markdown, 'utf8'), decoded, 'Requirements are not exact valid UTF-8');
  assert.deepEqual(body.text.format.schema.properties.jobId.enum, [metadata.jobId]);
  assert.deepEqual(body.text.format.schema.properties.requirementsSha256.enum, [metadata.requirementsSha256]);
  assert.equal(metadata.requirementsSha256, digest(decoded));
  if (purpose === 'identityAnalysis') {
    assert.deepEqual(Object.keys(metadata).sort(), ['jobId', 'purpose', 'requirementsSha256']);
    const blocks = jsonBlocks(markdown), manifest = blocks.find(value => value?.sourceId);
    const files = blocks.filter(value => value?.fileId);
    assert.ok(manifest?.sourceName && /^[0-9a-f]{64}$/.test(manifest.sourceSha256));
    assert.equal(new Set(files.map(value => value.fileId)).size, files.length, 'Identity evidence file IDs must be unique');
    const images = content.filter(value => value.type === 'input_image');
    assert.equal(content.length, 2 + 2 * images.length);
    const labels = content.filter(value => value.type === 'input_text' && value !== text);
    assert.equal(labels.length, images.length);
    labels.forEach((label, index) => assert.match(label.text, new RegExp(`^Identity image ${index + 1}\\.`)));
    const raster = files.filter(value => value.asset?.analysisImage != null);
    assert.equal(raster.length, images.length);
    raster.forEach(value => {
      assert.ok(Number.isInteger(value.asset.analysisImage));
      const image = images[value.asset.analysisImage - 1];
      assert.ok(image, 'An identity image must match its frozen provenance index');
      const encodedImage = image.image_url.match(/^data:(image\/(?:png|jpeg|webp));base64,([A-Za-z0-9+/]*={0,2})$/);
      assert.ok(encodedImage, 'Identity vision inputs must be portable raster data URIs');
      assert.equal(encodedImage[1], value.asset.mime);
      const bytes = Buffer.from(encodedImage[2], 'base64');
      assert.equal(bytes.toString('base64'), encodedImage[2]);
      assert.equal(digest(bytes), value.asset.sha256, 'Identity vision pixels must match the frozen source hash');
    });
    assert.ok(body.text.format.schema.properties.findings?.items?.additionalProperties === false);
    assert.ok(body.text.format.schema.required.includes('logoAssetIds'));
  } else {
    assert.equal(content.filter(value => value.type === 'input_text').length, 1);
    assert.equal(content.length, 2);
    assert.equal((markdown.match(/"generationMode"\s*:/g) ?? []).length, 1);
    assert.deepEqual(jsonBlocks(markdown).filter(value => value?.generationMode), [{ generationMode: metadata.generationMode }]);
    assert.ok(['fuzzy', 'strict'].includes(metadata.generationMode));
  }
  for (const key of syntheticKeys) assert.equal(markdown.includes(key) || bytes.toString().includes(key), false, 'A credential leaked into the provider body');
  return { ordinal, mode, purpose, authentication, method: 'POST', path: '/v1/responses', body, markdown, metadata, requestBytes: bytes.length, markdownBytes: decoded.length, sha256: digest(decoded), authenticated: true, responseId: `resp_native_fixture_${ordinal}`, startedAt: new Date().toISOString(), closed: false, completed: false };
}

export async function startProviderServer() {
  const keys = { primary: `sk-desegni-native-synthetic-${randomUUID().replaceAll('-', '')}-A001`, replacement: `sk-desegni-native-synthetic-${randomUUID().replaceAll('-', '')}-B002` };
  Object.values(keys).forEach(key => syntheticKeys.add(key));
  let expectedKey = keys.primary;
  let mode = { kind: 'complete', options: {} };
  let endpoint;
  const records = []; const errors = []; const probes = []; const sockets = new Set(); const timers = new Set(); const pending = new Map();
  const oauth = oauthFixture(() => endpoint.replace(/\/v1$/, ''), value => syntheticKeys.add(value), errors);
  const schedule = (fn, ms) => { const timer = setTimeout(() => { timers.delete(timer); fn(); }, ms); timers.add(timer); };
  const server = createServer(async (request, response) => {
    const path = new URL(request.url, 'http://127.0.0.1').pathname;
    if (path.startsWith('/images/')) {
      assert.equal(request.headers.authorization, undefined); assert.equal(request.headers.cookie, undefined); assert.equal(request.headers.referer, undefined);
      probes.push({ method: request.method, path, at: new Date().toISOString() });
      if (path === '/images/photo.png') response.writeHead(200, { 'content-type': 'image/png' }).end(Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAEElEQVR4nGOwaQoAIgYIBQAdRgQ5DejjuQAAAABJRU5ErkJggg==', 'base64'));
      else response.writeHead(404, { 'content-type': 'text/plain' }).end('Missing fixture image');
      return;
    }
    if (path.startsWith('/probe/')) { probes.push({ method: request.method, path, at: new Date().toISOString() }); response.writeHead(204).end(); return; }
    if (await oauth.handle(request, response)) return;
    const account = oauth.authenticate(request.headers.authorization);
    const authentication = account ? 'chatgpt' : 'apiKey';
    const authenticated = !!account || request.headers.authorization === `Bearer ${expectedKey}`;
    if (!authenticated) { errors.push('The native provider sent an unexpected credential'); response.writeHead(401, { 'content-type': 'application/json' }).end('{"error":{"code":"invalid_api_key"}}'); return; }
    if (request.method === 'GET' && path === '/v1/models/gpt-6.1-sol') {
      records.push({ method: 'GET', path, authenticated, at: new Date().toISOString() });
      response.writeHead(200, { 'content-type': 'application/json' }).end('{"id":"gpt-6.1-sol","object":"model"}'); return;
    }
    if (request.method === 'GET' && path === '/v1/models' && account) {
      records.push({ method: 'GET', path, authenticated, authentication, clientId: account.clientId, at: new Date().toISOString() });
      response.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify({ models: [
        { slug: 'future-unverified-model', display_name: 'Unverified model', visibility: 'list' },
        { slug: 'gpt-6.1-sol', display_name: 'GPT-6.1 Sol', visibility: 'list' },
        { slug: 'hidden-model', display_name: 'Hidden model', visibility: 'hidden' },
      ] })); return;
    }
    if (request.method !== 'POST' || path !== '/v1/responses') { errors.push(`Unexpected provider route ${request.method} ${path}`); response.writeHead(404).end(); return; }
    const selected = clone(mode);
    try {
      const chunks = []; let total = 0;
      for await (const chunk of request) { total += chunk.length; assert.ok(total <= 16 * 1024 * 1024, 'Provider request exceeds its 16 MiB boundary'); chunks.push(chunk); }
      const record = readRequest(Buffer.concat(chunks), records.filter(value => value.method === 'POST').length + 1, selected.kind, authentication);
      if (account) record.clientId = account.clientId;
      record.candidate = record.purpose === 'identityAnalysis' ? identityCandidateFor(record) : candidateFor(record, endpoint, selected.options); records.push(record);
      response.on('close', () => { record.closed = true; pending.delete(record.metadata.jobId); });
      if (selected.kind === 'non-stream-error') {
        response.writeHead(200, { 'content-type': 'application/json', 'x-request-id': 'req_native_non_stream_error' }).end(JSON.stringify({ error: { code: selected.options.code === undefined ? 'subscription_sharing_unsupported_capability' : selected.options.code, param: 'text.format', message: selected.options.explanation ?? `Do not retain ${request.headers.authorization}` } })); return;
      }
      if (selected.kind === 'non-stream-html') {
        response.writeHead(200, { 'content-type': 'text/html; charset=utf-8', 'x-request-id': 'req_native_non_stream_html' }).end(`<!doctype html><html>Do not retain ${request.headers.authorization}${selected.options.largeReply ? 'Synthetic HTML diagnostic padding. '.repeat(3000) : ''}</html>`); return;
      }
      if (['auth', 'quota', 'rate-limit', 'plan-limit'].includes(selected.kind)) {
        if (selected.kind === 'plan-limit') {
          response.writeHead(429, { 'content-type': 'application/json', 'x-request-id': 'req_native_plan_limit' }).end(JSON.stringify({ error: { code: 'subscription_sharing_usage_limit_exceeded', param: null, message: 'Synthetic ChatGPT app allowance limit' } })); return;
        }
        const code = selected.kind === 'auth' ? 'invalid_api_key' : selected.kind === 'quota' ? 'insufficient_quota' : 'rate_limit_exceeded';
        response.writeHead(selected.kind === 'auth' ? 401 : 429, { 'content-type': 'application/json' }).end(JSON.stringify({ error: { code, message: 'Synthetic fixture error' } })); return;
      }
      const beginStream = () => {
        const headers = { 'cache-control': 'no-cache', connection: 'keep-alive' };
        if (!selected.options.omitContentType) headers['content-type'] = selected.options.contentType ?? 'text/event-stream';
        record.responseContentType = headers['content-type'] ?? null;
        response.writeHead(200, headers);
        record.responseFinalizedItems = 0;
        record.responseEmptyAddedItems = 0;
        const event = (type, data) => {
          if (response.destroyed || response.writableEnded) return;
          if (type === 'response.output_item.done') record.responseFinalizedItems += 1;
          if (type === 'response.output_item.added' && data.item == null) record.responseEmptyAddedItems += 1;
          response.write(`event: ${type}\ndata: ${JSON.stringify({ type, ...data })}\n\n`);
        };
        const text = selected.kind === 'malformed' ? '{"not-complete-json":' : JSON.stringify(record.candidate);
        event('response.created', { response: { id: record.responseId, status: 'in_progress' } });
        const itemId = `msg_${record.metadata.jobId.replaceAll('-', '')}`;
        const commentaryId = `msg_update_${record.metadata.jobId.replaceAll('-', '')}`;
        const commentary = 'Preparing the requested page.';
        const finalizedItems = selected.options.sparse || selected.options.finalizedItems;
        const outputIndex = selected.options.sparse ? 9 : finalizedItems ? (selected.options.phased ? 2 : 1) : selected.options.phased ? 1 : 0;
        const commentaryIndex = selected.options.sparse ? 5 : finalizedItems ? 1 : 0;
        const codepoints = Array.from(text), midpoint = Math.floor(codepoints.length / 2);
        const parts = selected.options.splitParts ? [codepoints.slice(0, midpoint).join(''), codepoints.slice(midpoint).join('')] : [text];
        record.responsePhases = selected.options.phased ? ['commentary', 'final_answer'] : [];
        record.responseTextParts = parts.length;
        record.responseWireIndices = selected.options.sparse || (finalizedItems && selected.options.phased) ? [commentaryIndex, outputIndex] : finalizedItems ? [outputIndex] : null;
        record.omittedFinalizedCommentary = selected.options.omitFinalizedCommentary === true;
        const commentaryItem = { id: commentaryId, type: 'message', role: 'assistant', status: 'completed', phase: 'commentary', content: [{ type: 'output_text', text: commentary }] };
        if (finalizedItems) {
          if (selected.options.sparse) event('response.output_item.added', { output_index: 0, item: null });
          const reasoning = { id: `rs_${record.metadata.jobId.replaceAll('-', '')}`, type: 'reasoning', summary: [] };
          const reasoningIndex = selected.options.sparse ? 2 : 0;
          event('response.output_item.added', { output_index: reasoningIndex, item: reasoning });
          event('response.output_item.done', { output_index: reasoningIndex, item: reasoning });
        }
        if (selected.options.phased) {
          if (finalizedItems) event('response.output_item.added', { output_index: commentaryIndex, item: { ...commentaryItem, status: 'in_progress', content: [] } });
          event('response.output_text.delta', { output_index: commentaryIndex, content_index: 0, item_id: commentaryId, delta: commentary });
          event('response.output_text.done', { output_index: commentaryIndex, content_index: 0, item_id: commentaryId, text: commentary });
          if (finalizedItems) event('response.output_item.done', { output_index: commentaryIndex, item: commentaryItem });
        }
        if (finalizedItems) event('response.output_item.added', { output_index: outputIndex, item: { id: itemId, type: 'message', role: 'assistant', status: 'in_progress', ...(selected.options.phased ? { phase: 'final_answer' } : {}), content: [] } });
        let offset = 0, contentIndex = 0;
        const complete = () => {
          if (response.destroyed || response.writableEnded) return;
          if (selected.kind === 'truncated') { response.end(); return; }
          if (selected.kind === 'plan-stream-limit') {
            event('response.failed', { response: { id: record.responseId, status: 'failed', error: { code: 'subscription_sharing_usage_limit_exceeded', message: 'Synthetic late plan limit' }, usage } }); response.end(); return;
          }
          const content = selected.kind === 'refusal' ? [{ type: 'refusal', refusal: 'Synthetic fixture refusal' }] : parts.map(text => ({ type: 'output_text', text }));
          const output = [{ id: itemId, type: 'message', role: 'assistant', status: 'completed', ...(selected.options.phased ? { phase: 'final_answer' } : {}), content }];
          if (finalizedItems) event('response.output_item.done', { output_index: outputIndex, item: output[0] });
          if (selected.options.phased && !selected.options.omitFinalizedCommentary) output.unshift(commentaryItem);
          if (selected.options.emptyTerminalOutput) output.length = 0;
          record.responseTerminalItems = output.length;
          event('response.completed', { response: { id: record.responseId, status: 'completed', usage, output } });
          record.completed = true; response.end();
        };
        const pump = () => {
          if (response.destroyed || response.writableEnded) return;
          if (contentIndex >= parts.length) { complete(); return; }
          const part = parts[contentIndex];
          if (offset >= part.length) {
            event('response.output_text.done', { output_index: outputIndex, content_index: selected.options.missingContentPart && contentIndex === 1 ? 7 : contentIndex, item_id: itemId, text: part });
            contentIndex += 1; offset = 0; schedule(pump, selected.options.chunkDelayMs ?? 25); return;
          }
          let end = Math.min(part.length, offset + (selected.options.chunkSize ?? 384));
          if (end < part.length && /[\uDC00-\uDFFF]/.test(part[end]) && /[\uD800-\uDBFF]/.test(part[end - 1])) end += 1;
          const delta = part.slice(offset, end); offset = end;
          event('response.output_text.delta', { output_index: outputIndex, content_index: selected.options.missingContentPart && contentIndex === 1 ? 7 : contentIndex, item_id: itemId, delta });
          schedule(pump, selected.options.chunkDelayMs ?? 25);
        };
        if (selected.kind === 'waiting') {
          pending.set(record.metadata.jobId, pump);
        } else if (selected.kind === 'slow') {
          event('response.output_text.delta', { output_index: outputIndex, content_index: 0, item_id: itemId, delta: parts[0].slice(0, 192) }); offset = 192;
          pending.set(record.metadata.jobId, pump);
        } else schedule(pump, 50);
      };
      if (selected.kind === 'waiting') pending.set(record.metadata.jobId, beginStream);
      else beginStream();
    } catch (error) {
      errors.push(error.message); response.writeHead(400, { 'content-type': 'application/json' }).end('{"error":{"code":"fixture_contract_failure"}}');
    }
  });
  server.on('connection', socket => { sockets.add(socket); socket.on('close', () => sockets.delete(socket)); });
  server.on('upgrade', (request, socket) => { probes.push({ method: 'UPGRADE', path: new URL(request.url, 'http://127.0.0.1').pathname, at: new Date().toISOString() }); socket.destroy(); });
  await new Promise((accept, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', accept); });
  endpoint = `http://127.0.0.1:${server.address().port}/v1`;
  return {
    endpoint,
    key(which = 'primary') { assert.ok(keys[which]); return keys[which]; },
    expectKey(which) { assert.ok(keys[which]); expectedKey = keys[which]; },
    setMode(kind, options = {}) { assert.ok(['complete', 'waiting', 'slow', 'malformed', 'refusal', 'truncated', 'auth', 'quota', 'rate-limit', 'conflict', 'plan-limit', 'plan-stream-limit', 'non-stream-error', 'non-stream-html'].includes(kind)); mode = { kind, options: { ...options, conflict: kind === 'conflict' || options.conflict === true } }; },
    oauth,
    requests: () => clone(records), generationRequests: () => clone(records.filter(value => value.method === 'POST' && value.purpose === 'generation')),
    identityRequests: () => clone(records.filter(value => value.method === 'POST' && value.purpose === 'identityAnalysis')),
    modelRequests: () => clone(records.filter(value => value.method === 'GET')), networkProbes: () => clone(probes), errors: () => [...errors],
    release(jobId) { const pump = pending.get(jobId); pending.delete(jobId); pump?.(); },
    async close() { await oauth.settle(); timers.forEach(clearTimeout); timers.clear(); pending.clear(); sockets.forEach(socket => socket.destroy()); await new Promise(accept => server.close(accept)); },
  };
}

export async function assertNoSyntheticKeys(paths, reportPath) {
  const checked = []; const seen = new Set();
  async function visit(path) {
    const absolute = resolve(path); if (seen.has(absolute)) return; seen.add(absolute);
    let info; try { info = await lstat(absolute); } catch (error) { if (error.code === 'ENOENT') return; throw error; }
    assert.equal(info.isSymbolicLink(), false, 'Secret scanning is limited to task-owned ordinary files, not symlink targets');
    if (info.isDirectory()) { for (const child of await readdir(absolute)) await visit(join(absolute, child)); return; }
    if (!info.isFile()) return;
    const bytes = await readFile(absolute);
    for (const key of syntheticKeys) assert.equal(bytes.includes(Buffer.from(key)), false, `Synthetic credential leaked into ${absolute}`);
    checked.push({ path: absolute, bytes: bytes.length, sha256: digest(bytes) });
  }
  for (const path of paths) await visit(path);
  const report = { passed: true, credentialCount: syntheticKeys.size, files: checked };
  if (reportPath) await writeFile(reportPath, JSON.stringify(report, null, 2), { mode: 0o600 });
  return report;
}
