import type { RequestAttachment } from './types';
import { MAX_ATTACHMENTS, MAX_NAME_CHARS } from './generated/limits';

export const ATTACHMENT_ACCEPT = 'image/png,image/jpeg,image/webp,image/gif,text/*,.txt,.md,.csv,.json,.htm,.html';
const id = () => crypto.randomUUID().replace(/-/g, '').slice(0, 16);

/** Shared UI reader. Native commands still validate content, URLs and payload sizes. */
export async function readRequestAttachments(existing: RequestAttachment[], files: File[], links: string[] = []) {
  const attachments = [...existing];
  const errors: string[] = [];
  const add = (attachment: RequestAttachment) => {
    if (attachments.length >= MAX_ATTACHMENTS) throw new Error(`A request supports at most ${MAX_ATTACHMENTS} attachments.`);
    attachments.push(attachment);
  };
  for (const file of files) {
    try {
      if (/^image\/(png|jpeg|webp|gif)$/.test(file.type)) {
        const dataUri = await new Promise<string>((resolve, reject) => {
          const reader = new FileReader();
          reader.onload = () => resolve(String(reader.result));
          reader.onerror = () => reject(new Error(`${file.name} could not be read.`));
          reader.readAsDataURL(file);
        });
        add({ id: id(), name: file.name.slice(0, MAX_NAME_CHARS), kind: 'image', dataUri });
      } else if (file.type.startsWith('text/') || /\.(txt|md|csv|json|html?)$/i.test(file.name)) {
        const text = await file.text();
        if (text.trim()) add({ id: id(), name: file.name.slice(0, MAX_NAME_CHARS), kind: 'text', text: text.slice(0, 65536) });
      } else throw new Error(`${file.name}: only pictures (PNG, JPEG, WebP, GIF) and text files can be attached.`);
    } catch (error) { errors.push(error instanceof Error ? error.message : String(error)); }
  }
  for (const value of links) {
    try {
      const url = new URL(value);
      if (url.protocol !== 'https:' && url.protocol !== 'http:') throw new Error('Use an HTTP or HTTPS attachment address.');
      add({ id: id(), name: url.hostname.slice(0, MAX_NAME_CHARS), kind: 'link', url: value });
    } catch (error) { errors.push(error instanceof Error ? error.message : String(error)); }
  }
  return { attachments, errors: [...new Set(errors)] };
}
