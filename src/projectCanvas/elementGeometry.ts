import type { Appearance, Element, ElementKind, ElementLayout, ElementStyle, InputType, Page, Project, Rect } from '../types';
import type { Point } from '../projectCanvasGeometry';
import { resolveElements } from './links';
import { resolveForSize } from './sizes';

export const ELEMENT_GRID = 8;
export const MIN_ELEMENT_SIZE = 1;
export type ElementCorner = 'nw' | 'n' | 'ne' | 'e' | 'se' | 's' | 'sw' | 'w';

export type ElementPathEntry = {
  element: Element;
  parentId: string | null;
  index: number;
  /** Page-local top-left of the element's own box. */
  origin: Point;
  /** Page-local origin of the box the element's rect is relative to. */
  parentOrigin: Point;
  depth: number;
};

export type ElementLocation = { pageId: string; parentId: string | null; index: number; element: Element };

const TEXT_COLOUR = '#1a1a1a';
const DARK_TEXT_COLOUR = '#f2f2f2';

/** The last colours picked in Properties; new layers start with them, like a paint program. */
const picked: { fill?: string | null; stroke?: string | null; text?: string } = {};
export function rememberColour(kind: 'fill' | 'stroke' | 'text', value: string | null) {
  if (kind === 'text') picked.text = value ?? TEXT_COLOUR;
  else picked[kind] = value;
}
export function withPickedColours(element: Element): Element {
  const style = element.style;
  const shape = element.kind === 'box' || element.kind === 'ellipse';
  return { ...element, style: {
    ...style,
    fill: shape && picked.fill !== undefined ? picked.fill : style.fill,
    stroke: (shape || element.kind === 'line') && picked.stroke !== undefined ? { ...style.stroke, color: picked.stroke } : style.stroke,
    textStyle: element.kind === 'text' && picked.text !== undefined ? { ...style.textStyle, color: picked.text } : style.textStyle,
  } };
}

/** New layers on a dark boundary start with light text and lines so they stay readable. */
export function forAppearance(element: Element, appearance: Appearance): Element {
  if (appearance !== 'dark') return element;
  const swap = (colour: string | null) => colour === TEXT_COLOUR ? DARK_TEXT_COLOUR : colour;
  const style = element.style;
  return { ...element, style: { ...style, stroke: { ...style.stroke, color: swap(style.stroke.color) }, textStyle: { ...style.textStyle, color: swap(style.textStyle.color)! } } };
}
const MUTED_COLOUR = '#6b6b6b';
const BORDER_COLOUR = '#d2d2d2';
const SURFACE_COLOUR = '#ffffff';
const MUTED_SURFACE = '#efefef';

export const ELEMENT_KIND_LABELS: Record<ElementKind, string> = {
  box: 'Box', group: 'Group', text: 'Text', ellipse: 'Ellipse', line: 'Line', image: 'Image',
  button: 'Button', link: 'Link', input: 'Input', textarea: 'Textarea', select: 'Select',
  checkbox: 'Checkbox', radio: 'Radio', video: 'Video', audio: 'Audio', embed: 'Embed', code: 'Code',
};

/** Insert groups: every element kind a website can hold, by what it is for. */
export const INSERT_GROUPS: { label: string; kinds: ElementKind[] }[] = [
  { label: 'Shapes', kinds: ['box', 'ellipse', 'line'] },
  { label: 'Text', kinds: ['text'] },
  { label: 'Media', kinds: ['image', 'video', 'audio', 'embed'] },
  { label: 'Controls', kinds: ['button', 'link', 'input', 'textarea', 'select', 'checkbox', 'radio'] },
  { label: 'Code', kinds: ['code'] },
];

export const TEXT_KINDS: ElementKind[] = ['text', 'button', 'link', 'input', 'textarea', 'select', 'checkbox', 'radio'];
export const isTextKind = (kind: ElementKind) => TEXT_KINDS.includes(kind);
export const isSingleLineText = (kind: ElementKind) => kind === 'button' || kind === 'link' || kind === 'input' || kind === 'select';
export const holdsChildren = (kind: ElementKind) => kind === 'box' || kind === 'group';

export const DEFAULT_TEXT_STYLE = { fontFamily: 'system-ui', size: 16, weight: 400, italic: false, lineHeight: 1.4, letterSpacing: 0, color: TEXT_COLOUR, align: 'left' as const };

