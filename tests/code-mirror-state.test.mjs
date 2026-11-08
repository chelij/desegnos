import assert from 'node:assert/strict';
import test from 'node:test';
import { compileModules } from './typescript.mjs';
import { ChangeSet, Compartment, EditorState, StateEffect, Transaction } from '@codemirror/state';
import { history, isolateHistory, redo, undo, undoDepth } from '@codemirror/commands';
import { SearchQuery, search, setSearchQuery } from '@codemirror/search';
import { html } from '@codemirror/lang-html';
import { css } from '@codemirror/lang-css';
import { javascript } from '@codemirror/lang-javascript';
import { syntaxTree } from '@codemirror/language';

const modules = await compileModules(['src/editorHtml.ts', 'src/codeMirrorMetadata.ts']);
try {
  const { htmlMetadata, metadataHistory, replaceMetadata, mapHtmlMetadata, metadataEcho } = await modules.load('codeMirrorMetadata.js');
  const { projectEditorHtml, restoreEditorHtml } = await modules.load('editorHtml.js');
  const stateFor = raw => { const projected = projectEditorHtml(raw); return EditorState.create({ doc: projected.text, extensions: [htmlMetadata.init(() => projected), metadataHistory, history(), html()] }); };
  const apply = (state, changes, isolated = false) => state.update({ changes, userEvent: 'input.type', annotations: isolated ? isolateHistory.of('full') : undefined }).state;
  const travel = (state, command) => { assert.ok(command({ state, dispatch: transaction => { state = transaction.state; } })); return state; };
  const raw = state => restoreEditorHtml(state.field(htmlMetadata));

  await test('CM exact insert ranges keep existing IDs on same-tag siblings through Undo and Redo', () => {
    let state = stateFor('<p data-desegnos-id="original">Original</p>');
    const before = raw(state);
    state = apply(state, { from: 0, insert: '<p>New</p>\n' }, true);
    assert.equal(raw(state), '<p>New</p>\n<p data-desegnos-id="original">Original</p>');
    const after = raw(state);
    state = travel(state, undo); assert.equal(raw(state), before);
    state = travel(state, redo); assert.equal(raw(state), after);
  });
  await test('grouped typing undoes in one text step and restores metadata exactly', () => {
    let state = stateFor('<h1 data-desegnos-id="heading">A</h1>');
    const before = raw(state);
    const position = state.doc.toString().indexOf('A');
    state = apply(state, { from: position, insert: 'B' });
    state = apply(state, { from: position + 1, insert: 'C' });
    assert.equal(undoDepth(state), 1);
    const after = raw(state);
    state = travel(state, undo); assert.equal(raw(state), before);
    state = travel(state, redo); assert.equal(raw(state), after);
  });
  await test('metadata, search and read-only echoes do not split adjacent fast typing', () => {
    const editable = new Compartment(), marks = StateEffect.define();
    const initial = projectEditorHtml('// Before');
    let state = EditorState.create({ doc: initial.text, extensions: [htmlMetadata.init(() => initial), metadataHistory, history(), search(), editable.of(EditorState.readOnly.of(false))] });
    for (const [index, char] of [...'\n// Immediate project save'].entries()) {
      state = state.update({ changes: { from: state.doc.length, insert: char }, userEvent: 'input.type', annotations: Transaction.time.of(1000 + index) }).state;
      state = state.update(metadataEcho(state, projectEditorHtml(state.doc.toString()))).state;
      state = state.update({ effects: editable.reconfigure(EditorState.readOnly.of(false)) }).state;
      state = state.update({ effects: marks.of([]) }).state;
      state = state.update({ effects: setSearchQuery.of(new SearchQuery({ search: '', caseSensitive: false, literal: true })) }).state;
      assert.equal(undoDepth(state), 1);
    }
    state = travel(state, undo); assert.equal(state.doc.toString(), '// Before');
  });
  await test('lagging React echoes preserve newer editor text, IDs and complete undo history', () => {
    let state = stateFor('<p data-desegnos-id="original">A</p>');
    const before = raw(state), position = state.doc.toString().indexOf('A');
    state = apply(state, { from: position, insert: 'B' });
    const lagged = state.field(htmlMetadata);
    state = apply(state, { from: position + 1, insert: 'C' });
    const newer = raw(state);
    assert.equal(metadataEcho(state, lagged), null, 'An older echo must never replace the current document or metadata');
    assert.equal(raw(state), newer); assert.equal(undoDepth(state), 1);
    state = state.update(metadataEcho(state, state.field(htmlMetadata))).state;
    assert.equal(undoDepth(state), 1);
    state = travel(state, undo); assert.equal(raw(state), before);
    state = travel(state, redo); assert.equal(raw(state), newer);
  });
  await test('multi-range changes remove only deleted tags and retain surviving IDs', () => {
    let state = stateFor('<p data-desegnos-id="first">A</p>\n<p data-desegnos-id="second">B</p>\n<p data-desegnos-id="third">C</p>');
    const text = state.doc.toString(), second = text.indexOf('<p>B');
    const before = raw(state);
    state = apply(state, [{ from: 0, to: '<p>A</p>\n'.length }, { from: second + 3, to: second + 4, insert: 'Changed' }], true);
    assert.equal(raw(state), '<p data-desegnos-id="second">Changed</p>\n<p data-desegnos-id="third">C</p>');
    state = travel(state, undo); assert.equal(raw(state), before);
  });
  await test('native assignment of IDs on save preserves text history and restored IDs on Redo', () => {
    let state = stateFor('<p data-desegnos-id="original">A</p>');
    state = apply(state, { from: 0, insert: '<p>New</p>\n' }, true);
    const saved = projectEditorHtml('<p data-desegnos-id="assigned">New</p>\n<p data-desegnos-id="original">A</p>');
    state = state.update({ effects: replaceMetadata.of(saved), annotations: Transaction.addToHistory.of(false) }).state;
    assert.equal(undoDepth(state), 1);
    state = travel(state, undo); assert.equal(raw(state), '<p data-desegnos-id="original">A</p>');
    state = travel(state, redo); assert.equal(raw(state), '<p data-desegnos-id="assigned">New</p>\n<p data-desegnos-id="original">A</p>');
  });
  await test('IDs assigned by a save rebase through later typing without moving to new siblings', () => {
    const submitted = '<p>New</p>\n<p>Original</p>';
    const native = projectEditorHtml('<p data-desegnos-id="assigned">New</p>\n<p data-desegnos-id="original">Original</p>');
    const first = ChangeSet.of({ from: 0, insert: '<p>Later</p>\n' }, submitted.length);
    const afterFirst = first.apply(EditorState.create({ doc: submitted }).doc);
    const second = ChangeSet.of({ from: afterFirst.toString().indexOf('Original'), insert: 'Changed ' }, afterFirst.length);
    const changes = first.compose(second), text = changes.apply(EditorState.create({ doc: submitted }).doc).toString();
    const projected = mapHtmlMetadata(native, changes, text);
    assert.equal(restoreEditorHtml(projected), '<p>Later</p>\n<p data-desegnos-id="assigned">New</p>\n<p data-desegnos-id="original">Changed Original</p>');
  });
  await test('each code file keeps an independent local undo history', () => {
    let script = EditorState.create({ doc: 'const n = 1;', extensions: [history(), javascript()] });
    let style = EditorState.create({ doc: 'p { color: red; }', extensions: [history(), css()] });
    script = apply(script, { from: script.doc.length, insert: '\nconsole.log(n);' }, true);
    style = apply(style, { from: style.doc.length, insert: '\nh1 { margin: 0; }' }, true);
    script = travel(script, undo); assert.equal(script.doc.toString(), 'const n = 1;');
    assert.ok(style.doc.toString().includes('margin'));
    style = travel(style, undo); assert.equal(style.doc.toString(), 'p { color: red; }');
  });
  await test('official HTML CSS and JavaScript parsers and Unicode search operate locally', () => {
    for (const [doc, extension, top] of [['<main><h1>Hello</h1></main>', html(), 'Document'], ['p { color: red; }', css(), 'StyleSheet'], ['const n = 1;', javascript(), 'Script']]) {
      const state = EditorState.create({ doc, extensions: [extension] });
      assert.equal(syntaxTree(state).topNode.name, top);
    }
    const state = EditorState.create({ doc: 'A😀B\nHELLO hello' });
    const cursor = new SearchQuery({ search: 'hello', caseSensitive: false, literal: true }).getCursor(state);
    const positions = []; while (!cursor.next().done) positions.push(cursor.value.from);
    assert.deepEqual(positions, [5, 11]);
  });
} finally { await modules.cleanup(); }
