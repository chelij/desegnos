import type { Rect } from '../types';
import type { Point } from './camera';

// Copying layers leaves a marker on the system clipboard. A paste whose text is not that
// marker comes from another app, so its content is placed instead of Desegnos' layers.
const CLIPBOARD_MARKER = 'desegnos-layers:';

/** Pasted pictures keep their natural size but never grow wider than this. */
export const MAX_PASTED_IMAGE_WIDTH = 480;
export const PASTED_NOTE_WIDTH = 240;
export const PASTED_NOTE_HEIGHT = 140;

export const isClipboardMarker = (text: string) => text.startsWith(CLIPBOARD_MARKER);

export function newClipboardMarker(): string {
  return `${CLIPBOARD_MARKER}${crypto.randomUUID()}`;
}

export function pastedImageRect(point: Point, width: number, height: number): Rect {
  const naturalWidth = Math.max(1, Math.round(width) || 1);
  const naturalHeight = Math.max(1, Math.round(height) || 1);
  const scale = Math.min(1, MAX_PASTED_IMAGE_WIDTH / naturalWidth);
  return {
    x: Math.round(point.x), y: Math.round(point.y),
    width: Math.max(1, Math.round(naturalWidth * scale)), height: Math.max(1, Math.round(naturalHeight * scale)),
  };
}

export const pastedNoteRect = (point: Point): Rect => ({
  x: Math.round(point.x), y: Math.round(point.y), width: PASTED_NOTE_WIDTH, height: PASTED_NOTE_HEIGHT,
});

/** Pictures on the clipboard arrive either as item files or as files, depending on the platform. */
export function pastedImageFiles(data: DataTransfer | null): File[] {
  if (!data) return [];
  const items = Array.from(data.items ?? [])
    .filter(item => item.kind === 'file' && item.type.startsWith('image/'))
    .map(item => item.getAsFile())
    .filter((file): file is File => !!file);
  if (items.length) return items;
  return Array.from(data.files ?? []).filter(file => file.type.startsWith('image/'));
}

export type PastedImage = { uri: string; width: number; height: number; name: string };

export function readPastedImage(file: File): Promise<PastedImage | null> {
  return new Promise(resolve => {
    const reader = new FileReader();
    reader.onerror = () => resolve(null);
    reader.onload = () => {
      const uri = String(reader.result ?? '');
      if (!uri.startsWith('data:image/')) { resolve(null); return; }
      const name = file.name || 'Pasted image';
      const image = new Image();
      image.onload = () => resolve({ uri, width: image.naturalWidth || 320, height: image.naturalHeight || 200, name });
      image.onerror = () => resolve({ uri, width: 320, height: 200, name });
      image.src = uri;
    };
    reader.readAsDataURL(file);
  });
}
