import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';

const root = fileURLToPath(new URL('../', import.meta.url));
const temporary = await mkdtemp(join(root, '.preview-measurement-'));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022',
    '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/previewMeasurement.ts'], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout || String(result.error));
  const { measurableDocument } = await import(pathToFileURL(join(temporary, 'previewMeasurement.js')).href);

  // Preview documents are opaque tokens: only identity matters for the guard.
  const promotedDocument = { id: 'promoted' };
  const pendingDocument = { id: 'pending' };
  const replacedDocument = { id: 'replaced' };
  const identity = { pageId: 'page-1', revision: 'revision-2', session: 'session-b' };
  const promoted = { ...identity, document: promotedDocument };
  const visible = (overrides = {}) => ({ ...identity, document: promotedDocument, invalidated: false, ...overrides });

  await test('the promoted frame of the current revision is measurable', () => {
    assert.equal(measurableDocument(promoted, visible()), promotedDocument);
  });

  await test('a hidden pending frame is never measured', () => {
    // A pending frame loads the next revision while the previous one is still visible.
    assert.equal(measurableDocument({ ...identity, document: pendingDocument }, visible()), null);
    // Promotion swaps the displayed document before the promoted identity is recorded.
    assert.equal(measurableDocument(promoted, visible({ document: replacedDocument })), null);
  });

  await test('a replaced frame from an earlier revision is never measured', () => {
    // The canvas shows revision 3 while the recorded promotion still names revision 2.
    assert.equal(measurableDocument(promoted, visible({ revision: 'revision-3' })), null);
    assert.equal(measurableDocument({ ...promoted, revision: 'revision-3' }, visible()), null);
  });

  await test('measurement requires the same page, session and an intact document', () => {
    assert.equal(measurableDocument(promoted, visible({ pageId: 'page-2' })), null);
    assert.equal(measurableDocument(promoted, visible({ session: 'session-c' })), null);
    assert.equal(measurableDocument(promoted, visible({ invalidated: true })), null);
    assert.equal(measurableDocument(promoted, visible({ document: null })), null);
    assert.equal(measurableDocument(null, visible()), null);
  });

  await test('only the document stored with the promotion is measured, not an equivalent one', () => {
    // A reload can produce the same page and revision twice; the frame holds one document.
    assert.equal(measurableDocument({ ...identity, document: { id: 'promoted' } }, visible()), null);
  });
} finally {
  await rm(temporary, { recursive: true, force: true });
}
