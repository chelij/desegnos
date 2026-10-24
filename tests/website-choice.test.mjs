import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, rm, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

const root = fileURLToPath(new URL('../', import.meta.url));
const temporary = await mkdtemp(join(root, '.website-choice-'));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--jsx', 'react-jsx', '--module', 'esnext',
    '--target', 'ES2022', '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/WebsiteChoice.tsx'], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout || String(result.error));
  // Node ESM needs extensions; preserve the app's bundler-style source imports.
  for (const file of ['WebsiteChoice.js', 'api.js']) {
    const path = join(temporary, file);
    await writeFile(path, (await readFile(path, 'utf8')).replaceAll("'./api'", "'./api.js'").replaceAll("'./usage'", "'./usage.js'"));
  }
  const { WebsiteChoice } = await import(pathToFileURL(join(temporary, 'WebsiteChoice.js')).href);
  const website = { path: '/site', framework: 'Vite', output: '/site/dist', fresh: false,
    pages: ['index.html'], buildCommand: ['npm', 'run', 'build'], installCommand: null, buildUnavailable: 'Needs Node.js to rebuild.' };
  const render = (site, brand = false) => renderToStaticMarkup(React.createElement(WebsiteChoice, {
    folder: { id: 'folder', name: 'Site', website: site }, brand, disabled: false,
    chooseBrand() {}, chooseEdit() {}, async edit() {}, update() {},
  }));
  await test('missing Node hides Build site and offers existing output with a warning', () => {
    const html = render(website);
    assert.ok(html.includes('This website cannot be updated on this computer.'));
    assert.ok(html.includes('may be out of date'));
    assert.ok(html.includes('Open website for editing'));
    assert.ok(!html.includes('>Build site<'));
  });
  await test('project-folder opening still offers brand-only review without build actions', () => {
    const html = render(website, true);
    assert.ok(html.includes('Use it for my brand only'));
    assert.ok(html.includes('aria-pressed="true"'));
    assert.ok(!html.includes('Open website for editing'));
    assert.ok(!html.includes('>Build site<'));
  });
  await test('missing tool without output offers no build or open button', () => {
    const html = render({ ...website, output: null });
    assert.ok(html.includes('This website cannot be prepared for editing on this computer.'));
    assert.ok(!html.includes('>Build site<'));
    assert.ok(!html.includes('Open website for editing'));
  });
  await test('installed tools and dependencies allow Build site', () => {
    assert.ok(render({ ...website, buildUnavailable: null }).includes('>Build site<'));
  });
  await test('missing package manager takes precedence over installation instructions', () => {
    const html = render({ ...website, installCommand: 'pnpm install', buildUnavailable: 'Needs pnpm to rebuild.' });
    assert.ok(html.includes('This website cannot be updated on this computer.'));
    assert.ok(!html.includes('<code>pnpm install</code>'));
    assert.ok(!html.includes('>Build site<'));
  });
} finally {
  await rm(temporary, { recursive: true, force: true });
}
