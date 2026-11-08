import assert from 'node:assert/strict';
import test from 'node:test';
import { compileModules } from './typescript.mjs';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

const modules = await compileModules(['src/RecoveryBanner.tsx']);
try {
  const { RecoveryChoice, isPendingRecovery } = await modules.load('RecoveryBanner.js');
  const pending = { code: 'pendingRecovery', projectId: 'project-id', path: '/chosen/site.desegnos' };
  const choices = (overrides = {}) => ({ path: pending.path, busy: false, recover() {}, openSaved() {}, dismiss() {}, ...overrides });

  await test('recovery errors are identified by code and typed fields, never by prose', () => {
    assert.equal(isPendingRecovery(pending), true);
    for (const error of [null, pending.code, new Error('This project has unsaved changes from an earlier session.'), { ...pending, code: 'other' }, { ...pending, path: null }, { ...pending, projectId: 3 }, { missing: true, message: 'Missing project' }]) {
      assert.equal(isPendingRecovery(error), false);
    }
  });
  await test('the choice has only the sentence, two actions, and dismiss', () => {
    const html = renderToStaticMarkup(React.createElement(RecoveryChoice, choices()));
    assert.equal(html.replace(/<[^>]+>/g, ''), 'Unsaved changes from an earlier sessionRecoverOpen saved version');
    assert.match(html, /role="alert"/);
    assert.match(html, /<button\b[^>]*>Recover<\/button>/);
    assert.match(html, />Open saved version</);
    assert.match(html, /aria-label="Dismiss recovery choice"/);
    const busy = renderToStaticMarkup(React.createElement(RecoveryChoice, choices({ busy: true })));
    assert.equal((busy.match(/<button[^>]*disabled=""/g) ?? []).length, 2, 'both actions wait while busy');
  });
  await test('the card variant keeps the same sentence and actions', () => {
    const html = renderToStaticMarkup(React.createElement(RecoveryChoice, { ...choices(), variant: 'card' }));
    assert.match(html, /role="alert"/);
    assert.match(html, />Unsaved changes from an earlier session</);
    assert.match(html, />Recover</);
  });

} finally { await modules.cleanup(); }
