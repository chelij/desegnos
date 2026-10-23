import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { once } from 'node:events';
import test from 'node:test';

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
      import '/src/styles.css'; import '/src/brand.css'; import '/src/canvas.css'; import '/src/shell.css'; import '/src/notice.css';
      const target = document.querySelector('#root'); const root = createRoot(target);
      const calls = []; const check = (ok, message) => { if (!ok) throw Error(message); };
      const pause = () => new Promise(resolve => setTimeout(resolve, 40));
      const items = [
        { key: 'info1', message: 'First info' }, { key: 'info2', message: 'Second info' },
        { key: 'result', variant: 'success', priority: 'result', title: 'Page updated', message: 'Also changed page styles, page script and 2 other boxes' },
        { key: 'warning', variant: 'warning', message: 'A warning' },
      ];
      function render() { flushSync(() => root.render(<React.StrictMode><NoticeProvider><NoticeHost />{items.map(item => <Notice {...item} dismiss={() => calls.push(item.key)} />)}</NoticeProvider></React.StrictMode>)); }
      const row = () => document.querySelector('.notice');
      const dismiss = async () => { flushSync(() => row().querySelector('.notice-dismiss').click()); await pause(); };
      function contrast(fg, bg) {
        const luminance = color => { const rgb = color.match(/\\d+/g).slice(0, 3).map(Number).map(n => n / 255).map(n => n <= .04045 ? n / 12.92 : ((n + .055) / 1.055) ** 2.4); return rgb[0] * .2126 + rgb[1] * .7152 + rgb[2] * .0722; };
        const a = luminance(fg), b = luminance(bg); return (Math.max(a, b) + .05) / (Math.min(a, b) + .05);
      }
      try {
        render(); await pause(); check(row().textContent.includes('A warning'), 'warning precedes result and info');
        items.push({ key: 'error1', variant: 'error', title: 'Error', message: 'A long message '.repeat(40), action: { label: 'Try again', onClick: () => calls.push('primary') }, secondaryAction: { label: 'Details', onClick: () => calls.push('secondary') } });
        items.push({ ...items.at(-1), key: 'error2' }); render(); await pause();
        check(document.querySelectorAll('.notice').length === 1, 'only one bar');
        check(row().getAttribute('role') === 'alert' && row().classList.contains('notice-error'), 'error preempts');
        for (const theme of ['light', 'dark']) {
          document.documentElement.dataset.theme = theme;
          const style = getComputedStyle(row()); check(contrast(style.color, style.backgroundColor) >= 4.5, theme + ' error contrast');
          for (const width of [900, 360]) {
            target.style.width = width + 'px'; await pause();
            const message = row().querySelector('.notice-copy span');
            check(getComputedStyle(message).whiteSpace === 'nowrap' && message.getBoundingClientRect().height === 20, 'message stays one line');
            check(message.scrollWidth > message.clientWidth, 'long copy truncates');
            check(row().querySelector('.notice-copy').title.includes('A long message'), 'full text tooltip');
            const buttons = [...row().querySelectorAll('button')].map(button => button.getBoundingClientRect());
            check(buttons.every(rect => rect.top === buttons[0].top), 'actions and dismiss stay together');
            check(buttons.at(-1).right <= row().getBoundingClientRect().right, 'actions fit');
            check(width === 900 ? row().getBoundingClientRect().height === 48 : buttons[0].top >= message.getBoundingClientRect().bottom, 'only narrow widths wrap actions');
          }
        }
        row().querySelector('.primary').click(); row().querySelectorAll('button')[1].click();
        check(calls.includes('primary') && calls.includes('secondary'), 'action callbacks');
        await dismiss(); check(calls.includes('error1') && calls.includes('error2'), 'duplicate errors dismiss once');
        check(row().textContent.includes('A warning'), 'warning resumes');
        for (const theme of ['light', 'dark']) { document.documentElement.dataset.theme = theme; const style = getComputedStyle(row()); check(contrast(style.color, style.backgroundColor) >= 4.5, theme + ' warning contrast'); }
        await dismiss(); check(row().textContent.includes('Page updated'), 'result resumes');
        await dismiss(); check(row().textContent.includes('First info'), 'first info resumes');
        await dismiss(); check(row().textContent.includes('Second info'), 'second info resumes');
        await dismiss(); check(!row(), 'queue drains');
        items.splice(0, items.length, { key: 'timer', dismissKey: 'job-1', title: 'Building', message: '0:01', autoFocus: 'primary', action: { label: 'Stop', onClick() {} } });
        render(); await pause(); check(document.activeElement === row().querySelector('.primary'), 'safe default receives focus');
        await dismiss(); items[0].message = '0:02'; render(); await pause(); check(!row(), 'timer updates do not reopen dismissed notices');
        items[0].dismissKey = 'job-2'; render(); await pause(); check(row(), 'a new operation becomes visible');
        items.length = 0; render(); await pause(); check(!row(), 'unmounted sources leave the queue');
        document.querySelector('#result').textContent = 'PASS';
      } catch (error) { document.querySelector('#result').textContent = 'FAIL: ' + error.message; }
    `);
    server = spawn(process.execPath, ['node_modules/vite/bin/vite.js', '--host', '127.0.0.1', '--port', '0'], { stdio: ['ignore', 'pipe', 'pipe'] });
    const url = await new Promise((resolve, reject) => {
      let output = '';
      const timer = setTimeout(() => reject(Error('Vite did not start: ' + output)), 10000);
      server.on('error', error => { clearTimeout(timer); reject(error); });
      server.on('exit', code => { clearTimeout(timer); reject(Error('Vite exited ' + code + ': ' + output)); });
      server.stdout.on('data', chunk => { output += chunk; const match = output.match(/http:\/\/127\.0\.0\.1:\d+\//); if (match) { clearTimeout(timer); resolve(match[0]); } });
      server.stderr.on('data', chunk => { output += chunk; });
    });
    // Async spawn lets the Vite server respond while Chromium loads the fixture.
    const browser = spawn('chromium', ['--headless', '--no-sandbox', '--disable-gpu', '--disable-dev-shm-usage', '--no-first-run', '--no-default-browser-check', '--user-data-dir=' + join(folder, 'profile'), '--virtual-time-budget=5000', '--dump-dom', url + folder.split('/').at(-1) + '/index.html'], { stdio: ['ignore', 'pipe', 'pipe'], timeout: 20000 });
    let html = '', errors = '';
    browser.stdout.on('data', chunk => { html += chunk; }); browser.stderr.on('data', chunk => { errors += chunk; });
    const [code] = await once(browser, 'exit');
    assert.equal(code, 0, errors);
    assert.match(html, /<pre id="result">PASS<\/pre>/, html);
  } finally { if (server && server.exitCode === null && server.signalCode === null) { server.kill(); await once(server, 'exit'); } await rm(folder, { recursive: true, force: true }); }
});
