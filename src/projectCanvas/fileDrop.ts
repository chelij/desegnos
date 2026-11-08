import type { NativeFile } from './surface';
import { MAX_PROMPT_CHARS } from '../generated/limits';

/** Text dropped from the file manager is stored as a note or layer, so keep it readable. */
const MAX_DROPPED_TEXT = MAX_PROMPT_CHARS;

export type DroppedFileKind = 'image' | 'text' | 'other';

/** Only what the native drop reader accepts reaches here; html and zip attach to requests. */
export function droppedFileKind(file: NativeFile): DroppedFileKind | null {
  const extension = file.name.split('.').pop()?.toLowerCase() ?? '';
  if (file.mime.startsWith('image/')) return 'image';
  if (file.mime === 'application/zip') return 'other';
  if (extension === 'txt' || extension === 'md' || extension === 'csv' || extension === 'json') return 'text';
  if (extension === 'html' || extension === 'htm') return 'other';
  return null;
}

/** The native reader sends base64; a data URI is already in the form layers expect. */
export function droppedDataUri(file: NativeFile): string {
  return file.data.startsWith('data:') ? file.data : `data:${file.mime};base64,${file.data}`;
}

function droppedBytes(file: NativeFile): Uint8Array<ArrayBuffer> {
  const base64 = file.data.startsWith('data:') ? file.data.slice(file.data.indexOf(',') + 1) : file.data;
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index++) bytes[index] = binary.charCodeAt(index);
  return bytes;
}

/** A File for the request attachment path, without reading the file again from disk. */
export function droppedFile(file: NativeFile): File {
  return new File([droppedBytes(file)], file.name, { type: file.mime });
}

export function droppedText(file: NativeFile): string {
  return new TextDecoder().decode(droppedBytes(file)).slice(0, MAX_DROPPED_TEXT);
}

/** Natural picture size, with the same fallback the paste path uses when decoding fails. */
export function droppedImageSize(uri: string): Promise<{ width: number; height: number }> {
  return new Promise(resolve => {
    const image = new Image();
    image.onload = () => resolve({ width: image.naturalWidth || 320, height: image.naturalHeight || 200 });
    image.onerror = () => resolve({ width: 320, height: 200 });
    image.src = uri;
  });
}
