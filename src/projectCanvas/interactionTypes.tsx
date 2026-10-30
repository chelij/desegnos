import { type BoundaryToolKind } from './boundaryDefaults';
import {
  drawnBoundaryRect, MIN_BOUNDARY_DRAW_HEIGHT, MIN_BOUNDARY_WIDTH, snapToBoundaryGrid, type Boundary,
  type BoundaryMove, type CanvasCamera, type CanvasTarget, type ElementTarget, type FrameCorner, type Point,
  type ReferenceMove, type RequestBox,
} from '../projectCanvasGeometry';
import type { Command, WorkspaceView, Element, ElementKind, ElementMove, ElementPatch, LinkedPasteInstance, NamedView, PendingRequest, Pointer, PointerSource, PointerStateView, PointerTarget, Project, Rect, ReferenceItem } from '../types';
import type { ProjectTool, RequestToolShape } from '../EditorTools';
import { type ScreenRect } from './surface';
import { type LinkCopy } from './linkedChoices';
import { type PasteMode } from '../pastePreference';
import { type ShortcutAction } from '../shortcuts';
import { type PlayState } from './play';
import { elementPath, elementParents, findPageElement, axisLockedDelta, snapValue, type ElementCorner } from './elementGeometry';

export type PanDrag = { kind: 'pan'; pointerId: number; start: Point; camera: CanvasCamera; moved: boolean; pageId: string | null };

export type MoveBoundaryDrag = { kind: 'move-boundary'; pointerId: number; start: Point; pageId: string; sizeId: string; origin: Rect; rect: Rect; moved: boolean };

export type DrawBoundaryDrag = { kind: 'draw-boundary'; pointerId: number; start: Point; rect: Rect; moved: boolean };

export type DrawRequestAreaDrag = { kind: 'draw-request-area'; pointerId: number; start: Point; current: Point; shape: 'rect' | 'ellipse' | 'freehand'; points: Point[]; keys?: { square: boolean; fromCentre: boolean }; moved: boolean };

export type DrawNoteDrag = { kind: 'draw-note'; pointerId: number; start: Point; rect: Rect; moved: boolean };

export type DrawPointerDrag = { kind: 'draw-pointer'; pointerId: number; start: Point; source: PointerSource; sourceRect: Rect; current: Point; moved: boolean };

export type ReattachPointerDrag = { kind: 'reattach-pointer'; pointerId: number; start: Point; pointer: string; end: 'from' | 'to'; original: PointerSource | PointerTarget; fixed: Point; current: Point; moved: boolean };

export type MoveTarget = { kind: 'reference'; itemId: string; origin: Rect };

export type MoveSelectionDrag = { kind: 'move-selection'; pointerId: number; start: Point; targets: MoveTarget[]; rects: Record<string, Rect>; moved: boolean; axisLocked: boolean };

export type ResizeReferenceDrag = { kind: 'resize-reference'; pointerId: number; start: Point; corner: FrameCorner; itemId: string; origin: Rect; rect: Rect; moved: boolean };

export type ElementEntry = { id: string; pageId: string; sizeId: string; allSizes: boolean; parentId: string | null; index: number; element: Element; rect: Rect; parentOrigin: Point; boundaryOrigin: Point; world: Rect };

export type ClipboardEntry = { id: string; pageId: string; parentId: string | null; index: number; element: Element; world: Rect; rect: Rect };

export type PasteTarget = { pageId: string; sizeId: string; allSizes: boolean; parentId: string | null; parentOrigin: Point; boundary: Rect };

export type PasteChoice = { target: PasteTarget; offset: number; value: PasteMode; useDefault: boolean; screen: ScreenRect };

export type LinkDecision = { copies: LinkCopy[]; error: string | null; screen: ScreenRect };

export type MoveElementsDrag = { kind: 'move-elements'; pointerId: number; start: Point; entries: ElementEntry[]; rects: Record<string, Rect>; moved: boolean; duplicate: boolean; axisLocked: boolean };

export type ResizeElementDrag = { kind: 'resize-element'; pointerId: number; start: Point; corner: ElementCorner; entry: ElementEntry; rect: Rect; moved: boolean };

