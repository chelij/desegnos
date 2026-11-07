import type { Element, ElementKind } from '../types';

export const ELEMENT_KIND_LABELS: Record<ElementKind, string> = {
  box: 'Box', group: 'Group', text: 'Text', ellipse: 'Ellipse', line: 'Line', image: 'Image',
  button: 'Button', link: 'Link', input: 'Input', textarea: 'Textarea', select: 'Select',
  checkbox: 'Checkbox', radio: 'Radio', video: 'Video', audio: 'Audio', embed: 'Embed', code: 'Code',
};

export function defaultText(kind: ElementKind): string {
  switch (kind) {
    case 'text': return 'Text';
    case 'button': return 'Button';
    case 'link': return 'Link';
    case 'input': return 'Input';
    case 'textarea': return 'Textarea';
    case 'select': return 'Option 1';
    case 'checkbox': return 'Checkbox';
    case 'radio': return 'Radio';
    default: return '';
  }
}

export function elementLabel(element: Element): string {
  const name = element.name?.trim() ?? '';
  if (name && !isDefaultLayerName(element)) return name;
  const line = element.text.trim().split('\n')[0];
  if (line && line !== defaultText(element.kind)) return line.slice(0, 80);
  if (element.kind === 'code') return `${element.language || 'Code'} code`;
  return name || line.slice(0, 80) || ELEMENT_KIND_LABELS[element.kind];
}

export function defaultLayerNameBase(kind: ElementKind): string {
  return kind === 'box' ? 'Rectangle' : ELEMENT_KIND_LABELS[kind];
}

export function isDefaultLayerName(element: Element): boolean {
  const name = element.name?.trim() ?? '';
  const base = defaultLayerNameBase(element.kind);
  const number = name.startsWith(`${base} `) ? name.slice(base.length + 1) : '';
  return /^[1-9]\d*$/.test(number);
}

export function findElementIn(elements: Element[], id: string): Element | null {
  for (const element of elements) {
    if (element.id === id) return element;
    const found = findElementIn(element.children, id);
    if (found) return found;
  }
  return null;
}
