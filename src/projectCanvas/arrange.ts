import { sizeTarget } from './canvasTargets';
import type { ElementMove, Rect } from '../types';
import type { ElementEntry } from './interactionTypes';
import { unionBounds } from './camera';

export type ArrangeOperation = 'forward' | 'backward' | 'front' | 'back';

/** Reorder selected siblings as stable blocks, without mutating the input. */
export function arrangeSiblingOrder(ids: readonly string[], selected: readonly string[], operation: ArrangeOperation): string[] {
  const picked = new Set(selected);
  if (operation === 'front') return [...ids.filter(id => !picked.has(id)), ...ids.filter(id => picked.has(id))];
  if (operation === 'back') return [...ids.filter(id => picked.has(id)), ...ids.filter(id => !picked.has(id))];
  const order = [...ids];
  if (operation === 'forward') {
    for (let i = order.length - 2; i >= 0; i--) {
      if (picked.has(order[i]) && !picked.has(order[i + 1])) [order[i], order[i + 1]] = [order[i + 1], order[i]];
    }
  } else {
    for (let i = 1; i < order.length; i++) {
      if (picked.has(order[i]) && !picked.has(order[i - 1])) [order[i - 1], order[i]] = [order[i], order[i - 1]];
    }
  }
  return order;
}

export type SelectionAlign = 'left' | 'centre' | 'right' | 'top' | 'middle' | 'bottom';
export type SelectionDistribution = 'horizontal' | 'vertical';

const positionOrder = (a: ElementMove, b: ElementMove) => a.to_page_id.localeCompare(b.to_page_id)
  || (a.to_parent_id ?? '').localeCompare(b.to_parent_id ?? '') || a.index - b.index;

const moved = (entry: ElementEntry, dx: number, dy: number): ElementMove => ({
  id: entry.id, to_page_id: entry.pageId, to_parent_id: entry.parentId, index: entry.index,
  rect: { ...entry.rect, x: entry.rect.x + dx, y: entry.rect.y + dy },
  ...sizeTarget(entry),
});

function selectionGroups(entries: ElementEntry[]): ElementEntry[][] {
  const groups = new Map<string, ElementEntry[]>();
  for (const entry of entries) {
    const key = `${entry.pageId}\u0000${entry.allSizes ? '*' : entry.sizeId}`;
    const group = groups.get(key);
    if (group) group.push(entry); else groups.set(key, [entry]);
  }
  return [...groups.values()];
}

export function alignSelectionMoves(entries: ElementEntry[], edge: SelectionAlign, container: (entry: ElementEntry) => Rect): ElementMove[] {
  return selectionGroups(entries).flatMap(group => {
    const box = group.length === 1 ? container(group[0]) : unionBounds(group.map(entry => entry.world));
    return group.flatMap(entry => {
      let dx = 0; let dy = 0;
      if (edge === 'left') dx = box.x - entry.world.x;
      else if (edge === 'centre') dx = box.x + (box.width - entry.world.width) / 2 - entry.world.x;
      else if (edge === 'right') dx = box.x + box.width - entry.world.width - entry.world.x;
      else if (edge === 'top') dy = box.y - entry.world.y;
      else if (edge === 'middle') dy = box.y + (box.height - entry.world.height) / 2 - entry.world.y;
      else if (edge === 'bottom') dy = box.y + box.height - entry.world.height - entry.world.y;
      return dx || dy ? [moved(entry, dx, dy)] : [];
    });
  }).sort(positionOrder);
}

/** Keep the two outer layers fixed and give every pair the same edge-to-edge gap. */
export function distributeSelectionMoves(entries: ElementEntry[], direction: SelectionDistribution): ElementMove[] {
  const axis = direction === 'horizontal' ? 'x' : 'y';
  const extent = direction === 'horizontal' ? 'width' : 'height';
  return selectionGroups(entries).flatMap(group => {
    if (group.length < 3) return [];
    const ordered = [...group].sort((a, b) => a.world[axis] - b.world[axis] || a.index - b.index);
    const first = ordered[0]; const last = ordered[ordered.length - 1];
    const span = last.world[axis] + last.world[extent] - first.world[axis];
    const gap = (span - ordered.reduce((sum, entry) => sum + entry.world[extent], 0)) / (ordered.length - 1);
    let position = first.world[axis] + first.world[extent] + gap;
    return ordered.slice(1, -1).map(entry => {
      const delta = position - entry.world[axis];
      position += entry.world[extent] + gap;
      return moved(entry, axis === 'x' ? delta : 0, axis === 'y' ? delta : 0);
    });
  }).sort(positionOrder);
}
