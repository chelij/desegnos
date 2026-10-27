import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

const temporary = await mkdtemp(new URL('../.generation-status-', import.meta.url));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022', '--jsx', 'react-jsx', '--moduleResolution', 'bundler', '--types', 'vite/client', '--skipLibCheck', '--outDir', temporary, 'src/Generation.tsx'], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const source = (await readFile(join(temporary, 'Generation.js'), 'utf8'))
    .replace(/^import \{ MenuButton \} from '\.\/Menus';$/m, 'const MenuButton = () => null;')
    .replace(/^import \{ scopeLabel \} from '\.\/Notes';$/m, "const scopeLabel = () => 'Whole site';")
    .replace(/^import \{ documentKey \} from '\.\/elementTargets';$/m, "const documentKey = () => 'page';")
    .replace(/^import \{ SourceView \} from '\.\/Preview';$/m, 'const SourceView = () => null;')
    .replace(/^import .* from '\.\/Disclosure';$/m, "import { Disclosure } from './Disclosure.js';");
  const banner = join(temporary, 'ProblemBanner.js');
  await writeFile(banner, (await readFile(banner, 'utf8')).replaceAll("from './icons'", "from './icons.js'"));
  const disclosure = join(temporary, 'Disclosure.js');
  await writeFile(disclosure, (await readFile(disclosure, 'utf8')).replaceAll("from './icons'", "from './icons.js'"));
  await writeFile(join(temporary, 'Generation.mjs'), source);
  const { GenerationFeedback } = await import(pathToFileURL(join(temporary, 'Generation.mjs')));
  const id = '12345678-1234-1234-1234-123456789abc';
  const project = { id: 'project', pages: [{ id: 'page', name: 'Home', path: '/', sizes: [], elements: [{ id, kind: 'box', name: null, rect: { x: 0, y: 0, width: 10, height: 10 }, rotation: 0, visible: true, locked: false, style: {}, text: '', image: null, url: null, layout: { kind: 'free' }, children: [], meta: { requests: [], context: [], pending: { prompt: 'Add a booking form', attachments: [], requestedAt: 'now' } } }] }] };
  const controller = job => ({ job, operation: null, elapsedSeconds: null, error: '', stopped: false, canStop: false, prepare() {}, cancel() {} });
  const render = (job, extra = {}) => renderToStaticMarkup(React.createElement(GenerationFeedback, { controller: controller(job), project, openSettings() {}, openImport() {}, openBrief() {}, changeRequest() {}, ...extra }));
  const frozen = '```json\n' + JSON.stringify({ pages: [{ pageId: 'page' }], requests: [{ elementId: id, request: 'Add a booking form', sizeId: null }] }) + '\n```';
  const base = { jobId: 'job', status: 'blocked', requirements: { markdown: frozen }, areas: [{ pageId: 'page', elementId: id }], candidate: { summary: 'Booking needs a service', requests: [{ elementId: id, status: 'conflict', explanation: 'Which booking service should this use?' }] } };

  await test('the status bar asks for a change instead of resending a declared question', () => {
    const html = render(base);
    assert.match(html, /Needs your input/);
    assert.match(html, /Which booking service should this use\?/);
    assert.match(html, />Change request</);
    assert.match(html, />AI settings</);
    assert.doesNotMatch(html, />Try again</);
  });
  await test('an app rejection keeps Try again in the status bar', () => {
    const html = render({ ...base, candidate: { ...base.candidate, requests: [{ elementId: id, status: 'conflict', explanation: `Area ${id}: The generation result omitted a requested layer.` }] } });
    assert.match(html, />Try again</);
    assert.doesNotMatch(html, /Needs your input|>Change request</);
  });
  await test('editing the request restores Try again and failures without areas can retry', () => {
    const edited = render(base, { project: { ...project, pages: [{ ...project.pages[0], elements: [{ ...project.pages[0].elements[0], meta: { ...project.pages[0].elements[0].meta, pending: { ...project.pages[0].elements[0].meta.pending, prompt: 'Add a support chat' } } }] }] } });
    assert.match(edited, />Try again</);
    assert.doesNotMatch(edited, /Needs your input/);
    const failed = render({ jobId: 'job', status: 'failed', requirements: { markdown: '' }, areas: [], candidate: null });
    assert.match(failed, />Try again</);
  });
  await test('a request whose editor is already open keeps only its fix', () => {
    const opened = render(base, { requestEditorOpen: elementId => elementId === id });
    assert.doesNotMatch(opened, />Change request</);
    assert.match(opened, />AI settings</);
    const closed = render(base, { requestEditorOpen: elementId => elementId !== id });
    assert.match(closed, />Change request</);
  });
} finally { await rm(temporary, { recursive: true, force: true }); }
