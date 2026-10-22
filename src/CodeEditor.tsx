import { forwardRef, useImperativeHandle, useLayoutEffect, useRef } from 'react';
import { Annotation, ChangeSet, Compartment, EditorState, StateEffect, StateField, Transaction, type Range } from '@codemirror/state';
import { Decoration, drawSelection, EditorView, highlightActiveLineGutter, keymap, lineNumbers, type DecorationSet } from '@codemirror/view';
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
import { bracketMatching, HighlightStyle, indentOnInput, indentUnit, syntaxHighlighting } from '@codemirror/language';
import { search, SearchQuery, searchKeymap, setSearchQuery } from '@codemirror/search';
import { html } from '@codemirror/lang-html';
import { css } from '@codemirror/lang-css';
import { javascript } from '@codemirror/lang-javascript';
import { tags } from '@lezer/highlight';
import { htmlMetadata, metadataChanges, mapHtmlMetadata, metadataEcho, metadataHistory, replaceMetadata } from './codeMirrorMetadata';
import type { HtmlProjection } from './editorHtml';
import type { SiteContent } from './types';

type Tab = keyof SiteContent;
export type CodeLineMark = { line: number; className: string; title?: string };
export type CodeSaveSnapshot = { text: string; projection: HtmlProjection; changes: ChangeSet };
export type CodeEditorHandle = { reveal: (line: number) => void; snapshot: () => CodeSaveSnapshot; rebase: (projection: HtmlProjection, snapshot: CodeSaveSnapshot) => HtmlProjection; release: (snapshot: CodeSaveSnapshot) => void; replaceSource: (projection: HtmlProjection) => void };
const externalUpdate = Annotation.define<boolean>();
const setLineMarks = StateEffect.define<CodeLineMark[]>();
const lineMarks = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, transaction) {
    const effect = transaction.effects.find(effect => effect.is(setLineMarks));
    if (!effect) return value.map(transaction.changes);
    const ranges: Range<Decoration>[] = [];
    for (const mark of (effect.value as CodeLineMark[]).slice(0, 2000)) {
      if (mark.line < 1 || mark.line > transaction.newDoc.lines) continue;
      ranges.push(Decoration.line({ attributes: { class: `code-line ${mark.className}`, ...(mark.title ? { title: mark.title } : {}) } }).range(transaction.newDoc.line(mark.line).from));
    }
    return Decoration.set(ranges, true);
  },
  provide: field => EditorView.decorations.from(field),
});
const colours = HighlightStyle.define([
  { tag: [tags.tagName, tags.keyword, tags.bool, tags.null], class: 'tok-tag' },
  { tag: [tags.attributeName, tags.propertyName, tags.variableName], class: 'tok-attr' },
  { tag: [tags.string, tags.attributeValue, tags.number], class: 'tok-string' },
  { tag: tags.comment, class: 'tok-comment' },
]);

