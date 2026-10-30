import assert from 'node:assert/strict';
import test from 'node:test';
import { compileModules } from './typescript.mjs';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

const modules = await compileModules(['src/Generation.tsx']);
try {
  const { GenerationFeedback } = await modules.load('Generation.js');
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
} finally { await modules.cleanup(); }
