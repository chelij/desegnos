import { ChangeSet, EditorState, StateEffect, StateField, Transaction, type Extension, type TransactionSpec } from '@codemirror/state';
import { invertedEffects } from '@codemirror/commands';
import { projectEditorHtml, restoreEditorHtml, type HtmlProjection } from './editorHtml.js';

export const replaceMetadata = StateEffect.define<HtmlProjection>();

// CodeMirror gives exact edit ranges, including multi-cursor and undo changes.
// Never infer an edit by comparing repeated tags in the resulting strings.
export function mapHtmlMetadata(previous: HtmlProjection, changes: ChangeSet, text: string): HtmlProjection {
  const anchors = previous.anchors.flatMap(anchor => {
    let removed = false;
    changes.iterChangedRanges((from, to) => { if (from < anchor.at && to > anchor.at) removed = true; });
    return removed ? [] : [{ ...anchor, at: changes.mapPos(anchor.at, 1) }];
  });
  return { text, anchors, rawToVisible: offset => offset };
}

export const htmlMetadata = StateField.define<HtmlProjection>({
  create: state => projectEditorHtml(state.doc.toString()),
  update(value, transaction) {
    const explicit = transaction.effects.filter(effect => effect.is(replaceMetadata)).at(-1);
    if (explicit) return explicit.value as HtmlProjection;
    return transaction.docChanged ? mapHtmlMetadata(value, transaction.changes, transaction.newDoc.toString()) : value;
  },
});

export const metadataHistory: Extension = invertedEffects.of(transaction =>
  transaction.docChanged || transaction.effects.some(effect => effect.is(replaceMetadata))
    ? [replaceMetadata.of(transaction.startState.field(htmlMetadata))] : []);

// React may acknowledge an older local edit after CodeMirror already accepted
// newer input. Such an echo cannot replace text or assign older metadata.
export function metadataEcho(state: EditorState, projection: HtmlProjection): TransactionSpec | null {
  return state.doc.toString() === projection.text
    ? { effects: replaceMetadata.of(projection), annotations: Transaction.addToHistory.of(false) } : null;
}

// Toggle using small edits in the existing view. setState tears down every line
// and its native accessibility/selection objects. Avoid that during a toggle.
export function metadataChanges(previous: HtmlProjection, next: HtmlProjection): ChangeSet {
  const raw = previous.source ?? restoreEditorHtml(previous);
  if (raw !== (next.source ?? restoreEditorHtml(next))) return ChangeSet.of({ from: 0, to: previous.text.length, insert: next.text }, previous.text.length);
  const old = previous.replacements ?? [], updated = next.replacements ?? [];
  const key = (item: { from: number; to: number; insert: string }) => `${item.from}:${item.to}:${item.insert}`;
  const oldKeys = new Set(old.map(key)), nextKeys = new Set(updated.map(key));
  const spans = [...old.filter(item => !nextKeys.has(key(item))), ...updated.filter(item => !oldKeys.has(key(item)))].sort((a, b) => a.from - b.from);
  const merged: { from: number; to: number }[] = [];
  for (const span of spans) {
    const last = merged.at(-1);
    // A data URI can itself be inside a metadata attribute. Replace that whole
    // span once rather than emitting overlapping asset/attribute changes.
    if (last && span.from <= last.to) last.to = Math.max(last.to, span.to);
    else merged.push({ from: span.from, to: span.to });
  }
  const edits = merged.map(({ from, to }) => ({ from: previous.rawToVisible(from), to: previous.rawToVisible(to), insert: next.text.slice(next.rawToVisible(from), next.rawToVisible(to)) }));
  return ChangeSet.of(edits, previous.text.length);
}
