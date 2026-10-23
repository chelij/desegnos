import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import test from 'node:test';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

const temporary = await mkdtemp(new URL('../.draft-review-', import.meta.url));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022', '--jsx', 'react-jsx', '--moduleResolution', 'bundler', '--types', 'vite/client', '--skipLibCheck', '--outDir', temporary, 'src/GenerationDrafts.tsx'], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const source = (await readFile(join(temporary, 'GenerationDrafts.js'), 'utf8'))
    .replace(/^import .* from '\.\/api';$/m, "const native = () => { throw new Error('Server rendering must not inspect drafts'); };")
    .replace(/^import .* from '\.\/WorkspaceSurface';$/m, 'const WorkspaceSurface = () => null;')
    .replace(/^import .* from '\.\/Preview';$/m, 'const PagePreview = () => null;')
    .replace("from './generationDraftState'", "from './generationDraftState.mjs'");
  await writeFile(join(temporary, 'drafts.mjs'), source);
  await writeFile(join(temporary, 'generationDraftState.mjs'), await readFile(join(temporary, 'generationDraftState.js'), 'utf8'));
  const { GenerationDrafts, plainReason } = await import(pathToFileURL(join(temporary, 'drafts.mjs')));
  const draft = { id: 'internal-draft', pageId: 'internal-page', regionIds: ['internal-area'], reason: 'The draft uses dynamic or executable script access. Copy code to keep it.', code: '<script>SECRET SOURCE</script>' };
  const job = { jobId: 'internal-job', drafts: [draft, draft, { ...draft, id: 'second-retained-copy' }], areas: [{ pageId: draft.pageId, regionId: 'internal-area' }] };
  const project = { pages: [{ id: draft.pageId, name: 'Home', regions: [{ id: 'internal-area' }] }] };
  const props = { job, project, controller: {} };
  await test('compact mode renders no draft content', () => {
    assert.equal(renderToStaticMarkup(React.createElement(GenerationDrafts, { ...props, compact: true })), '');
  });
  await test('side panel renders each draft once with a human label and repair action', () => {
    const html = renderToStaticMarkup(React.createElement(GenerationDrafts, props));
    assert.equal((html.match(/Draft · Home · Box 1/g) ?? []).length, 1);
    assert.equal((html.match(/The AI added a script that affects the wrong part of the page\./g) ?? []).length, 1);
    assert.match(html, /class="primary">Try again/);
    assert.match(html, /Ask AI to fix/);
    assert.match(html, /Dismiss/);
    assert.doesNotMatch(html, /Copy code|Retained code|SECRET SOURCE|internal-/);
  });
  await test('repeated technical reasons collapse to one plain-language reason', () => {
    assert.equal(new Set([draft.reason, 'Region script selectors must be rooted at root.'].map(plainReason)).size, 1);
  });
} finally { await rm(temporary, { recursive: true, force: true }); }
