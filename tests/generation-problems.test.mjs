import assert from 'node:assert/strict';
import test from 'node:test';
import { compileModules } from './typescript.mjs';

const modules = await compileModules(['src/generationProblems.ts']);
try {
  const { areaProblems, placeCallouts, areaOutcome, jobOutcome, resolutionAction, replyCutOff, replyCutOffMessage, staleResult, staleResultMessage } = await modules.load('generationProblems.js');
  const rect = { x: 100, y: 200, width: 200, height: 80 };
  const other = { x: 110, y: 50, width: 200, height: 80 };
  const intersects = (a, b) => a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y;
  await test('callouts avoid all area content and each other at every zoom', () => {
    for (const zoom of [0.25, 1, 4]) {
      const origin = { x: -1000, y: -1000 };
      const cards = placeCallouts([{ key: 'one', rect }, { key: 'two', rect: other }], [rect, other], origin, zoom);
      assert.equal(cards.length, 2);
      assert.ok(!intersects(cards[0], cards[1]));
      for (const card of cards) {
        assert.equal(card.width, 360);
        for (const area of [rect, other]) assert.ok(!intersects(card, { x: (area.x-origin.x)*zoom, y:(area.y-origin.y)*zoom, width:area.width*zoom, height:area.height*zoom }));
      }
    }
  });
  await test('callouts track pan and dismissal is scoped to one job', () => {
    const item = { key: 'job-one:area', rect };
    const a = placeCallouts([item], [rect], { x: 0, y: 0 }, 1)[0];
    const b = placeCallouts([item], [rect], { x: 30, y: 40 }, 1)[0];
    assert.equal(a.x-b.x, 30); assert.equal(a.y-b.y, 40);
    assert.deepEqual(placeCallouts([item], [rect], { x: 0, y: 0 }, 1, [item.key]), []);
    assert.equal(placeCallouts([{ ...item, key: 'job-two:area' }], [rect], { x: 0, y: 0 }, 1, [item.key]).length, 1);
  });

  const element = (id, prompt, elementRect) => ({ id, kind: 'box', name: null, rect: elementRect, rotation: 0, visible: true, locked: false,
    style: {}, text: '', image: null, url: null, layout: { kind: 'free' }, children: [], meta: { requests: [], context: [], pending: { prompt, attachments: [], requestedAt: 'now' } } });
  const page = { id: 'page', name: 'Home', path: '/', included: true, output: 'webpage', sizes: [], elements: [element('a', 'Add scrolling chat', rect), element('b', 'Keep heading', other)] };
  const frozen = (requests) => '```json\n' + JSON.stringify({ pages: [{ pageId: 'page' }], requests }) + '\n```';
  const requirement = (elementId, request) => ({ elementId, request, sizeId: null });
  const job = { jobId: 'job', status: 'blocked', areas: [{ pageId: 'page', elementId: 'a' }], requirements: { markdown: frozen([requirement('a', 'Add scrolling chat')]) },
    candidate: { summary: 'Chat needs a service', requests: [{ elementId: 'a', status: 'conflict', explanation: 'Which chat service should this use?' }] } };

  await test('a declared conflict shows one callout at the layer', () => {
    const problems = areaProblems(job, page);
    assert.equal(problems.length, 1);
    assert.equal(problems[0].elementId, 'a');
    assert.equal(problems[0].fix, 'aiSettings');
    assert.equal(problems[0].retryable, false);
    assert.deepEqual(problems[0].rect, rect);
    assert.deepEqual(areaProblems({ ...job, status: 'generating' }, page), []);
  });
  await test('no layer conflict has no callout', () => {
    assert.deepEqual(areaProblems({ ...job, candidate: { summary: 'Failed', requests: [] }, areas: [] }, page), []);
  });

  await test('stale result uses one plain sentence and stays retryable', () => {
    const stale = { ...job, message: 'The generated result is stale: Existing page source changed.', candidate: { summary: 'Done', requests: [{ elementId: 'a', status: 'implemented', explanation: '' }] } };
    const problem = areaProblems(stale, page)[0];
    assert.equal(problem.stale, true);
    assert.equal(problem.message, staleResultMessage);
    assert.equal(problem.retryable, true);
    assert.equal(staleResult(stale), true);
    assert.equal(staleResult({ ...stale, status: 'failed' }), false);
    assert.equal(staleResult(null), false);
  });

  await test('an explained AI conflict is not retried and offers a fix', () => {
    const outcome = areaOutcome(job, 'a', 'Add scrolling chat');
    assert.deepEqual(outcome, { retryable: false, reason: 'Which chat service should this use?', fix: 'aiSettings' });
    const review = jobOutcome(job, id => id === 'a' ? 'Add scrolling chat' : undefined);
    assert.deepEqual(review.retryable, []);
    assert.equal(review.blocked.length, 1);
    assert.equal(review.blocked[0].fix, 'aiSettings');
  });
  await test('an app rejection and a technical explanation stay retryable', () => {
    const rejected = { ...job, candidate: { summary: 'Rejected', requests: [{ elementId: 'a', status: 'conflict', explanation: 'The generation result omitted a requested layer.' }] } };
    assert.equal(areaOutcome(rejected, 'a', 'Add scrolling chat').retryable, true);
    assert.deepEqual(jobOutcome(rejected, () => 'Add scrolling chat').retryable, ['a']);
    assert.deepEqual(jobOutcome(rejected, () => 'Add scrolling chat').blocked, []);
  });
  await test('editing the request restores Try again', () => {
    assert.equal(areaOutcome(job, 'a', 'Add a support chat').retryable, true);
    const edited = jobOutcome(job, () => 'Add a support chat');
    assert.deepEqual(edited.retryable, ['a']);
    assert.deepEqual(edited.blocked, []);
  });
  await test('declared reasons map to plain routes', () => {
    assert.equal(resolutionAction('Which direction should the photo point?'), 'brief');
    assert.equal(resolutionAction('The logo file is missing.'), 'import');
    assert.equal(resolutionAction('This needs a payment provider.'), 'aiSettings');
    assert.equal(resolutionAction('Make it feel calmer.'), undefined);
  });
  await test('a stale result retries even an implemented request, and a fresh one does not', () => {
    const implemented = { jobId: 'job', status: 'blocked', areas: [{ pageId: 'page', elementId: 'a' }], candidate: { summary: 'Done', requests: [{ elementId: 'a', status: 'implemented', explanation: 'Added the chat' }] } };
    assert.deepEqual(jobOutcome(implemented).retryable, []);
    const stale = { ...implemented, message: 'The generated result is stale: Existing page source changed.' };
    assert.deepEqual(jobOutcome(stale).retryable, ['a']);
    assert.equal(jobOutcome({ jobId: 'failed', status: 'failed', areas: [], candidate: null }).retryAll, true);
    assert.deepEqual(jobOutcome({ jobId: 'working', status: 'generating', areas: [], candidate: null }), { retryable: [], blocked: [], retryAll: false, retryBatches: false });
  });
  await test('a reply cut off at the output limit is named, not left unknown', () => {
    const settings = { model: 'deepseek-flash', maxOutputTokens: 128000, reasoningEffort: 'low', spendingLimitUsd: 2, imageGeneration: false };
    assert.equal(replyCutOff({ jobId: 'job', status: 'unknown', areas: [], candidate: null, failureKind: 'incomplete', usage: null, settings }), true);
    assert.equal(replyCutOff({ jobId: 'job', status: 'failed', areas: [], candidate: null, failureKind: 'incomplete', usage: { inputTokens: 1, outputTokens: 128000, totalTokens: 128001 }, settings }), true);
    assert.equal(replyCutOff({ jobId: 'job', status: 'unknown', areas: [], candidate: null, failureKind: 'incomplete', usage: { inputTokens: 1, outputTokens: 12, totalTokens: 13 }, settings }), false);
    assert.equal(replyCutOff({ jobId: 'job', status: 'unknown', areas: [], candidate: null, failureKind: 'transport', usage: null, settings }), false);
    assert.equal(replyCutOff({ jobId: 'job', status: 'generating', areas: [], candidate: null, failureKind: 'incomplete', usage: null, settings }), false);
    assert.equal(replyCutOff(null), false);
    assert.match(replyCutOffMessage, /^The AI reached its output limit/);
  });
} finally { await modules.cleanup(); }
