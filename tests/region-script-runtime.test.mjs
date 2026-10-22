import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';

const template = readFileSync(new URL('../crates/desegni-core/src/region_script_runtime.js', import.meta.url), 'utf8');
const validator = readFileSync(new URL('../crates/desegni-core/src/region_scripts.rs', import.meta.url), 'utf8');
const events = validator.match(/const GLOBAL_READ_EVENTS:[\s\S]*?= &\[([\s\S]*?)\];/)[1].match(/"[^"]+"/g).map(JSON.parse);
const runtime = code => template.replace('__REGION_SELECTOR__', '"#source"')
  .replace('__GLOBAL_READ_EVENTS__', JSON.stringify(events)).replace('__REGION_CODE__', code);
const run = code => `<script>${runtime(code)}</script>`;
const folder = mkdtempSync(join(tmpdir(), 'desegni-region-runtime-'));
try {
  writeFileSync(join(folder, 'test.html'), `<!doctype html>
    <section id="root"><img><button>Inside</button><script id="source" type="application/x-desegnos-region"></script></section>
    <p id="outside">Outside</p><pre id="result"></pre>
    <script>
      const errors = [];
      console.error = (...args) => errors.push(args.map(String).join(' '));
      const root = document.querySelector('#root');
      const outside = document.querySelector('#outside');
      function check(value, message) { if (!value) throw new Error(message); }
    </script>
    ${run(`
      const portrait = root.querySelector('img');
      document.addEventListener('pointermove', event => {
        portrait.src = event.clientX > 100 ? '#right' : '#left';
        root.dataset.target = event.target.matches('#outside');
        root.dataset.page = event.pageX;
        root.dataset.bounds = event.target.closest('body').getBoundingClientRect().width > 0;
      });
      root.dataset.media = window.matchMedia('(min-width: 0px)').matches;
      root.dataset.width = document.body.getBoundingClientRect().width;
      const label = document.createElement('span'); label.textContent = 'Created'; root.appendChild(label);
      document.addEventListener('click', event => {
        if (!root.contains(event.target)) return;
        event.preventDefault(); event.stopPropagation();
      });
      root.addEventListener('custom', e => { root.dataset.local = e.target.matches('button'); });
      const removed = () => root.dataset.removed = 'bad';
      document.body.addEventListener('mousemove', removed);
      document.body.removeEventListener('mousemove', removed);
      document.documentElement.addEventListener('touchmove', e => root.dataset.touch = e.target.matches('#outside'));
    `)}
    <script>
      try {
        outside.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, clientX: 220, clientY: 30 }));
        check(root.querySelector('img').getAttribute('src') === '#right', 'portrait follows an outside cursor');
        check(root.dataset.target === 'true' && root.dataset.page === '220', 'real target and page coordinates');
        check(root.dataset.bounds === 'true' && root.dataset.media === 'true', 'global geometry and media reads');
        check(root.querySelector('span').textContent === 'Created', 'created local nodes stay writable');
        const externalClick = new MouseEvent('click', { bubbles: true, cancelable: true });
        outside.dispatchEvent(externalClick);
        check(!externalClick.defaultPrevented, 'outside click stays untouched');
        const localClick = new MouseEvent('click', { bubbles: true, cancelable: true });
        root.querySelector('button').dispatchEvent(localClick);
        check(localClick.defaultPrevented, 'guarded inside click can be cancelled');
        root.querySelector('button').dispatchEvent(new Event('custom', { bubbles: true }));
        check(root.dataset.local === 'true', 'local custom event');
        outside.dispatchEvent(new MouseEvent('mousemove', { bubbles: true }));
        check(!root.dataset.removed, 'listener removal uses the original target');
        outside.dispatchEvent(new Event('touchmove', { bubbles: true }));
        check(root.dataset.touch === 'true', 'documentElement listener');
        check(errors.length === 0, errors.join('\\n'));
      } catch (error) { document.querySelector('#result').textContent = 'FAIL: ' + error.stack; }
    </script>
    ${run(`document.body.style.backgroundColor = 'red';`)}
    ${run(`document.querySelectorAll('p').forEach(node => node.remove());`)}
    ${run(`root.closest('body').classList.add('changed');`)}
    ${run(`root.appendChild(document.querySelector('#outside'));`)}
    ${run(`document.addEventListener('wheel', e => { e.preventDefault(); e.stopPropagation(); });`)}
    <script>
      try {
        check(document.body.style.backgroundColor === '', 'outside style write blocked');
        check(document.querySelector('#outside') === outside, 'outside collection callback write blocked');
        check(!document.body.classList.contains('changed'), 'closest outside write blocked');
        check(outside.parentElement === document.body, 'moving outside nodes blocked');
        const wheel = new WheelEvent('wheel', { bubbles: true, cancelable: true });
        let bubbled = false;
        window.addEventListener('wheel', () => bubbled = true);
        outside.dispatchEvent(wheel);
        check(!wheel.defaultPrevented && bubbled, 'unguarded global event cancellation blocked at runtime');
        check(errors.length === 4, 'four outside writes rejected: ' + errors.join('\\n'));
        if (!document.querySelector('#result').textContent) document.querySelector('#result').textContent = 'PASS';
      } catch (error) { document.querySelector('#result').textContent = 'FAIL: ' + error.stack; }
    </script>`);
  const result = spawnSync(process.env.CHROMIUM || 'chromium', [
    '--headless', '--disable-gpu', '--no-sandbox', '--no-first-run', '--disable-background-networking',
    `--user-data-dir=${join(folder, 'profile')}`, '--dump-dom', `file://${join(folder, 'test.html')}`,
  ], { encoding: 'utf8', timeout: 30000 });
  assert.equal(result.status, 0, result.error?.message || result.stderr);
  assert.match(result.stdout, /<pre id="result">PASS<\/pre>/, result.stdout.match(/<pre id="result">[\s\S]*?<\/pre>/)?.[0] || result.stderr);
  console.log('Region script runtime checks passed in headless Chromium.');
} finally {
  rmSync(folder, { recursive: true, force: true });
}
