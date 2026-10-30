import assert from 'node:assert/strict';
import test from 'node:test';
import { compileModules } from './typescript.mjs';

const modules = await compileModules(['src/useGeneration.ts']);
const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; };
try {
  // Drive the hook's async actions without a browser. Effects (polling and automatic
  // apply) are deliberately excluded; apply is invoked explicitly at the ready boundary.
  await modules.rewrite('useGeneration.js', source => source
    .replace(/^import .* from 'react';$/m, 'const { useCallback, useEffect, useRef, useState } = globalThis.__generationCancellation.hooks;')
    .replace(/^import .* from '\.\/api\.js';$/m, 'const native = (...args) => globalThis.__generationCancellation.native(...args);'));
  let slots = [], cursor = 0, nativeCalls = [];
  globalThis.__generationCancellation = {
    hooks: {
      useState(initial) { const index = cursor++; if (!(index in slots)) slots[index] = typeof initial === 'function' ? initial() : initial; return [slots[index], value => { slots[index] = typeof value === 'function' ? value(slots[index]) : value; }]; },
      useRef(initial) { const index = cursor++; return slots[index] ??= { current: initial }; },
      useCallback(callback) { return callback; },
      useEffect() {},
    },
    async native(command, args) { nativeCalls.push({ command, args }); return { jobId: args.jobId, status: 'cancelled', attempts: [], candidate: null }; },
  };
  const { useGeneration } = await modules.load('useGeneration.js');
  function setup(call, applied = () => {}) {
    slots = []; nativeCalls = [];
    return () => { cursor = 0; return useGeneration('project', call, applied); };
  }
  const prepared = { jobId: 'job', status: 'prepared', attempts: [] };

  await test('preparing targets explicit layers or reference items and then starts the job', async () => {
    const calls = [];
    const render = setup(async (command, args) => { calls.push({ command, args }); return prepared; });
    await render().prepare(['element-1'], ['reference-1']);
    assert.deepEqual(calls, [
      { command: 'prepare_generation', args: { elementIds: ['element-1'], referenceIds: ['reference-1'] } },
      { command: 'start_generation', args: { jobId: 'job' } },
    ]);
    const plain = [];
    const renderPlain = setup(async (command, args) => { plain.push({ command, args }); return prepared; });
    await renderPlain().prepare();
    assert.deepEqual(plain, [
      { command: 'prepare_generation', args: undefined },
      { command: 'start_generation', args: { jobId: 'job' } },
    ]);
  });

  await test('start and retry follow a refreshed job ID for Stop and later actions', async () => {
    const calls = [];
    const render = setup(async (command, args) => {
      calls.push({ command, args });
      if (command === 'prepare_generation') return prepared;
      return { ...prepared, jobId: command === 'start_generation' ? 'fresh-start' : 'fresh-retry', status: 'generating' };
    });
    await render().prepare();
    assert.equal(render().job.jobId, 'fresh-start');
    await render().retry();
    assert.deepEqual(calls.at(-1), { command: 'retry_generation', args: { jobId: 'fresh-start' } });
    assert.equal(render().job.jobId, 'fresh-retry');
    await render().cancel();
    assert.deepEqual(nativeCalls.at(-1), { command: 'cancel_generation', args: { jobId: 'fresh-retry' } });
  });

  await test('Stop before queued preparation dispatch makes no native/provider request', async () => {
    const gate = deferred(); const calls = [];
    const render = setup(async (command, args, signal) => { await gate.promise; signal.throwIfAborted(); calls.push(command); return prepared; });
    const preparation = render().prepare();
    assert.equal(render().canStop, true);
    const stopped = render().cancel();
    assert.equal(render().operation, 'Stopping…');
    gate.resolve();
    await assert.rejects(preparation, { name: 'AbortError' }); await stopped;
    assert.deepEqual(calls, []); assert.deepEqual(nativeCalls, []);
    assert.equal(render().stopped, true); assert.equal(render().canStop, false);
  });

  await test('Stop while preparation returns records Stopped without starting the job', async () => {
    const gate = deferred(); const calls = [];
    const render = setup(async command => { calls.push(command); await gate.promise; return prepared; });
    const preparation = render().prepare();
    const stopped = render().cancel();
    gate.resolve();
    await assert.rejects(preparation, { name: 'AbortError' }); await stopped;
    assert.deepEqual(calls, ['prepare_generation']);
    assert.deepEqual(nativeCalls, [{ command: 'cancel_generation', args: { jobId: 'job' } }]);
    assert.equal(render().job.status, 'cancelled'); assert.equal(render().operation, null);
  });

  await test('Stop bypasses a queued start and blocks its later dispatch', async () => {
    const gate = deferred(), starting = deferred(); const calls = [];
    const render = setup(async (command, args, signal) => {
      if (command === 'start_generation') { starting.resolve(); await gate.promise; }
      signal.throwIfAborted(); calls.push(command); return prepared;
    });
    const preparation = render().prepare(); await starting.promise;
    await render().cancel();
    assert.equal(nativeCalls[0].command, 'cancel_generation');
    gate.resolve(); await assert.rejects(preparation, { name: 'AbortError' });
    assert.deepEqual(calls, ['prepare_generation']);
    assert.equal(render().job.status, 'cancelled'); assert.equal(render().canStop, false);
  });

  await test('Stop after Ready blocks a queued canvas apply', async () => {
    const gate = deferred(); const applied = [];
    const render = setup(async (command, args, signal) => {
      if (command === 'apply_generation') { await gate.promise; signal.throwIfAborted(); applied.push('native apply'); return {}; }
      return command === 'prepare_generation' ? prepared : { ...prepared, status: 'ready' };
    }, () => applied.push('canvas updated'));
    await render().prepare();
    const application = render().apply([]);
    await render().cancel();
    gate.resolve(); await assert.rejects(application, { name: 'AbortError' });
    assert.deepEqual(applied, []); assert.equal(render().job.status, 'cancelled');
  });
} finally {
  delete globalThis.__generationCancellation;
  await modules.cleanup();
}