export function defaultStyle(kind: ElementKind): ElementStyle {
  const control = kind === 'button' || kind === 'input' || kind === 'textarea' || kind === 'select';
  return {
    fill: kind === 'text' || kind === 'line' || kind === 'group' || kind === 'checkbox' || kind === 'radio' || kind === 'link' ? null : kind === 'code' ? '#f5f5f5' : SURFACE_COLOUR,
    stroke: { color: kind === 'text' || kind === 'group' ? null : kind === 'line' ? TEXT_COLOUR : BORDER_COLOUR, width: kind === 'line' ? 2 : control ? 1 : kind === 'box' ? 1 : 0 },
    radius: kind === 'button' || kind === 'input' || kind === 'select' || kind === 'textarea' ? 4 : 0,
    opacity: 1,
    shadow: { x: 0, y: 0, blur: 0, color: null },
    textStyle: { ...DEFAULT_TEXT_STYLE, size: kind === 'text' ? 20 : kind === 'button' ? 14 : 14, color: kind === 'link' ? '#0b5cad' : TEXT_COLOUR },
  };
}

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

export const DEFAULT_SIZES: Record<ElementKind, { width: number; height: number }> = {
  box: { width: 240, height: 160 }, group: { width: 0, height: 0 }, text: { width: 0, height: 0 },
  ellipse: { width: 160, height: 120 }, line: { width: 160, height: 0 }, image: { width: 320, height: 200 },
  button: { width: 160, height: 44 }, link: { width: 120, height: 28 }, input: { width: 240, height: 40 },
  textarea: { width: 240, height: 100 }, select: { width: 240, height: 40 }, checkbox: { width: 160, height: 24 },
  radio: { width: 160, height: 24 }, video: { width: 320, height: 180 }, audio: { width: 320, height: 64 },
  embed: { width: 320, height: 180 }, code: { width: 360, height: 160 },
};

export function createElement(kind: ElementKind, rect: Rect, patch: Partial<Element> = {}): Element {
  const base: Element = {
    id: crypto.randomUUID(),
    kind,
    name: null,
    rect: roundRect(rect),
    rotation: 0,
    visible: true,
    locked: false,
    style: defaultStyle(kind),
    text: defaultText(kind),
    image: null,
    url: null,
    layout: { kind: 'free' },
    children: [],
    meta: { requests: [], context: [] },
  };
  if (kind === 'input') return { ...base, inputType: 'text', ...patch };
  if (kind === 'select') return { ...base, options: ['Option 1', 'Option 2', 'Option 3'], ...patch };
  if (kind === 'code') return { ...base, language: 'html', source: '<div>\n  \n</div>', ...patch };
  if (kind === 'link' || kind === 'video' || kind === 'audio' || kind === 'embed') return { ...base, url: 'https://example.com', ...patch };
  return { ...base, ...patch };
}

export function createTextElement(rect: Rect, text = ''): Element {
  return createElement('text', rect, { text, style: { ...defaultStyle('text'), fill: null, stroke: { color: null, width: 0 } } });
}

/** A copy that never shares ids, for paste and duplicate. */
export function cloneElement(element: Element): Element {
  return { ...structuredClone(element), id: crypto.randomUUID(), children: element.children.map(cloneElement) };
}

export const roundRect = (rect: Rect): Rect => ({
  x: Math.round(rect.x), y: Math.round(rect.y), width: Math.round(rect.width), height: Math.round(rect.height),
});

export const snapValue = (value: number) => Math.round(value / ELEMENT_GRID) * ELEMENT_GRID;
export const snapPoint = (point: Point): Point => ({ x: snapValue(point.x), y: snapValue(point.y) });
export const sameRect = (first: Rect, second: Rect) => first.x === second.x && first.y === second.y && first.width === second.width && first.height === second.height;

export function unionRects(rects: Rect[]): Rect {
  if (!rects.length) return { x: 0, y: 0, width: 0, height: 0 };
  const left = Math.min(...rects.map(rect => rect.x));
  const top = Math.min(...rects.map(rect => rect.y));
  const right = Math.max(...rects.map(rect => rect.x + rect.width));
  const bottom = Math.max(...rects.map(rect => rect.y + rect.height));
  return { x: left, y: top, width: right - left, height: bottom - top };
}

export function findElementIn(elements: Element[], id: string): Element | null {
  for (const element of elements) {
    if (element.id === id) return element;
    const found = findElementIn(element.children, id);
    if (found) return found;
  }
  return null;
}

/** A boundary's layers as the canvas draws them in one size: linked copies resolved, then that
 * size's overrides. Without a size, the base size, the design every other size inherits. */
