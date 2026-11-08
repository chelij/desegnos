import type { Rect } from '../types';

type RequestSelection = { pageId: string; elementId: string; sizeId: string | null };

export type RequestBox = RequestSelection & { rect: Rect };

/** A selected layer names the size it was clicked in; `sizeId: null` means every linked size. */
export type ElementTarget = { kind: 'element'; pageId: string; elementId: string; sizeId: string | null };

export type CanvasTarget = { kind: 'reference'; itemId: string } | { kind: 'pointer'; pointerId: string } | { kind: 'request'; requestId: string } | ElementTarget;

export const canvasTargetKey = (target: CanvasTarget): string => target.kind === 'pointer' ? `pointer:${target.pointerId}`
  : target.kind === 'request' ? `request:${target.requestId}`
  : target.kind === 'element' ? `element:${target.pageId}:${target.elementId}:${target.sizeId ?? 'all'}` : `reference:${target.itemId}`;

export const selectedPointers = (selection: CanvasTarget[]): string[] => selection.flatMap(item => item.kind === 'pointer' ? [item.pointerId] : []);

// Shift or Ctrl/Cmd toggles one item; a plain click replaces the selection.
export function toggleCanvasTarget(selection: CanvasTarget[], target: CanvasTarget, additive: boolean): CanvasTarget[] {
  if (!additive) return [target];
  const key = canvasTargetKey(target);
  return selection.some(item => canvasTargetKey(item) === key) ? selection.filter(item => canvasTargetKey(item) !== key) : [...selection, target];
}

export const selectedReference = (selection: CanvasTarget[]): string[] => selection.flatMap(item => item.kind === 'reference' ? [item.itemId] : []);

export const selectedElementTargets = (selection: CanvasTarget[]): ElementTarget[] => selection.flatMap(item => item.kind === 'element' ? [item] : []);

/** The patch a selection's edits use: one size's override, or all sizes together. */
export const sizeTarget = (target: { sizeId?: string | null; allSizes?: boolean }): { size_id?: string | null; all_sizes?: boolean } =>
  target.allSizes || target.sizeId === null ? { all_sizes: true } : { size_id: target.sizeId };