export const CodeEditor = forwardRef<CodeEditorHandle, {
  language: Tab; label: string; text: string; projection: HtmlProjection; hiddenMetadata: boolean; readOnly: boolean; active: boolean;
  marks: CodeLineMark[]; query: string; focusLine: number | null;
  change: (text: string, projection: HtmlProjection) => void; cursor: (offset: number) => void; flush: () => void; saveProject: () => void; find: () => void;
}>(function CodeEditor(props, ref) {
  const host = useRef<HTMLDivElement>(null);
  const editor = useRef<EditorView | null>(null);
  const latest = useRef(props); latest.current = props;
  const snapshots = useRef(new Set<CodeSaveSnapshot>());
  const editable = useRef(new Compartment());
  const undoHistory = useRef(new Compartment());
  const representation = useRef(props.hiddenMetadata);
  const cachedMarks = useRef('');
  function makeState(projection = latest.current.projection) {
    const current = latest.current;
    return EditorState.create({ doc: projection.text, extensions: [
      htmlMetadata.init(() => projection), undoHistory.current.of([metadataHistory, history()]), lineMarks,
      current.language === 'html' ? html() : current.language === 'css' ? css() : javascript(),
      lineNumbers(), highlightActiveLineGutter(), drawSelection(), bracketMatching(), indentOnInput(), indentUnit.of('  '), syntaxHighlighting(colours), search(),
      editable.current.of([EditorState.readOnly.of(current.readOnly), EditorView.editable.of(!current.readOnly)]),
      EditorView.contentAttributes.of({ 'aria-label': current.label, role: 'textbox', 'aria-multiline': 'true', spellcheck: 'false', 'data-testid': `source-editor-${current.language}` }),
      keymap.of([
        { key: 'Mod-s', run: () => { latest.current.saveProject(); return true; }, stopPropagation: true },
        { key: 'Mod-f', run: () => { latest.current.find(); return true; }, stopPropagation: true },
        indentWithTab, ...historyKeymap, ...defaultKeymap, ...searchKeymap,
      ]),
      EditorView.domEventHandlers({ blur: () => { latest.current.flush(); } }),
      EditorView.updateListener.of(update => {
        for (const transaction of update.transactions) {
          if (transaction.docChanged) for (const snapshot of snapshots.current) snapshot.changes = snapshot.changes.compose(transaction.changes);
        }
        if (update.docChanged && !update.transactions.every(transaction => transaction.annotation(externalUpdate))) latest.current.change(update.state.doc.toString(), update.state.field(htmlMetadata));
        if (update.selectionSet && !update.docChanged && update.view.hasFocus) latest.current.cursor(update.state.selection.main.head);
      }),
    ] });
  }
  useLayoutEffect(() => {
    editor.current = new EditorView({ parent: host.current!, state: makeState() });
    return () => { editor.current?.destroy(); editor.current = null; snapshots.current.clear(); };
  }, []);
  useLayoutEffect(() => {
    const view = editor.current; if (!view) return;
    const changedRepresentation = representation.current !== props.hiddenMetadata;
    if (changedRepresentation) {
      snapshots.current.clear();
      const changes = metadataChanges(view.state.field(htmlMetadata), props.projection);
      // Reset representation-specific undo effects without replacing the view or
      // destroying its content DOM. CodeMirror maps selection and scroll anchors.
      view.dispatch({ effects: undoHistory.current.reconfigure([]), annotations: externalUpdate.of(true) });
      view.dispatch({ changes, effects: [replaceMetadata.of(props.projection), undoHistory.current.reconfigure([metadataHistory, history()])], annotations: [Transaction.addToHistory.of(false), externalUpdate.of(true)] });
      representation.current = props.hiddenMetadata; cachedMarks.current = '';
    } else if (!snapshots.current.size) {
      // A React echo can lag behind newer native input. Only acknowledge matching
      // text; authoritative source replacement is explicit below.
      const echo = metadataEcho(view.state, props.projection);
      if (echo) view.dispatch({ ...echo, annotations: [Transaction.addToHistory.of(false), externalUpdate.of(true)] });
    }
    view.dispatch({ effects: editable.current.reconfigure([EditorState.readOnly.of(props.readOnly), EditorView.editable.of(!props.readOnly)]), annotations: externalUpdate.of(true) });
    const marks = JSON.stringify(props.marks);
    if (marks !== cachedMarks.current) { view.dispatch({ effects: setLineMarks.of(props.marks), annotations: externalUpdate.of(true) }); cachedMarks.current = marks; }
    view.dispatch({ effects: setSearchQuery.of(new SearchQuery({ search: props.query, caseSensitive: false, literal: true })), annotations: externalUpdate.of(true) });
    if (props.active) view.requestMeasure();
  }, [props.text, props.projection, props.hiddenMetadata, props.readOnly, props.active, props.marks, props.query]);
  function reveal(line: number) {
    const view = editor.current; if (!view) return;
    view.dispatch({ effects: EditorView.scrollIntoView(view.state.doc.line(Math.min(view.state.doc.lines, Math.max(1, line))).from, { y: 'center' }), annotations: externalUpdate.of(true) });
  }
  useLayoutEffect(() => { if (props.focusLine !== null && props.active && !editor.current?.hasFocus) reveal(props.focusLine); }, [props.focusLine, props.active]);
  useImperativeHandle(ref, () => ({
    reveal,
    snapshot() {
      const state = editor.current!.state;
      const snapshot = { text: state.doc.toString(), projection: state.field(htmlMetadata), changes: ChangeSet.empty(state.doc.length) };
      snapshots.current.add(snapshot); return snapshot;
    },
    rebase(projection, snapshot) {
      const view = editor.current; if (!view) return projection;
      const mapped = mapHtmlMetadata(projection, snapshot.changes, view.state.doc.toString());
      view.dispatch({ effects: replaceMetadata.of(mapped), annotations: [Transaction.addToHistory.of(false), externalUpdate.of(true)] });
      return mapped;
    },
    release(snapshot) { snapshots.current.delete(snapshot); },
    replaceSource(projection) {
      const view = editor.current; if (!view) return;
      if (view.state.doc.toString() === projection.text) {
        view.dispatch({ effects: replaceMetadata.of(projection), annotations: [Transaction.addToHistory.of(false), externalUpdate.of(true)] });
      } else {
        snapshots.current.clear(); view.setState(makeState(projection));
        view.dispatch({ effects: [setLineMarks.of(latest.current.marks), setSearchQuery.of(new SearchQuery({ search: latest.current.query, caseSensitive: false, literal: true }))], annotations: externalUpdate.of(true) });
        cachedMarks.current = JSON.stringify(latest.current.marks);
      }
    },
  }));
  return <div ref={host} className="code-editor-host" hidden={!props.active} data-language={props.language} />;
});
