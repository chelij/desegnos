import { ChangeSet, EditorState, StateEffect, StateField, Transaction, type Extension, type TransactionSpec } from '@codemirror/state';
import { invertedEffects } from '@codemirror/commands';
import { projectEditorHtml, type HtmlProjection } from './editorHtml.js';

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
