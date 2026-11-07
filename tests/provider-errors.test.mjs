import assert from 'node:assert/strict';
import test from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { compileModules } from './typescript.mjs';

const modules = await compileModules(['src/userErrors.ts', 'src/Generation.tsx', 'src/projectCanvas/exportPanel.tsx']);
try {
  const { providerProblem, userError } = await modules.load('userErrors.js');
  const { needsAiSettings } = await modules.load('ProblemBanner.js');
  const { GenerationFeedback } = await modules.load('Generation.js');
  const { ExportPanel } = await modules.load('projectCanvas/exportPanel.js');
  const failure = (fields = {}) => ({ kind: 'transport', outcome: 'unknown', httpStatus: null, code: null, ...fields });
  const cases = [
    [{ kind: 'server' }, 'had a server problem. Try again in a moment.', false],
    [{ httpStatus: 500, code: 'internal_error' }, 'had a server problem. Try again in a moment.', false],
    [{ httpStatus: 503 }, 'had a server problem. Try again in a moment.', false],
    [{ code: 'overloaded' }, 'had a server problem. Try again in a moment.', false],
    [{ code: 'service_unavailable' }, 'had a server problem. Try again in a moment.', false],
    [{ httpStatus: 429 }, 'is rate-limiting requests. Wait a moment and try again.', false],
    [{ code: 'rate_limit_exceeded' }, 'is rate-limiting requests. Wait a moment and try again.', false],
    [{ kind: 'rateLimited' }, 'is rate-limiting requests. Wait a moment and try again.', false],
    [{ httpStatus: 402 }, 'account is out of credit.', true],
    [{ httpStatus: 429, code: 'insufficient_quota' }, 'account is out of credit.', true],
    [{ code: 'insufficient_balance' }, 'account is out of credit.', true],
    [{ code: 'billing_hard_limit_reached' }, 'account is out of credit.', true],
    [{ kind: 'quota' }, 'account is out of credit.', true],
    [{ httpStatus: 401 }, 'did not accept your key or its permissions. Check AI settings.', true],
    [{ httpStatus: 403 }, 'did not accept your key or its permissions. Check AI settings.', true],
    [{ code: 'invalid_api_key' }, 'did not accept your key or its permissions. Check AI settings.', true],
  ];
  for (const [provider, name] of Object.entries({ openai: 'OpenAI', anthropic: 'Anthropic', google: 'Google', openrouter: 'OpenRouter', deepseek: 'DeepSeek', local: 'Your local AI server' })) {
    await test(`${name}: status, code and kind choose the message and action`, () => {
      for (const [fields, message, settings] of cases) {
        const error = failure(fields);
        assert.deepEqual(providerProblem(error, provider), { message: `${name} ${message}`, settings, action: settings ? 'AI settings' : 'Try again' });
        // Misleading prose must never override structured data.
        const prose = 'The provider connection failed. API key network JSON error';
        assert.equal(needsAiSettings(prose, error, provider), settings);
        assert.equal(userError({ message: prose, providerError: error, provider }), `${name} ${message}${error.httpStatus >= 400 ? ' Nothing was changed.' : ''}`);
      }
    });
  }
  await test('network and deadline failures without an HTTP response keep the internet message and retry', () => {
    for (const kind of ['transport', 'deadline']) {
      const error = failure({ kind });
      assert.equal(userError({ providerError: error, provider: 'deepseek' }), "Desegnos couldn't connect. Check your internet connection and try again.");
      assert.equal(needsAiSettings('connection failed', error), false);
    }
    assert.equal(userError('network connection failed'), "Desegnos couldn't connect. Check your internet connection and try again.");
    assert.equal(needsAiSettings('network timeout'), false);
    assert.equal(userError({ message: 'JSON error', providerError: failure({ kind: 'malformed' }) }), 'The AI could not finish this request. Try again.');
  });
  await test('the reported DeepSeek 500 has a definite failure notice and Try again, including old unknown receipts', () => {
    for (const status of ['failed', 'unknown']) {
      const job = { status, provider: 'deepseek', providerError: failure({ httpStatus: 500, code: 'internal_error' }), message: 'The provider connection failed.', jobId: 'job', areas: [], candidate: null };
      const calls = [];
      const props = { controller: { job, phase: { name: 'idle' }, elapsedSeconds: null, prepare: async ids => calls.push(ids) }, openSettings() {} };
      const html = renderToStaticMarkup(React.createElement(GenerationFeedback, props));
      assert.match(html, /DeepSeek had a server problem/);
      assert.match(html, /Nothing was changed/);
      assert.match(html, />Try again</);
      assert.doesNotMatch(html, /Not sure|internet|>AI settings</);
      GenerationFeedback(props).props.action.onClick();
      assert.deepEqual(calls, [undefined]);
    }
  });
  await test('export behaviour failures use the same structured messages and actions', () => {
    for (const [httpStatus, label] of [[500, 'Try again'], [429, 'Try again'], [402, 'AI settings'], [401, 'AI settings']]) {
      const html = renderToStaticMarkup(React.createElement(ExportPanel, { project: { pages: [] }, exports: { job: { status: 'failed', provider: 'deepseek', providerError: failure({ httpStatus }), message: 'connection failed', pages: [], warnings: [] }, error: 'connection failed' } }));
      assert.match(html, /Nothing was changed/);
      assert.ok(html.includes(`>${label}</button>`));
      assert.doesNotMatch(html, /connection failed|internet/);
    }
  });
} finally { await modules.cleanup(); }
