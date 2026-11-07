import assert from 'node:assert/strict';
import test from 'node:test';
import { compileModules } from './typescript.mjs';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

const modules = await compileModules(['src/Generation.tsx', 'src/generationSession.ts']);
try {
  const { GenerationFeedback } = await modules.load('Generation.js');
  const { announcesStatus } = await modules.load('generationSession.js');
  const id = '12345678-1234-1234-1234-123456789abc';
  const project = { requests: [{ id: 'request', text: 'Add a booking form', attachments: [], requestedAt: 'now', area: null, targets: [{ kind: 'element', page: 'page', size: null, id }] }], id: 'project', pages: [{ id: 'page', name: 'Home', path: '/', sizes: [], elements: [{ id, kind: 'box', name: null, rect: { x: 0, y: 0, width: 10, height: 10 }, rotation: 0, visible: true, locked: false, style: {}, text: '', image: null, url: null, layout: { kind: 'free' }, children: [], meta: { requests: [], context: [] } }] }] };
  const controller = job => ({ job, phase: { name: 'idle' }, operation: null, elapsedSeconds: null, error: '', stopped: false, canStop: false, prepare() {}, cancel() {} });
  const render = (job, extra = {}) => renderToStaticMarkup(React.createElement(GenerationFeedback, { controller: controller(job), project, openSettings() {}, openImport() {}, openBrief() {}, changeRequest() {}, ...extra }));
  const frozen = '```json\n' + JSON.stringify({ pages: [{ pageId: 'page' }], requests: [{ text: 'Add a booking form', targets: [{ kind: 'element', elementId: id, pageId: 'page', sizeId: null }] }] }) + '\n```';
  const base = { jobId: 'job', status: 'blocked', requirements: { markdown: frozen }, areas: [{ pageId: 'page', elementId: id }], candidate: { summary: 'Booking needs a service', requests: [{ elementId: id, status: 'conflict', explanation: 'Which booking service should this use?' }] } };

  await test('automatic transport retries replace the timer with progress and keep Stop', () => {
    const job = { jobId: 'retry', status: 'generating', providerRetry: { attempt: 2, kind: 'server' }, message: 'DeepSeek had a server problem — retrying (2 of 3)…', areas: [], candidate: null };
    const html = render(job, { controller: { ...controller(job), canStop: true, elapsedSeconds: 20 } });
    assert.match(html, /DeepSeek had a server problem — retrying \(2 of 3\)…/);
    assert.match(html, />Stop</);
    assert.doesNotMatch(html, />AI settings</);
  });
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
  await test('a rejected whole reply shows its reason, retry and conversation details', () => {
    const rejected = { ...base, areas: [], candidate: null,
      message: 'Desegnos rejected the candidate: The generation result is invalid: missing field `requirementsSha256` Nothing was applied.',
      validationEvents: [{ elementId: null, kind: 'replyStructure', disposition: 'conflicted', message: 'missing field `requirementsSha256`' }] };
    const html = render(rejected, { details() {} });
    assert.match(html, /Needs attention/);
    assert.match(html, /reply was incomplete, so nothing was changed/);
    assert.match(html, />Try again</);
    assert.match(html, />Details</);
    assert.doesNotMatch(html, /requirementsSha256|rejected the candidate/);
    for (const status of ['blocked', 'failed', 'unknown']) {
      const fallback = render({ ...rejected, status, message: '', validationEvents: [] });
      assert.match(fallback, /nothing was changed|confirm whether the changes were applied/);
      assert.match(fallback, />Try again</);
    }
  });
  await test('Try again prepares the same layers or the full boundary request and respects retryability', () => {
    const calls = [];
    const rejected = { ...base, candidate: null, message: 'Desegnos rejected the candidate: invalid reply' };
    const feedback = job => GenerationFeedback({ controller: { ...controller(job), prepare: async ids => { calls.push(ids); } } });
    feedback(rejected).props.action.onClick();
    feedback({ ...rejected, areas: [] }).props.action.onClick();
    assert.deepEqual(calls, [[id], undefined]);
    assert.equal(feedback({ ...rejected, retryable: false }).props.action, undefined);
    const batches = { ...rejected, batches: [{ status: 'failed' }] };
    const notice = GenerationFeedback({ controller: { ...controller(batches), retry: async () => { calls.push('batch'); } } });
    notice.props.action.onClick();
    assert.equal(calls.at(-1), 'batch');
  });
  await test('partial success announces skipped changes with Details and no error styling', () => {
    const partial = { ...base, status: 'applied', areas: [], candidate: null,
      validationEvents: [{ elementId: null, kind: 'element', disposition: 'dropped', message: 'invalid operation targetId' }] };
    const html = render(partial, { details() {} });
    assert.match(html, /Layers updated/);
    assert.match(html, /Some layer changes were skipped/);
    assert.match(html, />Details</);
    assert.doesNotMatch(html, /notice-error|targetId|>Try again</);
    assert.equal(announcesStatus(partial, { owns: true, stopped: false, working: false }), true);
    assert.equal(announcesStatus({ ...partial, validationEvents: [] }, { owns: true, stopped: false, working: false }), false);
    assert.equal(announcesStatus(partial, { owns: false, stopped: false, working: false }), false);
  });
  await test('Details stays secondary and AI settings takes precedence over Try again', () => {
    const needsInput = GenerationFeedback({ controller: controller(base), project, changeRequest() {}, openSettings() {}, details() {} });
    assert.equal(needsInput.props.action.label, 'Change request');
    assert.equal(needsInput.props.secondaryAction.label, 'Details');
    const failed = { ...base, status: 'failed', candidate: null, message: 'API key is missing' };
    const html = render(failed, { details() {} });
    assert.match(html, />AI settings</);
    assert.match(html, />Details</);
    assert.doesNotMatch(html, />Try again</);
  });
  await test('batch operation warnings produce the same partial-success notice', () => {
    const partial = { ...base, status: 'applied', areas: [], candidate: null,
      batches: [{ status: 'applied', validationEvents: [{ elementId: null, kind: 'operation', disposition: 'dropped', message: 'invalid targetId' }] }] };
    const html = render(partial, { details() {} });
    assert.match(html, /Layers updated with warnings/);
    assert.match(html, /Some layer changes were skipped/);
    assert.match(html, /notice-warning/);
    assert.doesNotMatch(html, /targetId|>Try again</);
    assert.equal(announcesStatus(partial, { owns: true, working: false, stopped: false }), true);
  });
  await test('a reply cut off at the output limit names the cause instead of guessing', () => {
    const settings = { model: 'deepseek-flash', maxOutputTokens: 128000, reasoningEffort: 'low', spendingLimitUsd: 2, imageGeneration: false };
    const cutOff = render({ ...base, status: 'unknown', candidate: null, failureKind: 'incomplete', usage: null, settings });
    assert.match(cutOff, /Reply cut off/);
    assert.match(cutOff, /The AI reached its output limit before finishing\. Raise Maximum output tokens in AI settings or split the request\./);
    assert.match(cutOff, />AI settings</);
    assert.doesNotMatch(cutOff, /Not sure the layers were built/);
    // A stopped provider that reports less than the request's limit keeps the cautious heading.
    const stopped = render({ ...base, status: 'unknown', candidate: null, failureKind: 'incomplete', usage: { inputTokens: 10, outputTokens: 900, totalTokens: 910 }, settings });
    assert.match(stopped, /Not sure the layers were built/);
    assert.doesNotMatch(stopped, /Reply cut off/);
    const other = render({ ...base, status: 'unknown', candidate: null, failureKind: 'transport', usage: null, settings });
    assert.match(other, /Not sure the layers were built/);
    assert.doesNotMatch(other, /Reply cut off/);
  });
} finally { await modules.cleanup(); }
