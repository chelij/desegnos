import assert from 'node:assert/strict';
import test from 'node:test';
import { compileModules } from './typescript.mjs';

const modules = await compileModules(['src/crashReportText.ts']);
try {
  const { CRASH_NOTICE, crashOpenFailure } = await modules.load('crashReportText.js');

  await test('the notice uses one short plain sentence', () => {
    assert.equal(CRASH_NOTICE, 'Desegnos closed unexpectedly last time.');
  });

  await test('a failed open keeps the issue address for copying', () => {
    const url = 'https://github.com/chelij/desegnos/issues/new?title=Crash&body=Text';
    const failure = crashOpenFailure({ message: 'Couldn\'t open your browser.', url });
    assert.equal(failure.url, url);
  });

  await test('a plain failure keeps its native message and has no fallback address', () => {
    const failure = crashOpenFailure('boom');
    assert.equal(failure.url, null);
    assert.equal(failure.message, 'boom');
  });

  await test('an ordinary rejection is not a crash address', () => {
    const failure = crashOpenFailure(new Error('ordinary'));
    assert.equal(failure.url, null);
    assert.equal(failure.message, 'ordinary');
  });
} finally {
  await modules.cleanup();
}
