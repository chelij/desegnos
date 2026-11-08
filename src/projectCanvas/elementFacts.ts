import { ELEMENT_DEFAULTS, MAX_LAYER_LABEL_CHARS } from '../generated/limits';
import type { Element, ElementKind } from '../types';

export const ELEMENT_KIND_LABELS = Object.fromEntries(Object.entries(ELEMENT_DEFAULTS).map(([kind, facts]) => [kind, facts.label])) as Record<ElementKind, string>;
export const defaultText = (kind: ElementKind): string => ELEMENT_DEFAULTS[kind].text;

export function elementLabel(element: Element): string {
  const name = element.name?.trim() ?? '';
  if (name && !isDefaultLayerName(element)) return name;
  const line = element.text.trim().split('\n')[0];
  if (line && line !== defaultText(element.kind)) return Array.from(line).slice(0, MAX_LAYER_LABEL_CHARS).join('');
  if (element.kind === 'code') return `${element.language || 'Code'} code`;
  return name || Array.from(line).slice(0, MAX_LAYER_LABEL_CHARS).join('') || ELEMENT_KIND_LABELS[element.kind];
}

export const defaultLayerNameBase = (kind: ElementKind): string => ELEMENT_DEFAULTS[kind].name;

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
