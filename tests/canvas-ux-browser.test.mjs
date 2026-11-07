// Real dock clicks and rendered text geometry, without a release build or desktop display.
import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { promisify } from 'node:util';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';

const root = fileURLToPath(new URL('../', import.meta.url));
await mkdir(join(root, 'test-results'), { recursive: true });
const folder = await mkdtemp(join(root, 'test-results/canvas-ux-'));
const fixture = join(folder, 'index.html');
let server;
try {
  await writeFile(fixture, `<!doctype html><html><body><div id="dock"></div><div id="canvas" style="position:relative"></div><pre id="result">pending</pre>
  <script type="module">
  import React from 'react';
  import { createRoot } from 'react-dom/client';
  import { BoundaryProperties } from '/src/dock/properties.tsx';
  import { ElementView } from '/src/projectCanvas/elements.tsx';
  import { SIZE_PRESETS } from '/src/projectCanvas/sizes.ts';
  const check = (ok, message) => { if (!ok) throw Error(message); };
  const wait = async (read, message) => {
    for (let i = 0; i < 200; i++) { if (read()) return; await new Promise(resolve => setTimeout(resolve, 20)); }
    throw Error(message);
  };
  const noop = () => {};
  const size = (id, label, width, height) => ({ id, label, width, height, kind:'page', x:0, y:0 });
  let page = { id:'page', name:'PULSE', output:'webpage', path:'/', background:'#fff',
    sizes:[size('base','Desktop',1440,900), size('tablet','Tablet',820,1180), size('phone','Phone',390,1200)] };
  let selectedSizeId = 'base';
  const edits = [], additions = [];
  const dock = createRoot(document.getElementById('dock'));
  const render = () => dock.render(React.createElement(BoundaryProperties, {
    page, selectedSizeId, busy:false, commit:noop, adaptSize:noop, removeSize:noop, remove:noop,
    selectSize:id => { selectedSizeId = id; render(); },
    updateSize:(id, patch) => { edits.push({id, patch}); page = {...page, sizes:page.sizes.map(s => s.id === id ? {...s,...patch} : s)}; render(); },
    addSize:preset => { additions.push(preset); const p = SIZE_PRESETS.find(p => p.preset === preset); page = {...page,sizes:[...page.sizes,size(preset,p.label,p.width,p.height)]}; selectedSizeId = preset; render(); },
  }));
  const chip = label => [...document.querySelectorAll('.dock-size-add .boundary-add-row button')].find(b => b.textContent === label);
  const pressed = label => chip(label)?.getAttribute('aria-pressed') === 'true';
  try {
    render();
    await wait(() => chip('Phone'), 'Size chips did not render');
    check(pressed('Desktop') && !pressed('Phone'), 'Pressed state must follow focused size');
    chip('Phone').click();
    await wait(() => pressed('Phone'), 'Phone chip did not focus phone');
    check(selectedSizeId === 'phone' && edits.length === 0 && additions.length === 0, 'Existing phone must select, not resize or add');
    check(page.sizes[0].width === 1440 && page.sizes[0].height === 900, 'Desktop was resized');
    check(document.querySelector('[title="Adapt Phone layout"]'), 'Focused phone must offer Adapt layout');
    chip('Desktop').click();
    await wait(() => pressed('Desktop'), 'Desktop chip did not focus base');
    check(!document.querySelector('[title^="Adapt "]'), 'Base must not offer Adapt layout');
    page = {...page,sizes:[page.sizes[0],size('custom-phone','Small',390,999)]}; render();
    await wait(() => !document.querySelector('[data-size-row="tablet"]'), 'Size list did not update');
    chip('Phone').click();
    await wait(() => selectedSizeId === 'custom-phone' && pressed('Phone'), 'Matching width must select despite custom label/height');
    page = {...page,sizes:[page.sizes[0],size('named-phone','Phone',400,999)]}; selectedSizeId='base'; render();
    await wait(() => document.querySelector('[data-size-row="named-phone"]'), 'Named phone did not render');
    chip('Phone').click();
    await wait(() => selectedSizeId === 'named-phone' && pressed('Phone'), 'Matching label must select despite custom width');
    chip('Tablet').click();
    await wait(() => pressed('Tablet'), 'Missing tablet was not added and focused');
    check(additions.join() === 'tablet' && edits.length === 0 && page.sizes[0].width === 1440, 'Missing preset must add exactly once and preserve base');
    chip('Tablet').click();
    check(additions.length === 1, 'Repeated click duplicated a linked size');
    for (const [output,label,width,height] of [['slide','4:3',1024,768],['image','Story',1080,1920]]) {
      page = {...page,output,sizes:[size('base','Base',1920,1080)]}; selectedSizeId='base'; render();
      await wait(() => chip(label), output + ' presets missing');
      chip(label).click();
      await wait(() => pressed(label), output + ' preset did not resize');
      check(page.sizes.length === 1 && page.sizes[0].width === width && page.sizes[0].height === height, output + ' must retain single-size presets');
    }
    const canvas = createRoot(document.getElementById('canvas'));
    for (const kind of ['text','link','button','input','textarea','select','checkbox','radio']) {
      for (const align of ['left','center','right']) {
        for (const decoration of [undefined,'underline','line-through']) {
          const element = {id:'pill',kind,rect:{x:0,y:0,width:300,height:60},visible:true,rotation:0,
            text:'GET TICKETS',children:[],layout:{kind:'free'},style:{fill:'#ff0',radius:30,opacity:1,
              stroke:{color:null,width:0},shadow:{color:null,x:0,y:0,blur:0},
              textStyle:{fontFamily:'Archivo',size:16,weight:600,italic:false,lineHeight:1.4,letterSpacing:0,color:'#111',align,decoration}}};
          canvas.render(React.createElement(ElementView,{project:{},pageId:'page',element,flow:false,editingId:null}));
          await wait(() => {
            const node = document.querySelector('[data-element="pill"] > div');
            const css = node && getComputedStyle(node);
            return css?.visibility === 'visible' && css.textAlign === align && css.textDecorationLine === (decoration ?? 'none');
          }, kind + ': stored text alignment/decoration not applied');
          if (kind === 'link' || kind === 'button') {
            const node = document.querySelector('[data-element="pill"] > div');
            const range = document.createRange(); range.selectNodeContents(node);
            const text = range.getBoundingClientRect(), box = node.getBoundingClientRect();
            const distance = align === 'center' ? Math.abs((text.left+text.right-box.left-box.right)/2)
              : align === 'left' ? Math.abs(text.left-box.left) : Math.abs(text.right-box.right);
            check(distance < 3, kind + ': label geometry does not follow ' + align + ' alignment: ' + distance);
          }
        }
      }
    }
    document.getElementById('result').textContent = 'PASS: preset selection/addition, base preservation, focused state, Adapt layout, slide/image presets; 8 text kinds × 3 alignments × 3 decorations and pill label geometry';
  } catch (error) { document.getElementById('result').textContent = 'FAIL: ' + error.stack; }
  </script></body></html>`);
  server = await createServer({ root, configFile: false, esbuild: { jsx: 'automatic' },
    plugins: [{ name: 'fixture-canvas-font', enforce: 'pre', transform(code, id) {
      if (id.endsWith('/src/projectCanvas/fonts.ts')) return code.replace("import { native } from '../api';",
        "const native = async <T,>() => ({ id: 'fixture-archivo', bundled: true, fellBack: false, italic: false } as T);");
    } }], cacheDir: join(folder, 'cache'),
    server: { host: '127.0.0.1', port: 0, strictPort: false }, optimizeDeps: { entries: [fixture] } });
  await server.listen();
  const { stdout } = await promisify(execFile)(process.env.CHROMIUM ?? 'chromium', [
    '--headless', '--no-first-run', '--disable-background-networking', '--disable-dev-shm-usage',
    '--no-default-browser-check', '--password-store=basic', '--disable-extensions',
    '--user-data-dir=' + join(folder, 'profile'), '--dump-dom', '--virtual-time-budget=15000',
    'http://127.0.0.1:' + server.httpServer.address().port + fixture.slice(root.length - 1),
  ], { timeout: 45000, maxBuffer: 2 * 1024 * 1024 });
  const result = stdout.match(/<pre id="result">([\s\S]*?)<\/pre>/)?.[1];
  assert.match(result ?? stdout.slice(-2000), /^PASS:/);
  console.log(result);
} finally {
  await server?.close();
  await rm(folder, { recursive: true, force: true });
}
