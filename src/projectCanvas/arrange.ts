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
