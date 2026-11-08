import assert from 'node:assert/strict';
import test from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { compileModules } from './typescript.mjs';

globalThis.HTMLElement = class {};
globalThis.document = {
  activeElement: null,
  querySelector() { return null; },
  getElementById() { return null; },
  addEventListener() {}, removeEventListener() {},
};
const modules = await compileModules(['src/GenerationLog.tsx']);
try {
  await modules.rewrite('GenerationLog.js', source => source.replace(
    /import \{ WorkspaceSurface \} from '\.\/WorkspaceSurface\.js';/,
    'const WorkspaceSurface = ({ children }) => children;'));
  const { GenerationLog } = await modules.load('GenerationLog.js');
  const id = '12345678-1234-1234-1234-123456789abc';
  const element = {
    id, kind: 'box', name: null, rect: { x: 0, y: 0, width: 100, height: 60 }, rotation: 0, visible: true, locked: false,
    style: {}, text: '', image: null, url: null, layout: { kind: 'free' }, children: [],
    meta: { requests: [], context: [] },
  };
  const project = { requests: [{ id: 'request', text: 'Add a booking form', attachments: [], requestedAt: 'now', area: null, targets: [{ kind: 'element', page: 'page', size: null, id }] }], id: 'project', name: 'Site', pages: [{ id: 'page', name: 'Home', path: '/', included: true, output: 'webpage', sizes: [], elements: [element] }] };
  const job = {
    jobId: 'job', status: 'blocked', provider: 'openai', settings: { model: 'model' }, generationMode: 'fuzzy',
    requirements: { markdown: '' }, areas: [{ pageId: 'page', elementId: id }], usage: { inputTokens: 10, outputTokens: 20 },
    candidate: { summary: 'Booking form needs a service.', requests: [{ elementId: id, status: 'conflict', operations: [], explanation: 'Which booking service should this use?' }], criticism: [], decisionProposals: [] },
  };
  const controller = { job, operation: null, elapsedSeconds: null, error: '', stopped: false, canStop: false, prepare() {}, cancel() {} };
  const render = (overrides = {}) => renderToStaticMarkup(React.createElement(GenerationLog, {
    job, project, controller, settings: null, operation: null, error: null,
    close() {}, openSettings() {}, openImport() {}, openBrief() {}, editRequest() {}, reply: async () => {}, ...overrides,
  }));

  await test('a declared question stays in the conversation with its resolving actions', () => {
    const html = render();
    assert.match(html, /Sent to the AI/);
    assert.match(html, /Resend this request/);
    assert.match(html, /Needs your input/);
    assert.match(html, /Which booking service should this use\?/);
    assert.match(html, /Change request/);
    assert.match(html, /AI settings/);
    assert.doesNotMatch(html, />Try again</);
    assert.match(html, /Booking form needs a service\./);
  });

  await test('behaviour operations appear in the conversation in plain words', () => {
    const behaviourJob = { ...job, candidate: { ...job.candidate, requests: [{ elementId: id, status: 'implemented', explanation: '', operations: [{ kind: 'setBehaviour', elementId: id, behaviour: 'Rotate toward the mouse' }] }] } };
    assert.match(render({ job: behaviourJob }), /Behaviour: Rotate toward the mouse/);
  });

  await test('an app rejection keeps Try again and hides the declared-question route', () => {
    const rejected = { ...job, candidate: { ...job.candidate, requests: [{ elementId: id, status: 'conflict', operations: [], explanation: `Area ${id}: The generation result omitted a requested layer.` }] } };
    const html = render({ job: rejected });
    assert.match(html, />Try again</);
    assert.doesNotMatch(html, /Needs your input|>Change request</);
  });

  await test('a whole-reply rejection explains the Needs attention result and offers Try again', () => {
    const rejected = { ...job, areas: [], candidate: null,
      message: 'Desegnos rejected the candidate: The generation result is invalid: missing field `requirementsSha256` Nothing was applied.',
      validationEvents: [{ elementId: null, kind: 'replyStructure', disposition: 'conflicted', message: 'missing field `requirementsSha256`' }] };
    const html = render({ job: rejected });
    assert.match(html, /reply was incomplete, so nothing was changed/);
    assert.match(html, />Try again</);
  });

  await test('a boundary-only request counts as the one request that was sent', () => {
    const boundaryJob = {
      ...job, areas: [], requirements: { markdown: '', requestCount: 1 },
      candidate: { summary: 'Built the page', requests: [{ requestId: 'request', elementId: null, status: 'implemented', explanation: '', operations: [] }], criticism: [], decisionProposals: [] },
    };
    const html = render({ job: boundaryJob });
    assert.match(html, /1 request</);
    assert.doesNotMatch(html, /0 requests/);
    assert.match(html, /Built the page/);
  });

  await test('page edits and dropped operations appear once alongside partial success', () => {
    const warning = { elementId: null, kind: 'operation', disposition: 'dropped', message: 'Request 1 operation 2 skipped: invalid target' };
    const partial = { ...job, status: 'applied', areas: [], validationEvents: [warning],
      batches: [{ status: 'applied', validationEvents: [warning] }],
      candidate: { ...job.candidate, requests: [{ requestId: 'request', elementId: null, status: 'implemented', explanation: '',
        operations: [{ kind: 'setPage', pageId: 'page', name: 'Welcome', path: '/welcome', background: null }] }] } };
    const html = render({ job: partial });
    assert.match(html, /Updated Welcome: name and path \/welcome/);
    assert.equal(html.split('Some layer changes were skipped. The rest was applied.').length - 1, 1);
    const warnings = html.match(/<section aria-label="Generation warnings">(.*?)<\/section>/)[1];
    assert.equal(warnings.split(warning.message).length - 1, 1);
    assert.doesNotMatch(html, />Try again</);
  });
  await test('request actions and past generations remain reachable', () => {
    const html = render();
    assert.match(html, />Change request</);
    assert.match(html, /AI settings/);
    assert.match(html, /Past generations/);
    assert.match(html, /Reply to the AI/);
  });
} finally {
  delete globalThis.document;
  delete globalThis.HTMLElement;
  await modules.cleanup();
}