export function pageElements(project: Project, pageId: string, sizeId?: string | null): Element[] {
  const page = project.pages.find(entry => entry.id === pageId);
  if (!page) return [];
  const size = sizeId ? page.sizes.find(entry => entry.id === sizeId) : page.sizes[0];
  return size ? resolveForSize(project, page, size.id) : resolveElements(project, page.elements);
}

export function findPageElement(project: Project, pageId: string, id: string, sizeId?: string | null): Element | null {
  return findElementIn(pageElements(project, pageId, sizeId), id);
}

export function findElementPage(project: Project, id: string): Page | null {
  return project.pages.find(page => findElementIn(page.elements, id) || findElementIn(pageElements(project, page.id), id)) ?? null;
}

export function elementLocation(project: Project, pageId: string, id: string): ElementLocation | null {
  const elements = pageElements(project, pageId);
  if (!elements.length && !project.pages.some(entry => entry.id === pageId)) return null;
  const walk = (children: Element[], parentId: string | null): ElementLocation | null => {
    for (const [index, element] of children.entries()) {
      if (element.id === id) return { pageId, parentId, index, element };
      const found = walk(element.children, element.id);
      if (found) return found;
    }
    return null;
  };
  return walk(elements, null);
}

export function anyElementLocation(project: Project, id: string): ElementLocation | null {
  for (const page of project.pages) {
    const found = elementLocation(project, page.id, id);
    if (found) return found;
  }
  return null;
}

/** Root to element, with page-local origins, so drags, inserts and outlines agree. */
export function elementPath(project: Project, pageId: string, id: string, sizeId?: string | null): ElementPathEntry[] | null {
  if (!project.pages.some(entry => entry.id === pageId)) return null;
  const elements = pageElements(project, pageId, sizeId);
  const out: ElementPathEntry[] = [];
  const walk = (elements: Element[], parentId: string | null, base: Point, depth: number): boolean => {
    for (const [index, element] of elements.entries()) {
      const origin = { x: base.x + element.rect.x, y: base.y + element.rect.y };
      out.push({ element, parentId, index, origin, parentOrigin: base, depth });
      if (element.id === id) return true;
      if (element.children.length && walk(element.children, element.id, origin, depth + 1)) return true;
      out.pop();
    }
    return false;
  };
  return walk(elements, null, { x: 0, y: 0 }, 0) ? out : null;
}

export function elementParents(project: Project, pageId: string, id: string): string[] {
  const path = elementPath(project, pageId, id);
  return path ? path.slice(0, -1).map(entry => entry.element.id) : [];
}

export function elementChildrenOf(project: Project, pageId: string, parentId: string | null): Element[] {
  const elements = pageElements(project, pageId);
  if (!parentId) return elements;
  return findElementIn(elements, parentId)?.children ?? [];
}

export function elementSiblings(project: Project, pageId: string, id: string): Element[] {
  const location = elementLocation(project, pageId, id);
  return location ? elementChildrenOf(project, pageId, location.parentId) : [];
}

export function elementWorldRect(project: Project, pageId: string, id: string, boundaryOrigin: Point): Rect | null {
  const path = elementPath(project, pageId, id);
  if (!path?.length) return null;
  const { origin, element } = path[path.length - 1];
  return { x: boundaryOrigin.x + origin.x, y: boundaryOrigin.y + origin.y, width: element.rect.width, height: element.rect.height };
}

export function elementParentWorldOrigin(project: Project, pageId: string, id: string, boundaryOrigin: Point): Point {
  const path = elementPath(project, pageId, id);
  const entry = path?.[path.length - 1];
  return entry ? { x: boundaryOrigin.x + entry.parentOrigin.x, y: boundaryOrigin.y + entry.parentOrigin.y } : boundaryOrigin;
}

export function elementBoundsHeight(elements: Element[], minimum: number): number {
  const bottom = elements.reduce((value, element) => Math.max(value, elementBottom(element)), 0);
  return Math.max(minimum, Math.ceil(bottom));
}

function elementBottom(element: Element): number {
  const own = element.rect.y + element.rect.height;
  const children = element.children.reduce((value, child) => Math.max(value, element.rect.y + elementBottom(child)), 0);
  return Math.max(own, children);
}

