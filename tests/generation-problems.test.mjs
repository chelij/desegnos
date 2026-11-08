import assert from 'node:assert/strict';
import test from 'node:test';
import { compileModules } from './typescript.mjs';

const modules = await compileModules(['src/generationProblems.ts']);
try {
  const { areaOutcome, generationWarnings, jobOutcome, resolutionAction, replyCutOff, replyCutOffMessage, staleResult } = await modules.load('generationProblems.js');
  const frozen = (requests) => '```json\n' + JSON.stringify({ pages: [{ pageId: 'page' }], requests }) + '\n```';
  const requirement = (elementId, request) => ({ text: request, targets: [{ kind: 'element', elementId, pageId: 'page', sizeId: null }] });
  const job = { jobId: 'job', status: 'blocked', areas: [{ pageId: 'page', elementId: 'a' }], requirements: { markdown: frozen([requirement('a', 'Add scrolling chat')]) },
    candidate: { summary: 'Chat needs a service', requests: [{ elementId: 'a', status: 'conflict', explanation: 'Which chat service should this use?' }] } };

  await test('stale result is recognised only for a job that still needs retrying', () => {
    const stale = { ...job, problemKind: 'stale', message: 'The result can no longer be applied.', candidate: { summary: 'Done', requests: [{ elementId: 'a', status: 'implemented', explanation: '' }] } };
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
    const implemented = { jobId: 'job', status: 'applied', areas: [{ pageId: 'page', elementId: 'a' }], candidate: { summary: 'Done', requests: [{ elementId: 'a', status: 'implemented', explanation: 'Added the chat' }] } };
    assert.deepEqual(jobOutcome(implemented).retryable, []);
    assert.deepEqual(jobOutcome({ ...implemented, status: 'blocked' }).retryable, ['a']);
    const stale = { ...implemented, status: 'blocked', problemKind: 'stale', message: 'The result can no longer be applied.' };
    assert.deepEqual(jobOutcome(stale).retryable, ['a']);
    assert.equal(jobOutcome({ jobId: 'failed', status: 'failed', areas: [], candidate: null }).retryAll, true);
    assert.deepEqual(jobOutcome({ jobId: 'working', status: 'generating', areas: [], candidate: null }), { reason: '', retryable: [], blocked: [], retryAll: false, retryBatches: false });
  });
  await test('whole-reply rejections explain the failure and can resend boundary-only requests', () => {
    const rejected = { jobId: 'rejected', status: 'blocked', areas: [], candidate: null,
      message: 'Desegnos rejected the candidate: The generation result is invalid: missing field `requirementsSha256` Nothing was applied.',
      validationEvents: [{ elementId: null, kind: 'replyStructure', disposition: 'conflicted', message: 'missing field `requirementsSha256`' }] };
    assert.equal(jobOutcome(rejected).reason, "The AI's reply was incomplete, so nothing was changed.");
    assert.equal(jobOutcome(rejected).retryAll, true);
    assert.equal(jobOutcome({ ...rejected, message: '' }).reason, jobOutcome(rejected).reason);
    const layers = { ...rejected, areas: job.areas, candidate: { requests: [{ elementId: 'a', status: 'implemented' }] } };
    assert.deepEqual(jobOutcome(layers).retryable, ['a']);
  });
  await test('terminal failures always have a reason, including without diagnostics', () => {
    for (const status of ['blocked', 'failed', 'unknown']) {
      const review = jobOutcome({ jobId: 'failure', status, areas: [], candidate: null });
      assert.ok(review.reason.length > 0);
      assert.equal(review.retryAll, true);
    }
  });
  await test('applied jobs describe skipped changes without exposing validation internals', () => {
    const applied = { jobId: 'partial', status: 'applied', areas: [], candidate: null,
      validationEvents: [{ elementId: null, kind: 'image', disposition: 'dropped', message: 'unknown imageKey' }] };
    assert.equal(jobOutcome(applied).reason, 'Some image changes were skipped. The rest was applied.');
    assert.equal(jobOutcome(applied).retryAll, false);
    assert.equal(jobOutcome({ ...applied, validationEvents: [{ kind: 'element', disposition: 'repaired', message: 'Fixed' }] }).reason, '');
    assert.equal(jobOutcome({ ...applied, validationEvents: [], problemKind: 'partial', message: 'Some original wording' }).reason, 'Some changes were skipped. The rest was applied.');
  });
  await test('operation warnings share one partial-success path across job and batch snapshots', () => {
    const dropped = { elementId: null, kind: 'operation', disposition: 'dropped', message: 'Request 1 operation 2 (update) skipped: invalid target' };
    const recovered = { elementId: null, kind: 'pagePatch', disposition: 'warning', message: 'Recovered the page background. Other layer patch fields were ignored.' };
    const partial = { jobId: 'partial', status: 'applied', areas: [], candidate: null,
      validationEvents: [dropped], batches: [{ status: 'applied', validationEvents: [dropped, recovered] }] };
    assert.deepEqual(generationWarnings(partial), [dropped.message, recovered.message]);
    assert.equal(jobOutcome(partial).reason, 'Some layer changes and page changes were skipped. The rest was applied.');
    assert.equal(jobOutcome(partial).retryAll, false);
    assert.deepEqual(generationWarnings({ ...partial, validationEvents: [], batches: [{ status: 'applied', validationEvents: [dropped] }] }), [dropped.message]);
    assert.deepEqual(generationWarnings({ validationEvents: [{ ...dropped, kind: 'replyStructure', disposition: 'conflicted' }] }), []);
    assert.deepEqual(generationWarnings({ validationEvents: [{ ...recovered, kind: 'telemetryStorage' }] }), []);
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
