import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { waitFor } from './webdriver.mjs';
import { PhysicalInput } from './physical-workflow.mjs';

export async function imageResizeWorkflow(driver, physical, step, output, provider) {
  let project, original, pageId, originalData;
  const picture = join(output, 'large-original.png');
  await step('Create a picture slot and a decodable PNG larger than the former 32 MB limit', async () => {
    await driver.click('//button[.//strong[normalize-space(.)="Blank website"]]', 'xpath'); await driver.fill('Project name', 'Large picture'); await driver.button('Create project');
    await driver.button('Skip tips'); await physical.initialize(output);
    project = (await driver.invoke('get_workspace')).project; pageId = project.homePageId;
    await driver.invoke('edit_page_source', { projectId: project.id, pageId, revision: project.revision, html: '<main><p>Keep this text</p><img id="picture" width="240" height="120" alt="Image placeholder: Studio" data-desegnos-placeholder="image" src="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNQCi3/DwADeQHuOPZfDAAAAABJRU5ErkJggg=="></main>', css: 'body{margin:0}main{padding:24px}img{display:block;object-fit:cover}', javascript: '' });
    await driver.execute(() => location.reload());
    await driver.ready('[data-testid="canvas"]');
    // Reloading the fixture also resets the runner's browser error listeners.
    await driver.execute(() => {
      window.__desegniTestErrors = [];
      window.addEventListener('error', event => window.__desegniTestErrors.push({ kind: 'error', message: event.message }));
      window.addEventListener('unhandledrejection', event => window.__desegniTestErrors.push({ kind: 'rejection', message: String(event.reason) }));
      const original = console.error;
      console.error = (...values) => { window.__desegniTestErrors.push({ kind: 'console', message: values.map(String).join(' ') }); original.apply(console, values); };
    });
    await driver.page(pageId); await driver.ready('.image-drop-target');
    original = (await driver.invoke('get_workspace')).project.pages[0].content;
    const data = await driver.execute(() => {
      const canvas = document.createElement('canvas'); canvas.width = 800; canvas.height = 400;
      const ctx = canvas.getContext('2d'), pixels = ctx.createImageData(canvas.width, canvas.height);
      let seed = 12345;
      for (let i = 0; i < pixels.data.length; i += 4) { seed = (1664525 * seed + 1013904223) >>> 0; pixels.data[i] = seed & 255; pixels.data[i+1] = seed >>> 8 & 255; pixels.data[i+2] = seed >>> 16 & 255; pixels.data[i+3] = 255; }
      ctx.putImageData(pixels, 0, 0); return canvas.toDataURL('image/png').split(',')[1];
    });
    originalData = data;
    // Trailing padding exercises byte-size acceptance without oversized pixel buffers.
    const bytes = Buffer.alloc(33 * 1024 * 1024); Buffer.from(data, 'base64').copy(bytes);
    await writeFile(picture, bytes);
  });
  async function previewed() {
    await waitFor(() => driver.execute(() => document.querySelector('.static-preview iframe')?.contentDocument?.querySelector('#picture')?.naturalWidth === 800), 'Original large picture did not appear');
    assert.ok(await driver.execute(() => document.querySelector('.static-preview iframe').contentDocument.querySelector('#picture').getAttribute('src').length > 32 * 1024 * 1024 * 4 / 3));
    assert.deepEqual((await driver.invoke('get_workspace')).project.pages[0].content, original, 'Picture was resized or committed before Generate');
    assert.equal(await driver.execute(() => [...document.querySelectorAll('button')].find(button => button.textContent === 'Generate').disabled), false);
  }
  async function generate() {
    await driver.button('Generate');
    await waitFor(async () => (await driver.invoke('get_workspace')).project.pages[0].content.html !== original.html, 'Generate did not commit the picture');
    const saved = (await driver.invoke('get_workspace')).project;
    const next = saved.pages[0].content;
    const reference = next.html.match(/src="desegnos-asset:([0-9a-f-]{36})"/);
    assert.ok(reference, 'The resized picture must be stored as a project reference');
    assert.doesNotMatch(next.html, /data:image\//);
    const uri = saved.pictures[reference[1]];
    assert.match(uri, /^data:image\//);
    await waitFor(() => driver.execute(expected => { const image = document.querySelector('.static-preview iframe')?.contentDocument?.querySelector('#picture'); return image?.naturalWidth === 800 && image.getAttribute('src') === expected; }, uri), 'The saved reference did not resolve to the resized preview');
    assert.ok(Buffer.from(uri.split(',')[1], 'base64').length <= 256 * 1024);
    assert.equal(next.css, original.css); assert.equal(next.javascript, original.javascript); assert.match(next.html, /Keep this text/);
    assert.equal(provider.generationRequests().length, 0, 'Local resizing should not spend AI usage');
    await waitFor(() => driver.execute(() => !document.querySelector('.image-drop-target')), 'Placeholder was not removed');
  }
  await step('File selection shows the original immediately; Generate resizes and commits without an AI call', async () => {
    await driver.execute(data => {
      const inputElement = document.querySelector('input[accept="image/png,image/jpeg,image/webp,image/gif"]');
      const click = inputElement.click; inputElement.click = () => {};
      document.querySelector('.image-drop-target').click(); inputElement.click = click;
      const bytes = new Uint8Array(33 * 1024 * 1024); bytes.set(Uint8Array.from(atob(data), c => c.charCodeAt(0)));
      const file = new File([bytes], 'large-original.png', { type: 'image/png' });
      const transfer = new DataTransfer(); transfer.items.add(file);
      const input = document.querySelector('input[accept="image/png,image/jpeg,image/webp,image/gif"]');
      input.files = transfer.files; input.dispatchEvent(new Event('change', { bubbles: true }));
    }, originalData);
    await previewed(); await generate();
    await driver.button('Undo last change'); await driver.ready('.image-drop-target');
    assert.deepEqual((await driver.invoke('get_workspace')).project.pages[0].content, original);
  });
  await step('Native file drag accepts the same large picture and defers resizing until Generate', async () => {
    const source = spawn('/usr/bin/python3', [fileURLToPath(new URL('./file-drag-source.py', import.meta.url)), picture], { env: process.env, stdio: ['ignore', 'pipe', 'pipe'] });
    let log = ''; source.stdout.on('data', chunk => log += chunk);
    try {
      await waitFor(() => log.includes('"ready": true'), 'File drag source did not open');
      const snapshot = await new PhysicalInput(driver, source.pid).inspect();
      const window = snapshot.window;
      assert.ok(window); assert.equal(snapshot.display, process.env.DISPLAY);
      const point = await driver.execute(() => { const frame = document.querySelector('.static-preview iframe'), outer = frame.getBoundingClientRect(), r = frame.contentDocument.querySelector('#picture').getBoundingClientRect(); const scale = outer.width / frame.clientWidth; return { x: outer.left + (r.x + r.width/2)*scale, y: outer.top + (r.y + r.height/2)*scale }; });
      await physical.raw({ action: 'drag', sourcePid: source.pid, from: { x: window.x + window.width/2, y: window.y + window.height/2 }, to: physical.rootPoint(point) });
      await previewed(); await generate();
    } finally { source.kill('SIGTERM'); }
  });
  return (await driver.invoke('get_workspace')).project;
}
