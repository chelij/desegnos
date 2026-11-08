import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { once } from 'node:events';
import test from 'node:test';
import { createServer } from 'vite';

// Headless only: exercises the actual React queue and computed layout without desktop windows.
test('notice queue preempts, resumes and dismisses duplicates; layout stays readable in both themes', async () => {
  const folder = await mkdtemp(new URL('../.notice-system-', import.meta.url));
  let server;
  try {
    await writeFile(join(folder, 'index.html'), '<div id="root" style="width:900px"></div><pre id="result">pending</pre><script type="module" src="./check.jsx"></script>');
    await writeFile(join(folder, 'check.jsx'), `
      import React from 'react';
      import { createRoot } from 'react-dom/client';
      import { flushSync } from 'react-dom';
      import { Notice, NoticeHost, NoticeProvider } from '/src/ProblemBanner.tsx';
      import '/src/styles.css'; import '/src/brand.css'; import '/src/canvas.css'; import '/src/shell.css'; import '/src/notice.css'; import '/src/ui.css';
      const target = document.querySelector('#root'); const root = createRoot(target);
      const calls = []; const check = (ok, message) => { if (!ok) throw Error(message); };
      const wait = async (condition, message) => {
        const deadline = Date.now() + 3000;
        while (!condition()) {
          if (Date.now() >= deadline) throw Error(message);
          await new Promise(resolve => setTimeout(resolve, 0));
        }
      };
      const shows = text => row()?.textContent.includes(text);
      const items = [
        { key: 'info1', message: 'First info' }, { key: 'info2', message: 'Second info' },
        { key: 'result', variant: 'success', priority: 'result', title: 'Page updated', message: 'Also changed page styles, page script and 2 other boxes' },
        { key: 'warning', variant: 'warning', message: 'A warning' },
      ];
      function render() { flushSync(() => root.render(<React.StrictMode><NoticeProvider><NoticeHost />{items.map(({ key, ...item }) => <Notice key={key} {...item} dismiss={() => calls.push(key)} />)}</NoticeProvider></React.StrictMode>)); }
      const row = () => document.querySelector('.notice');
      const dismiss = async condition => { flushSync(() => row().querySelector('.notice-dismiss').click()); await wait(condition, 'dismiss did not advance the queue'); };
      function contrast(fg, bg) {
        const luminance = color => { const rgb = color.match(/\\d+/g).slice(0, 3).map(Number).map(n => n / 255).map(n => n <= .04045 ? n / 12.92 : ((n + .055) / 1.055) ** 2.4); return rgb[0] * .2126 + rgb[1] * .7152 + rgb[2] * .0722; };
        const a = luminance(fg), b = luminance(bg); return (Math.max(a, b) + .05) / (Math.min(a, b) + .05);
      }
      try {
        render(); await wait(() => shows('A warning'), 'warning did not appear'); check(row().textContent.includes('A warning'), 'warning precedes result and info');
        items.push({ key: 'error1', variant: 'error', title: 'Error', message: 'A long message '.repeat(40), action: { label: 'Try again', onClick: () => calls.push('primary') }, secondaryAction: { label: 'Details', onClick: () => calls.push('secondary') } });
        items.push({ ...items.at(-1), key: 'error2' }); render(); await wait(() => shows('A long message'), 'error did not preempt');
        check(document.querySelectorAll('.notice').length === 1, 'only one bar');
        check(row().getAttribute('role') === 'alert' && row().classList.contains('notice-error'), 'error preempts');
        for (const theme of ['light', 'dark']) {
          document.documentElement.dataset.theme = theme;
          const style = getComputedStyle(row()); check(contrast(style.color, style.backgroundColor) >= 4.5, theme + ' error contrast');
          for (const width of [900, 360]) {
            target.style.width = width + 'px';
            await wait(() => target.getBoundingClientRect().width === width, 'container did not resize');
            const message = row().querySelector('.notice-copy span');
            check(getComputedStyle(message).whiteSpace === 'nowrap' && message.getBoundingClientRect().height === 20, 'message stays one line');
            check(message.scrollWidth > message.clientWidth, 'long copy truncates');
            check(row().querySelector('.notice-copy').title.includes('A long message'), 'full text tooltip');
            const buttons = [...row().querySelectorAll('button')].map(button => button.getBoundingClientRect());
            check(buttons.every(rect => rect.top === buttons[0].top), 'actions and dismiss stay together');
            check(buttons.at(-1).right <= row().getBoundingClientRect().right, 'actions fit');
            check(width === 900 ? buttons[0].top < message.getBoundingClientRect().bottom : buttons[0].top >= message.getBoundingClientRect().bottom, 'only narrow widths wrap actions');
          }
        }
        row().querySelector('.primary').click(); row().querySelectorAll('button')[1].click();
        check(calls.includes('primary') && calls.includes('secondary'), 'action callbacks');
        await dismiss(() => shows('A warning')); check(calls.includes('error1') && calls.includes('error2'), 'duplicate errors dismiss once');
        check(row().textContent.includes('A warning'), 'warning resumes');
        for (const theme of ['light', 'dark']) { document.documentElement.dataset.theme = theme; const style = getComputedStyle(row()); check(contrast(style.color, style.backgroundColor) >= 4.5, theme + ' warning contrast'); }
        await dismiss(() => shows('Page updated')); check(row().textContent.includes('Page updated'), 'result resumes');
        check(!row().querySelector('.notice-dismiss'), 'a toast that closes itself has no second close');
        items.splice(items.findIndex(item => item.key === 'result'), 1); render(); await wait(() => shows('First info'), 'first info did not resume');
        check(row().textContent.includes('First info'), 'first info resumes');
        items.splice(items.findIndex(item => item.key === 'info1'), 1); render(); await wait(() => shows('Second info'), 'second info did not resume');
        check(row().textContent.includes('Second info'), 'second info resumes');
        items.splice(items.findIndex(item => item.key === 'info2'), 1); render(); await wait(() => !row(), 'queue did not drain');
        check(!row(), 'queue drains');
        items.splice(0, items.length, { key: 'timer', dismissKey: 'job-1', title: 'Building', message: '0:01', autoFocus: 'primary', action: { label: 'Stop', onClick() {} } });
        render(); await wait(() => row() && document.activeElement === row().querySelector('.primary'), 'focus did not settle'); check(document.activeElement === row().querySelector('.primary'), 'safe default receives focus');
        await dismiss(() => !row()); items[0].message = '0:02'; render(); check(!row(), 'timer updates do not reopen dismissed notices');
        items[0].dismissKey = 'job-2'; render(); await wait(() => shows('0:02'), 'new operation did not appear'); check(row(), 'a new operation becomes visible');
        items.length = 0; render(); await wait(() => !row(), 'unmounted notice stayed visible'); check(!row(), 'unmounted sources leave the queue');
        document.querySelector('#result').textContent = 'PASS';
      } catch (error) { document.querySelector('#result').textContent = 'FAIL: ' + error.message; }
    `);
    server = await createServer({ configLoader: 'runner', cacheDir: join(folder, 'cache'),
      server: { host: '127.0.0.1', port: 0, strictPort: false }, optimizeDeps: { entries: [join(folder, 'index.html')] } });
    await server.listen();
    const url = server.resolvedUrls.local[0];
    const browser = spawn('chromium', ['--headless', '--no-sandbox', '--disable-gpu', '--disable-dev-shm-usage', '--no-first-run', '--no-default-browser-check', '--user-data-dir=' + join(folder, 'profile'), '--virtual-time-budget=5000', '--dump-dom', url + folder.split('/').at(-1) + '/index.html'], { stdio: ['ignore', 'pipe', 'pipe'], timeout: 20000 });
    let html = '', errors = '';
    browser.stdout.on('data', chunk => { html += chunk; }); browser.stderr.on('data', chunk => { errors += chunk; });
    const [code] = await once(browser, 'exit');
    assert.equal(code, 0, errors);
    assert.match(html, /<pre id="result">PASS<\/pre>/, html);
  } finally { await server?.close(); await rm(folder, { recursive: true, force: true }); }
});
