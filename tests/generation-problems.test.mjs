import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

const temporary = await mkdtemp(new URL('../.problems-', import.meta.url));
try {
  const compiler = new URL('bin/tsc', import.meta.resolve('typescript/package.json')).pathname;
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022', '--outDir', temporary, 'src/generationProblems.ts'], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const { areaProblems, placeCallouts, areaOutcome, jobOutcome, resolutionAction } = await import(pathToFileURL(join(temporary, 'generationProblems.js')));
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
  const page = { id: 'page', content: { html: 'now', css: '', javascript: '' }, regions: [{ id: 'a', rect, prompt: 'Add scrolling chat' }, { id: 'b', rect: other, prompt: 'Keep heading' }] };
  const job = { jobId: 'job', status: 'blocked', areas: [{ pageId: 'page', regionId: 'a' }], drafts: [{ id: 'old', pageId: 'page', regionIds: ['a'], reason: 'old reason' }, { id: 'new', pageId: 'page', regionIds: ['a'], reason: 'Region script also affects b' }], candidate: { regions: [{ regionId: 'a', status: 'conflict' }] } };
  await test('only latest rejected area result appears and uses request and affected box', () => {
    const problems = areaProblems(job, page);
    assert.equal(problems.length, 1); assert.equal(problems[0].draftId, 'new');
    assert.equal(problems[0].message, '“Add scrolling chat” would also change Box 2.');
    for (const flag of ['applied', 'dismissed']) assert.deepEqual(areaProblems({ ...job, drafts: [...job.drafts.slice(0, 1), { ...job.drafts[1], [flag]: true }] }, page), []);
    assert.deepEqual(areaProblems({ ...job, status: 'generating' }, page), []);
  });
  await test('an undecodable whole reply has no area callouts', () => {
    assert.deepEqual(areaProblems({ ...job, candidate: null, status: 'failed', drafts: [{ id: 'reply', pageId: null, regionIds: ['a'], reason: 'reply could not be decoded' }] }, page), []);
  });
  await test('stale area uses one plain sentence without losing saved validator reason', () => {
    const stale = { ...job, drafts: [{ ...job.drafts[1], baseContent: { ...page.content, html: 'before' } }] };
    const problem = areaProblems(stale, page)[0];
    assert.equal(problem.stale, true);
    assert.equal(problem.message, 'The page was edited after the AI made this draft.');
    assert.equal(stale.drafts[0].reason, job.drafts[1].reason);
  });
  const frozen = (prompt) => '```json\n' + JSON.stringify({ pageId: 'page', regionId: 'a', prompt, placementHint: rect }) + '\n```';
  const declared = { ...job, requirements: { markdown: frozen('Add scrolling chat') }, validationEvents: [], candidate: { regions: [{ regionId: 'a', status: 'conflict', explanation: `Area 12345678-1234-1234-1234-123456789abc: Which chat service should this use?` }] } };
  await test('an explained AI conflict is not retried and offers the resolving surface', () => {
    const outcome = areaOutcome(declared, 'a', 'Add scrolling chat');
    assert.deepEqual(outcome, { retryable: false, reason: 'Which chat service should this use?', fix: 'aiSettings' });
    assert.deepEqual(areaOutcome(declared, 'a', 'Add a support chat'), { retryable: true, reason: '' });
    const review = jobOutcome(declared, id => id === 'a' ? 'Add scrolling chat' : undefined);
    assert.deepEqual(review.retryable, []);
    assert.equal(review.blocked.length, 1); assert.equal(review.blocked[0].fix, 'aiSettings');
  });
  await test('an app rejection stays retryable even beside a declared conflict', () => {
    const rejected = { ...declared, candidate: { regions: [{ regionId: 'a', status: 'conflict', explanation: `Area 12345678-1234-1234-1234-123456789abc: Region script also affects another area.` }] } };
    assert.equal(areaOutcome(rejected, 'a', 'Add scrolling chat').retryable, true);
    const review = jobOutcome(rejected, () => 'Add scrolling chat');
    assert.deepEqual(review.retryable, ['a']); assert.deepEqual(review.blocked, []);
  });
  await test('retry returns after the request text is edited', () => {
    assert.equal(areaOutcome(declared, 'a', 'Add a support chat').retryable, true);
    const edited = jobOutcome(declared, () => 'Add a support chat');
    assert.deepEqual(edited.retryable, ['a']); assert.deepEqual(edited.blocked, []);
  });
  await test('declared reasons map to plain routes', () => {
    assert.equal(resolutionAction('Which direction should the photo point?'), 'brief');
    assert.equal(resolutionAction('The logo file is missing.'), 'import');
    assert.equal(resolutionAction('This needs a payment provider.'), 'aiSettings');
    assert.equal(resolutionAction('Make it feel calmer.'), undefined);
  });
  await test('a rejected reply is recognised from its app text while repair limits do not retry', () => {
    const whole = { ...declared, candidate: { regions: [{ regionId: 'a', status: 'conflict', explanation: `Area 12345678-1234-1234-1234-123456789abc: The generation result omitted a requested area.` }] } };
    assert.equal(areaOutcome(whole, 'a', 'Add scrolling chat').retryable, true);
    const limited = { ...declared, validationEvents: [{ kind: 'repairLimit', disposition: 'conflicted', regionId: null, message: 'Skipped the repair because it would exceed the spending limit.' }] };
    assert.equal(areaOutcome(limited, 'a', 'Add scrolling chat').retryable, false);
  });
  await test('unavailable capabilities and missing areas keep their classification', () => {
    const capability = { jobId: 'job', status: 'blocked', areas: [{ pageId: 'page', regionId: 'a' }], drafts: [], candidate: { regions: [{ regionId: 'a', status: 'implemented', explanation: 'Done' }], componentRequests: [{ regionId: 'a', capability: 'Video player', resolution: 'unavailable', reason: 'No video service is connected.' }] } };
    const outcome = areaOutcome(capability, 'a', 'Add a video');
    assert.equal(outcome.retryable, false); assert.equal(outcome.fix, 'aiSettings');
    assert.equal(jobOutcome(capability, () => 'Add a video').retryable.length, 0);
    const failed = { jobId: 'job', status: 'failed', areas: [], drafts: [], candidate: null };
    assert.equal(jobOutcome(failed).retryAll, true);
  });
} finally { await rm(temporary, { recursive: true, force: true }); }
