import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

const temporary = await mkdtemp(new URL('../.preview-repair-', import.meta.url));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022', '--jsx', 'react-jsx', '--moduleResolution', 'bundler', '--types', 'vite/client', '--skipLibCheck', '--outDir', temporary, 'src/Preview.tsx'], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const source = (await readFile(join(temporary, 'Preview.js'), 'utf8'))
    .replace(/^import .* from 'react';$/m, `import { forwardRef, useRef, useImperativeHandle, useState as realUseState } from 'react';
      const useEffect = () => {}; const useLayoutEffect = () => {};
      const useState = initial => realUseState(Array.isArray(initial) ? ['TypeError: pause is null', 'TypeError: null is not an object (evaluating pause.addEventListener)'] : initial);`)
    .replace(/^import (?!.*(?:userErrors|ProblemBanner)).* from '\.\/[^']+';$/gm, '');
  await writeFile(join(temporary, 'Preview.mjs'), source);
  const { PreviewFrame } = await import(pathToFileURL(join(temporary, 'Preview.mjs')));
  const props = { pageId: 'page', title: 'Live page', fixRuntimeErrors() {} };
  await test('only the live preview offers a primary runtime repair action', () => {
    const html = renderToStaticMarkup(React.createElement(PreviewFrame, props));
    assert.match(html, /class="primary">Fix with AI<\/button>/);
    assert.match(html, /role="alert"/);
    assert.match(html, /<span>A script on this page looks for something that isn&#x27;t there\.<\/span>/);
    assert.doesNotMatch(html, /TypeError|pause|addEventListener/);
    assert.match(html, /aria-label="Dismiss notice"/);
    for (const extra of [{ staticView: true }, { draftId: 'draft' }, { jobId: 'job' }, { fixRuntimeErrors: undefined }]) {
      assert.doesNotMatch(renderToStaticMarkup(React.createElement(PreviewFrame, { ...props, ...extra })), /Fix with AI/);
    }
  });
  await test('canvas-owned runtime problems suppress the preview banner', () => {
    const html = renderToStaticMarkup(React.createElement(PreviewFrame, { ...props, onRuntimeProblems() {} }));
    assert.doesNotMatch(html, /Fix with AI|preview-problem|role="alert"/);
  });
  await test('AI connection and generation gating disable the repair action', () => {
    const html = renderToStaticMarkup(React.createElement(PreviewFrame, { ...props, aiDisabled: true }));
    assert.match(html, /class="primary" disabled="">Fix with AI/);
  });
} finally { await rm(temporary, { recursive: true, force: true }); }
