import assert from 'node:assert/strict';
import { copyFile, mkdir, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
import { setTimeout as delay } from 'node:timers/promises';
import { codeMetaFixture } from './code-meta-fixture.mjs';
import { waitFor } from './webdriver.mjs';

// Screenshot-only tour. All state belongs to the runner's disposable XDG profile.
export async function uiTour(driver, physical, step, evidence, provider, restart) {
  const output = process.env.DESEGNI_UI_TOUR_OUTPUT ?? '/tmp/claude-1000/-home-cheli-Projects-WebMaker/538ce88b-ecfe-4675-909c-75c49eb385c1/scratchpad/tour';
  await mkdir(output, { recursive: true });
  const entries = [], failures = [], captures = [], layoutChecks = [];
  // A focused rerun preserves already verified captures and replaces only chosen surfaces.
  const selected = process.env.DESEGNI_UI_TOUR_SURFACES ? new Set(process.env.DESEGNI_UI_TOUR_SURFACES.split(',').map(Number)) : null;
  if (selected) {
    const previous = await readFile(join(output, 'index.md'), 'utf8').catch(() => '');
    entries.push(...previous.split('\n').filter(line => line.startsWith('- [') && !selected.has(Number(line.match(/^- \[(\d+)/)?.[1]))));
    failures.push(...previous.split('\n').filter(line => /^- \d+\. /.test(line) && !selected.has(Number(line.match(/^- (\d+)/)[1]))).map(line => line.slice(2)));
  }
  const { x, y, width, height } = (await physical.inspect()).window;
  const original = { x, y, width, height };
  const workspace = () => driver.invoke('get_workspace');
  const project = async () => (await workspace()).project;
  async function index() {
    await writeFile(join(output, 'index.md'), '# Desegnos UI tour\n\nFull native window captures on guarded Xvfb; real Rust state and synthetic loopback AI.\n' +
      `Default window: ${original.width} × ${original.height}; narrow: 900 × ${original.height}. Both themes at both sizes.\n\n` + entries.join('\n') + '\n\n' +
      (failures.length ? '## Tour issues\n\n' + failures.map(value => `- ${value}`).join('\n') : 'All requested tour steps captured.') + '\n');
  }
  async function close() {
    await driver.execute(() => {
      document.querySelector('.delete-inline button:last-of-type')?.click();
      document.querySelector('.draft-preview-bar button:last-child')?.click();
      for (const node of [...document.querySelectorAll('.workspace-surface')].reverse()) node.querySelector('button[aria-label^="Close "]')?.click();
    });
    await physical.keys('Escape');
  }
  async function capture(number, name, description, ready) {
    if (selected && !selected.has(number)) return;
    for (const [theme, narrow, suffix] of [['dark', false, 'dark'], ['light', false, 'light'], ['dark', true, 'narrow'], ['light', true, 'light-narrow']]) {
      await driver.command('POST', '/window/rect', { width: narrow ? 900 : original.width, height: original.height });
      await waitFor(() => driver.execute(w => innerWidth === w, narrow ? 900 : original.width), 'Window width did not settle');
      await driver.select('Editor theme', theme);
      await waitFor(() => driver.execute(value => document.documentElement.dataset.theme === value, theme), 'Theme did not settle');
      const release = await ready?.();
      await delay(180);
      const file = `${String(number).padStart(2, '0')}-${name}-${suffix}.png`;
      let geometry;
      try { geometry = await physical.captureWindow(join(evidence, file)); }
      finally { await release?.(); }
      await copyFile(join(evidence, file), join(output, file));
      entries.push(`- [${file}](${file}) — ${description} (${theme}, ${narrow ? '900 px' : 'default size'}).`);
      captures.push({ file, ...geometry });
      const layout = await driver.execute(() => {
        const issues = [];
        const rect = node => node.getBoundingClientRect();
        const inside = (child, parent) => { const a = rect(child), b = rect(parent); return a.left >= b.left - 1 && a.right <= b.right + 1; };
        const overlap = (a, b) => a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;
        if (document.documentElement.scrollWidth > innerWidth + 1 || document.body.scrollWidth > innerWidth + 1) issues.push('Horizontal document overflow');
        for (const row of document.querySelectorAll('.draft-actions')) {
          const draft = row.closest('.generation-draft'), panel = row.closest('[aria-label="AI conversation"]');
          if (!draft || !panel || !inside(row, draft) || !inside(row, panel) || ['absolute', 'fixed'].includes(getComputedStyle(row).position)) issues.push('Draft actions escaped their panel item');
          for (const button of row.children) if (!inside(button, row)) issues.push('Draft action button overflow');
        }
        const conversation = document.querySelector('[aria-label="AI conversation"]');
        if (conversation) {
          if (conversation.querySelector('.generation-status, .generation-dialog')) issues.push('Old generation details card in conversation');
          if ([...conversation.querySelectorAll('summary')].filter(node => node.textContent === 'Request details').length !== 1) issues.push('Missing or repeated request details');
          if (/Copy code|Retained code|[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/i.test(conversation.textContent)) issues.push('Code action or internal ID in conversation');
          const tokens = conversation.querySelector('.token-footer');
          if (tokens && tokens.parentElement.lastElementChild !== tokens) issues.push('Tokens do not end the AI reply');
        }
        const preview = document.querySelector('[aria-label="Previewing draft"]');
        if (preview) {
          if (preview.querySelector('.surface-close')) issues.push('Redundant preview close button');
          for (const button of preview.querySelectorAll('.draft-preview-bar button')) if (parseFloat(getComputedStyle(button).fontSize) > 15) issues.push('Preview actions are styled as title text');
        }
        const retry = document.querySelector('.retry-confirmation');
        if (retry) {
          const buttons = [...retry.querySelectorAll('button')];
          if (Math.abs(rect(buttons[0]).top - rect(buttons[1]).top) > 1 || getComputedStyle(retry).whiteSpace !== 'nowrap') issues.push('Retry confirmation wraps');
        }
        const save = document.querySelector('.save-conflict');
        if (save && (save.closest('.workspace-surface').querySelector('header') || rect(save).height > 90)) issues.push('Save conflict is not a compact bar');
        const deletion = document.querySelector('.delete-inline');
        if (deletion && !deletion.closest('.element-toolbar')) issues.push('Delete confirmation escaped selection toolbar');
        const accountMenu = document.querySelector('.ai-account-options .editor-menu');
        const model = document.querySelector('.ai-model-row select');
        if (accountMenu && model && overlap(rect(accountMenu), rect(model))) issues.push('Account menu covers the model field');
        const brief = document.querySelector('[aria-label="Project brief"]');
        if (brief && /[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/i.test(brief.textContent)) issues.push('Internal ID in project brief');
        const tabs = [...document.querySelectorAll('.dossier-navigation [role="tab"]')];
        if (new Set(tabs.map(node => Math.round(rect(node).top))).size > 1) issues.push('Project brief tabs wrap');
        const prompt = document.querySelector('.annotation textarea');
        if (prompt && getComputedStyle(prompt).backgroundColor === 'rgba(0, 0, 0, 0)') issues.push('Request text background is transparent');
        return { issues, width: innerWidth, documentWidth: document.documentElement.scrollWidth };
      });
      layoutChecks.push({ file, ...layout });
      for (const issue of layout.issues) failures.push(`${number}. ${file}: ${issue}`);
      await writeFile(join(output, 'layout-checks.json'), JSON.stringify(layoutChecks, null, 2));
      await index();
    }
    await driver.command('POST', '/window/rect', original);
  }
  async function surface(number, name, description, action, ready, after) {
    if (selected && !selected.has(number) && ![18, 19].includes(number) && !(number === 9 && selected.has(25))) return;
    try {
      await close();
      await driver.command('POST', '/window/rect', original);
      await action();
      await capture(number, name, description, ready);
      await after?.();
      console.log(`TOUR ${number} ${name}`);
    } catch (error) {
      failures.push(`${number}. ${name}: ${error.message}`);
      await writeFile(join(evidence, `tour-${number}-failure.html`), await driver.command('GET', '/source')).catch(() => {});
      console.error(`TOUR GAP ${number} ${name}: ${error.message}`);
      await index();
    } finally { await close().catch(() => {}); }
  }
  async function mutate(command) {
    const before = await project();
    return driver.invoke('edit_project', { revision: before.revision, command });
  }
  async function previewReady() {
    await waitFor(() => driver.execute(() => document.querySelector('.static-preview')?.dataset.previewReady === 'true'), 'Canvas preview did not mount');
  }
  async function selectElement(query, extra = false) {
    await previewReady();
    const point = await driver.execute(selector => {
      const frame = document.querySelector('.static-preview iframe'), node = frame?.contentDocument?.querySelector(selector);
      if (!node) throw new Error(`Missing element ${selector}`);
      const r = node.getBoundingClientRect(), b = frame.getBoundingClientRect(), z = Number(document.querySelector('[data-testid="canvas"]').dataset.zoom);
      return { x: b.x + (r.x + Math.min(12, r.width / 2)) * z, y: b.y + (r.y + Math.min(12, r.height / 2)) * z };
    }, query);
    await physical.raw({ action: 'click', point: physical.rootPoint(point), modifiers: extra ? ['Shift'] : [] });
  }
  await physical.initialize(evidence);
  await surface(18, 'start', 'Start screen with blank website, open and recent-project choices', async () => { await driver.ready('.new-project'); });
  await surface(19, 'new-project-location', 'Blank website creation and project location settings', async () => { await driver.button('Blank website'); });
  await driver.fill('Project name', 'Northstar Studio — website tour');
  await driver.button('Create project');
  await driver.ready('[data-testid="canvas"]');
  if (await driver.execute(() => !!document.querySelector('.first-run-tips'))) await driver.button('Skip tips');
  const { pictures, ...source } = codeMetaFixture();
  let paragraph = 0;
  source.html = source.html.replace(/<p\b[^>]*>[\s\S]*?<\/p>/g, text => paragraph++ % 15 < 2 ? text : '');
  source.html = source.html.replace(/data:image\/png;base64,[^"]+/g, (_, i) => `https://example.org/picture-${i}`);
  source.css = 'body{margin:0;background:#faf8f2;color:#18232a;font:16px system-ui}main{padding:32px}main::before{content:"NORTHSTAR STUDIO · SELECTED WORK";display:block;font-size:28px;font-weight:700;margin-bottom:24px}section{display:inline-block;vertical-align:top;width:28%;margin:0 3% 24px 0}img{width:100%;height:140px;object-fit:cover;border-radius:8px}.copy{line-height:1.6}';
  // Reuse the six-picture fixture; each edit stays below the WebDriver body limit.
  for (const picture of pictures) {
    source.html = source.html.replace(/https:\/\/example.org\/picture-\d+/, picture);
    const p = await project();
    const next = await driver.invoke('edit_page_source', { projectId: p.id, pageId: p.homePageId, revision: p.revision, ...source });
    source.html = next.project.pages[0].content.html;
  }
  const p = await project(), pageId = p.homePageId;
  await mutate({ type: 'add_region', page_id: pageId, rect: { x: 80, y: 340, width: 420, height: 180 } });
  let region = (await project()).pages[0].regions.at(-1);
  await mutate({ type: 'update_region', page_id: pageId, region_id: region.id, rect: region.rect, prompt: 'Add our studio contact details and a clear invitation to discuss a project.' });
  await driver.button('Save');
  await previewReady();
  await driver.invoke('set_ai_key', { key: provider.key(), storage: 'session' });
  // A real provider result first exposes the inline status-bubble retry question.
  provider.setMode('complete', { tourSuccess: true, chunkDelayMs: 1, chunkSize: 8192 });
  await mutate({ type: 'set_region_whole_page', page_id: pageId, region_id: region.id, whole_page: true });
  await driver.button('Save');
  await driver.button('Generate');
  await waitFor(async () => ['applied', 'blocked', 'failed'].includes((await driver.invoke('get_generation', { projectId: p.id }))?.status), 'Initial generation did not finish', 30000);
  await writeFile(join(evidence, 'initial-job.json'), JSON.stringify(await driver.invoke('get_generation', { projectId: p.id }), null, 2));
  await surface(3, 'retry-confirmation', 'Inline retry confirmation inside the Page updated status bubble', async () => {
    await driver.click('.canvas-pill button[title^="Undo this result"]');
    await driver.ready('.canvas-pill [aria-label="Confirm retry"]');
  });
  if (await driver.execute(() => !!document.querySelector('.canvas-pill [aria-label="Confirm retry"]'))) await driver.button('No');
  // Restore the pictured fixture through acknowledged source edits before creating a retained draft.
  let current = await project();
  await driver.invoke('edit_page_source', { projectId: current.id, pageId, revision: current.revision, ...source });
  await mutate({ type: 'add_region', page_id: pageId, rect: { x: 70, y: 320, width: 400, height: 180 } });
  region = (await project()).pages[0].regions.at(-1);
  await mutate({ type: 'update_region', page_id: pageId, region_id: region.id, rect: region.rect, prompt: 'Add a booking form here. Ask me for the booking service address if it is missing.' });
  await driver.button('Save');
  const previousJobId = (await driver.invoke('get_generation', { projectId: p.id }))?.jobId;
  provider.setMode('complete', { tourDraft: true, chunkDelayMs: 1, chunkSize: 8192 });
  await driver.button('Generate');
  await waitFor(async () => {
    const job = await driver.invoke('get_generation', { projectId: p.id });
    return job?.jobId !== previousJobId && ['blocked', 'applied', 'failed'].includes(job?.status) && job;
  }, 'Draft generation did not finish', 30000);
  await waitFor(() => driver.execute(() => [...document.querySelectorAll('button')].some(b => b.textContent.trim() === 'Generate' && !b.disabled)), 'Generation did not settle in the UI');
  await writeFile(join(evidence, 'draft-job.json'), JSON.stringify(await driver.invoke('get_generation', { projectId: p.id }), null, 2));
  await surface(1, 'ai-conversation', 'AI conversation, needs-details action, Build remaining draft and collapsed feedback/request details', async () => {
    await driver.button('AI log');
    await driver.ready('[aria-label="Finish your draft"]');
    await driver.execute(() => document.querySelector('.generation-feedback')?.scrollIntoView({ block: 'center' }));
    assert.ok((await driver.execute(() => document.querySelector('[aria-label="AI conversation"]').textContent)).includes('Build remaining draft'));
  });
  await surface(2, 'draft-preview', 'Retained draft overlay on the canvas with Apply anyway and Back bar', async () => {
    await driver.button('AI log');
    await driver.button('Preview');
    await waitFor(() => driver.execute(() => {
      const frame = document.querySelector('[aria-label="Previewing draft"] iframe');
      return frame?.getBoundingClientRect().width > 0 && !document.querySelector('[aria-label="Previewing draft"] .preview-status');
    }), 'The inert draft preview did not render');
  });
  await surface(4, 'delete-many', 'Delete selected items confirmation within the selection toolbar', async () => {
    await driver.button('Fit');
    await selectElement('img'); await selectElement('p', true);
    await driver.button('Delete selected items');
    await driver.ready('.delete-inline');
  });
  await surface(5, 'save-conflict', 'Save conflict banner after a simulated external change to the disposable project', async () => {
    const state = await workspace();
    const result = spawnSync('python3', ['-c', 'import sys,json,zipfile\np=sys.argv[1]\nwith zipfile.ZipFile(p) as z: f={n:z.read(n) for n in z.namelist()}\nf["manifest.json"]=json.dumps(json.loads(f["manifest.json"]),indent=3).encode()\nwith zipfile.ZipFile(p,"w",zipfile.ZIP_DEFLATED) as z:\n for n,v in f.items(): z.writestr(n,v)', state.path], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    await driver.button('Save'); await driver.ready('.save-conflict');
  });
  await surface(6, 'element-editor', 'Element editor in the side panel', async () => {
    await driver.button('Fit'); await selectElement('img'); await physical.keys('F2'); await driver.ready('.element-dialog');
  });
  await surface(7, 'ai-settings', 'AI settings account card with its account options menu open', async () => {
    await driver.button('AI settings'); await driver.ready('.ai-settings form');
  }, async () => {
    if (!await driver.execute(() => !!document.querySelector('.ai-account-options [role="menu"]'))) await driver.button('Account options');
  });
  await surface(8, 'import-context', 'Import context step 1: source choices', async () => { await driver.button('Import context'); await driver.ready('.import-context'); });
  await surface(9, 'import-review', 'Choose what to keep: brand colours, fonts and pages', async () => {
    await driver.button('Import context');
    await driver.fill('Website address', `${provider.endpoint.replace(/\/v1$/, '')}/brand-site/`);
    await driver.button('Open website');
    await driver.ready('//button[normalize-space(.)="Read my brand"]', 'xpath');
    provider.setMode('complete', { chunkDelayMs: 1, chunkSize: 8192 });
    await driver.button('Read my brand'); await driver.ready('.identity-result[aria-label="Brand review"]');
  }, undefined, async () => {
    await capture(25, 'import-review-pages', 'Choose what to keep: imported page choices and content', async () => {
      await driver.execute(() => document.querySelector('.identity-review-section:last-of-type')?.scrollIntoView({ block: 'center' }));
    });
    await driver.button('Add to project');
    await driver.page(pageId);
  });
  for (const [number, name, title, button] of [[10, 'project-brief', 'Project brief', 'Project dossier'], [11, 'style-guide', 'Style guide'], [12, 'keyboard-shortcuts', 'Keyboard shortcuts'], [13, 'grid-settings', 'Grid and rulers', 'Grid and rulers'], [14, 'add-page', 'Add page', 'Add page'], [15, 'rename-page', 'Page settings', 'Page settings'], [16, 'rename-project', 'Rename project']]) {
    await surface(number, name, title + ' side panel', async () => {
      if (name === 'rename-project') await driver.click('.project-title');
      else if (button) await driver.button(button); else await driver.menu([title]);
      await driver.ready(`.workspace-surface[aria-label="${title}"]`);
    });
  }
  await surface(17, 'live-mode', 'Live mode with device controls', async () => { await driver.menu(['Live preview']); await driver.ready('.live-bar'); });
  if (await driver.execute(() => !!document.querySelector('.live-bar'))) await driver.button('Exit live');
  await surface(20, 'request-resize', 'Request resized with its size label outside the box', async () => {
    await driver.button('Layers');
    await driver.click(`.layer-row[data-region-id="${region.id}"] button.layer-select`); await driver.button('Close Layers');
    await driver.ready('.resize-grip');
  }, async () => {
    await driver.button('Fit');
    const grip = await physical.center('.resize-grip');
    const native = await physical.inspect();
    const from = physical.rootPoint(grip), to = physical.rootPoint({ x: grip.x + 40, y: grip.y + 25 });
    for (const point of [from, to]) assert.ok(point.x >= native.window.x && point.x < native.window.x + native.window.width && point.y >= native.window.y && point.y < native.window.y + native.window.height);
    assert.ok(/^:(\d+)$/.test(native.display) && Number(native.display.slice(1)) >= 90);
    // Hold a trusted XTEST drag until captureWindow finishes. The child always releases.
    const child = spawn('/usr/bin/python3', ['-u', '-c', `import ctypes as c,ctypes.util,sys,time,select
x=c.CDLL(ctypes.util.find_library('X11'));t=c.CDLL(ctypes.util.find_library('Xtst'))
x.XOpenDisplay.argtypes=[c.c_char_p];x.XOpenDisplay.restype=c.c_void_p
x.XSync.argtypes=[c.c_void_p,c.c_int]
t.XTestFakeMotionEvent.argtypes=[c.c_void_p,c.c_int,c.c_int,c.c_int,c.c_ulong]
t.XTestFakeButtonEvent.argtypes=[c.c_void_p,c.c_uint,c.c_int,c.c_ulong]
d=x.XOpenDisplay(sys.argv[1].encode());assert d
fx,fy,tx,ty=map(int,sys.argv[2:])
t.XTestFakeMotionEvent(d,0,fx,fy,0);x.XSync(d,0);time.sleep(.05)
try:
 t.XTestFakeButtonEvent(d,1,1,0);x.XSync(d,0);time.sleep(.05)
 t.XTestFakeMotionEvent(d,0,tx,ty,0);x.XSync(d,0);time.sleep(.1)
 print('held',flush=True);select.select([sys.stdin],[],[],8)
finally:
 t.XTestFakeButtonEvent(d,1,0,0);x.XSync(d,0)
`, native.display, String(from.x), String(from.y), String(to.x), String(to.y)], { stdio: ['pipe', 'pipe', 'pipe'] });
    let held = false; child.stdout.on('data', bytes => { held ||= String(bytes).includes('held'); });
    const done = new Promise(resolve => child.once('close', resolve));
    const release = async () => { child.stdin.end(); await done; };
    try {
      await waitFor(() => held, 'Native resize pointer did not press');
      await driver.ready('.workspace-measurement');
      const outside = await driver.execute(() => {
        const a = document.querySelector('.workspace-measurement').getBoundingClientRect(), b = document.querySelector('.annotation.selected').getBoundingClientRect();
        return a.right <= b.left || a.left >= b.right || a.bottom <= b.top || a.top >= b.bottom;
      });
      assert.ok(outside, 'The size label overlaps the request box');
      return release;
    } catch (error) { await release(); throw error; }
  });
  await surface(21, 'whole-page', 'Whole page request badge and toggle', async () => {
    await driver.button('Layers'); await driver.click(`.layer-row[data-region-id="${region.id}"] button.layer-select`); await driver.button('Close Layers');
    if (!await driver.execute(() => document.querySelector('.element-toolbar button[aria-pressed="true"]')?.textContent === 'Whole page')) await driver.button('Whole page');
    await driver.ready('.request-phone-tag');
  });
  await surface(22, 'project-location', 'Default project location settings', async () => { await driver.menu(['Default project location']); await driver.ready('[aria-label="Project location"]'); });
  await surface(23, 'new-project', 'New project choices over the editor', async () => { await driver.button('New project'); await driver.ready('[aria-label="New project"]'); });
  if (restart && (!selected || selected.has(24))) {
    await mutate({ type: 'rename_project', name: 'Northstar Studio — recovered work' });
    ({ driver, physical } = await restart());
    await physical.initialize(evidence);
    await surface(24, 'recovery-start', 'Fresh-process recovery and start screen', async () => {
      await driver.menu(['Recover unsaved projects']);
      await driver.ready('.recovery-list');
    });
  }
  await writeFile(join(evidence, 'ui-tour.json'), JSON.stringify({ captures, failures, output, totalScreenshots: entries.length }, null, 2));
  await index();
  console.log(`UI TOUR: ${captures.length} new screenshots (${entries.length} total); ${failures.length} tour issues. Index: ${join(output, 'index.md')}`);
  return { captures, failures };
}
