import { documentKey } from './elementTargets.js';
import type { AcrossPagesMatch, DocumentTarget, ElementTarget } from './types';

/// One change across many pages stays bounded like a batch edit.
export const MAX_ACROSS_PAGES = 200;

/// The toggle state for the element currently selected. A new selection starts off.
export type AcrossPagesState = { key: string; enabled: boolean; excluded: string[] };

export const ACROSS_PAGES_OFF: AcrossPagesState = { key: '', enabled: false, excluded: [] };

/// The element a toggle belongs to; edits that keep its identity keep the toggle on.
export function acrossSelectionKey(target: ElementTarget | null | undefined): string {
  return target ? `${documentKey(target.document)}|${target.id || target.selector}` : '';
}

export function acrossPagesFor(target: ElementTarget | null | undefined, previous: AcrossPagesState): AcrossPagesState {
  const key = acrossSelectionKey(target);
  if (!key) return ACROSS_PAGES_OFF;
  return previous.key === key ? previous : { key, enabled: false, excluded: [] };
}

export function setAcrossPages(state: AcrossPagesState, enabled: boolean): AcrossPagesState {
  return { ...state, enabled };
}

/// Ticking a page in the inline list keeps it; unticking leaves that page alone.
export function toggleAcrossPage(state: AcrossPagesState, document: DocumentTarget, checked: boolean): AcrossPagesState {
  const key = documentKey(document);
  const excluded = state.excluded.filter(item => item !== key);
  return { ...state, excluded: checked ? excluded : [...excluded, key] };
}

/// The other documents the next edit reaches. Off means the selected page alone.
export function chosenAcrossPages(state: AcrossPagesState, matches: AcrossPagesMatch[]): DocumentTarget[] {
  if (!state.enabled) return [];
  return matches
    .filter(match => !state.excluded.includes(documentKey(match.document)))
    .slice(0, MAX_ACROSS_PAGES)
    .map(match => match.document);
}

export function acrossPagesLabel(count: number): string {
  return `Also on ${count} ${count === 1 ? 'page' : 'pages'}`;
}

export function acrossPagesChangedLabel(count: number): string {
  return `Changed on ${count} ${count === 1 ? 'page' : 'pages'}`;
}
