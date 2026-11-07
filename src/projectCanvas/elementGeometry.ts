import { defaultText, ELEMENT_KIND_LABELS, findElementIn } from './elementFacts';
export { ELEMENT_KIND_LABELS, defaultText, elementLabel, defaultLayerNameBase, isDefaultLayerName } from './elementFacts';
import { GRID_SPACING, MIN_SIZE, snapValue } from '../projectCanvasGeometry';
export { MIN_SIZE, snapValue, sameRect, unionBounds as unionRects, findElementIn } from '../projectCanvasGeometry';
import { canvasFont } from './fonts';
import type { Element, ElementKind, ElementLayout, ElementPadding, ElementStyle, InputType, OutputType, Page, Project, Rect } from '../types';
import type { Point } from '../projectCanvasGeometry';
import { resolveElements } from './links';
import { resolveForSize } from './sizes';

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
const sameColour = (first: string | null, second: string | null | undefined) =>
  !!first && !!second && first.trim().toLowerCase() === second.trim().toLowerCase();
export function withPickedColours(element: Element, surface?: string | null): Element {
  const style = element.style;
  const shape = element.kind === 'box' || element.kind === 'ellipse';
  // A repeated fill that matches the box the layer is drawn into would make it vanish, so the
  // default fill wins there; other layers keep the paint-program memory.
  return { ...element, style: {
    ...style,
    fill: shape && picked.fill !== undefined && !sameColour(picked.fill, surface) ? picked.fill : style.fill,
    stroke: (shape || element.kind === 'line') && picked.stroke !== undefined ? { ...style.stroke, color: picked.stroke } : style.stroke,
    textStyle: element.kind === 'text' && picked.text !== undefined ? { ...style.textStyle, color: picked.text } : style.textStyle,
  } };
}

export function darkBackground(background: string): boolean {
  const hex = background.slice(1);
  const rgb = hex.length === 3 ? hex.split('').map(value => value + value).join('') : hex;
  const channels = [0, 2, 4].map(index => {
    const value = parseInt(rgb.slice(index, index + 2), 16) / 255;
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });
  return channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722 < 0.179;
}

/** New layers on a dark boundary start with light text and lines so they stay readable. */
export function forBackground(element: Element, background: string): Element {
  if (!darkBackground(background)) return element;
  const swap = (colour: string | null) => colour === TEXT_COLOUR ? DARK_TEXT_COLOUR : colour;
  const style = element.style;
  // On a dark page the default button turns light so it does not disappear.
  if (element.kind === 'button' && style.fill === TEXT_COLOUR) return { ...element, style: { ...style, fill: SURFACE_COLOUR, textStyle: { ...style.textStyle, color: TEXT_COLOUR } } };
  return { ...element, style: { ...style, stroke: { ...style.stroke, color: swap(style.stroke.color) }, textStyle: { ...style.textStyle, color: swap(style.textStyle.color)! } } };
}
const SHAPE_FILL = '#d9d9d9';
const BORDER_COLOUR = '#d2d2d2';
const SURFACE_COLOUR = '#ffffff';

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
export const holdsChildren = (kind: ElementKind) => kind === 'box' || kind === 'group';

export const DEFAULT_TEXT_STYLE = { fontFamily: 'system-ui', size: 16, weight: 400, italic: false, lineHeight: 1.4, letterSpacing: 0, color: TEXT_COLOUR, align: 'left' as const };

export function defaultStyle(kind: ElementKind): ElementStyle {
  // A new button reads as a button: solid, rounded, with a centred label.
  if (kind === 'button') return {
    fill: TEXT_COLOUR, stroke: { color: null, width: 0 }, radius: 8, opacity: 1,
    shadow: { x: 0, y: 0, blur: 0, color: null },
    textStyle: { ...DEFAULT_TEXT_STYLE, size: 15, weight: 600, color: SURFACE_COLOUR, align: 'center' },
  };
  const control = kind === 'input' || kind === 'textarea' || kind === 'select';
  return {
    fill: kind === 'box' || kind === 'ellipse' ? SHAPE_FILL
      : kind === 'text' || kind === 'line' || kind === 'group' || kind === 'checkbox' || kind === 'radio' || kind === 'link' ? null : kind === 'code' ? '#f5f5f5' : SURFACE_COLOUR,
    stroke: { color: kind === 'text' || kind === 'group' ? null : kind === 'line' ? TEXT_COLOUR : BORDER_COLOUR, width: kind === 'line' ? 2 : control ? 1 : kind === 'box' ? 1 : 0 },
    radius: control ? 4 : 0,
    opacity: 1,
    shadow: { x: 0, y: 0, blur: 0, color: null },
    textStyle: { ...DEFAULT_TEXT_STYLE, size: kind === 'text' ? 20 : 14, color: kind === 'link' ? '#0b5cad' : TEXT_COLOUR },
  };
}