export type DrawElementDrag = { kind: 'draw-element'; pointerId: number; start: Point; pointerStart: Point; pageId: string; sizeId: string; parentId: string | null; parentOrigin: Point; boundary: Rect; rect: Rect; elementKind: ElementKind; line: boolean; rotation: number; moved: boolean };

export type MarqueeDrag = { kind: 'marquee'; pointerId: number; start: Point; current: Point; pageId: string | null; additive: boolean; moved: boolean };

export type ZoomDrag = { kind: 'zoom-rect'; pointerId: number; start: Point; current: Point; zoomOut: boolean; moved: boolean };

export type PolygonDrag = { kind: 'polygon-request'; pointerId: number; points: Point[] };

export type Drag = PolygonDrag | PanDrag | MoveBoundaryDrag | DrawBoundaryDrag | DrawRequestAreaDrag | DrawNoteDrag | DrawPointerDrag | ReattachPointerDrag | MoveSelectionDrag | ResizeReferenceDrag | MoveElementsDrag | ResizeElementDrag | DrawElementDrag | MarqueeDrag | ZoomDrag;

export const CORNERS: FrameCorner[] = ['nw', 'ne', 'sw', 'se'];

export const NIL_UUID = '00000000-0000-0000-0000-000000000000';

export const DRAW_TOOLS: ProjectTool[] = ['rect', 'ellipse', 'line', 'insert'];


export const REQUEST_EDITOR_WIDTH = 280;

export const REQUEST_EDITOR_HEIGHT = 84;

export type DragInput = Pick<PointerEvent, 'pointerId' | 'clientX' | 'clientY' | 'button' | 'shiftKey' | 'altKey' | 'ctrlKey' | 'metaKey'>;

export type PreviewState = { pageId: string; url: string; stale: boolean };

export type CanvasActionOptions = { key?: string; slot?: number; repeat?: boolean };

export type ProjectCanvasHandle = { focusCanvas: () => void; fit: () => void; fitSelection: () => void; zoomStep: (direction: number) => void; zoomTo: (zoom: number) => void; focusBoundary: (pageId: string) => void; armPointer: (pointerId: string) => void; linkSelection: () => void; unlinkSelection: () => void;
  copyStyle: () => void; pasteStyle: () => void; paintFormat: () => void;
  currentCamera: () => CanvasCamera; goToView: (view: NamedView) => void;
  /** Runs one keymap action through the same path its key uses; true when the canvas took it. */
  runCanvasAction: (action: ShortcutAction, options?: CanvasActionOptions) => boolean;
  /** True when the action applies now, false when it cannot, and null when the canvas does not own it. */
  canRunCanvasAction: (action: ShortcutAction) => boolean | null; };

export type DuplicateRepeat = { key: string; offset: Point };