/** The canvas rect of an element rect measured inside a boundary and its parent box. */
export function elementCanvasRect(rect: Rect, boundaryOrigin: Point, parentOrigin: Point): Rect {
  return { x: rect.x + boundaryOrigin.x + parentOrigin.x, y: rect.y + boundaryOrigin.y + parentOrigin.y, width: rect.width, height: rect.height };
}

/** A canvas rect from an element rect measured inside a boundary and its parent box. */
export function elementLocalRect(world: Rect, boundaryOrigin: Point, parentOrigin: Point): Rect {
  return { x: world.x - boundaryOrigin.x - parentOrigin.x, y: world.y - boundaryOrigin.y - parentOrigin.y, width: world.width, height: world.height };
}

/** Move a layer to another box or boundary while it keeps the same spot on the canvas. */
export function reparentedElementRect(
  rect: Rect,
  fromBoundary: Point,
  fromParent: Point,
  toBoundary: Point,
  toParent: Point,
): Rect {
  return elementLocalRect(elementCanvasRect(rect, fromBoundary, fromParent), toBoundary, toParent);
}

export function movedElementRect(rect: Rect, delta: Point, snap: boolean): Rect {
  return snap ? { ...rect, x: snapValue(rect.x + delta.x), y: snapValue(rect.y + delta.y) } : { ...rect, x: rect.x + delta.x, y: rect.y + delta.y };
}

export function resizedElementRect(rect: Rect, corner: ElementCorner, point: Point, snap: boolean): Rect {
  const px = snap ? snapValue(point.x) : point.x, py = snap ? snapValue(point.y) : point.y;
  const left = corner === 'nw' || corner === 'w' || corner === 'sw';
  const right = corner === 'ne' || corner === 'e' || corner === 'se';
  const top = corner === 'nw' || corner === 'n' || corner === 'ne';
  const bottom = corner === 'sw' || corner === 's' || corner === 'se';
  let x = left ? px : rect.x, y = top ? py : rect.y;
  let width = left ? rect.x + rect.width - px : right ? px - rect.x : rect.width;
  let height = top ? rect.y + rect.height - py : bottom ? py - rect.y : rect.height;
  if (width < MIN_ELEMENT_SIZE) {
    if (left) x = rect.x + rect.width - MIN_ELEMENT_SIZE;
    width = MIN_ELEMENT_SIZE;
  }
  if (height < MIN_ELEMENT_SIZE) {
    if (top) y = rect.y + rect.height - MIN_ELEMENT_SIZE;
    height = MIN_ELEMENT_SIZE;
  }
  return { x, y, width, height };
}

/** Shift-resize keeps the original ratio; the corner that is not dragged stays anchored. */
export function ratioResizedRect(rect: Rect, corner: ElementCorner, point: Point, snap: boolean): Rect {
  const free = resizedElementRect(rect, corner, point, snap);
  if (!rect.width || !rect.height) return free;
  const ratio = rect.width / rect.height;
  const widthDrives = corner === 'n' || corner === 's' ? false : free.width / ratio >= free.height;
  const width = widthDrives ? free.width : free.height * ratio;
  const height = widthDrives ? free.width / ratio : free.height;
  const left = corner === 'nw' || corner === 'w' || corner === 'sw';
  const top = corner === 'nw' || corner === 'n' || corner === 'ne';
  return {
    x: left ? rect.x + rect.width - width : rect.x,
    y: top ? rect.y + rect.height - height : rect.y,
    width, height,
  };
}

export type ResizeKeys = { snap?: boolean; ratio?: boolean; fromCentre?: boolean };

/**
 * A handle drag with the transform keys applied: Shift keeps the ratio, Alt resizes from the
 * centre, and both combine. The caller decides what Shift means for the layer kind.
 */
export function resizedRectWithKeys(rect: Rect, corner: ElementCorner, point: Point, keys: ResizeKeys = {}): Rect {
  const next = keys.ratio
    ? ratioResizedRect(rect, corner, point, keys.snap === true)
    : resizedElementRect(rect, corner, point, keys.snap === true);
  if (!keys.fromCentre) return next;
  const width = Math.max(MIN_ELEMENT_SIZE, next.width);
  const height = Math.max(MIN_ELEMENT_SIZE, next.height);
  return { x: rect.x + rect.width / 2 - width / 2, y: rect.y + rect.height / 2 - height / 2, width, height };
}

export type DrawKeys = { snap?: boolean; square?: boolean; fromCentre?: boolean };

/**
 * A shape drag with the transform keys applied: Shift constrains to a square (a circle inside
 * the square), Alt draws from the press point as the centre, and both combine.
 */
