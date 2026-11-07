import { type BoundaryToolKind, boundaryDefaults } from './boundaryDefaults';
import { type PointerEvent as ReactPointerEvent } from 'react';
import type { Boundary, Point } from '../projectCanvasGeometry';
import type { Element, ElementKind, Project, Rect } from '../types';
import { hasManySizes } from './sizes';
import { elementLocalRect, findPageElement, snapPoint } from './elementGeometry';
import { createElement, createTextElement, forBackground, withPickedColours, elementChildrenOf } from './elementGeometry';
import { DrawElementDrag, Drag, boxParentAt } from './interactionTypes';

export type PendingText = { element: Element; pageId: string; parentId: string | null; index: number; sizeId: string | null; worldRect: Rect };

export type TextSession = { pageId: string; sizeId: string; elementId: string; autoWidth: boolean; created: boolean };
export type TextEditing =
  | { kind: 'idle' }
  | { kind: 'pending'; draft: PendingText }
  | { kind: 'existing'; session: TextSession; bounds: Rect | null }
  | { kind: 'draft'; session: TextSession; bounds: Rect | null; draft: PendingText };
export type TextEditingEvent =
  | { kind: 'prepare'; draft: PendingText }
  | { kind: 'open'; session: TextSession | null }
  | { kind: 'measure'; session: TextSession; bounds: Rect }
  | { kind: 'finish' }
  | { kind: 'retry'; session: TextSession; draft: PendingText };

type Context = {
  transitionText: (event: TextEditingEvent) => void;
  setPendingImage: import("react").Dispatch<import("react").SetStateAction<{ uri: string; width: number; height: number; name: string; } | null>>;
  project: Project;
  boundaryRect: (sizeId: string) => Rect | null;
  createdElement: import("react").RefObject<string | null>;
  editingAllSizes: (pageId: string) => boolean;
  insertElements: (pageId: string, parentId: string | null, index: number, elements: Element[], pictureDataUri?: string, sizeId?: string | null) => Promise<void>;
  selectElement: (pageId: string, elementId: string, sizeId: string | null, additive?: boolean) => void;
  boundaryUnder: (point: Point) => Boundary | null;
  boundaryNear: (point: Point) => Boundary | null;
  beginPointerSession: (event: { pointerId: number; }) => void;
  drag: { current: Drag | null; };
  busy: import("react").RefObject<boolean>;
  boundaryKind: BoundaryToolKind;
  createBoundary: (bounds: Rect, kind: BoundaryToolKind) => Promise<string | null>;
  onTool: (tool: import("../EditorTools").EditorTool) => void;
  selectBoundary: (pageId: string | null, sizeId?: string | null, additive?: boolean) => void;
};

export function createDrawingActions(context: () => Context) {
  function chooseImage(file: File | undefined) {
    const { setPendingImage } = context();
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => {
      const uri = String(reader.result);
      const image = new Image();
      image.onload = () => setPendingImage({ uri, width: image.naturalWidth || 320, height: image.naturalHeight || 200, name: file.name });
      image.onerror = () => setPendingImage({ uri, width: 320, height: 200, name: file.name });
      image.src = uri;
    };
    reader.readAsDataURL(file);
  }

  function insertionPoint(pageId: string, parentId: string | null) {
    const { project } = context();
    return elementChildrenOf(project, pageId, parentId).length;
  }

  function elementRectAt(sizeId: string, parentOrigin: Point, boundary: Rect, rect: Rect): Rect {
    const { boundaryRect } = context();
    return elementLocalRect(rect, boundaryRect(sizeId) ?? boundary, parentOrigin);
  }

  async function placeElement(kind: ElementKind, pageId: string, sizeId: string, parentId: string | null, parentOrigin: Point, boundary: Rect, rect: Rect, picture?: string, rotation?: number) {
    const { project, createdElement, editingAllSizes, insertElements, selectElement, transitionText } = context();
    const page = project.pages.find(item => item.id === pageId);
    // A new shape must not repeat the fill of the box, or boundary, it is drawn into.
    const container = parentId ? findPageElement(project, pageId, parentId, sizeId) : null;
    const surface = container?.style.fill ?? page?.background ?? null;
    const element = withPickedColours(forBackground(kind === 'text' ? createTextElement(rect, '', page?.output)
      : createElement(kind, rect, kind === 'image' ? { image: null } : {}), page?.background ?? '#ffffff'), surface);
    const local = elementRectAt(sizeId, parentOrigin, boundary, rect);
    element.rect = { ...local, width: rect.width, height: rect.height };
    if (kind === 'line' && rotation !== undefined) element.rotation = rotation;
    createdElement.current = element.id;
    const singleSize = !!page && hasManySizes(page) && !editingAllSizes(pageId);
    if (kind === 'text') {
      transitionText({ kind: 'prepare', draft: { element, pageId, parentId, index: insertionPoint(pageId, parentId), sizeId, worldRect: rect } });
      return element;
    }
    await insertElements(pageId, parentId, insertionPoint(pageId, parentId), [element], picture, sizeId);
    selectElement(pageId, element.id, singleSize ? sizeId : null);
    return element;
  }

  function drawElementDrag(event: ReactPointerEvent<HTMLDivElement>, point: Point, kind: ElementKind, line: boolean): DrawElementDrag | null {
    const { boundaryUnder, boundaryNear, project, beginPointerSession, drag } = context();
    const boundary = boundaryUnder(point) ?? boundaryNear(point);
    if (!boundary) return null;
    // A press just outside the edge starts on the edge itself.
    const { rect: edge } = boundary;
    point = { x: Math.min(Math.max(point.x, edge.x), edge.x + edge.width), y: Math.min(Math.max(point.y, edge.y), edge.y + edge.height) };
    const { parentId, parentOrigin } = boxParentAt(project, boundary.pageId, boundary.sizeId, event.target);
    event.preventDefault();
    beginPointerSession(event);
    const pointerStart = point;
    point = project.gridSettings.snap && !event.altKey ? snapPoint(point, project.gridSettings.spacing) : point;
    const rect = { ...point, width: 0, height: 0 };
    drag.current = { kind: 'draw-element', pointerId: event.pointerId, start: point, pointerStart, pageId: boundary.pageId, sizeId: boundary.sizeId, parentId, parentOrigin, boundary: boundary.rect, rect, elementKind: kind, line, rotation: 0, moved: false };
    return drag.current;
  }

  async function persistDraw(rect: Rect, clicked = false) {
    const { busy, boundaryKind, createBoundary, selectBoundary, onTool } = context();
    if (clicked) rect = boundaryDefaults(context().project, boundaryKind);
    busy.current = true;
    try {
      const pageId = await createBoundary(rect, boundaryKind);
      if (pageId) { selectBoundary(pageId); onTool('select'); }
    } finally { busy.current = false; }
  }

  return { chooseImage, insertionPoint, elementRectAt, placeElement, drawElementDrag, persistDraw };
}
