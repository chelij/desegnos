import { execFile } from 'node:child_process';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { promisify } from 'node:util';
import { createServer } from 'vite';
import { root } from './typescript.mjs';

/** Vite plugin that replaces Tauri's IPC with `globalThis.__invoke(command, args)`, set by the test. */
export const tauriStub = {
  name: 'tauri-ipc-stub', enforce: 'pre',
  resolveId: id => id === '@tauri-apps/api/core' ? '\0tauri-core-stub' : null,
  load: id => id === '\0tauri-core-stub'
    ? 'export const isTauri = () => true; export const invoke = (command, args) => globalThis.__invoke(command, args);' : null,
};

/** Runs `script` (an ES module, JSX allowed) in headless Chromium against a Vite dev server rooted at
 * the project, so it can import `/src/...` directly. The script reports through `#result`: text
 * starting with PASS passes. Returns that text; throws with the page's FAIL text or DOM otherwise.
 * `body` is the markup before the result element; `plugins` are extra Vite plugins (module stubs). */
export async function runInBrowser({ name, script, body = '<div id="root"></div>', bodyStyle = '', plugins = [] }) {
  await mkdir(join(root, 'test-results'), { recursive: true });
  const folder = await mkdtemp(join(root, `test-results/${name}-`));
  let server;
  try {
    await writeFile(join(folder, 'index.html'), `<!doctype html><html><body style="${bodyStyle}">${body}<pre id="result">pending</pre><script type="module" src="./main.jsx"></script></body></html>`);
    await writeFile(join(folder, 'main.jsx'), script);
    server = await createServer({ root, configFile: false, esbuild: { jsx: 'automatic' }, plugins, cacheDir: join(folder, 'cache'), clearScreen: false,
      server: { host: '127.0.0.1', port: 0, strictPort: false }, optimizeDeps: { entries: [join(folder, 'index.html')] } });
    await server.listen();
    const url = `http://127.0.0.1:${server.httpServer.address().port}${folder.slice(root.length - 1)}/index.html`;
    const { stdout } = await promisify(execFile)(process.env.CHROMIUM ?? 'chromium', [
      '--headless', '--no-sandbox', '--disable-gpu', '--no-first-run', '--disable-background-networking', '--disable-dev-shm-usage',
      '--no-default-browser-check', '--password-store=basic', '--disable-extensions',
      `--user-data-dir=${join(folder, 'profile')}`, '--dump-dom', '--virtual-time-budget=15000', url,
    ], { timeout: 45000, maxBuffer: 2 * 1024 * 1024 });
    const result = stdout.match(/<pre id="result">([\s\S]*?)<\/pre>/)?.[1] ?? '';
    if (!result.startsWith('PASS')) throw new Error(result || stdout.slice(-2000));
    return result;
  } finally {
    await server?.close();
    await rm(folder, { recursive: true, force: true });
  }
}
