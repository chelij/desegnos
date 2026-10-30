import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import test from 'node:test';
import { compileModules, root } from './typescript.mjs';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

const modules = await compileModules(['src/RecoveryBanner.tsx', 'src/userErrors.ts']);
try {
  const { RecoveryChoice, isPendingRecovery } = await modules.load('RecoveryBanner.js');
  const { userError } = await modules.load('userErrors.js');
  await test('startup recovery failures do not become AI failures when diagnostics name elements', () => {
    const reason = 'de94f600-bf86-4db4-84e1-8af71e5f1522: The unsaved work in this project is damaged: unknown field `appearance`, expected one of `id`, `name`, `path`, `included`, `output`, `sizes`, `background`, `elements`';
    assert.equal(userError(reason), "Desegnos couldn't restore an unsaved project. Its original file is still on disk.");
  });
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
    assert.match(html, /class="recovery-choice recovery-choice-row" role="alert"/);
    assert.match(html, /class="primary">Recover</);
    assert.match(html, />Open saved version</);
    assert.match(html, /aria-label="Dismiss recovery choice"/);
    assert.doesNotMatch(html, /role="dialog"|<dialog|modal|helper/i);
    const busy = renderToStaticMarkup(React.createElement(RecoveryChoice, choices({ busy: true })));
    assert.equal((busy.match(/<button[^>]*disabled=""/g) ?? []).length, 2, 'both actions wait while busy');
  });
  await test('the card variant keeps the same sentence and actions', () => {
    const html = renderToStaticMarkup(React.createElement(RecoveryChoice, { ...choices(), variant: 'card' }));
    assert.match(html, /class="recovery-choice recovery-choice-card" role="alert"/);
    assert.match(html, />Unsaved changes from an earlier session</);
    assert.match(html, />Recover</);
  });

  await test('App keeps the choice in the recovery surface, never in the global top bar', async () => {
    const source = await readFile(join(root, 'src/App.tsx'), 'utf8');
    assert.doesNotMatch(source, /<RecoveryBanner/);
    assert.equal((source.match(/<RecoveryChoice \{\.\.\.recovery\} \/>/g) ?? []).length, 1);
  });
} finally { await modules.cleanup(); }
