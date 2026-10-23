import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

const temporary = await mkdtemp(new URL('../.conversation-', import.meta.url));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022', '--jsx', 'react-jsx', '--moduleResolution', 'bundler', '--types', 'vite/client', '--skipLibCheck', '--outDir', temporary, 'src/GenerationLog.tsx'], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  for (const name of ['GenerationLog']) {
    let source = await readFile(join(temporary, `${name}.js`), 'utf8');
    source = source.replace(/^import .* from '\.\/WorkspaceSurface';$/m, 'const WorkspaceSurface = ({ children }) => children;')
      .replace(/^import .* from '\.\/api';$/m, "const native = () => { throw new Error('Rendering must not call the native bridge'); };")
      .replace(/^import .* from '\.\/Preview';$/m, 'const PagePreview = () => null;')
      .replace(/^import .* from '\.\/Notes';$/m, "const scopeLabel = () => 'Whole site';")
      .replace(/from '\.\/(GenerationDrafts|generationDraftState)'/g, "from './$1.mjs'");
    await writeFile(join(temporary, `${name}.mjs`), source);
  }
  const { GenerationLog } = await import(pathToFileURL(join(temporary, 'GenerationLog.mjs')));
  const id = '12345678-1234-1234-1234-123456789abc';
  const project = { id: 'project', revision: 1, pages: [{ id: 'page', name: 'Home', regions: [{ id, prompt: 'Add a booking form' }] }] };
  const job = { jobId: 'job', status: 'blocked', settings: { model: 'model' }, requirements: { markdown: '' }, generationMode: 'fuzzy', areas: [{ pageId: 'page', regionId: id }],
    candidate: { summary: 'Booking form needs a service address.', regions: [{ regionId: id, status: 'conflict', explanation: `Area ${id}: Which booking service should this use?` }], criticism: [{ message: 'Keep the heading concise.', suggestion: 'Shorten the heading.' }], decisionProposals: [{ topic: `Booking links ${id}`, value: 'Use the booking service', scope: { kind: 'project' } }] },
    drafts: [{ id: 'draft', pageId: 'page', regionIds: [id], reason: 'CSS outside the region', code: '<script>PRIVATE GENERATED SOURCE</script>' }], usage: { inputTokens: 10, outputTokens: 20 }, repairRounds: 0 };
  const props = { job, project, controller: { job }, settings: null, operation: null, error: null, close() {}, openSettings() {}, manageUsage: async () => {}, editRequest() {} };
  await test('conflict explanations hide diagnostics without changing the saved job', () => {
    const explanation = `Area ${id} ([data-desegnos-region=${id}]): Region script root contains another region. Excerpt: <section id="private">`;
    const conflicted = { ...job, candidate: { ...job.candidate, regions: [{ regionId: id, status: 'conflict', explanation }] } };
    const html = renderToStaticMarkup(React.createElement(GenerationLog, { ...props, job: conflicted }));
    const visible = html.split('<summary>Technical details</summary>')[0];
    assert.match(visible, /Needs attention/);
    assert.equal((visible.match(/>Try again</g) ?? []).length, 1);
    assert.match(html, /<summary>Technical details<\/summary>/);
    assert.match(html, /Excerpt/);
    assert.doesNotMatch(visible, /data-desegnos-|Excerpt|private|script root|12345678-1234/);
    assert.equal(conflicted.candidate.regions[0].explanation, explanation);
  });
  await test('conversation presents one ordered review without the legacy generation card or raw source', () => {
    const html = renderToStaticMarkup(React.createElement(GenerationLog, props));
    const order = ['Sent to the AI', 'AI reply', 'Design feedback', 'Suggested rules', 'Request details', 'token-footer'];
    let previous = -1;
    for (const marker of order) { const position = html.indexOf(marker); assert.ok(position > previous, `${marker} is missing or out of order`); previous = position; }
    assert.equal((html.match(/<summary>Request details<\/summary>/g) ?? []).length, 1);
    assert.doesNotMatch(html, /Build remaining draft|Change request|Ask AI to fix|problem-banner|Needs details/);
    assert.doesNotMatch(html, /generation-status|generation-dialog|Copy code|PRIVATE GENERATED SOURCE/);
  });
  await test('preparation does not mount the old generation header inside the conversation', () => {
    const html = renderToStaticMarkup(React.createElement(GenerationLog, { ...props, job: null, operation: 'Preparing AI instructions…' }));
    assert.match(html, /Preparing AI instructions/);
    assert.doesNotMatch(html, /generation-status|generation-dialog|0:00/);
  });
} finally { await rm(temporary, { recursive: true, force: true }); }
