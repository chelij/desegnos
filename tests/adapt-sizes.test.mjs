// Optional integration check: no AI, no release build, no changes to the source fixture.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { toolchainEnvironment } from '../scripts/toolchain.mjs';

const scratch = '/tmp/claude-1000/-home-cheli-Projects-WebMaker/d472d531-9403-4f03-b2c9-4b4dcbdb7b60/scratchpad';
const fixture = process.env.DESEGNOS_ADAPT_FIXTURE ?? join(scratch, 'fixtures/pulse.desegnos');
const chromium = process.env.CHROMIUM ?? ['chromium', 'chromium-browser', 'google-chrome'].find(bin => spawnSync('which', [bin]).status === 0);
if (!existsSync(fixture) || !chromium) {
  console.log('SKIP responsive fixture: fixture or Chromium unavailable');
  process.exit(0);
}
const output = await mkdtemp(join(tmpdir(), 'desegnos-adapt-'));
const shots = process.env.DESEGNOS_ADAPT_SHOTS ?? join(scratch, 'adapt-shots');
await mkdir(shots, { recursive: true });
const run = spawnSync('cargo', ['run', '-p', 'desegnos-storage', '--example', 'adapt_fixture', '--', fixture, output], { env: toolchainEnvironment(), encoding: 'utf8' });
assert.equal(run.status, 0, run.stderr + run.stdout);
console.log(run.stdout.trim());
const browser = spawn(chromium, ['--headless', '--no-sandbox', '--disable-dev-shm-usage', '--hide-scrollbars', '--remote-debugging-port=0', `--user-data-dir=${join(output, 'chromium')}`, 'about:blank'], { stdio: ['ignore', 'ignore', 'pipe'] });
try {
  const endpoint = await new Promise((resolve, reject) => {
    let stderr = '';
    const timeout = setTimeout(() => reject(new Error('Chromium startup timeout')), 15000);
    browser.stderr.on('data', data => { stderr += data; const match = stderr.match(/DevTools listening on (ws:\/\/[^\s]+)/); if (match) { clearTimeout(timeout); resolve(match[1]); } });
    browser.on('exit', code => reject(new Error(`Chromium exited ${code}: ${stderr}`)));
  });
  const socket = new WebSocket(endpoint);
  await new Promise(resolve => socket.addEventListener('open', resolve, { once: true }));
  let sequence = 0;
  const pending = new Map();
  socket.addEventListener('message', ({ data }) => { const reply = JSON.parse(data); if (reply.id) { const { resolve, reject } = pending.get(reply.id); pending.delete(reply.id); reply.error ? reject(new Error(JSON.stringify(reply.error))) : resolve(reply.result); } });
  const send = (method, params = {}, sessionId) => new Promise((resolve, reject) => { const id = ++sequence; pending.set(id, { resolve, reject }); socket.send(JSON.stringify({ id, method, params, sessionId })); });
  const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await send('Target.attachToTarget', { targetId, flatten: true });
  const cdp = (method, params) => send(method, params, sessionId);
  const evaluate = async expression => {
    const result = await cdp('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
    assert.ok(!result.exceptionDetails, JSON.stringify(result.exceptionDetails));
    return result.result.value;
  };
  const inspect = async (version, page, width, height) => {
    await cdp('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: false });
    await cdp('Page.navigate', { url: pathToFileURL(join(output, version, page)).href });
    // Wait for navigation and local/remote font loading without depending on a GUI display.
    for (let i = 0; i < 100; i++) {
      if (await evaluate(`document.readyState === 'complete' && location.pathname.endsWith('/${version}/${page}')`)) break;
      await new Promise(resolve => setTimeout(resolve, 50));
    }
    await evaluate('Promise.race([document.fonts.ready, new Promise(resolve => setTimeout(resolve, 3000))]).then(() => true)');
    // Accepted fixture behaviour types the hero heading over 550 ms. Inspect completed text.
    await new Promise(resolve => setTimeout(resolve, 750));
    return evaluate(`(() => {
      const nodes = [...document.querySelectorAll('[data-desegnos-id]')];
      const visible = node => { const s = getComputedStyle(node), r = node.getBoundingClientRect(); return s.display !== 'none' && s.visibility !== 'hidden' && Number(s.opacity) > 0 && r.width > 0 && r.height > 0; };
      const all = nodes.filter(visible).map(node => { const r = node.getBoundingClientRect(), s = getComputedStyle(node); return { id: node.dataset.desegnosId, text: [...node.childNodes].filter(n => n.nodeType === 3).map(n => n.textContent).join('').trim(), x:r.x, y:r.y, w:r.width, h:r.height, font:parseFloat(s.fontSize) }; });
      const text = all.filter(n => n.text);
      const overlaps = [];
      for (let i=0;i<text.length;i++) for (let j=i+1;j<text.length;j++) { const a=text[i], b=text[j]; if (Math.min(a.x+a.w,b.x+b.w)-Math.max(a.x,b.x)>2 && Math.min(a.y+a.h,b.y+b.h)-Math.max(a.y,b.y)>2) overlaps.push([a.text,b.text,a.id,b.id]); }
      const cssGeometry = nodes.map(node => { const s=getComputedStyle(node); return [node.dataset.desegnosId, s.position,s.left,s.top,s.width,s.height,s.fontSize,s.lineHeight,s.display]; });
      return { scroll: document.documentElement.scrollWidth, width:innerWidth, all, cssGeometry, small:text.filter(n => n.font<13), overlaps };
    })()`);
  };
  for (const page of ['index.html', 'lineup.html', 'tickets.html']) {
    const before = await inspect('before', page, 1440, 900);
    const after = await inspect('after', page, 1440, 900);
    assert.ok(before.all.length > 0, 'selector must inspect actual exported elements');
    assert.ok(after.scroll <= 1440, `${page}/desktop: horizontal scroll ${after.scroll}`);
    // Responsive visibility adds an inline display hook; the element/content markup stays shared.
    const markup = async version => (await readFile(join(output, version, page), 'utf8')).replaceAll(' style="display:var(--desegnos-initial-display)"', '');
    assert.equal(await markup('after'), await markup('before'), `${page}: element markup changed`);
    assert.deepEqual(after.cssGeometry, before.cssGeometry, `${page}: desktop CSS geometry changed`);
    for (const [label, width, height] of [['phone', 390, 844], ['tablet', 820, 1180]]) {
      const result = await inspect('after', page, width, height);
      const { data } = await cdp('Page.captureScreenshot', { format: 'png', captureBeyondViewport: true });
      const path = join(shots, `${page.replace('.html', '')}-${label}.png`);
      await writeFile(path, Buffer.from(data, 'base64'));
      console.log(path);
      assert.ok(result.scroll <= width, `${page}/${label}: horizontal scroll ${result.scroll}`);
      if (label === 'phone') assert.deepEqual(result.small, [], `${page}: fonts below 13px`);
      assert.deepEqual(result.overlaps, [], `${page}/${label}: overlapping text`);
    }
  }
  socket.close();
  console.log('PASS: 3 pages × 3 sizes; desktop unchanged, no narrow text overlap/overflow.');
} finally {
  if (browser.exitCode === null && browser.signalCode === null) {
    const exited = new Promise(resolve => browser.once('exit', resolve));
    browser.kill();
    await exited;
  }
  if (!process.env.DESEGNOS_KEEP_ADAPT_OUTPUT) await rm(output, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  else console.log(`Kept exports: ${output}`);
}