export type ProjectCanvasProps = {
  editProject: (command: Command | ((project: Project) => Command | null)) => Promise<WorkspaceView>;
  project: Project; openPage: (pageId: string) => void;
  createBoundary: (bounds: Rect, kind: BoundaryToolKind, sourceId?: string) => Promise<string | null>;
  savePlacements: (placements: BoundaryMove[], reference: ReferenceMove[]) => Promise<void>;
  /** Persists the camera as a numbered view, or as the next free one when slot is null. */
  saveView: (slot: number | null, camera: CanvasCamera) => void;
  selection: CanvasTarget[]; focused: string | null;
  selectBoundary: (pageId: string | null, sizeId?: string | null, additive?: boolean) => void;
  selectReference: (itemId: string, additive?: boolean) => void; selectPointer: (pointerId: string, additive?: boolean) => void; selectElement: (pageId: string, elementId: string, sizeId: string | null, additive?: boolean) => void; clearSelection: () => void;
  removeSelection: (referenceIds: string[], pointerIds: string[], requestIds: string[], elementIds?: { pageId: string; elementId: string }[]) => void;
  removeElements: (elements: { pageId: string; elementId: string }[]) => void;
  deleteBoundary: (pageId: string, sizeId: string | null) => void;
  addRequest: (request: PendingRequest) => Promise<void>;
  updateRequest: (request: PendingRequest) => Promise<void>;
  selectRequest: (requestId: string) => void;
  addReference: (item: ReferenceItem, pictureDataUri?: string) => Promise<void>;
  updateReference: (itemId: string, patch: { rect?: Rect; text?: string; url?: string; title?: string; color?: string }) => void;
  elementToReference: (pageId: string, elementId: string, rect: Rect, sizeId?: string | null) => Promise<void>;
  referenceToElement: (itemId: string, pageId: string, rect: Rect, sizeId?: string | null) => Promise<void>;
  attachToRequest: (requestId: string, files: File[], links: string[]) => void;
  insertElements: (pageId: string, parentId: string | null, index: number, elements: Element[], pictureDataUri?: string, sizeId?: string | null) => Promise<void>;
  updateElements: (pageId: string, patches: ElementPatch[]) => Promise<unknown> | void;
  moveElements: (moves: ElementMove[]) => Promise<unknown> | void;
  groupElements: (pageId: string, ids: string[]) => Promise<string | null>;
  ungroupElements: (pageId: string, id: string) => Promise<string[]>;
  linkElements: (ids: string[], sourceId: string | null) => Promise<unknown>;
  unlinkElements: (ids: string[]) => Promise<unknown>;
  pasteLinked: (items: LinkedPasteInstance[]) => Promise<unknown>;
  addPointer: (pointer: Pointer) => Promise<void>;
  /** Selects the layer or boundary behind a behaviour and puts the caret in its Behaviour field. */
  focusBehaviour: (source: PointerSource) => void;
  updatePointer: (pointerId: string, patch: { from?: PointerSource; to?: PointerTarget; comment?: string; kind?: Pointer['kind'] }) => Promise<unknown>;
  pointerStates: Record<string, PointerStateView>;
  tool: ProjectTool; insertKind: ElementKind | null; onTool: (tool: ProjectTool) => void;
  boundaryKind: BoundaryToolKind; requestShape: RequestToolShape; onRequestShape: (shape: RequestToolShape) => void; focusedSize: string | null;
  widthPreview?: { sizeId: string; width: number } | null;
  onZoom: (zoom: number) => void; onLayout: (boundaries: Boundary[]) => void;
  onPlayBoundary: (id: string) => void;
  play: PlayState; onPlayElement: (pageId: string, elementId: string) => void; onPlayExit: () => void;
  preview: PreviewState | null; previewClose: () => void; previewRefresh: () => void;
};

export const isElementTarget = (target: CanvasTarget): target is ElementTarget => target.kind === 'element';

export function elementChain(target: EventTarget | null): HTMLElement[] {
  if (!(target instanceof Element)) return [];
  const nodes: HTMLElement[] = [];
  let node = target.closest<HTMLElement>('[data-element]');
  while (node) {
    nodes.push(node);
    const boundary = node.closest('[data-boundary]');
    const parent = node.parentElement?.closest<HTMLElement>('[data-element]') ?? null;
    if (!parent || (boundary && !boundary.contains(parent))) break;
    node = parent;
  }
  return nodes;
}

export function chainPage(nodes: HTMLElement[]): string | null {
  return nodes[0]?.closest<HTMLElement>('[data-boundary]')?.dataset.boundary ?? null;
}

export function enteredGroup(project: Project, selection: CanvasTarget[], groupId: string): boolean {
  return selection.some(target => target.kind === 'element' && (target.elementId === groupId || elementParents(project, target.pageId, target.elementId).includes(groupId)));
}

export function elementTargetAt(project: Project, selection: CanvasTarget[], node: EventTarget | null): ElementTarget | null {
  const nodes = elementChain(node);
  const pageId = chainPage(nodes);
  if (!pageId) return null;
  const sizeId = nodes[0].closest<HTMLElement>('[data-size]')?.dataset.size ?? null;
  const elements = nodes.map(entry => findPageElement(project, pageId, entry.dataset.element!)).filter((entry): entry is Element => !!entry);
  if (elements.some(element => element.locked)) return null;
  for (const entry of [...nodes].reverse()) {
    const element = elements.find(item => item.id === entry.dataset.element);
    if (element?.kind === 'group' && !enteredGroup(project, selection, element.id)) return { kind: 'element', pageId, elementId: element.id, sizeId };
  }
  return { kind: 'element', pageId, elementId: nodes[0].dataset.element!, sizeId };
}