export function createElement(kind: ElementKind, rect: Rect, patch: Partial<Element> = {}): Element {
  const base = {
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
  if (kind === 'input') return { ...base, inputType: 'text', ...patch } as Element;
  if (kind === 'select') return { ...base, options: ['Option 1', 'Option 2', 'Option 3'], ...patch } as Element;
  if (kind === 'code') return { ...base, language: 'html', source: '<div>\n  \n</div>', ...patch } as Element;
  if (kind === 'link' || kind === 'video' || kind === 'audio' || kind === 'embed') return { ...base, url: 'https://example.com', ...patch } as Element;
  return { ...base, ...patch } as Element;
}

export function createTextElement(rect: Rect, text = '', output?: OutputType): Element {
  const style = defaultStyle('text');
  return createElement('text', rect, { text, style: { ...style, fill: null, stroke: { color: null, width: 0 },
    textStyle: { ...style.textStyle, size: output === 'slide' ? 48 : output === 'image' ? 40 : 20 },
    sizing: { ...style.sizing, width: rect.width > 0 ? 'fixed' : 'fit', height: 'fit' },
  } });
}

/** A copy that never shares ids, for paste and duplicate. */
export function cloneElement(element: Element): Element {
  return { ...structuredClone(element), id: crypto.randomUUID(), children: element.children.map(cloneElement) };
}

export const roundRect = (rect: Rect): Rect => ({
  x: Math.round(rect.x), y: Math.round(rect.y), width: Math.round(rect.width), height: Math.round(rect.height),
});

export const snapPoint = (point: Point, spacing = GRID_SPACING): Point => ({ x: snapValue(point.x, spacing), y: snapValue(point.y, spacing) });

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
  const index = elementIndex(elements);
  const out: ElementPathEntry[] = [];
  let entry = index.get(id);
  while (entry) {
    out.unshift(entry);
    entry = entry.parentId ? index.get(entry.parentId) : undefined;
  }
  return out.length ? out : null;
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

export function movedElementRect(rect: Rect, delta: Point, snap: boolean, spacing = GRID_SPACING): Rect {
  return snap ? { ...rect, x: snapValue(rect.x + delta.x, spacing), y: snapValue(rect.y + delta.y, spacing) } : { ...rect, x: rect.x + delta.x, y: rect.y + delta.y };
}

export function resizedElementRect(rect: Rect, corner: ElementCorner, point: Point, snap: boolean, spacing = GRID_SPACING): Rect {
  const px = snap ? snapValue(point.x, spacing) : point.x, py = snap ? snapValue(point.y, spacing) : point.y;
  const left = corner === 'nw' || corner === 'w' || corner === 'sw';
  const right = corner === 'ne' || corner === 'e' || corner === 'se';
  const top = corner === 'nw' || corner === 'n' || corner === 'ne';
  const bottom = corner === 'sw' || corner === 's' || corner === 'se';
  let x = left ? px : rect.x, y = top ? py : rect.y;
  let width = left ? rect.x + rect.width - px : right ? px - rect.x : rect.width;
  let height = top ? rect.y + rect.height - py : bottom ? py - rect.y : rect.height;
  if (width < MIN_SIZE) {
    if (left) x = rect.x + rect.width - MIN_SIZE;
    width = MIN_SIZE;
  }
  if (height < MIN_SIZE) {
    if (top) y = rect.y + rect.height - MIN_SIZE;
    height = MIN_SIZE;
  }
  return { x, y, width, height };
}

/** Shift-resize keeps the original ratio; the corner that is not dragged stays anchored. */
export function ratioResizedRect(rect: Rect, corner: ElementCorner, point: Point, snap: boolean, spacing = GRID_SPACING): Rect {
  const free = resizedElementRect(rect, corner, point, snap, spacing);
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

export type ResizeKeys = { spacing?: number; snap?: boolean; ratio?: boolean; fromCentre?: boolean };

/**
 * A handle drag with the transform keys applied: Shift keeps the ratio, Alt resizes from the
 * centre, and both combine. The caller decides what Shift means for the layer kind.
 */
export function resizedRectWithKeys(rect: Rect, corner: ElementCorner, point: Point, keys: ResizeKeys = {}): Rect {
  const next = keys.ratio
    ? ratioResizedRect(rect, corner, point, keys.snap === true, keys.spacing)
    : resizedElementRect(rect, corner, point, keys.snap === true, keys.spacing);
  if (!keys.fromCentre) return next;
  const width = Math.max(MIN_SIZE, next.width);
  const height = Math.max(MIN_SIZE, next.height);
  return { x: rect.x + rect.width / 2 - width / 2, y: rect.y + rect.height / 2 - height / 2, width, height };
}

export type DrawKeys = { spacing?: number; snap?: boolean; square?: boolean; fromCentre?: boolean };

/**
 * A shape drag with the transform keys applied: Shift constrains to a square (a circle inside
 * the square), Alt draws from the press point as the centre, and both combine.
 */
export function drawRectWithKeys(start: Point, point: Point, keys: DrawKeys = {}): Rect {
  const end = keys.snap === true ? snapPoint(point, keys.spacing) : point;
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

export function draggedElementRect(rect: Rect, start: Point, point: Point, snap: boolean, spacing = GRID_SPACING): Rect {
  return movedElementRect(rect, { x: point.x - start.x, y: point.y - start.y }, snap, spacing);
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

/** Stack or grid padding per side: the layer's own sides, else the layout's all-sides value. */
export function elementPadding(element: Element): ElementPadding {
  const base = element.layout.kind === 'free' ? 0 : element.layout.padding;
  return element.style.padding ?? { top: base, right: base, bottom: base, left: base };
}

/** The one value all four sides share, or null when they differ. */
export function uniformPadding(padding: ElementPadding): number | null {
  return padding.top === padding.right && padding.right === padding.bottom && padding.bottom === padding.left ? padding.top : null;
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
  const font = canvasFont(style);
  // Keep pending text unmeasured; callers redraw after the exact face loads.
  if (!font) return { width: 0, height: 0 };
  const lines = text.split('\n');
  const canvas = document.createElement('canvas');
  const context = canvas.getContext('2d');
  context!.font = `${style.italic && !font.italic ? 'italic ' : ''}${style.weight} ${style.size}px "${font.family}"`;
  const width = Math.max(...lines.map(line => context!.measureText(line).width + style.letterSpacing * Math.max(0, line.length - 1)), style.size);
  return { width: Math.ceil(width) + 1, height: Math.ceil(Math.max(1, lines.length) * style.size * style.lineHeight) + 1 };
}

/** Intrinsic container bounds. A Fill child contributes its stored size when its parent hugs,
 * avoiding a circular dependency between parent and child. Text metrics come from the caller. */
export function intrinsicElementSize(element: Element, textSize?: (element: Element) => { width: number; height: number }): { width: number; height: number } {
  const sizing = element.style.sizing;
  let { width, height } = element.rect;
  const measured = isTextKind(element.kind) ? textSize?.(element) : undefined;
  if (measured) {
    if (sizing?.width === 'fit' || width <= 0) width = measured.width;
    if (sizing?.height === 'fit' || height <= 0) height = measured.height;
  } else if (sizing?.width === 'fit' || sizing?.height === 'fit') {
    const children = element.children.filter(child => child.visible).map(child => ({ ...intrinsicElementSize(child, textSize), x: child.rect.x, y: child.rect.y }));
    const layout = element.layout;
    let contentWidth = 0, contentHeight = 0;
    if (layout.kind === 'free') {
      contentWidth = Math.max(0, ...children.map(child => child.x + child.width));
      contentHeight = Math.max(0, ...children.map(child => child.y + child.height));
    } else if (layout.kind === 'stack') {
      const gaps = layout.gap * Math.max(0, children.length - 1);
      const padding = elementPadding(element);
      contentWidth = layout.direction === 'row' ? children.reduce((sum, child) => sum + child.width, gaps) : Math.max(0, ...children.map(child => child.width));
      contentHeight = layout.direction === 'column' ? children.reduce((sum, child) => sum + child.height, gaps) : Math.max(0, ...children.map(child => child.height));
      contentWidth += padding.left + padding.right; contentHeight += padding.top + padding.bottom;
    } else {
      const padding = elementPadding(element);
      contentWidth = Math.max(0, ...children.map(child => child.width)) * layout.columns + layout.gap * (layout.columns - 1) + padding.left + padding.right;
      const rows: number[] = [];
      children.forEach((child, index) => { const row = Math.floor(index / layout.columns); rows[row] = Math.max(rows[row] ?? 0, child.height); });
      contentHeight = rows.reduce((sum, height) => sum + height, 0) + layout.gap * Math.max(0, rows.length - 1) + padding.top + padding.bottom;
    }
    const border = element.style.stroke.color ? element.style.stroke.width * 2 : 0;
    if (sizing?.width === 'fit') width = contentWidth + border;
    if (sizing?.height === 'fit') height = contentHeight + border;
  }
  return {
    width: Math.min(sizing?.maxWidth ?? Infinity, Math.max(sizing?.minWidth ?? 0, width)),
    height: Math.min(sizing?.maxHeight ?? Infinity, Math.max(sizing?.minHeight ?? 0, height)),
  };
}

/** Depth-first traversal shared by commands and selection. */
export function elementTree(elements: Element[]): Element[] {
  return elements.flatMap(element => [element, ...elementTree(element.children)]);
}
export function elementTreeIds(elements: Element[]): string[] {
  return elementTree(elements).map(element => element.id);
}

const indices = new WeakMap<Element[], Map<string, ElementPathEntry>>();
/** Parent and origin lookup built once for a resolved tree. */
export function elementIndex(elements: Element[]): Map<string, ElementPathEntry> {
  const cached = indices.get(elements);
  if (cached) return cached;
  const index = new Map<string, ElementPathEntry>();
  const walk = (children: Element[], parentId: string | null, base: Point, depth: number) => {
    children.forEach((element, position) => {
      const origin = { x: base.x + element.rect.x, y: base.y + element.rect.y };
      index.set(element.id, { element, parentId, index: position, origin, parentOrigin: base, depth });
      walk(element.children, element.id, origin, depth + 1);
    });
  };
  walk(elements, null, { x: 0, y: 0 }, 0);
  indices.set(elements, index);
  return index;
}
