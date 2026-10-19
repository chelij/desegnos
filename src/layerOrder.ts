import type { LayerDirection } from './types';

export function reorderedLayers<T>(layers: T[], selected: T, direction: LayerDirection): T[] | null {
  const index = layers.indexOf(selected);
  if (index < 0) return null;
  const destination = direction === 'front' ? layers.length - 1 : direction === 'back' ? 0 : index + (direction === 'forward' ? 1 : -1);
  if (destination < 0 || destination >= layers.length || destination === index) return null;
  const next = [...layers]; next.splice(index, 1); next.splice(destination, 0, selected); return next;
}
