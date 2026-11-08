import type { Command, Pointer, PointerSource, PointerStateKind, Project, Rect } from '../types';
import { pointerEndpointRect, type PointerWorld } from './pointerGeometry';

/** The two ends of a behaviour pointer are the same layer or boundary. */
function sameEndpoints(first: PointerSource, second: PointerSource): boolean {
  if (first.kind === 'element' && second.kind === 'element') {
    return first.pageId === second.pageId && first.elementId === second.elementId;
  }
  return first.kind === 'boundary' && second.kind === 'boundary' && first.pageId === second.pageId;
}

/** A behaviour is an ordinary pointer whose target is its own source: the comment says what it does. */
export function isBehaviour(pointer: Pointer): boolean {
  if (pointer.to.kind === 'reference' || pointer.to.kind === 'url') return false;
  return pointer.kind === 'change' && sameEndpoints(pointer.from, pointer.to);
}

/** The behaviour pointer that belongs to one layer or boundary, or null when it has none. */
export function behaviourFor(project: Project, source: PointerSource): Pointer | null {
  return (project.pointers ?? []).find(pointer => isBehaviour(pointer) && sameEndpoints(pointer.from, source)) ?? null;
}

/** One source's behaviour key, so the canvas, the dock and the pointer tool agree on identity. */
export function behaviourSourceKey(source: PointerSource): string {
  return source.kind === 'element' ? `element:${source.pageId}:${source.elementId}` : `boundary:${source.pageId}`;
}

function copySource(source: PointerSource): PointerSource {
  return source.kind === 'element'
    ? { kind: 'element', pageId: source.pageId, elementId: source.elementId, sizeId: source.sizeId ?? null }
    : { kind: 'boundary', pageId: source.pageId, sizeId: source.sizeId ?? null };
}

/** A new behaviour pointer, created the way the Pointer tool creates its pointers. */
export function behaviourPointer(source: PointerSource, comment: string): Pointer {
  return { id: crypto.randomUUID(), from: copySource(source), to: copySource(source), comment, kind: 'change', wiring: null };
}

/** Typing, changing or clearing a behaviour as one edit: add, update or remove. */
export function behaviourCommand(existing: Pointer | null, source: PointerSource, comment: string): Command | null {
  const text = comment.trim();
  if (!text) return existing ? { type: 'remove_pointers', ids: [existing.id] } : null;
  if (!existing) return { type: 'add_pointer', pointer: behaviourPointer(source, text) };
  return existing.comment === text ? null : { type: 'update_pointer', pointer_id: existing.id, comment: text };
}

/** Behaviour pointers draw as badges on their own layer or boundary, never as a line. */
export type BehaviourEntry = { pointer: Pointer; rect: Rect };

export function behaviourEntries(pointers: Pointer[], world: PointerWorld): BehaviourEntry[] {
  return pointers.flatMap(pointer => {
    if (!isBehaviour(pointer)) return [];
    const rect = pointerEndpointRect(pointer.from, world);
    return rect ? [{ pointer, rect }] : [];
  });
}

/** The layer rows that carry a behaviour, keyed by page and layer. */
export function behaviourLayerKeys(pointers: Pointer[]): Set<string> {
  const keys = new Set<string>();
  for (const pointer of pointers) {
    if (pointer.from.kind === 'element' && isBehaviour(pointer)) keys.add(`${pointer.from.pageId}:${pointer.from.elementId}`);
  }
  return keys;
}

/** The words the dock shows beside a behaviour, using the pointer's own state. */
export function behaviourStateLabel(state: PointerStateKind): string {
  if (state === 'wired') return 'Built';
  if (state === 'unwired' || state === 'needsGenerating' || state === 'needsExporting') return 'Runs after Preview or Export';
  return '';
}

/** Behaviours and non-simple changes only run as exported code, so Play previews them instead. */
export function playShouldPreview(project: Project, pageId: string): boolean {
  const page = project.pages.find(item => item.id === pageId);
  if (!page || (page.output !== 'webpage')) return false;
  return (project.pointers ?? []).some(pointer => pointer.from.pageId === pageId
    && (isBehaviour(pointer) || (pointer.kind === 'change' && !pointer.play)));
}