export function drawRectWithKeys(start: Point, point: Point, keys: DrawKeys = {}): Rect {
  const end = keys.snap === true ? snapPoint(point) : point;
  let dx = end.x - start.x, dy = end.y - start.y;
  if (keys.square) {
    const size = Math.max(Math.abs(dx), Math.abs(dy));
    dx = dx < 0 ? -size : size;
    dy = dy < 0 ? -size : size;
  }
  if (!keys.fromCentre) return rectFromPoints(start, { x: start.x + dx, y: start.y + dy });
  return { x: start.x - Math.abs(dx), y: start.y - Math.abs(dy), width: Math.abs(dx) * 2, height: Math.abs(dy) * 2 };
}

/** Shift-move locks to whichever axis the pointer has travelled furthest along. */
export function axisLockedDelta(delta: Point): Point {
  return Math.abs(delta.x) >= Math.abs(delta.y) ? { x: delta.x, y: 0 } : { x: 0, y: delta.y };
}

/** A line's other end with its angle snapped to `step` degrees, keeping its length. */
export function snappedLineEnd(start: Point, end: Point, step = 15): Point {
  const dx = end.x - start.x, dy = end.y - start.y;
  const length = Math.hypot(dx, dy);
  const radians = (step * Math.PI) / 180;
  if (!radians || !length) return end;
  const angle = Math.round(Math.atan2(dy, dx) / radians) * radians;
  return { x: start.x + Math.cos(angle) * length, y: start.y + Math.sin(angle) * length };
}

export function lineEnd(start: Point, end: Point, constrain: boolean): Point {
  return constrain ? snappedLineEnd(start, end, 45) : end;
}

export function rectFromPoints(start: Point, end: Point): Rect {
  return { x: Math.min(start.x, end.x), y: Math.min(start.y, end.y), width: Math.abs(end.x - start.x), height: Math.abs(end.y - start.y) };
}

export function draggedElementRect(rect: Rect, start: Point, point: Point, snap: boolean): Rect {
  return movedElementRect(rect, { x: point.x - start.x, y: point.y - start.y }, snap);
}

/** A grid or stack box lays its children out itself; its own rect still places it. */
export function layoutChildren(layout: ElementLayout): boolean {
  return layout.kind !== 'free';
}

export const pictureUri = (project: Project, id: string | null | undefined): string | null => {
  if (!id) return null;
  const stored = project.pictures?.[id];
  if (stored) return stored;
  const identity = project.importedIdentity;
  return identity?.assets.find(asset => asset.id === id)?.dataUri ?? identity?.photos?.find(photo => photo.id === id)?.dataUri ?? null;
};

export const pictureName = (project: Project, id: string | null | undefined): string => {
  if (!id) return '';
  const identity = project.importedIdentity;
  return identity?.assets.find(asset => asset.id === id)?.name ?? identity?.photos?.find(photo => photo.id === id)?.name ?? `Picture ${id.slice(0, 8)}`;
};

export function elementLabel(element: Element): string {
  if (element.name?.trim()) return element.name.trim();
  if (element.kind === 'code') return `${element.language || 'Code'} code`;
  const line = element.text.trim().split('\n')[0];
  return line ? line.slice(0, 80) : ELEMENT_KIND_LABELS[element.kind];
}

export function elementKindLabel(element: Element): string {
  if (element.kind === 'input') return `${element.inputType ?? 'text'} input`;
  return ELEMENT_KIND_LABELS[element.kind];
}

export const inputTypes: InputType[] = ['text', 'email', 'password', 'number', 'search', 'tel', 'url', 'date', 'time', 'color', 'range', 'file'];

export function commonValue<T>(values: T[]): T | undefined {
  if (!values.length) return undefined;
  return values.every(value => JSON.stringify(value) === JSON.stringify(values[0])) ? values[0] : undefined;
}

export function measureText(text: string, style: Element['style']['textStyle']): { width: number; height: number } {
  const lines = text.split('\n');
  const canvas = document.createElement('canvas');
  const context = canvas.getContext('2d');
  context!.font = `${style.italic ? 'italic ' : ''}${style.weight} ${style.size}px ${style.fontFamily}`;
  const width = Math.max(...lines.map(line => context!.measureText(line).width + style.letterSpacing * Math.max(0, line.length - 1)), style.size);
  return { width: Math.ceil(width) + 1, height: Math.ceil(Math.max(1, lines.length) * style.size * style.lineHeight) + 1 };
}
