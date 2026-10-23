import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

const temporary = await mkdtemp(new URL('../.problem-banner-', import.meta.url));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022', '--jsx', 'react-jsx', '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/ProblemBanner.tsx'], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const { ProblemBanner, needsAiSettings } = await import(pathToFileURL(join(temporary, 'ProblemBanner.js')));
  await test('problems announce one sentence, a primary fix, and Dismiss without a popup', () => {
    const html = renderToStaticMarkup(React.createElement(ProblemBanner, {
      message: 'The AI could not finish. Try again.', action: { label: 'Try again', onClick() {} },
    }));
    assert.match(html, /role="alert"/);
    assert.match(html, /<p>The AI could not finish\.<\/p>/);
    assert.match(html, /type="button" class="primary">Try again/);
    assert.equal((html.match(/Try again/g) ?? []).length, 1);
    assert.match(html, />Dismiss<\/button>/);
    assert.doesNotMatch(html, /dialog|modal|Technical details|helper/i);
  });
  await test('without a one-click fix, a short instruction stays visible', () => {
    const html = renderToStaticMarkup(React.createElement(ProblemBanner, { message: 'This code has a writing mistake. Check the highlighted line.' }));
    assert.match(html, /Check the highlighted line/);
    assert.doesNotMatch(html, /class="primary"/);
    assert.equal(renderToStaticMarkup(React.createElement(ProblemBanner, { message: '' })), '');
  });
  await test('disabled fixes stay disabled and connections, authentication and limits route to settings', () => {
    const html = renderToStaticMarkup(React.createElement(ProblemBanner, { message: 'The page stopped.', action: { label: 'Fix with AI', disabled: true, onClick() {} } }));
    assert.match(html, /class="primary" disabled=""/);
    for (const error of ['quota', 'authentication expired', 'network timeout', 'API key missing', 'usage limit', 'secure storage']) assert.equal(needsAiSettings(error), true);
    assert.equal(needsAiSettings('Region script affects another region'), false);
  });
  // Exercise action and dismissal callbacks with a tiny state boundary, without a DOM.
  const source = (await readFile(join(temporary, 'ProblemBanner.js'), 'utf8')).replace(/^import .* from 'react';$/m, 'let state = null; const useState = () => [state, value => { state = value; }];');
  await writeFile(join(temporary, 'events.mjs'), source);
  const { ProblemBanner: banner } = await import(pathToFileURL(join(temporary, 'events.mjs')));
  await test('fix and dismiss invoke their existing flows; a new error becomes visible', () => {
    const calls = [];
    const props = { message: 'A problem occurred.', action: { label: 'Fix', onClick: () => calls.push('fix') }, dismiss: () => calls.push('dismiss') };
    const buttons = banner(props).props.children[1].props.children;
    buttons[0].props.onClick(); buttons[2].props.onClick();
    assert.deepEqual(calls, ['fix', 'dismiss']);
    assert.equal(banner(props), null);
    assert.ok(banner({ ...props, message: 'Another problem occurred.' }));
  });
} finally { await rm(temporary, { recursive: true, force: true }); }