export function boxParentAt(project: Project, pageId: string, sizeId: string | null, node: EventTarget | null): { parentId: string | null; parentOrigin: Point } {
  const nodes = elementChain(node);
  const elements = nodes.map(entry => findPageElement(project, pageId, entry.dataset.element!, sizeId)).filter((entry): entry is Element => !!entry);
  const box = elements.find(element => element.kind === 'box');
  if (!box) return { parentId: null, parentOrigin: { x: 0, y: 0 } };
  const path = elementPath(project, pageId, box.id, sizeId);
  const origin = path?.[path.length - 1]?.origin ?? { x: 0, y: 0 };
  return { parentId: box.id, parentOrigin: origin };
}

export function elementAt<T extends RequestBox>(boxes: T[], point: Point): T | null {
  for (let index = boxes.length - 1; index >= 0; index--) {
    const { rect } = boxes[index];
    if (point.x >= rect.x && point.x <= rect.x + rect.width && point.y >= rect.y && point.y <= rect.y + rect.height) return boxes[index];
  }
  return null;
}

export const intersects = (first: Rect, second: Rect) => first.x < second.x + second.width && first.x + first.width > second.x && first.y < second.y + second.height && first.y + first.height > second.y;

export function elementMoveDelta(entries: ElementEntry[], start: Point, point: Point, snap: boolean, axisLock: boolean): Point {
  const travel = { x: point.x - start.x, y: point.y - start.y };
  if (!snap || !entries.length) return axisLock ? axisLockedDelta(travel) : travel;
  const first = entries[0].world;
  const delta = { x: snapValue(first.x + travel.x) - first.x, y: snapValue(first.y + travel.y) - first.y };
  if (!axisLock) return delta;
  // The locked axis stays exactly where it was; snapping only adjusts the axis being dragged.
  const axis = axisLockedDelta(travel);
  return { x: axis.x === 0 ? 0 : delta.x, y: axis.y === 0 ? 0 : delta.y };
}

export function boundaryDrawRect(start: Point, point: Point, keys: { square?: boolean; fromCentre?: boolean } = {}): Rect {
  if (!keys.square && !keys.fromCentre) return drawnBoundaryRect(start, point);
  const from = { x: snapToBoundaryGrid(start.x), y: snapToBoundaryGrid(start.y) };
  let dx = snapToBoundaryGrid(point.x) - from.x;
  let dy = snapToBoundaryGrid(point.y) - from.y;
  if (keys.square) {
    const size = Math.max(Math.abs(dx), Math.abs(dy));
    dx = dx < 0 ? -size : size;
    dy = dy < 0 ? -size : size;
  }
  if (keys.fromCentre) {
    return { x: from.x - Math.abs(dx), y: from.y - Math.abs(dy),
      width: Math.max(MIN_BOUNDARY_WIDTH, Math.abs(dx) * 2), height: Math.max(MIN_BOUNDARY_DRAW_HEIGHT, Math.abs(dy) * 2) };
  }
  return { x: Math.min(from.x, from.x + dx), y: Math.min(from.y, from.y + dy),
    width: Math.max(MIN_BOUNDARY_WIDTH, Math.abs(dx)), height: Math.max(MIN_BOUNDARY_DRAW_HEIGHT, Math.abs(dy)) };
}

export const targetKey = (targets: { pageId: string; elementId: string }[]): string =>
  targets.map(target => `${target.pageId}:${target.elementId}`).sort().join('|');

export const entryTargetKey = (entries: { pageId: string; id: string }[]): string => targetKey(entries.map(entry => ({ pageId: entry.pageId, elementId: entry.id })));

export function defaultSize(kind: ElementKind): { width: number; height: number } {
  switch (kind) {
    case 'ellipse': return { width: 160, height: 120 };
    case 'line': return { width: 160, height: 0 };
    case 'button': return { width: 160, height: 44 };
    case 'link': return { width: 120, height: 28 };
    case 'input': case 'select': return { width: 240, height: 40 };
    case 'textarea': return { width: 240, height: 100 };
    case 'checkbox': case 'radio': return { width: 160, height: 24 };
    case 'video': case 'embed': return { width: 320, height: 180 };
    case 'audio': return { width: 320, height: 64 };
    case 'code': return { width: 360, height: 160 };
    case 'image': return { width: 320, height: 200 };
    default: return { width: 240, height: 160 };
  }
}
