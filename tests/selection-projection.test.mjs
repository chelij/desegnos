import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';
import { codeMetaFixture } from './native/code-meta-fixture.mjs';
import { ChangeSet, EditorState } from '@codemirror/state';

const root = fileURLToPath(new URL('../', import.meta.url));
const temporary = await mkdtemp(join(root, '.selection-projection-'));
try {
  const compiler = fileURLToPath(new URL('bin/tsc', import.meta.resolve('typescript/package.json')));
  const result = spawnSync(process.execPath, [compiler, '--ignoreConfig', '--module', 'esnext', '--target', 'ES2022', '--moduleResolution', 'bundler', '--skipLibCheck', '--outDir', temporary, 'src/elementTargets.ts', 'src/editorHtml.ts', 'src/codeMirrorMetadata.ts'], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout || String(result.error));
  const { canonicalSelections, documentKey, nativeTarget, sameDocument, sameTarget, sameTargets, regionTargets, targetKey, unionRect } = await import(pathToFileURL(join(temporary, 'elementTargets.js')).href);
  const { projectCodeSource, projectEditorHtml, restoreEditorHtml, sourceOffset, sourcePosition } = await import(pathToFileURL(join(temporary, 'editorHtml.js')).href);
  const { metadataChanges, mapHtmlMetadata } = await import(pathToFileURL(join(temporary, 'codeMirrorMetadata.js')).href);
  const updateProjection = (projection, from, to, insert = '') => { const change = ChangeSet.of({ from, to, insert }, projection.text.length); return mapHtmlMetadata(projection, change, change.apply(EditorState.create({ doc: projection.text }).doc).toString()); };
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
  await test('element identity, keys and native payloads stay inside their document', () => {
    const site = { kind: 'siteFile', path: 'play/game.html' };
    const framed = { ...target('x'), document: site };
    const other = { ...target('x'), document: { kind: 'page', pageId: 'page-2' } };
    assert.ok(!sameTarget(framed, other));
    assert.ok(!sameTarget(framed, target('x')));
    assert.ok(sameTarget(framed, { ...framed, selector: 'body > p:nth-of-type(2)' }));
    assert.ok(sameDocument(undefined, undefined));
    assert.ok(!sameDocument(site, undefined));
    assert.equal(documentKey(site), 'site:play/game.html');
    assert.equal(documentKey({ kind: 'page', pageId: 'page-2' }), 'page:page-2');
    assert.equal(targetKey(framed), `site:play/game.html|${framed.selector}`);
    assert.deepEqual(nativeTarget(framed), target('x'));
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
    projection = updateProjection(projection, projection.text.indexOf('Hello'), projection.text.indexOf('Hello') + 5, 'Welcome');
    projection = updateProjection(projection, projection.text.indexOf('title'), projection.text.indexOf('title') + 5, 'lead');
    projection = updateProjection(projection, 1, 3, 'h2');
    const raw = restoreEditorHtml(projection);
    assert.ok(raw.includes('data-desegnos-id="abcdefgh"'));
    assert.ok(raw.includes('data-desegnos-found="https://example.com/source"'));
    assert.ok(raw.includes('class="lead">Welcome'));
    assert.ok(raw.startsWith('<h2'));
  });
  await test('inserting and deleting sibling code keeps each surviving element ID', () => {
    let projection = projectEditorHtml('<p data-desegnos-id="first">A</p>\n<p data-desegnos-id="second">B</p>');
    projection = updateProjection(projection, 0, 0, '<p>New</p>\n');
    let raw = restoreEditorHtml(projection);
    assert.ok(raw.startsWith('<p>New</p>\n<p data-desegnos-id="first">'));
    projection = updateProjection(projection, '<p>New</p>\n'.length, '<p>New</p>\n<p>A</p>\n'.length);
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
  await test('six large inline pictures never enter either editor projection and survive edits', () => {
    const { html, pictures } = codeMetaFixture();
    assert.ok(html.length > 2_000_000);
    for (const shown of [true, false]) {
      const projection = projectEditorHtml(html, shown);
      assert.ok(projection.text.length < 22_000);
      assert.doesNotMatch(projection.text, /data:image\//);
      for (const picture of pictures) assert.ok(restoreEditorHtml(projection).includes(picture));
      const edited = updateProjection(projection, 0, 0, '<!-- edit -->\n');
      for (const picture of pictures) assert.ok(restoreEditorHtml(edited).includes(picture));
    }
    const script = 'const payload = {data:123, metadata:456};';
    assert.equal(projectCodeSource(script).text, script);
    const css = `img{background:url(${pictures[0]})}`;
    const projection = projectCodeSource(css);
    assert.doesNotMatch(projection.text, /data:image\//);
    assert.equal(restoreEditorHtml(projection), css);
  });
  await test('metadata swaps use small edits and map selection through both projections', () => {
    const raw = '<main class="site" data-desegnos-id="main0001">\n<p title="Copy" data-desegnos-id="copy0001">A😀B</p>\n</main>';
    for (const source of [raw, codeMetaFixture().html, '<img data-desegnos-found="data:image/png;base64,' + 'A'.repeat(100_000) + '" src="desegnos-asset:picture">']) {
      const shown = projectEditorHtml(source, true), hidden = projectEditorHtml(source);
      for (const [before, after] of [[shown, hidden], [hidden, shown]]) {
        const changes = metadataChanges(before, after);
        assert.equal(changes.apply(EditorState.create({ doc: before.text }).doc).toString(), after.text);
        const position = before.text.indexOf('A😀B');
        if (position >= 0) assert.equal(changes.mapPos(position), after.text.indexOf('A😀B'));
        changes.iterChanges((from, to) => assert.ok(to - from < 100, 'Must not replace the document'));
      }
    }
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
