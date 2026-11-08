import assert from 'node:assert/strict';
import test from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { compileModules } from './typescript.mjs';

const modules = await compileModules(['src/userErrors.ts', 'src/Generation.tsx', 'src/projectCanvas/exportPanel.tsx']);
try {
  const { providerProblem, userError } = await modules.load('userErrors.js');
  await test('startup recovery failures do not become AI failures when diagnostics name elements', () => {
    const reason = 'de94f600-bf86-4db4-84e1-8af71e5f1522: The unsaved work in this project is damaged: unknown field `appearance`, expected one of `id`, `name`, `path`, `included`, `output`, `sizes`, `background`, `elements`';
    assert.equal(userError(reason), "Desegnos couldn't restore an unsaved project. Its original file is still on disk.");
  });
  const { needsAiSettings } = await modules.load('ProblemBanner.js');
  const { GenerationFeedback } = await modules.load('Generation.js');
  const { ExportPanel } = await modules.load('projectCanvas/exportPanel.js');
  const failure = (fields = {}) => ({ kind: 'transport', outcome: 'unknown', httpStatus: null, code: null, ...fields });
  await test('native kinds choose actions while native wording stays intact', () => {
    for (const [kind, settings] of [['server', false], ['transport', false], ['rateLimited', false], ['quota', true], ['authentication', true], ['modelUnavailable', true]]) {
      const error = failure({ kind, httpStatus: settings ? 500 : 401, code: 'misleading_transport_code' });
      const message = 'The native service explains this failure.';
      assert.deepEqual(providerProblem(error, 'deepseek', message), { message, settings, action: settings ? 'AI settings' : 'Try again' });
      assert.equal(needsAiSettings('API key network JSON error', error), settings);
      assert.equal(userError({ message, providerError: error, provider: 'deepseek' }), message);
    }
  });
  await test('untyped connection failures retain the fallback translation', () => {
    assert.equal(userError('network connection failed'), "Desegnos couldn't connect. Check your internet connection and try again.");
    assert.equal(needsAiSettings('network timeout'), false);
    assert.equal(userError({ providerError: failure() }), 'The AI could not finish this request.');
  });
  await test('generation displays native failure wording and can explicitly retry', () => {
    const calls = [];
    const job = { status: 'failed', provider: 'deepseek', providerError: failure({ kind: 'server', httpStatus: 500 }), message: 'The AI service had a server problem.', jobId: 'job', areas: [], candidate: null };
    const props = { controller: { job, phase: { name: 'idle' }, elapsedSeconds: null, prepare: async ids => calls.push(ids) }, openSettings() {} };
    const html = renderToStaticMarkup(React.createElement(GenerationFeedback, props));
    assert.match(html, /The AI service had a server problem/);
    assert.match(html, />Try again</);
    GenerationFeedback(props).props.action.onClick();
    assert.deepEqual(calls, [undefined]);
  });
  await test('export failures retain native wording and use typed recovery actions', () => {
    for (const [kind, label] of [['server', 'Try again'], ['rateLimited', 'Try again'], ['quota', 'AI settings'], ['authentication', 'AI settings']]) {
      const html = renderToStaticMarkup(React.createElement(ExportPanel, { project: { pages: [] }, exports: { job: { status: 'failed', provider: 'deepseek', providerError: failure({ kind }), message: 'The native export failure.', pages: [], warnings: [] }, error: 'The native export failure.' } }));
      assert.match(html, /The native export failure/);
      assert.ok(html.includes(`>${label}</button>`));
    }
  });
} finally { await modules.cleanup(); }
