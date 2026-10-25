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
  const { measurableDocument, needsReferenceMeasure, pendingFrameHeight, promotionHeight, shownSelectionRect, shownSelectionRects } = await import(pathToFileURL(join(temporary, 'previewMeasurement.js')).href);

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

  // A viewport-relative page (html,body{height:100%}, .layout{height:100vh}) lays itself
  // out against the frame it is in, so the pending frame has to be the size the visible
  // frame has; otherwise the promotion commit resizes the frame that was just measured.
  const tallPage = { pageId: 'page-1', extent: 1400 };

  await test('the pending frame is laid out at the visible extent', () => {
    assert.equal(pendingFrameHeight(tallPage, 'page-1', 900), 1400);
    // Nothing is shown yet, so the reference height is the only size to lay out at.
    assert.equal(pendingFrameHeight(null, 'page-1', 900), 900);
    // Another page must not inherit the previous page's extent.
    assert.equal(pendingFrameHeight(tallPage, 'page-2', 900), 900);
  });

  await test('promotion keeps the size the pending frame was laid out at', () => {
    // Viewport-relative content measured in a 1400px frame reports 1400px again, so the
    // frame is not resized by the promotion and the promotion-time box is already final.
    const staged = pendingFrameHeight(tallPage, 'page-1', 900);
    assert.equal(staged, 1400);
    assert.equal(promotionHeight(900, 1400), staged);
  });

  await test('content that needs more room grows the frame; another page starts at the reference height', () => {
    assert.equal(promotionHeight(900, 1700), 1700);
    assert.equal(promotionHeight(900, 400), 900);
    assert.equal(promotionHeight(900, 150_000), 100_000);
  });

  await test('a viewport-relative page that overflows is not grown on every reload', () => {
    // Header above a 100vh layout: the page reports 60px more than the frame it is in.
    const referenceHeight = 900;
    const overflowPage = height => height + 60;
    const promotion = staged => {
      const content = overflowPage(staged);
      return needsReferenceMeasure(staged, referenceHeight, content) ? promotionHeight(referenceHeight, overflowPage(referenceHeight)) : promotionHeight(referenceHeight, content);
    };
    // First load stages at the reference height; later loads stage at the shown extent.
    assert.equal(promotion(referenceHeight), 960);
    assert.ok(needsReferenceMeasure(960, referenceHeight, overflowPage(960)));
    assert.equal(promotion(960), 960);
    assert.equal(promotion(960), 960);
  });

  await test('content-driven pages still grow and shrink to their own height', () => {
    // Content height does not depend on the frame height, so one reading decides it.
    assert.equal(promotionHeight(900, 2000), 2000);
    assert.equal(promotionHeight(900, 700), 900);
    assert.ok(!needsReferenceMeasure(2000, 900, 2000));
    assert.ok(!needsReferenceMeasure(2000, 900, 700));
    assert.ok(!needsReferenceMeasure(900, 900, 2000));
  });

  await test('an unsettled measurement never replaces the box the edit aimed at', () => {
    const aimed = { x: 40, y: 120, width: 200, height: 60 };
    const promotion = { x: 40, y: 30, width: 200, height: 60 };
    const settled = { x: 40, y: 121, width: 200, height: 60 };
    // Promotion measures before the frame layout has settled: draw the aimed box, not the reading.
    assert.deepEqual(shownSelectionRect(promotion, aimed, false), aimed);
    assert.deepEqual(shownSelectionRect(settled, aimed, false), aimed);
    // Once settled, only the measurement is drawn, so the box matches the frame on screen.
    assert.deepEqual(shownSelectionRect(settled, aimed, true), settled);
    assert.notDeepEqual(shownSelectionRect(settled, aimed, true), promotion);
    assert.deepEqual(shownSelectionRect(promotion, aimed, true), promotion);
  });

  await test('a multi-selection holds every aimed box until the settled measurement', () => {
    // Two pictures dragged together: both boxes are the drag targets, neither the reading.
    const aimed = [{ x: 40, y: 120, width: 200, height: 60 }, { x: 300, y: 90, width: 120, height: 120 }];
    const promotion = [{ x: 40, y: 30, width: 200, height: 60 }, { x: 300, y: 10, width: 120, height: 120 }];
    const settled = [{ x: 40, y: 121, width: 200, height: 60 }, { x: 300, y: 91, width: 120, height: 120 }];
    assert.deepEqual(shownSelectionRects(promotion, aimed, false), aimed);
    assert.deepEqual(shownSelectionRects(settled, aimed, false), aimed);
    assert.deepEqual(shownSelectionRects(settled, aimed, true), settled);
    // One element aimed at (a resize of the whole selection) still holds the other boxes drawn.
    const resized = { x: 300, y: 90, width: 200, height: 120 };
    const held = shownSelectionRects(settled, [aimed[0], resized], false);
    assert.deepEqual(held, [aimed[0], resized]);
    // An element the edit did not aim at keeps the box already drawn, not the reading.
    assert.deepEqual(shownSelectionRects(promotion, [aimed[0], undefined], false), [aimed[0], promotion[1]]);
    // A settled reading of every element replaces every held box.
    assert.deepEqual(shownSelectionRects(settled, [aimed[0], null], true), settled);
    // Boxes line up with the measured selection, so a shorter list cannot borrow another element's hold.
    assert.deepEqual(shownSelectionRects([settled[1]], [undefined, aimed[1]], false), [settled[1]]);
  });
} finally {
  await rm(temporary, { recursive: true, force: true });
}
