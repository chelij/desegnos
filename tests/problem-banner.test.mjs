import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

const temporary = await mkdtemp(new URL('../.problem-banner-', import.meta.url));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022', '--jsx', 'react-jsx', '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/ProblemBanner.tsx', 'src/noticeSummary.ts'], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  // Node ESM needs extensions; keep the app's bundler-style source import.
  const bannerPath = join(temporary, 'ProblemBanner.js');
  await writeFile(bannerPath, (await readFile(bannerPath, 'utf8')).replaceAll("from './icons'", "from './icons.js'"));
  const { Notice, NoticeRow, ProblemBanner, needsAiSettings, firstNotice } = await import(pathToFileURL(bannerPath));
  const { summarizeOutsideChanges } = await import(pathToFileURL(join(temporary, 'noticeSummary.js')));
  for (const variant of ['success', 'info', 'warning', 'error']) {
    await test(`${variant} has one message, a full-text tooltip, two actions and an accessible dismiss`, () => {
      const message = 'A long message that must remain on one line when the available space is tight.';
      const html = renderToStaticMarkup(React.createElement(Notice, {
        variant, title: 'Update', message,
        action: { label: 'Fix', onClick() {} }, secondaryAction: { label: 'Details', disabled: true, onClick() {} },
      }));
      assert.match(html, new RegExp(`class="notice floating-bar notice-${variant}" role="${['error', 'warning'].includes(variant) ? 'alert' : 'status'}"`));
      assert.ok(html.includes(`title="Update · ${message}"`));
      assert.match(html, /<strong>Update<\/strong><span>/);
      assert.match(html, /class="primary">Fix/);
      assert.match(html, /disabled="">Details/);
      assert.match(html, /aria-label="Dismiss notice" title="Dismiss"><svg/);
      assert.equal((html.match(/<button/g) ?? []).length, 3);
      assert.doesNotMatch(html, /<p\b|dialog|modal|helper/i);
    });
  }
  await test('a notice that closes itself keeps no second close', () => {
    for (const variant of ['success', 'info', undefined]) {
      const html = renderToStaticMarkup(React.createElement(NoticeRow, { variant, message: 'Saved', dismiss() {} }));
      assert.doesNotMatch(html, /notice-dismiss/, `${variant} toast carries no dismiss button`);
      assert.equal((html.match(/<button/g) ?? []).length, 0);
    }
    for (const notice of [
      { variant: 'warning', message: 'Changed on disk' },
      { message: 'Page updated', action: { label: 'Undo', onClick() {} } },
      { message: 'Page updated', secondaryAction: { label: 'Details', onClick() {} } },
    ]) assert.match(renderToStaticMarkup(React.createElement(NoticeRow, { ...notice, dismiss() {} })), /notice-dismiss/);
  });
  await test('problems default to the error variant and keep existing fixes', () => {
    const html = renderToStaticMarkup(React.createElement(ProblemBanner, { message: 'The page stopped.', action: { label: 'Fix with AI', disabled: true, onClick() {} } }));
    assert.match(html, /notice-error/);
    assert.match(html, /class="primary" disabled=""/);
    assert.equal(renderToStaticMarkup(React.createElement(Notice, { message: '' })), '');
    for (const error of ['quota', 'authentication expired', 'API key missing', 'usage limit', 'secure storage']) assert.equal(needsAiSettings(error), true);
    assert.equal(needsAiSettings('Region script affects another region'), false);
    for (const error of ['network timeout', 'rate limit', 'too many requests']) assert.equal(needsAiSettings(error), false);
  });
  await test('primary, secondary and dismiss preserve callbacks', () => {
    const calls = [];
    const row = NoticeRow({ message: 'Update', action: { label: 'Undo', onClick: () => calls.push('undo') }, secondaryAction: { label: 'Details', onClick: () => calls.push('details') }, dismiss: () => calls.push('dismiss') });
    for (const button of row.props.children[1].props.children) button.props.onClick();
    assert.deepEqual(calls, ['undo', 'details', 'dismiss']);
  });
  await test('queue selects errors, warnings, results and then info in FIFO order', () => {
    const entries = [{ variant: 'info', message: 'First' }, { variant: 'success', priority: 'result', message: 'Result' }, { variant: 'warning', message: 'Warning' }, { variant: 'error', message: 'Error' }, { variant: 'info', message: 'Second' }];
    const ordered = [];
    while (entries.length) { const next = firstNotice(entries); ordered.push(next.message); entries.splice(entries.indexOf(next), 1); }
    assert.deepEqual(ordered, ['Error', 'Warning', 'Result', 'First', 'Second']);
    assert.equal(firstNotice([]), undefined);
  });
  const outside = message => ({ kind: 'outsideChanges', message });
  await test('Also changed deduplicates events, repeated lines and individual labels', () => {
    const repeated = 'Also changed: page styles, page script, Box 12, Box 15';
    const summary = summarizeOutsideChanges([outside(repeated), outside(`${repeated}  ${repeated}`), outside('Also changed: Box 12'), { kind: 'other', message: 'Ignore this' }]);
    assert.equal(summary, 'Also changed page styles, page script and 2 other boxes');
    assert.equal((summary.match(/Also changed/g) ?? []).length, 1);
  });
  await test('Also changed handles empty, short, one-box and non-box lists', () => {
    assert.equal(summarizeOutsideChanges([]), '');
    assert.equal(summarizeOutsideChanges([outside('Also changed: ')]), '');
    assert.equal(summarizeOutsideChanges([outside('Also changed: page styles, page styles')]), 'Also changed page styles');
    assert.equal(summarizeOutsideChanges([outside('Also changed: page styles, Box 1')]), 'Also changed page styles and Box 1');
    assert.equal(summarizeOutsideChanges([outside('Also changed: a, b, c, Box 1')]), 'Also changed a, b, 1 other box and 1 other change');
  });
} finally { await rm(temporary, { recursive: true, force: true }); }
