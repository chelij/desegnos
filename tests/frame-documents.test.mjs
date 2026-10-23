import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';

const root = fileURLToPath(new URL('../', import.meta.url));
const temporary = await mkdtemp(join(root, '.frame-documents-'));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022',
    '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/frameDocuments.ts'], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout || String(result.error));
  const {
    canvasToInner, frameAt, frameContentRect, frameDocumentBase, frameDocumentSource, frameMapping,
    innerDocumentToPage, innerRectToPage, innerToCanvas, innerViewportToPage, pageRectToInnerDocument,
    adoptRequestFrame, frameTargetShown, pageRectToInnerViewport, pageToInnerDocument,
    pageToInnerViewport, remapFramedRequest, requestFrame,
  } = await import(pathToFileURL(join(temporary, 'frameDocuments.js')).href);

  const plain = () => frameMapping({ frame: { x: 100, y: 50, width: 400, height: 300 }, layoutWidth: 400, layoutHeight: 300 });
  const siteDocument = { kind: 'siteFile', path: 'play/game.html' };
  const pageDocument = { kind: 'page', pageId: 'page-2' };

  await test('an untransformed frame maps inner pixels straight onto the page', () => {
    const mapping = plain();
    assert.deepEqual(innerViewportToPage(mapping, { x: 20, y: 30 }), { x: 120, y: 80 });
    assert.deepEqual(pageToInnerViewport(mapping, { x: 120, y: 80 }), { x: 20, y: 30 });
    assert.deepEqual(innerRectToPage(mapping, { x: 10, y: 20, width: 30, height: 40 }),
      { x: 110, y: 70, width: 30, height: 40 });
    assert.deepEqual(frameContentRect({ mapping, viewport: { width: 400, height: 300 } }),
      { x: 100, y: 50, width: 400, height: 300 });
  });

  await test('a scaled frame keeps inner and page coordinates proportional', () => {
    // The iframe is laid out 400x300 and drawn at half size.
    const mapping = frameMapping({ frame: { x: 100, y: 50, width: 200, height: 150 }, layoutWidth: 400, layoutHeight: 300 });
    assert.equal(mapping.scaleX, 0.5);
    assert.equal(mapping.scaleY, 0.5);
    assert.deepEqual(innerViewportToPage(mapping, { x: 40, y: 60 }), { x: 120, y: 80 });
    assert.deepEqual(pageToInnerViewport(mapping, { x: 120, y: 80 }), { x: 40, y: 60 });
    assert.deepEqual(innerRectToPage(mapping, { x: 10, y: 20, width: 30, height: 40 }),
      { x: 105, y: 60, width: 15, height: 20 });
    const rect = { x: 120, y: 90, width: 40, height: 20 };
    assert.deepEqual(pageRectToInnerViewport(mapping, rect), { x: 40, y: 80, width: 80, height: 40 });
    assert.deepEqual(innerRectToPage(mapping, pageRectToInnerViewport(mapping, rect)), rect);
    // A zero layout width cannot divide; the mapping stays usable.
    assert.equal(frameMapping({ frame: { x: 0, y: 0, width: 10, height: 10 }, layoutWidth: 0, layoutHeight: 0 }).scaleX, 1);
  });

  await test('a frame border and padding offset its content box', () => {
    const mapping = frameMapping({ frame: { x: 100, y: 50, width: 420, height: 320 }, layoutWidth: 420, layoutHeight: 320, borderLeft: 4, borderTop: 6, paddingLeft: 8, paddingTop: 2 });
    assert.equal(mapping.insetX, 12);
    assert.equal(mapping.insetY, 8);
    assert.deepEqual(innerViewportToPage(mapping, { x: 0, y: 0 }), { x: 112, y: 58 });
    assert.deepEqual(pageToInnerViewport(mapping, { x: 112, y: 58 }), { x: 0, y: 0 });
  });

  await test('document coordinates add the frame scroll on top of its viewport', () => {
    const mapping = frameMapping({ frame: { x: 100, y: 50, width: 400, height: 300 }, layoutWidth: 400, layoutHeight: 300, scrollX: 30, scrollY: 120 });
    assert.deepEqual(innerDocumentToPage(mapping, { x: 10, y: 20 }), { x: 80, y: -50 });
    assert.deepEqual(pageToInnerDocument(mapping, { x: 80, y: -50 }), { x: 10, y: 20 });
    const rect = { x: 0, y: 0, width: 40, height: 60 };
    assert.deepEqual(pageRectToInnerDocument(mapping, { x: 100, y: 50, width: 20, height: 30 }),
      { x: 30, y: 120, width: 20, height: 30 });
    assert.deepEqual(innerRectToPage(mapping, pageRectToInnerViewport(mapping, rect)), rect);
  });

  await test('inner points compose with the canvas offset and zoom for pointer routing', () => {
    const mapping = frameMapping({ frame: { x: 100, y: 50, width: 200, height: 150 }, layoutWidth: 400, layoutHeight: 300 });
    const view = { origin: { x: 80, y: 20 }, zoom: 0.5 };
    assert.deepEqual(innerToCanvas({ x: 40, y: 60 }, mapping, view), { x: 20, y: 30 });
    assert.deepEqual(canvasToInner({ x: 20, y: 30 }, mapping, view), { x: 40, y: 60 });
    // Scrolled document coordinates still route from canvas pixels through the viewport.
    const scrolled = frameMapping({ frame: { x: 100, y: 50, width: 200, height: 150 }, layoutWidth: 400, layoutHeight: 300, scrollY: 40 });
    assert.deepEqual(innerToCanvas({ x: 0, y: 40 }, scrolled, view), { x: 10, y: 15 });
    assert.deepEqual(canvasToInner({ x: 10, y: 15 }, scrolled, view), { x: 0, y: 40 });
  });

  await test('a page point routes to the frame drawn on top of it', () => {
    const back = { mapping: frameMapping({ frame: { x: 0, y: 0, width: 400, height: 300 }, layoutWidth: 400, layoutHeight: 300 }), viewport: { width: 400, height: 300 } };
    const front = { mapping: frameMapping({ frame: { x: 100, y: 50, width: 200, height: 150 }, layoutWidth: 200, layoutHeight: 150 }), viewport: { width: 200, height: 150 } };
    assert.deepEqual(frameAt([back, front], { x: 120, y: 80 }), { index: 1, point: { x: 20, y: 30 } });
    assert.deepEqual(frameAt([back, front], { x: 20, y: 20 }), { index: 0, point: { x: 20, y: 20 } });
    assert.equal(frameAt([back, front], { x: 500, y: 500 }), null);
    // The frame's content box ends at its viewport, not at its border box.
    assert.equal(frameAt([{ ...front, viewport: { width: 0, height: 0 } }], { x: 120, y: 80 }), null);
  });

  const editable = document => ({
    mapping: frameMapping({ frame: { x: 100, y: 50, width: 400, height: 300 }, layoutWidth: 400, layoutHeight: 300, scrollX: 0, scrollY: 0 }),
    viewport: { width: 400, height: 300 },
    iframe: { id: 'iframe1', selector: 'body > iframe:nth-of-type(1)', tag: 'iframe', label: '' },
    selector: 'body > iframe:nth-of-type(1)',
    document,
    sourceSha256: 'a'.repeat(64),
    contentRect: { x: 100, y: 50, width: 400, height: 300 },
  });

  await test('a request box over a frame maps its rect into that document', () => {
    const frame = editable(siteDocument);
    const request = requestFrame([frame], { x: 150, y: 100, width: 100, height: 60 });
    assert.ok(request);
    assert.equal(request.frame, frame);
    assert.deepEqual(request.rect, { x: 50, y: 50, width: 100, height: 60 });
    // The box is clamped only by its centre: a corner clip stays a page request.
    assert.equal(requestFrame([frame], { x: 60, y: 10, width: 20, height: 20 }), null);
    assert.equal(requestFrame([], { x: 150, y: 100, width: 100, height: 60 }), null);
  });

  await test('a box that was drawn before framed editing adopts the frame it covers', () => {
    const frame = editable(pageDocument);
    // The box's centre is outside the frame, so it was not a framed request when drawn.
    const adopted = adoptRequestFrame([frame], { x: 60, y: 10, width: 120, height: 100 });
    assert.ok(adopted);
    assert.equal(adopted.frame, frame);
    assert.deepEqual(adopted.rect, { x: -40, y: -40, width: 120, height: 100 });
    // A corner clip without a meaningful shared area stays a page request.
    assert.equal(adoptRequestFrame([frame], { x: 60, y: 10, width: 20, height: 20 }), null);
    // A large frame the box mostly covers, and a small frame around the box's centre: the
    // centre rule wins over the larger shared area.
    const large = editable(siteDocument);
    large.mapping = frameMapping({ frame: { x: 0, y: 0, width: 1000, height: 400 }, layoutWidth: 1000, layoutHeight: 400 });
    large.contentRect = { x: 0, y: 0, width: 1000, height: 400 };
    const small = editable(pageDocument);
    small.mapping = frameMapping({ frame: { x: 450, y: 450, width: 100, height: 100 }, layoutWidth: 100, layoutHeight: 100 });
    small.contentRect = { x: 450, y: 450, width: 100, height: 100 };
    assert.equal(adoptRequestFrame([large, small], { x: 0, y: 300, width: 1000, height: 400 }).frame, small);
    assert.equal(adoptRequestFrame([], { x: 150, y: 100, width: 100, height: 60 }), null);
  });

  await test('an existing frame target stays while its iframe still shows the document', () => {
    const frame = editable(pageDocument);
    assert.equal(frameTargetShown([frame], { iframeId: 'iframe1', document: pageDocument }), true);
    assert.equal(frameTargetShown([frame], { iframeId: 'iframe1', document: siteDocument }), false);
    assert.equal(frameTargetShown([frame], { iframeId: 'iframe2', document: pageDocument }), false);
    assert.equal(frameTargetShown([], { iframeId: 'iframe1', document: pageDocument }), false);
  });

  await test('a moved framed request re-maps while it stays inside its frame', () => {
    const frame = editable(pageDocument);
    assert.deepEqual(remapFramedRequest([frame], { document: pageDocument, iframeId: 'iframe1' }, { x: 200, y: 150, width: 40, height: 40 }),
      { x: 100, y: 100, width: 40, height: 40 });
    assert.equal(remapFramedRequest([frame], { document: pageDocument, iframeId: 'iframe1' }, { x: 600, y: 500, width: 40, height: 40 }), null);
    // Another document through the same iframe, or another iframe, is not this request's frame.
    assert.equal(remapFramedRequest([frame], { document: siteDocument, iframeId: 'iframe1' }, { x: 200, y: 150, width: 40, height: 40 }), null);
    assert.equal(remapFramedRequest([frame], { document: pageDocument, iframeId: 'iframe2' }, { x: 200, y: 150, width: 40, height: 40 }), null);
  });

  await test('framed documents use their original site path or the page session path', () => {
    const website = { website: true, url: 'desegni-preview://session-1.localhost/play/solopong/index.html', sessionId: 's1' };
    const generated = { url: 'desegni-preview://localhost/session-2/host-page/index.html', sessionId: 's2' };
    assert.equal(frameDocumentBase(website, 'play/game/index.html'), 'desegni-preview://session-1.localhost/play/game/index.html');
    assert.equal(frameDocumentBase(website, null), null);
    assert.equal(frameDocumentBase(generated, 'page/index.html'), null);
    assert.equal(frameDocumentBase(null, 'page/index.html'), null);
    assert.equal(frameDocumentSource(website, null, 'play/game/index.html'), 'desegni-preview://session-1.localhost/play/game/index.html');
    assert.equal(frameDocumentSource(generated, 'framed-page', null), 'desegni-preview://localhost/session-2/framed-page/index.html');
    assert.equal(frameDocumentSource(generated, null, null), null);
    assert.equal(frameDocumentSource(null, 'framed-page', null), null);
  });
} finally {
  await rm(temporary, { recursive: true, force: true });
}
