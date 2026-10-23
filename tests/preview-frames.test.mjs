import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';

const root = fileURLToPath(new URL('../', import.meta.url));
const temporary = await mkdtemp(join(root, '.preview-frames-'));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022',
    '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/previewDocument.ts'], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout || String(result.error));
  const { hardenStaticFrames, staticBase, staticDocument, staticFrameDocument, staticPolicy } = await import(pathToFileURL(join(temporary, 'previewDocument.js')).href);

  const website = { website: true, url: 'desegni-preview://session-1.localhost/play/solopong/index.html' };
  const generated = { url: 'desegni-preview://localhost/session-2/page-1/index.html' };
  const directive = name => staticPolicy.split(';').map(part => part.trim()).find(part => part.startsWith(`${name} `)) ?? '';

  await test('the static canvas policy renders frames from the session and an https embed', () => {
    const frames = directive('frame-src');
    assert.match(frames, /desegni-preview:/);
    assert.match(frames, /http:\/\/\*\.desegni-preview\.localhost/);
    assert.match(frames, /http:\/\/desegni-preview\.localhost/);
    assert.match(frames, / https: about:$/);
    assert.equal(directive('child-src'), frames.replace('frame-src ', 'child-src '));
  });

  await test('the static canvas keeps scripting and fetching switched off', () => {
    assert.equal(directive('script-src'), "script-src 'none'");
    assert.equal(directive('script-src-attr'), "script-src-attr 'none'");
    assert.equal(directive('connect-src'), "connect-src 'none'");
    assert.equal(directive('object-src'), "object-src 'none'");
    assert.equal(directive('worker-src'), "worker-src 'none'");
    assert.ok(!staticPolicy.includes('unsafe-eval') && !staticPolicy.includes("script-src 'unsafe-inline'"));
  });

  await test('the base element may point at the session that serves the page', () => {
    const base = directive('base-uri');
    assert.match(base, /desegni-preview:/);
    assert.ok(!base.includes("'none'"), base);
  });

  await test('an imported website resolves relative frames against its own page URL', () => {
    assert.equal(staticBase(website), website.url);
    assert.equal(staticBase(null), null);
    // A generated page has no imported files, so it keeps the app's document as its base.
    assert.equal(staticBase(generated), null);
  });

  await test('the base href sits in the head and leaves the page body untouched', () => {
    const html = '<div class="llama"><iframe src="../llama-wrap-preview.html"></iframe></div>';
    const document = staticDocument(html, staticBase(website));
    assert.ok(document.startsWith('<!doctype html><html><head>'));
    assert.ok(document.indexOf('http-equiv="Content-Security-Policy"') < document.indexOf('<base '));
    assert.ok(document.indexOf('<base href="desegni-preview://session-1.localhost/play/solopong/index.html">') < document.indexOf('</head>'));
    assert.ok(document.endsWith(`<body>${html}</body></html>`));
  });

  await test('a page rendered without a session base adds no base element', () => {
    const document = staticDocument('<p>Hello</p>', staticBase(generated));
    assert.ok(!document.includes('<base '));
    assert.ok(document.endsWith('<body><p>Hello</p></body></html>'));
  });

  await test('a base URL cannot break out of its attribute', () => {
    const document = staticDocument('<p>Hello</p>', 'desegni-preview://session-1.localhost/a"b.html');
    assert.ok(document.includes('<base href="desegni-preview://session-1.localhost/a&quot;b.html">'), document);
  });

  await test('a framed page renders scriptless with its own CSS and the page policy', () => {
    const document = staticFrameDocument({ html: '<main><h1>Framed</h1></main>', css: 'main { color: red; }' }, staticBase(website));
    assert.ok(document.startsWith('<!doctype html><html><head>'));
    assert.ok(document.includes(`content="${staticPolicy}"`) || document.includes('http-equiv="Content-Security-Policy"'));
    assert.ok(document.includes('<style data-desegnos-framed-css>main { color: red; }</style>'), document);
    assert.ok(document.includes('<base href="desegni-preview://session-1.localhost/play/solopong/index.html">'), document);
    assert.ok(document.endsWith('<body><main><h1>Framed</h1></main></body></html>'));
    assert.ok(!document.includes('<script'));
  });

  await test('a framed site file keeps its authored document and gains policy, base and CSS in its head', () => {
    const html = '<!doctype html>\n<html><head><title>Game</title></head><body><canvas></canvas></body></html>';
    const document = staticFrameDocument({ html, css: '' }, 'desegni-preview://session-1.localhost/play/game/index.html');
    assert.ok(document.startsWith('<!doctype html>\n<html><head>'), document);
    assert.ok(document.indexOf('Content-Security-Policy') < document.indexOf('<title>Game</title>'), document);
    assert.ok(document.indexOf('<base href="desegni-preview://session-1.localhost/play/game/index.html">') < document.indexOf('<title>Game</title>'), document);
    assert.ok(document.includes('<body><canvas></canvas></body></html>'));
    assert.ok(!document.includes('data-desegnos-framed-css'));
    // A document without a head element still gets one, after its doctype or html tag.
    const headless = staticFrameDocument({ html: '<html><body><p>Hi</p></body></html>', css: '' }, null);
    assert.ok(headless.startsWith('<html><head>') && headless.includes('</head><body><p>Hi</p></body></html>'), headless);
    const doctypeOnly = staticFrameDocument({ html: '<!doctype html><p>Hi</p>', css: '' }, null);
    assert.ok(doctypeOnly.startsWith('<!doctype html><head>'), doctypeOnly);
  });

  function frame(sandbox) {
    return { attrs: sandbox === undefined ? {} : { sandbox }, getAttribute(name) { return this.attrs[name] ?? null; }, setAttribute(name, value) { this.attrs[name] = value; } };
  }

  await test('frames asking for scripts are narrowed so the still canvas never runs them', () => {
    const authored = frame('allow-scripts');
    const combined = frame('allow-scripts allow-same-origin');
    const narrowed = frame('allow-scripts allow-forms allow-popups');
    const sameOrigin = frame('allow-same-origin');
    const plain = frame(undefined);
    hardenStaticFrames({ querySelectorAll: () => [authored, combined, narrowed, sameOrigin, plain] });
    assert.equal(authored.attrs.sandbox, 'allow-same-origin');
    assert.equal(combined.attrs.sandbox, 'allow-same-origin');
    // Author narrowing is kept for the live overlay that reads this attribute.
    assert.equal(narrowed.attrs.sandbox, 'allow-same-origin allow-forms allow-popups');
    assert.equal(sameOrigin.attrs.sandbox, 'allow-same-origin');
    assert.equal(plain.attrs.sandbox, undefined);
    for (const node of [authored, combined, narrowed, sameOrigin, plain]) assert.ok(!(node.attrs.sandbox ?? '').includes('allow-scripts'));
  });
} finally {
  await rm(temporary, { recursive: true, force: true });
}
