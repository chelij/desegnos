import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';

const root = fileURLToPath(new URL('../', import.meta.url));
const temporary = await mkdtemp(join(root, '.selection-projection-'));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022', '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/elementTargets.ts', 'src/editorHtml.ts'], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout || String(result.error));
  const { canonicalSelections, sameTarget, sameTargets, regionTargets, unionRect } = await import(pathToFileURL(join(temporary, 'elementTargets.js')).href);
  const { projectEditorHtml, updateHtmlProjection, restoreEditorHtml, sourceOffset, sourcePosition } = await import(pathToFileURL(join(temporary, 'editorHtml.js')).href);
  const target = (id, selector = 'body > div:nth-of-type(1)') => ({ id, selector, tag: 'div', label: '' });
  const select = (id, selector, x = 0, y = 0) => ({ element: target(id, selector), rect: { x, y, width: 100, height: 60 } });

  await test('stable identity survives source and sibling changes; legacy hashes remain bound', () => {
    assert.ok(sameTarget(target('a'), { ...target('a'), selector: 'body > div:nth-of-type(4)', sourceSha256: 'changed' }));
    assert.ok(!sameTarget(target('a'), target('b')));
    assert.ok(!sameTarget(target('a'), { selector: target('a').selector }));
    assert.ok(sameTarget({ selector: 'p', sourceSha256: 'old' }, { selector: 'p', sourceSha256: 'old' }));
    assert.ok(!sameTarget({ selector: 'p', sourceSha256: 'old' }, { selector: 'p', sourceSha256: 'new' }));
    assert.ok(sameTargets([target('a'), target('b')], [target('b'), target('a')]));
    assert.deepEqual(regionTargets({ elements: [target('a')], element: target('old') }), [target('a')]);
    assert.deepEqual(regionTargets({ element: target('old') }), [target('old')]);
  });
  await test('multi-selection keeps outer nodes, removes duplicate IDs and measures signed union', () => {
    const outer = select('a', 'body > article:nth-of-type(1)', -60, 20);
    const inner = select('b', 'body > article:nth-of-type(1) > h2:nth-of-type(1)', -50, 25);
    const other = select('c', 'body > article:nth-of-type(2)', 200, -40);
    assert.deepEqual(canonicalSelections([inner, outer, other, outer]), [outer, other]);
    assert.deepEqual(unionRect([outer.rect, other.rect]), { x: -60, y: -40, width: 360, height: 120 });
  });
  await test('hidden attributes survive text, tag name and normal attribute edits', () => {
    let projection = projectEditorHtml('<h1 data-desegnos-id="abcdefgh" data-desegnos-found="https://example.com/source" class="title">Hello</h1>');
    assert.equal(projection.text, '<h1 class="title">Hello</h1>');
    projection = updateHtmlProjection(projection, projection.text.replace('Hello', 'Welcome'));
    projection = updateHtmlProjection(projection, projection.text.replace('class="title"', 'class="lead"'));
    projection = updateHtmlProjection(projection, projection.text.replace('<h1', '<h2'));
    const raw = restoreEditorHtml(projection);
    assert.ok(raw.includes('data-desegnos-id="abcdefgh"'));
    assert.ok(raw.includes('data-desegnos-found="https://example.com/source"'));
    assert.ok(raw.includes('class="lead">Welcome'));
    assert.ok(raw.startsWith('<h2'));
  });
  await test('inserting and deleting sibling code keeps each surviving element ID', () => {
    let projection = projectEditorHtml('<p data-desegnos-id="first">A</p>\n<p data-desegnos-id="second">B</p>');
    projection = updateHtmlProjection(projection, '<p>New</p>\n' + projection.text, { from: 0, to: 0 });
    let raw = restoreEditorHtml(projection);
    assert.ok(raw.startsWith('<p>New</p>\n<p data-desegnos-id="first">'));
    projection = updateHtmlProjection(projection, projection.text.replace('<p>A</p>\n', ''), { from: '<p>New</p>\n'.length, to: '<p>New</p>\n<p>A</p>\n'.length });
    raw = restoreEditorHtml(projection);
    assert.ok(!raw.includes('data-desegnos-id="first"'));
    assert.ok(raw.includes('data-desegnos-id="second"'));
  });
  await test('quoted markup, comments and raw text never become hidden attributes', () => {
    const raw = '<!-- <div data-desegnos-id="comment"> -->\n<script>let t = `<p data-desegnos-id="example">`;</script>\n<textarea>Example <p data-desegnos-id="literal"></textarea>\n<img title="A > B data-desegnos-id=example" data-desegnos-id="image" alt="Photo">';
    const projection = projectEditorHtml(raw);
    assert.ok(projection.text.includes('data-desegnos-id="comment"'));
    assert.ok(projection.text.includes('data-desegnos-id="example"'));
    assert.ok(projection.text.includes('data-desegnos-id="literal"'));
    assert.ok(projection.text.includes('title="A > B data-desegnos-id=example"'));
    assert.ok(!projection.text.includes('data-desegnos-id="image"'));
    assert.equal((restoreEditorHtml(projection).match(/data-desegnos-id="image"/g) ?? []).length, 1);
  });
  await test('native source positions translate to visible UTF-16 positions with hidden multiline metadata', () => {
    const raw = '<div\n data-desegnos-id="outer">\n  <p data-desegnos-id="inner">A😀B</p>\n</div>';
    const projection = projectEditorHtml(raw);
    const rawAt = raw.indexOf('B</p>');
    const visibleAt = projection.text.indexOf('B</p>');
    assert.equal(projection.rawToVisible(rawAt), visibleAt);
    const position = sourcePosition(projection.text, visibleAt);
    assert.equal(sourceOffset(projection.text, position.line, position.column), visibleAt);
    assert.deepEqual(position, { line: 2, column: 9 });
  });
} finally { await rm(temporary, { recursive: true, force: true }); }
