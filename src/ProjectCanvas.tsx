import { forwardRef, useEffect, useImperativeHandle, useMemo, useRef, useState, type CSSProperties, type DragEvent as ReactDragEvent, type KeyboardEvent as ReactKeyboardEvent, type PointerEvent as ReactPointerEvent } from 'react';
import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { isTextField } from './WorkspaceSurface';
import { automaticLinks, boundaryAt, boundaryKey, boundaryMoves, canvasRectFromPage, canvasTargetKey, dotGrid, drawnBoundaryRect, fitCamera, insideAnyBoundary, layoutBoundaries, MAX_ZOOM, MIN_BOUNDARY_DRAW_HEIGHT, MIN_BOUNDARY_WIDTH, pointerAt, POINTER_HIT_PX, referenceFrameRect, referenceItemAt, referenceMoves, referenceNoteRect, movedReferenceRect, nearestEdgeAnchors, pageRectFromCanvas, panCamera, pointerRoute, pushApartBoundaries, pushApartReference, resizedReferenceRect, sameBoundaries, samePointerEndpoint, selectedPointers, snapToBoundaryGrid, viewportWorldRect, zoomCameraAt, type Boundary, type BoundaryMove, type CanvasCamera, type CanvasTarget, type ElementTarget, type FrameCorner, type Point, type ReferenceBox, type ReferenceMove, type RequestBox } from './projectCanvasGeometry';
import { steppedZoom } from './canvasGeometry';
import type { BoundaryKind, Element, ElementKind, ElementMove, ElementPatch, LinkedPasteInstance, NamedView, Page, PendingRequest, Pointer, PointerSource, PointerStateView, PointerTarget, Project, Rect, ReferenceItem, RequestArea, RequestShape, RequestTarget } from './types';
import type { ProjectTool, RequestToolShape } from './EditorTools';
import { BoundaryLabels } from './projectCanvas/boundaryLabels';
import { BoundaryLayer } from './projectCanvas/boundaries';
import { coveredTargets, dedupeTargets, elementBoxes, elementBoxesNow, pendingRequests, requestAreaContains, requestShapeBounds, requestTargetKey, RequestBoxEditor, RequestLayer } from './projectCanvas/requests';
import { ReferenceLayer, ReferenceNoteEditor, shownReference } from './projectCanvas/reference';
import { pointerEntries, pointerWorld, PointerComments, PointerHandles, PointerLines } from './projectCanvas/pointers';
import { centre, screenRect, useCanvasCamera, type NativeDrag, type ScreenRect } from './projectCanvas/surface';
import { Minimap } from './projectCanvas/Minimap';
import { contentFitsViewport, overviewRects } from './projectCanvas/minimap';
import { ElementSelectionOverlay, ElementTextEditor, textEditorScreen } from './projectCanvas/elements';
import { LinkChoiceRow, PasteChoiceRow, PlaceholderChoiceRow, type LinkCopy } from './projectCanvas/linkedChoices';
import { copySummary, linkNeedsChoice, nestedLayerIds, resolveElements, sameStructure } from './projectCanvas/links';
import { pasteDefault, setPasteDefault, usePasteDefault, type PasteMode } from './pastePreference';
import { baseSizeId, hasManySizes, resolveForSize } from './projectCanvas/sizes';
import { elementLocalRect } from './projectCanvas/elementGeometry';
import { isClipboardMarker, newClipboardMarker, pastedImageFiles, pastedImageRect, pastedNoteRect, readPastedImage } from './projectCanvas/clipboardPaste';
import { droppedDataUri, droppedFile, droppedFileKind, droppedImageSize, droppedText } from './projectCanvas/fileDrop';
import { Notice } from './ProblemBanner';
import { shortcutFor } from './shortcuts';
import { playIsActive, type PlayState } from './projectCanvas/play';
import {
  cloneElement, createElement, createTextElement, forAppearance, withPickedColours, elementBoundsHeight, elementChildrenOf, elementPath, elementParents, findElementIn, findPageElement, axisLockedDelta, drawRectWithKeys, rectFromPoints, resizedRectWithKeys, snappedLineEnd, snapPoint, snapValue,
  type ElementCorner,
} from './projectCanvas/elementGeometry';

type PanDrag = { kind: 'pan'; pointerId: number; start: Point; camera: CanvasCamera; moved: boolean; pageId: string | null };
type MoveBoundaryDrag = { kind: 'move-boundary'; pointerId: number; start: Point; pageId: string; sizeId: string; origin: Rect; rect: Rect; moved: boolean };
type DrawBoundaryDrag = { kind: 'draw-boundary'; pointerId: number; start: Point; rect: Rect; moved: boolean };
type DrawRequestAreaDrag = { kind: 'draw-request-area'; pointerId: number; start: Point; current: Point; shape: 'rect' | 'ellipse' | 'freehand'; points: Point[]; moved: boolean };
type DrawNoteDrag = { kind: 'draw-note'; pointerId: number; start: Point; rect: Rect; moved: boolean };
type DrawPointerDrag = { kind: 'draw-pointer'; pointerId: number; start: Point; source: PointerSource; sourceRect: Rect; current: Point; moved: boolean };
type ReattachPointerDrag = { kind: 'reattach-pointer'; pointerId: number; start: Point; pointer: string; end: 'from' | 'to'; original: PointerSource | PointerTarget; fixed: Point; current: Point; moved: boolean };
type MoveTarget = { kind: 'reference'; itemId: string; origin: Rect };
type MoveSelectionDrag = { kind: 'move-selection'; pointerId: number; start: Point; targets: MoveTarget[]; rects: Record<string, Rect>; moved: boolean; axisLocked: boolean };
type ResizeReferenceDrag = { kind: 'resize-reference'; pointerId: number; start: Point; corner: FrameCorner; itemId: string; origin: Rect; rect: Rect; moved: boolean };
type ElementEntry = { id: string; pageId: string; sizeId: string; allSizes: boolean; parentId: string | null; index: number; element: Element; rect: Rect; parentOrigin: Point; boundaryOrigin: Point; world: Rect };
type ClipboardEntry = { id: string; pageId: string; parentId: string | null; index: number; element: Element; world: Rect; rect: Rect };
type PasteTarget = { pageId: string; sizeId: string; allSizes: boolean; parentId: string | null; parentOrigin: Point; boundary: Rect };
type PasteChoice = { target: PasteTarget; offset: number; value: PasteMode; useDefault: boolean; screen: ScreenRect };
type LinkDecision = { copies: LinkCopy[]; error: string | null; screen: ScreenRect };
type MoveElementsDrag = { kind: 'move-elements'; pointerId: number; start: Point; entries: ElementEntry[]; rects: Record<string, Rect>; moved: boolean; duplicate: boolean; axisLocked: boolean };
type ResizeElementDrag = { kind: 'resize-element'; pointerId: number; start: Point; corner: ElementCorner; entry: ElementEntry; rect: Rect; moved: boolean };
type DrawElementDrag = { kind: 'draw-element'; pointerId: number; start: Point; pageId: string; sizeId: string; parentId: string | null; parentOrigin: Point; boundary: Rect; rect: Rect; elementKind: ElementKind; line: boolean; rotation: number; moved: boolean };
type MarqueeDrag = { kind: 'marquee'; pointerId: number; start: Point; current: Point; pageId: string | null; additive: boolean; moved: boolean };
type ZoomDrag = { kind: 'zoom-rect'; pointerId: number; start: Point; current: Point; zoomOut: boolean; moved: boolean };
type Drag = PanDrag | MoveBoundaryDrag | DrawBoundaryDrag | DrawRequestAreaDrag | DrawNoteDrag | DrawPointerDrag | ReattachPointerDrag | MoveSelectionDrag | ResizeReferenceDrag | MoveElementsDrag | ResizeElementDrag | DrawElementDrag | MarqueeDrag | ZoomDrag;

const CORNERS: FrameCorner[] = ['nw', 'ne', 'sw', 'se'];
const NIL_UUID = '00000000-0000-0000-0000-000000000000';
const DRAW_TOOLS: ProjectTool[] = ['rect', 'ellipse', 'line', 'insert'];
// A click with the Boundary tool places its default boundary instead of doing nothing.
const CLICK_BOUNDARY_SIZE = { width: 480, height: 360 };
const REQUEST_EDITOR_WIDTH = 280;
const REQUEST_EDITOR_HEIGHT = 84;
/** The pointer fields a drag reads, shared by React's synthetic events and native window ones. */
type DragInput = Pick<PointerEvent, 'pointerId' | 'clientX' | 'clientY' | 'button' | 'shiftKey' | 'altKey' | 'ctrlKey' | 'metaKey'>;

export type PreviewState = { pageId: string; url: string; stale: boolean };
export type ProjectCanvasHandle = { focusCanvas: () => void; fit: () => void; fitSelection: () => void; zoomStep: (direction: number) => void; zoomTo: (zoom: number) => void; focusBoundary: (pageId: string) => void; armPointer: (pointerId: string) => void; linkSelection: () => void; unlinkSelection: () => void;
  copyStyle: () => void; pasteStyle: () => void; paintFormat: () => void;
  currentCamera: () => CanvasCamera; goToView: (view: NamedView) => void; };

/** A repeated duplicate: the selection that was just made, and how far it sat from its source. */
type DuplicateRepeat = { key: string; offset: Point };

export type ProjectCanvasProps = {
  project: Project; openPage: (pageId: string) => void;
  createBoundary: (bounds: Rect, kind: BoundaryKind, moves: BoundaryMove[], reference: ReferenceMove[]) => Promise<string | null>;
  savePlacements: (placements: BoundaryMove[], reference: ReferenceMove[]) => Promise<void>;
  selection: CanvasTarget[]; focused: string | null;
  selectBoundary: (pageId: string | null, sizeId?: string | null, additive?: boolean) => void;
  selectReference: (itemId: string, additive?: boolean) => void; selectPointer: (pointerId: string, additive?: boolean) => void; selectElement: (pageId: string, elementId: string, sizeId: string | null, additive?: boolean) => void; clearSelection: () => void;
  removeSelection: (referenceIds: string[], pointerIds: string[], requestIds: string[], elementIds?: { pageId: string; elementId: string }[]) => void;
  removeElements: (elements: { pageId: string; elementId: string }[], ask?: boolean) => void;
  deleteBoundary: (pageId: string, sizeId: string | null) => void;
  addRequest: (request: PendingRequest) => Promise<void>;
  updateRequest: (request: PendingRequest) => Promise<void>;
  selectRequest: (requestId: string) => void;
  addReference: (item: ReferenceItem, pictureDataUri?: string) => Promise<void>;
  updateReference: (itemId: string, patch: { rect?: Rect; text?: string; url?: string; title?: string; color?: string }) => void;
  elementToReference: (pageId: string, elementId: string, rect: Rect) => Promise<void>;
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
  updatePointer: (pointerId: string, patch: { from?: PointerSource; to?: PointerTarget; comment?: string }) => Promise<unknown>;
  pointerStates: Record<string, PointerStateView>;
  tool: ProjectTool; insertKind: ElementKind | null; onTool: (tool: ProjectTool) => void;
  boundaryKind: BoundaryKind; requestShape: RequestToolShape; onRequestShape: (shape: RequestToolShape) => void; focusedSize: string | null;
  onZoom: (zoom: number) => void; onLayout: (boundaries: Boundary[]) => void;
  play: PlayState; onPlayElement: (pageId: string, elementId: string) => void; onPlayExit: () => void;
  preview: PreviewState | null; previewClose: () => void; previewRefresh: () => void;
};

const isElementTarget = (target: CanvasTarget): target is ElementTarget => target.kind === 'element';

function elementChain(target: EventTarget | null): HTMLElement[] {
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

function chainPage(nodes: HTMLElement[]): string | null {
  return nodes[0]?.closest<HTMLElement>('[data-boundary]')?.dataset.boundary ?? null;
}

function enteredGroup(project: Project, selection: CanvasTarget[], groupId: string): boolean {
  return selection.some(target => target.kind === 'element' && (target.elementId === groupId || elementParents(project, target.pageId, target.elementId).includes(groupId)));
}

/** A click selects the topmost layer in the size boundary it was clicked in. */
function elementTargetAt(project: Project, selection: CanvasTarget[], node: EventTarget | null): ElementTarget | null {
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

function boxParentAt(project: Project, pageId: string, sizeId: string | null, node: EventTarget | null): { parentId: string | null; parentOrigin: Point } {
  const nodes = elementChain(node);
  const elements = nodes.map(entry => findPageElement(project, pageId, entry.dataset.element!, sizeId)).filter((entry): entry is Element => !!entry);
  const box = elements.find(element => element.kind === 'box');
  if (!box) return { parentId: null, parentOrigin: { x: 0, y: 0 } };
  const path = elementPath(project, pageId, box.id, sizeId);
  const origin = path?.[path.length - 1]?.origin ?? { x: 0, y: 0 };
  return { parentId: box.id, parentOrigin: origin };
}

function elementAt<T extends RequestBox>(boxes: T[], point: Point): T | null {
  for (let index = boxes.length - 1; index >= 0; index--) {
    const { rect } = boxes[index];
    if (point.x >= rect.x && point.x <= rect.x + rect.width && point.y >= rect.y && point.y <= rect.y + rect.height) return boxes[index];
  }
  return null;
}

const intersects = (first: Rect, second: Rect) => first.x < second.x + second.width && first.x + first.width > second.x && first.y < second.y + second.height && first.y + first.height > second.y;

export const ProjectCanvas = forwardRef<ProjectCanvasHandle, ProjectCanvasProps>(function ProjectCanvas({
  project, openPage, createBoundary, savePlacements, selection, focused, selectBoundary, selectReference, selectPointer, selectElement, clearSelection,
  removeSelection, removeElements, deleteBoundary, addReference, updateReference, elementToReference, referenceToElement, attachToRequest,
  addRequest, updateRequest, selectRequest,
  insertElements, updateElements, moveElements, groupElements, ungroupElements, linkElements, unlinkElements, pasteLinked,
  addPointer, updatePointer, pointerStates, tool, insertKind, onTool, boundaryKind, requestShape, onRequestShape, focusedSize, onZoom, onLayout,
  play, onPlayElement, onPlayExit, preview, previewClose, previewRefresh,
}, ref) {
  const surface = useRef<HTMLDivElement>(null);
  const drag = useRef<Drag | null>(null);
  const busy = useRef(false);
  const created = useRef<string | null>(null);
  const createdItem = useRef<string | null>(null);
  const createdPointer = useRef<string | null>(null);
  const createdElement = useRef<string | null>(null);
  const lastPointer = useRef<Point | null>(null);
  const nativeDrop = useRef<{ id: string; x: number; y: number } | null>(null);
  const nativeOver = useRef<{ id: string; x: number; y: number } | null>(null);
  const dropNoticeId = useRef(0);
  const imagePicker = useRef<HTMLInputElement>(null);
  const clipboard = useRef<ClipboardEntry[]>([]);
  // The marker these layers left on the system clipboard; null when it could not be written.
  const clipboardMark = useRef<string | null>(null);
  const pasteCount = useRef(0);
  const pasteMode = usePasteDefault();
  // Format painter: the last copied style, and whether the next click applies it.
  const copiedStyle = useRef<Element['style'] | null>(null);
  const [painterArmed, setPainterArmed] = useState(false);
  // Smart duplicate: what the last duplicate was, so Ctrl/Cmd+D can repeat its offset.
  const duplicateRepeat = useRef<DuplicateRepeat | null>(null);
  const [pasteChoice, setPasteChoice] = useState<PasteChoice | null>(null);
  const [placeholderChoice, setPlaceholderChoice] = useState<{ pageId: string; sizeId: string; elementId: string; screen: ScreenRect } | null>(null);
  const [linkChoice, setLinkChoice] = useState<LinkDecision | null>(null);
  const [boundaries, setBoundaries] = useState<Boundary[]>([]);
  const { camera, setCamera, panning, setPanning, spaceHeld, space, zoomHeld, zoom, touched, fit, zoomStep, zoomTo, worldPoint, currentCamera, animateTo, stopTransition } = useCanvasCamera(surface, boundaries, project.id);
  // The minimap hides itself while the whole canvas fits; the shortcut can force it on or off.
  const [minimapPreference, setMinimapPreference] = useState<'auto' | 'on' | 'off'>('auto');
  const [moving, setMoving] = useState<{ pageId: string; sizeId: string; rect: Rect } | null>(null);
  const [draft, setDraft] = useState<Rect | null>(null);
  const [draftArea, setDraftArea] = useState<RequestShape | null>(null);
  const [requestDraft, setRequestDraft] = useState<{ targets: RequestTarget[]; area: RequestArea | null; screen: ScreenRect } | null>(null);
  const [polygonDraft, setPolygonDraft] = useState<Point[] | null>(null);
  const [draftNote, setDraftNote] = useState<Rect | null>(null);
  const [draftElement, setDraftElement] = useState<{ rect: Rect; line: boolean; rotation: number } | null>(null);
  const [marquee, setMarquee] = useState<Rect | null>(null);
  const [zoomRect, setZoomRect] = useState<Rect | null>(null);
  const [pointerDraft, setPointerDraft] = useState<{ from: Point; to: Point } | null>(null);
  const [armedPointer, setArmedPointer] = useState<{ pointerId: string; end: 'from' | 'to' } | null>(null);
  const [movingItems, setMovingItems] = useState<Record<string, Rect>>({});
  const [movingElements, setMovingElements] = useState<Record<string, Rect>>({});
  // A drag outlives a single pointer event. The window owns motion and release, so a failed
  // pointer capture, a retargeted event or an overlay layer cannot strand an active drag.
  const dragSession = useRef<{ move: (event: PointerEvent) => void; end: (event: PointerEvent) => void } | null>(null);
  const dragHandlers = useRef<{ move: (event: DragInput) => void; end: (event: DragInput) => void } | null>(null);
  const pasteHandler = useRef<(data: DataTransfer | null, target: EventTarget | null) => boolean>(() => false);
  const [editing, setEditing] = useState<{ pageId: string; sizeId: string; elementId: string; autoWidth: boolean; created: boolean } | null>(null);
  const [pendingImage, setPendingImage] = useState<{ uri: string; width: number; height: number; name: string } | null>(null);
  const [viewSize, setViewSize] = useState({ width: 0, height: 0 });
  // The minimap draws boundaries and reference; it stays hidden while all of it fits on screen.
  const minimapRects = overviewRects(boundaries, project.reference);
  const minimapFits = viewSize.width >= 1 && contentFitsViewport(minimapRects, viewportWorldRect(camera, viewSize));
  const minimapVisible = viewSize.width >= 1 && (minimapPreference === 'on' || (minimapPreference === 'auto' && !minimapFits));
  // The boundary (or the canvas bed) under a native file drag, and the last drop message.
  const [dropHover, setDropHover] = useState<{ key: string; rect: Rect | null } | null>(null);
  const [dropNotice, setDropNotice] = useState<{ id: number; message: string; variant: 'info' | 'warning' } | null>(null);

  // Drop and bridge listeners need the latest canvas state without re-registering on every pan.
  const latest = useRef({ camera, boundaries, project, selection, focused, addReference, attachToRequest, selectPointer, selectReference, selectElement, clearSelection, removeElements, insertElements, updateElements, moveElements, groupElements, ungroupElements, tool, insertKind, editing, play, onPlayElement });
  latest.current = { camera, boundaries, project, selection, focused, addReference, attachToRequest, selectPointer, selectReference, selectElement, clearSelection, removeElements, insertElements, updateElements, moveElements, groupElements, ungroupElements, tool, insertKind, editing, play, onPlayElement };
  dragHandlers.current = { move: pointerMove, end: pointerUp };
  pasteHandler.current = pasteClipboard;

  const pageKey = project.pages.map(page => `${page.id}:${page.sizes.map(size => `${size.id}:${size.label}:${size.width}:${size.height}:${size.kind}:${size.x}:${size.y}:${elementBoundsHeight(resolveForSize(project, page, size.id), size.height)}`).join(',')}:${page.elements.length}`).join('|');
  useEffect(() => {
    const laid = layoutBoundaries(project.pages.map(page => ({ id: page.id, sizes: page.sizes.map(size => ({
      ...size, height: Math.max(size.height, elementBoundsHeight(resolveForSize(project, page, size.id), size.height)),
    })) })), project.homePageId);
    setBoundaries(current => sameBoundaries(current, laid) ? current : laid);
  }, [pageKey, project.homePageId]);
  useEffect(() => { onLayout(boundaries); }, [boundaries, onLayout]);
  useEffect(() => { onZoom(camera.zoom); }, [camera.zoom, onZoom]);
  useEffect(() => { setPasteChoice(null); setLinkChoice(null); setPlaceholderChoice(null); }, [project.id]);
  // A repeat duplicate ends as soon as the selection changes any other way.
  const selectedElementKey = targetKey(selection.filter(isElementTarget));
  useEffect(() => {
    if (duplicateRepeat.current && duplicateRepeat.current.key !== selectedElementKey) duplicateRepeat.current = null;
  }, [selectedElementKey]);
  useEffect(() => { if (tool !== 'request') { setRequestDraft(null); setPolygonDraft(null); setDraftArea(null); } }, [tool]);
  useEffect(() => { if (tool !== 'select') setPainterArmed(false); }, [tool]);
  useEffect(() => { if (requestShape === 'pick') setPolygonDraft(null); }, [requestShape]);

  // Keep the visible world in step with the surface, so far-away pointers can be culled.
  useEffect(() => {
    const node = surface.current;
    if (!node) return;
    const update = () => { const box = node.getBoundingClientRect(); setViewSize({ width: box.width, height: box.height }); };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  function escapeCanvas() {
    if (painterArmed) { setPainterArmed(false); return; }
    if (playIsActive(latest.current.play)) { onPlayExit(); return; }
    if (requestDraft || polygonDraft || draftArea) { cancelRequestDraft(); return; }
    if (editing) { cancelEditingText(); return; }
    if (selection.length) {
      const target = selection.find(target => target.kind === 'element');
      selectBoundary(target?.pageId ?? null);
      return;
    }
    if (focused) selectBoundary(null);
  }

  // Delete and Escape follow the canvas selection even when focus sits outside the surface.
  useEffect(() => {
    const handle = (event: KeyboardEvent) => {
      if (event.defaultPrevented || isTextField(event.target)) return;
      // A choice row owns the keyboard until it closes.
      if (pasteChoice || linkChoice) return;
      if (event.key === 'Escape') {
        if (!selection.length && !focused && !editing && !painterArmed) return;
        event.preventDefault();
        escapeCanvas();
        return;
      }
      if (shortcutFor(event, false) !== 'delete') return;
      event.preventDefault();
      if (!selection.length) {
        if (focused) deleteBoundary(focused, focusedSize);
        return;
      }
      const elements = selection.flatMap(target => target.kind === 'element' ? [{ pageId: target.pageId, elementId: target.elementId }] : []);
      const requests = selection.flatMap(target => target.kind === 'request' ? [target.requestId] : []);
      const items = selection.flatMap(target => target.kind === 'reference' ? [target.itemId] : []);
      removeSelection(items, selectedPointers(selection), requests, elements);
    };
    window.addEventListener('keydown', handle);
    return () => window.removeEventListener('keydown', handle);
  }, [selection, focused, focusedSize, editing, selectBoundary, removeSelection, removeElements, deleteBoundary, pasteChoice, linkChoice, painterArmed]);

  // Files dropped on the project canvas: pictures and text land inside the boundary under
  // the pointer or become reference outside every boundary, and other accepted files attach
  // to a request. File access still starts only at the operating-system drop.
  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false; let stop: (() => void) | undefined;

    /** Native drag positions are physical pixels; the canvas draws in CSS pixels. */
    function clientPoint(position: { x: number; y: number }): Point {
      const ratio = window.devicePixelRatio || 1;
      return { x: position.x / ratio, y: position.y / ratio };
    }
    function worldFromClient(x: number, y: number): Point | null {
      const node = surface.current;
      if (!node) return null;
      const box = node.getBoundingClientRect();
      if (x < box.left || x > box.right || y < box.top || y > box.bottom) return null;
      const current = latest.current.camera;
      return { x: (x - box.left - current.x) / current.zoom, y: (y - box.top - current.y) / current.zoom };
    }
    function highlight(point: Point | null) {
      if (!point) { setDropHover(null); return; }
      const boundary = boundaryAt(latest.current.boundaries, point);
      const key = boundary ? `${boundary.pageId}:${boundary.sizeId}` : 'bed';
      setDropHover(current => current?.key === key ? current : { key, rect: boundary?.rect ?? null });
    }
    function requestAt(point: Point): string | null {
      const rows = pendingRequests(latest.current.project);
      for (let index = rows.length - 1; index >= 0; index--) {
        const request = rows[index].request;
        if (request.area && requestAreaContains(request.area.shape, point)) return request.id;
      }
      return null;
    }
    function requestForElement(elementId: string): string | null {
      const project = latest.current.project;
      const page = project.pages.find(item => !!findElementIn(item.elements, elementId));
      const ids = page ? [elementId, ...elementParents(project, page.id, elementId)] : [elementId];
      return (project.requests ?? []).find(request => request.targets.some(target => target.kind === 'element' && ids.includes(target.id)))?.id ?? null;
    }
    function showNotice(message: string, variant: 'info' | 'warning') {
      if (!message) return;
      setDropNotice({ id: ++dropNoticeId.current, message, variant });
    }

    async function dropFiles(point: Point, client: Point, value: NativeDrag) {
      const state = latest.current;
      const files = value.files.length ? value.files : value.picture ? [value.picture] : [];
      const prepared = files.map(file => ({ file, kind: droppedFileKind(file) }));
      const usable = prepared.filter(item => item.kind !== null);
      const messages = [...value.fileErrors];
      // The native fallback message is only useful when nothing else was readable; a text or
      // zip drop is valid even though it is not a picture.
      if (value.message && !value.fileErrors.length && !usable.length) messages.push(value.message);

      // A request marker, a request area, or a layer a request targets all take attachments.
      const hit = document.elementFromPoint(client.x, client.y);
      const markerRequest = hit instanceof Element ? hit.closest<HTMLElement>('[data-request]')?.dataset.request ?? null : null;
      const domElementId = hit instanceof Element ? hit.closest<HTMLElement>('[data-element]')?.dataset.element ?? null : null;
      // Geometry covers overlays and the brief moment after a drag when hit testing is busy.
      const elementId = domElementId ?? elementAt(elementBoxesNow(state.project, state.boundaries), point)?.elementId ?? null;
      const targetRequest = requestAt(point) ?? markerRequest ?? (elementId ? requestForElement(elementId) : null);

      const boundary = boundaryAt(state.boundaries, point);
      const placeable = usable.filter(item => item.kind === 'image' || item.kind === 'text');
      const other = usable.filter(item => item.kind === 'other');
      const unplaceable = prepared.filter(item => item.kind === null);
      const info: string[] = [];

      if (other.length) {
        if (targetRequest) state.attachToRequest(targetRequest, other.map(item => droppedFile(item.file)), []);
        else info.push(`${other.map(item => item.file.name).join(', ')}: this file type can't be placed on the canvas.`);
      }
      if (unplaceable.length) info.push(`${unplaceable.map(item => item.file.name).join(', ')}: this file type can't be placed on the canvas.`);

      const dropped: { pageId: string; elementId: string; sizeId: string | null }[] = [];
      const references: string[] = [];

      if (boundary && placeable.length) {
        const page = state.project.pages.find(item => item.id === boundary.pageId);
        if (page) {
          const allSizes = state.selection.some(target => target.kind === 'element' && target.pageId === boundary.pageId && target.sizeId === null);
          const sizeId = hasManySizes(page) && !allSizes ? boundary.sizeId : null;
          const elements: { element: Element; picture?: string }[] = [];
          for (const [index, item] of placeable.entries()) {
            const offset = index * 24;
            if (item.kind === 'image') {
              const picture = droppedDataUri(item.file);
              const size = await droppedImageSize(picture);
              const worldRect = pastedImageRect({ x: point.x + offset, y: point.y + offset }, size.width, size.height);
              const element = withPickedColours(forAppearance(createElement('image', worldRect, { image: null }), page.appearance));
              element.rect = { ...elementLocalRect(worldRect, boundary.rect, { x: 0, y: 0 }), width: worldRect.width, height: worldRect.height };
              elements.push({ element, picture });
            } else {
              const worldRect = { x: Math.round(point.x + offset), y: Math.round(point.y + offset), width: 0, height: 0 };
              const element = withPickedColours(forAppearance(createTextElement(worldRect, droppedText(item.file)), page.appearance));
              element.rect = { ...elementLocalRect(worldRect, boundary.rect, { x: 0, y: 0 }), width: 0, height: 0 };
              elements.push({ element });
            }
          }
          const images = elements.filter(item => item.picture);
          let index = elementChildrenOf(state.project, boundary.pageId, null).length;
          try {
            if (images.length <= 1) {
              // One command keeps the drop to one undo step; it carries one picture for the batch.
              await state.insertElements(boundary.pageId, null, index, elements.map(item => item.element), images[0]?.picture, sizeId);
              dropped.push(...elements.map(item => ({ pageId: boundary.pageId, elementId: item.element.id, sizeId })));
            } else {
              const text = elements.filter(item => !item.picture);
              if (text.length) {
                await state.insertElements(boundary.pageId, null, index, text.map(item => item.element), undefined, sizeId);
                index += text.length;
                dropped.push(...text.map(item => ({ pageId: boundary.pageId, elementId: item.element.id, sizeId })));
              }
              for (const item of images) {
                await state.insertElements(boundary.pageId, null, index, [item.element], item.picture, sizeId);
                index += 1;
                dropped.push({ pageId: boundary.pageId, elementId: item.element.id, sizeId });
              }
            }
          } catch { /* App surfaces the failed command. */ }
        }
      } else if (placeable.length) {
        for (const [index, item] of placeable.entries()) {
          const offset = index * 24;
          const id = crypto.randomUUID();
          try {
            if (item.kind === 'image') {
              const picture = droppedDataUri(item.file);
              const size = await droppedImageSize(picture);
              const rect = pastedImageRect({ x: point.x + offset, y: point.y + offset }, size.width, size.height);
              await state.addReference({ id, rect, kind: { kind: 'image', picture: NIL_UUID, name: item.file.name.slice(0, 120) } }, picture);
            } else {
              const rect = { ...pastedNoteRect(point), x: Math.round(point.x + offset), y: Math.round(point.y + offset) };
              await state.addReference({ id, rect, kind: { kind: 'note', text: droppedText(item.file) } });
            }
            references.push(id);
          } catch { /* App surfaces the failed command. */ }
        }
      }

      if (dropped.length || references.length) {
        state.clearSelection();
        for (const item of dropped) state.selectElement(item.pageId, item.elementId, item.sizeId, true);
        for (const id of references) state.selectReference(id, true);
      }
      const unique = [...new Set(messages.filter(Boolean))];
      const notices = info.filter(Boolean);
      if (unique.length) showNotice([...unique, ...notices].join(' '), 'warning');
      else if (notices.length) showNotice(notices.join(' '), 'info');
    }

    function handle(value: NativeDrag) {
      if (value.phase === 'leave') { nativeDrop.current = null; nativeOver.current = null; setDropHover(null); return; }
      const at = value.position ? clientPoint(value.position) : null;
      if (value.phase === 'over') {
        if (at) nativeOver.current = { id: value.id, x: at.x, y: at.y };
        highlight(at ? worldFromClient(at.x, at.y) : null);
        return;
      }
      if (value.phase === 'pending') {
        if (at) nativeDrop.current = { id: value.id, x: at.x, y: at.y };
        return;
      }
      if (value.phase !== 'drop') return;
      const pendingDrop = nativeDrop.current;
      // The drop names the pending event; fall back to the last over position if it is missing.
      const last = pendingDrop && pendingDrop.id === value.id ? pendingDrop : nativeOver.current;
      nativeDrop.current = null; nativeOver.current = null; setDropHover(null);
      if (!last) return;
      const point = worldFromClient(last.x, last.y);
      if (!point) return;
      void dropFiles(point, { x: last.x, y: last.y }, value);
    }

    void getCurrentWindow().listen<NativeDrag>('desegnos://image-drag', event => handle(event.payload)).then(unlisten => { if (disposed) unlisten(); else stop = unlisten; });
    return () => { disposed = true; stop?.(); };
  }, []);

  function armPointer(pointerId: string) {
    const pointer = (latest.current.project.pointers ?? []).find(item => item.id === pointerId);
    if (!pointer) return;
    const world = pointerWorld(latest.current.project, latest.current.boundaries, null, {}, {});
    const route = pointerRoute(pointer, world);
    setArmedPointer({ pointerId, end: route?.brokenEnd ?? 'to' });
    latest.current.selectPointer(pointerId);
    surface.current?.focus({ preventScroll: true });
  }

  /** Play's go-to: the camera fits the target boundary and Play continues there. */
  function focusBoundary(pageId: string) {
    const boundary = boundaries.find(item => item.pageId === pageId);
    const node = surface.current;
    if (!boundary || !node) return;
    const box = node.getBoundingClientRect();
    if (box.width < 1 || box.height < 1) return;
    setCamera(fitCamera(boundary.rect, { width: box.width, height: box.height }));
    touched.current = true;
    surface.current?.focus({ preventScroll: true });
  }

  /** Alt+1–9: glide back to a saved named view. */
  function goToView(view: NamedView) {
    animateTo({ x: view.x, y: view.y, zoom: view.zoom });
    surface.current?.focus({ preventScroll: true });
  }

  /** Shift+2: frame the selected layers, reference items or the focused boundary. */
  function fitSelection() {
    const node = surface.current;
    if (!node) return;
    const rects: Rect[] = [];
    for (const target of selection) {
      if (target.kind === 'element') rects.push(...elementEntries([target]).map(entry => entry.world));
      else if (target.kind === 'reference') {
        const item = project.reference.find(entry => entry.id === target.itemId);
        if (item) rects.push(shownReference(item, movingItems));
      }
    }
    if (!rects.length && focused) {
      const boundary = boundaries.find(item => item.pageId === focused);
      if (boundary) rects.push(boundary.rect);
    }
    const box = node.getBoundingClientRect();
    if (!rects.length || box.width < 1 || box.height < 1) { fit(); return; }
    const left = Math.min(...rects.map(rect => rect.x));
    const top = Math.min(...rects.map(rect => rect.y));
    const right = Math.max(...rects.map(rect => rect.x + rect.width));
    const bottom = Math.max(...rects.map(rect => rect.y + rect.height));
    touched.current = true;
    setCamera(fitCamera({ x: left, y: top, width: right - left, height: bottom - top }, { width: box.width, height: box.height }));
  }

  useImperativeHandle(ref, () => ({ focusCanvas: () => surface.current?.focus({ preventScroll: true }), fit, fitSelection, zoomStep, zoomTo, focusBoundary, armPointer, linkSelection, unlinkSelection, copyStyle, pasteStyle, paintFormat, currentCamera, goToView }));

  /** Double-click editing is the all-sizes mode: a layer drawn then belongs to every size. */
  const editingAllSizes = (pageId: string): boolean =>
    selection.some(target => target.kind === 'element' && target.pageId === pageId && target.sizeId === null);
  const editingSizeId = (pageId: string): string => {
    const page = project.pages.find(item => item.id === pageId);
    return page ? baseSizeId(page) : '';
  };
  /** A placeholder click offers Include (clears hidden) or Dismiss (keeps it hidden, no ghost). */
  function openPlaceholder(pageId: string, sizeId: string, elementId: string) {
    const screen = elementScreenRect(elementId, sizeId) ?? { left: 24, top: 24, width: 0, height: 0 };
    setPlaceholderChoice({ pageId, sizeId, elementId, screen });
    surface.current?.focus({ preventScroll: true });
  }
  async function resolvePlaceholder(choice: { pageId: string; sizeId: string; elementId: string }, include: boolean) {
    setPlaceholderChoice(null);
    await Promise.resolve(updateElements(choice.pageId, [{ id: choice.elementId, visible: include, size_id: choice.sizeId, dismissed: !include }])).catch(() => {});
    selectElement(choice.pageId, choice.elementId, choice.sizeId);
  }
  const boundaryRect = (sizeId: string): Rect | null => {
    const boundary = boundaries.find(item => item.sizeId === sizeId);
    if (!boundary) return null;
    return moving?.sizeId === sizeId ? moving.rect : boundary.rect;
  };
  const boundaryOfPage = (pageId: string, sizeId?: string | null): Boundary | null =>
    boundaries.find(item => item.pageId === pageId && (!sizeId || item.sizeId === sizeId)) ?? boundaries.find(item => item.pageId === pageId) ?? null;
  // Layers as the canvas draws them in one size: linked content resolved, then size overrides.
  const pageFor = (pageId: string, sizeId?: string | null): Page | null => {
    const page = project.pages.find(item => item.id === pageId);
    return page ? { ...page, elements: resolveForSize(project, page, sizeId ?? baseSizeId(page)) } : null;
  };
  const boundaryUnder = (point: Point): Boundary | null => boundaryAt(boundaries.map(item => moving?.sizeId === item.sizeId ? { ...item, rect: moving.rect } : item), point);
  const referenceBoxes = (): ReferenceBox[] => (project.reference ?? []).map(item => ({ itemId: item.id, rect: movingItems[item.id] ?? item.rect }));
  const requestBoxes = (): RequestBox[] => elementBoxes(project, boundaries, moving);
  const pointerWorldNow = () => pointerWorld(project, boundaries, moving, {}, movingItems);

  function endpointAt(point: Point): { endpoint: PointerSource | PointerTarget; rect: Rect } | null {
    const request = elementAt(requestBoxes(), point);
    if (request) return { endpoint: { kind: 'element', pageId: request.pageId, elementId: request.elementId, sizeId: request.sizeId }, rect: request.rect };
    const item = referenceItemAt(project.reference.map(entry => ({ itemId: entry.id, rect: shownReference(entry, movingItems), kind: entry.kind })), point);
    if (item) return { endpoint: item.kind.kind === 'link' ? { kind: 'url', url: item.kind.url } : { kind: 'reference', itemId: item.itemId }, rect: item.rect };
    const boundary = boundaryUnder(point);
    if (boundary) return { endpoint: { kind: 'boundary', pageId: boundary.pageId, sizeId: boundary.sizeId }, rect: boundary.rect };
    return null;
  }
  function pointerSourceAt(point: Point): { endpoint: PointerSource; rect: Rect } | null {
    const request = elementAt(requestBoxes(), point);
    if (request) return { endpoint: { kind: 'element', pageId: request.pageId, elementId: request.elementId, sizeId: request.sizeId }, rect: request.rect };
    const boundary = boundaryUnder(point);
    return boundary ? { endpoint: { kind: 'boundary', pageId: boundary.pageId, sizeId: boundary.sizeId }, rect: boundary.rect } : null;
  }

  /** The thing a Request-tool click picks: a layer, reference item, pointer line or boundary. */
  function requestTargetAt(point: Point): RequestTarget | null {
    const box = elementAt(requestBoxes(), point);
    if (box) {
      const page = project.pages.find(item => item.id === box.pageId);
      return { kind: 'element', page: box.pageId, size: page && page.sizes.length > 1 ? box.sizeId : null, id: box.elementId };
    }
    const item = referenceItemAt(project.reference.map(entry => ({ itemId: entry.id, rect: shownReference(entry, movingItems) })), point);
    if (item) return { kind: 'reference', id: item.itemId };
    const hits = pointerAt(pointerEntries(project.pointers ?? [], pointerWorldNow(), new Set(), viewportWorldRect(camera, viewSize)), point, POINTER_HIT_PX / camera.zoom);
    if (hits) return { kind: 'pointer', id: hits.pointer.id };
    const boundary = boundaryUnder(point);
    return boundary ? { kind: 'boundary', page: boundary.pageId, size: boundary.sizeId } : null;
  }

  /** Screen pixels for the request box: beside the targets, never inside the transformed layer. */
  function requestEditorScreen(targets: RequestTarget[], area: RequestArea | null): ScreenRect {
    let rect: ScreenRect | null = null;
    if (area) rect = screenRect(requestShapeBounds(area.shape), camera);
    for (const target of rect ? [] : targets) {
      if (target.kind === 'element') rect = elementScreenRect(target.id, target.size) ?? rect;
      else if (target.kind === 'boundary') {
        const boundary = boundaries.find(item => item.pageId === target.page && (!target.size || item.sizeId === target.size));
        if (boundary) rect = screenRect(boundary.rect, camera);
      } else if (target.kind === 'reference') {
        const item = project.reference.find(row => row.id === target.id);
        if (item) rect = screenRect(shownReference(item, movingItems), camera);
      }
    }
    if (!rect) rect = { left: 24, top: 24, width: 0, height: 0 };
    const width = Math.min(REQUEST_EDITOR_WIDTH, Math.max(180, viewSize.width - 16));
    // Prefer the right side so the box never covers the layers it targets or the next click.
    const right = rect.left + rect.width + 10;
    const left = right + width <= viewSize.width - 8 ? right
      : rect.left - width - 10 >= 8 ? rect.left - width - 10
      : Math.max(8, Math.min(rect.left, viewSize.width - width - 8));
    const above = rect.top - REQUEST_EDITOR_HEIGHT - 6;
    const top = above >= 8 ? above : Math.min(viewSize.height - REQUEST_EDITOR_HEIGHT - 8, rect.top + rect.height + 6);
    return { left, top: Math.max(8, top), width, height: REQUEST_EDITOR_HEIGHT };
  }

  /** Turn a drawn outline into a request area and its covered targets. */
  function closeRequestArea(input: Rect | Point[], kind: 'rect' | 'ellipse' | 'polygon' | 'freehand') {
    const shape: RequestShape = kind === 'polygon' || kind === 'freehand'
      ? { kind, points: (input as Point[]).map(point => ({ x: point.x, y: point.y })) }
      : { kind, rect: input as Rect };
    const bounds = requestShapeBounds(shape);
    const boundary = boundaryUnder({ x: bounds.x + bounds.width / 2, y: bounds.y + bounds.height / 2 });
    const page = boundary?.pageId ?? null;
    const area: RequestArea = { page, size: boundary?.sizeId ?? null, shape };
    const targets = coveredTargets(project, boundaries, area, movingItems);
    setDraftArea(null);
    setPolygonDraft(null);
    setRequestDraft({ targets, area, screen: requestEditorScreen(targets, area) });
    surface.current?.focus({ preventScroll: true });
  }

  function targetForSelection(): RequestTarget[] {
    const targets: RequestTarget[] = [];
    for (const target of selection) {
      if (target.kind === 'element') targets.push({ kind: 'element', page: target.pageId, size: target.sizeId, id: target.elementId });
      else if (target.kind === 'reference') targets.push({ kind: 'reference', id: target.itemId });
      else if (target.kind === 'pointer') targets.push({ kind: 'pointer', id: target.pointerId });
    }
    if (!targets.length && focused) {
      const boundary = boundaries.find(item => item.pageId === focused);
      if (boundary) targets.push({ kind: 'boundary', page: focused, size: focusedSize ?? boundary.sizeId });
    }
    return dedupeTargets(targets);
  }

  /** F: with something selected, start a request on it; otherwise choose the Request tool. */
  function startRequestOnSelection(): boolean {
    const targets = targetForSelection();
    if (!targets.length) return false;
    setRequestDraft({ targets, area: null, screen: requestEditorScreen(targets, null) });
    setPolygonDraft(null);
    setDraftArea(null);
    surface.current?.focus({ preventScroll: true });
    return true;
  }

  /** Shift+F: use the selected shape's own outline as the request area; the shape stays. */
  function startRequestFromShape(): boolean {
    const target = selection.find(isElementTarget);
    if (!target) return false;
    const entry = elementEntries([target])[0];
    if (!entry) return false;
    const shape: RequestShape = entry.element.kind === 'ellipse'
      ? { kind: 'ellipse', rect: entry.world }
      : { kind: 'rect', rect: entry.world };
    const page = project.pages.find(item => item.id === target.pageId);
    const area: RequestArea = { page: target.pageId, size: target.sizeId ?? (page ? baseSizeId(page) : null), shape };
    const targets = dedupeTargets([...coveredTargets(project, boundaries, area, movingItems),
      { kind: 'element', page: target.pageId, size: target.sizeId, id: target.elementId }]);
    setRequestDraft({ targets, area, screen: requestEditorScreen(targets, area) });
    setPolygonDraft(null);
    setDraftArea(null);
    surface.current?.focus({ preventScroll: true });
    return true;
  }

  /** Enter in the request box: queue it as a pending request on its targets. */
  function submitRequestDraft(text: string) {
    const draft = requestDraft;
    setRequestDraft(null);
    const trimmed = text.trim();
    if (!draft || !trimmed) { surface.current?.focus({ preventScroll: true }); return; }
    const request: PendingRequest = {
      id: crypto.randomUUID(), text: trimmed, attachments: [], targets: draft.targets, area: draft.area,
      requestedAt: new Date().toISOString(),
    };
    void addRequest(request).then(() => selectRequest(request.id)).catch(() => {})
      .finally(() => surface.current?.focus({ preventScroll: true }));
  }

  function cancelRequestDraft() {
    setRequestDraft(null);
    setPolygonDraft(null);
    setDraftArea(null);
    surface.current?.focus({ preventScroll: true });
  }

  function startPan(event: ReactPointerEvent<HTMLDivElement>, pageId: string | null) {
    event.preventDefault();
    beginPointerSession(event);
    drag.current = { kind: 'pan', pointerId: event.pointerId, start: { x: event.clientX, y: event.clientY }, camera, moved: false, pageId };
    setPanning(true);
  }

  function beginMove(event: ReactPointerEvent, point: Point, targets: CanvasTarget[]): MoveSelectionDrag | null {
    const movable: MoveTarget[] = targets.flatMap((target): MoveTarget[] => {
      if (target.kind === 'pointer' || target.kind === 'element' || target.kind === 'request') return [];
      const item = project.reference.find(entry => entry.id === target.itemId);
      return item ? [{ kind: 'reference' as const, itemId: item.id, origin: item.rect }] : [];
    });
    if (!movable.length) return null;
    return { kind: 'move-selection', pointerId: event.pointerId, start: point, targets: movable, rects: {}, moved: false, axisLocked: false };
  }

  function nextSelectionFor(target: CanvasTarget, additive: boolean): CanvasTarget[] {
    const key = canvasTargetKey(target);
    if (!additive) return [target];
    return selection.some(item => canvasTargetKey(item) === key) ? selection.filter(item => canvasTargetKey(item) !== key) : [...selection, target];
  }

  function elementEntries(targets: CanvasTarget[]): ElementEntry[] {
    const entries: ElementEntry[] = [];
    for (const target of targets.filter(isElementTarget)) {
      const page = project.pages.find(item => item.id === target.pageId);
      if (!page) continue;
      const allSizes = target.sizeId === null;
      const sizeId = target.sizeId ?? baseSizeId(page);
      const path = elementPath(project, target.pageId, target.elementId, sizeId);
      const boundary = boundaryRect(sizeId);
      const last = path?.[path.length - 1];
      if (!last || !boundary) continue;
      const over = last.element.overrides?.[sizeId];
      const rect = over?.rect ?? last.element.rect;
      const origin = { x: last.parentOrigin.x + rect.x, y: last.parentOrigin.y + rect.y };
      entries.push({
        id: last.element.id, element: last.element, pageId: target.pageId, sizeId, allSizes, parentId: last.parentId, index: last.index, rect,
        parentOrigin: last.parentOrigin, boundaryOrigin: { x: boundary.x, y: boundary.y },
        world: { x: boundary.x + origin.x, y: boundary.y + origin.y, width: rect.width, height: rect.height },
      });
    }
    // A selected ancestor moves its subtree; never move both.
    return entries.filter(entry => !entries.some(other => other.id !== entry.id && elementParents(project, entry.pageId, entry.id).includes(other.id)));
  }

  function selectTargets(targets: { pageId: string; elementId: string; sizeId?: string | null }[]) {
    clearSelection();
    for (const target of targets) selectElement(target.pageId, target.elementId, target.sizeId ?? null, true);
  }

  function startEditingElement(pageId: string, elementId: string, sizeId: string, created: boolean) {
    const element = findElementIn(pageFor(pageId, sizeId)?.elements ?? [], elementId);
    if (!element) return;
    const over = element.overrides?.[sizeId];
    const rect = over?.rect ?? element.rect;
    setEditing({ pageId, sizeId, elementId, autoWidth: rect.width === 0, created });
    selectElement(pageId, elementId, sizeId);
    surface.current?.focus({ preventScroll: true });
  }

  function selectAllSiblings() {
    const target = selection.find(isElementTarget);
    if (target) {
      const children = elementChildrenOf(project, target.pageId, elementParents(project, target.pageId, target.elementId).at(-1) ?? null);
      selectTargets(children.map(element => ({ pageId: target.pageId, elementId: element.id })));
      return;
    }
    const pageId = focused ?? project.homePageId;
    const page = pageFor(pageId);
    if (page) selectTargets(page.elements.map(element => ({ pageId, elementId: element.id })));
  }

  function arrangeElements(forward: boolean) {
    const entries = elementEntries(selection);
    if (!entries.length) return;
    const moves: ElementMove[] = [];
    for (const entry of [...entries].sort((first, second) => forward ? second.index - first.index : first.index - second.index)) {
      const siblings = elementChildrenOf(project, entry.pageId, entry.parentId);
      const index = Math.max(0, Math.min(siblings.length - 1, entry.index + (forward ? 1 : -1)));
      if (index === entry.index) continue;
      moves.push({ id: entry.id, to_page_id: entry.pageId, to_parent_id: entry.parentId, index, rect: entry.rect,
        size_id: entry.allSizes ? null : entry.sizeId, all_sizes: entry.allSizes });
    }
    if (moves.length) void Promise.resolve(moveElements(moves)).catch(() => {});
  }

  // Format painter: the style is the only thing that travels, never position, size or text.
  function copyStyle() {
    const entry = elementEntries(selection)[0];
    if (entry) copiedStyle.current = structuredClone(entry.element.style);
  }

  /** One command per page, so pasting a style over a selection is a single undo step. */
  function pasteStyle() {
    const style = copiedStyle.current;
    const entries = elementEntries(selection);
    if (!style || !entries.length) return;
    const byPage = new Map<string, ElementPatch[]>();
    for (const entry of entries) {
      const patch: ElementPatch = { id: entry.id, style: structuredClone(style), ...(entry.allSizes ? { all_sizes: true } : { size_id: entry.sizeId }) };
      byPage.set(entry.pageId, [...(byPage.get(entry.pageId) ?? []), patch]);
    }
    for (const [pageId, patches] of byPage) void Promise.resolve(updateElements(pageId, patches)).catch(() => {});
  }

  /** Shift+Ctrl/Cmd+Alt+C copies the selected style and arms the painter for one click. */
  function paintFormat() {
    if (elementEntries(selection).length) copyStyle();
    if (!copiedStyle.current) return;
    setPainterArmed(true);
    surface.current?.focus({ preventScroll: true });
  }

  function applyPainterStyle(target: ElementTarget) {
    const style = copiedStyle.current;
    if (!style) return;
    const patch: ElementPatch = { id: target.elementId, style: structuredClone(style), ...(target.sizeId === null ? { all_sizes: true } : { size_id: target.sizeId }) };
    void Promise.resolve(updateElements(target.pageId, [patch])).catch(() => {});
    setPainterArmed(false);
  }

  function copyElements() {
    const entries = elementEntries(selection);
    if (!entries.length) return;
    clipboard.current = entries.map(entry => ({ id: entry.id, pageId: entry.pageId, parentId: entry.parentId, index: entry.index,
      element: structuredClone(entry.element), world: entry.world, rect: entry.rect }));
    pasteCount.current = 0;
    setPasteChoice(null);
    // The marker says a later paste reads these layers, not whatever was on the clipboard before.
    // When it cannot be written, paste falls back to the layers copied here.
    const mark = newClipboardMarker();
    clipboardMark.current = mark;
    const unwritten = () => { if (clipboardMark.current === mark) clipboardMark.current = null; };
    try { void navigator.clipboard.writeText(mark).catch(unwritten); } catch { unwritten(); }
  }

  /** Where a paste lands: a selected box takes it as a child, otherwise the boundary under the pointer. */
  function pasteTarget(): PasteTarget | null {
    const selected = selection.find(isElementTarget);
    const shared = nestedLayerIds(project);
    if (selected && !shared.has(selected.elementId)) {
      const page = project.pages.find(item => item.id === selected.pageId);
      const sizeId = selected.sizeId ?? (page ? baseSizeId(page) : null);
      const element = findPageElement(project, selected.pageId, selected.elementId, sizeId);
      const path = sizeId ? elementPath(project, selected.pageId, selected.elementId, sizeId) : null;
      const boundary = sizeId ? boundaryRect(sizeId) : null;
      const last = path?.[path.length - 1];
      if (element?.kind === 'box' && last && boundary && sizeId) return { pageId: selected.pageId, sizeId, allSizes: selected.sizeId === null, parentId: element.id, parentOrigin: last.origin, boundary };
    }
    const point = lastPointer.current ?? { x: (viewSize.width / 2 - camera.x) / camera.zoom, y: (viewSize.height / 2 - camera.y) / camera.zoom };
    const boundary = boundaryUnder(point);
    if (!boundary) return null;
    return { pageId: boundary.pageId, sizeId: boundary.sizeId, allSizes: editingAllSizes(boundary.pageId), parentId: null, parentOrigin: { x: 0, y: 0 }, boundary: boundary.rect };
  }

  function pasteScreen(): ScreenRect {
    const point = lastPointer.current;
    if (!point) return { left: 24, top: 24, width: 0, height: 0 };
    return { left: point.x * camera.zoom + camera.x, top: point.y * camera.zoom + camera.y, width: 0, height: 0 };
  }

  function pasteEntries(entries: ClipboardEntry[], mode: PasteMode, target: PasteTarget, offset: Point,
    onInserted?: (targets: { pageId: string; elementId: string; sizeId: string | null }[]) => void, selectAfter = true) {
    if (!entries.length) return;
    const index = elementChildrenOf(project, target.pageId, target.parentId).length;
    if (mode === 'linked') {
      const items: LinkedPasteInstance[] = entries.map((entry, position) => {
        const world = { x: entry.world.x + offset.x, y: entry.world.y + offset.y };
        return {
          id: crypto.randomUUID(), sourceId: entry.id, pageId: target.pageId, parentId: target.parentId, index: index + position,
          rect: { x: world.x - target.boundary.x - target.parentOrigin.x, y: world.y - target.boundary.y - target.parentOrigin.y, width: entry.rect.width, height: entry.rect.height },
        };
      });
      const targets = items.map(item => ({ pageId: item.pageId, elementId: item.id, sizeId: target.allSizes ? null : target.sizeId }));
      void pasteLinked(items).then(() => { onInserted?.(targets); if (selectAfter) selectTargets(targets); }).catch(() => {});
      return;
    }
    const inserted: { pageId: string; elementId: string; sizeId: string | null }[] = [];
    const elements = entries.map(entry => {
      const world = { x: entry.world.x + offset.x, y: entry.world.y + offset.y };
      const element: Element = { ...structuredClone(entry.element), id: crypto.randomUUID(), link: null, overrides: {},
        rect: { x: world.x - target.boundary.x - target.parentOrigin.x, y: world.y - target.boundary.y - target.parentOrigin.y, width: entry.rect.width, height: entry.rect.height },
        children: cloneChildren(entry.element.children) };
      inserted.push({ pageId: target.pageId, elementId: element.id, sizeId: target.allSizes ? null : target.sizeId });
      return element;
    });
    // A layer pasted in one size is hidden in the others, exactly like a newly drawn layer.
    void insertElements(target.pageId, target.parentId, index, elements, undefined, target.allSizes ? null : target.sizeId)
      .then(() => { onInserted?.(inserted); if (selectAfter) selectTargets(inserted); }).catch(() => {});
  }

  /** Ctrl/Cmd+V: the default paste when one is set, otherwise the inline choice. */
  function pasteElements(alwaysAsk = false) {
    if (!clipboard.current.length) return;
    setLinkChoice(null);
    const target = pasteTarget();
    if (!target) return;
    const mode = alwaysAsk ? null : pasteMode ?? pasteDefault();
    if (mode) {
      pasteCount.current += 1;
      pasteEntries(clipboard.current, mode, target, { x: 16 * pasteCount.current, y: 16 * pasteCount.current });
      return;
    }
    setPasteChoice({ target, offset: 16 * (pasteCount.current + 1), value: 'linked', useDefault: false, screen: pasteScreen() });
  }

  function confirmPaste() {
    const choice = pasteChoice;
    if (!choice) return;
    setPasteChoice(null);
    pasteCount.current += 1;
    if (choice.useDefault) setPasteDefault(choice.value);
    pasteEntries(clipboard.current, choice.value, choice.target, { x: choice.offset, y: choice.offset });
  }

  function cloneChildren(children: Element[]): Element[] {
    return children.map(child => ({ ...structuredClone(child), id: crypto.randomUUID(), children: cloneChildren(child.children) }));
  }

  /** Ctrl/Cmd+D follows the paste default; each copy stays beside its original.
   *  A repeat duplicate keeps the offset of the copy that was just moved, Figma-style. */
  function duplicateElements() {
    const entries = elementEntries(selection);
    if (!entries.length) return;
    const repeat = duplicateRepeat.current;
    const key = entryTargetKey(entries);
    const offset = repeat?.key === key ? repeat.offset : { x: 16, y: 16 };
    const mode = pasteMode ?? pasteDefault() ?? 'unlinked';
    const shared = nestedLayerIds(project);
    const groups = new Map<string, { target: PasteTarget; entries: ClipboardEntry[] }>();
    for (const entry of entries) {
      const boundary = boundaryRect(entry.sizeId);
      if (!boundary) continue;
      // A layer inside a linked element is copied beside its copy, not into the shared box.
      const parentId = entry.parentId && !shared.has(entry.parentId) ? entry.parentId : null;
      const parentOrigin = parentId ? entry.parentOrigin : { x: 0, y: 0 };
      const groupKey = `${entry.pageId}\u0000${entry.sizeId}\u0000${parentId ?? ''}`;
      const group = groups.get(groupKey) ?? { target: { pageId: entry.pageId, sizeId: entry.sizeId, allSizes: entry.allSizes, parentId, parentOrigin, boundary }, entries: [] };
      group.entries.push({ id: entry.id, pageId: entry.pageId, parentId: entry.parentId, index: entry.index, element: entry.element, world: entry.world, rect: entry.rect });
      groups.set(groupKey, group);
    }
    // Copies can land in several boxes; select them all and arm the repeat once they exist.
    const inserted: { pageId: string; elementId: string; sizeId: string | null }[] = [];
    let pending = groups.size;
    for (const group of groups.values()) {
      pasteEntries(group.entries, mode, group.target, offset, targets => {
        inserted.push(...targets);
        pending -= 1;
        if (pending > 0) return;
        duplicateRepeat.current = { key: targetKey(inserted), offset };
        selectTargets(inserted);
      }, false);
    }
  }

  function linkSelection() {
    const targets = selection.filter(isElementTarget);
    const copies: { id: string; element: Element; pageId: string }[] = [];
    for (const target of targets) {
      const element = findPageElement(project, target.pageId, target.elementId);
      if (element && !copies.some(copy => copy.id === element.id)) copies.push({ id: element.id, element, pageId: target.pageId });
    }
    if (copies.length < 2) return;
    const screen = elementScreenRect(copies[0].id) ?? { left: 24, top: 24, width: 0, height: 0 };
    const boundaryName = (pageId: string) => project.pages.find(page => page.id === pageId)?.name ?? 'Boundary';
    const choices: LinkCopy[] = copies.map(copy => ({ id: copy.id, label: copySummary(copy.element), boundary: boundaryName(copy.pageId) }));
    const nested = nestedLayerIds(project);
    if (copies.some(copy => nested.has(copy.id))) {
      setLinkChoice({ copies: [], error: "These layers are already linked", screen });
      return;
    }
    if (!copies.every(copy => sameStructure(copies[0].element, copy.element))) {
      setLinkChoice({ copies: [], error: "Different structure, can't link", screen });
      return;
    }
    if (!linkNeedsChoice(copies.map(copy => copy.element))) { linkCopies(copies.map(copy => copy.id), copies[0].id); return; }
    setLinkChoice({ copies: choices, error: null, screen });
  }

  function linkCopies(ids: string[], sourceId: string | null) {
    setLinkChoice(null);
    void Promise.resolve(linkElements(ids, sourceId)).catch(() => {});
  }

  function unlinkSelection() {
    const ids = [...new Set(selection.filter(isElementTarget).map(target => target.elementId))];
    if (!ids.length) return;
    void Promise.resolve(unlinkElements(ids)).catch(() => {});
  }

  function groupSelection() {
    const targets = selection.filter(isElementTarget);
    if (targets.length < 2) return;
    const pageId = targets[0].pageId;
    if (!targets.every(target => target.pageId === pageId)) return;
    void groupElements(pageId, targets.map(target => target.elementId)).then(id => { if (id) selectTargets([{ pageId, elementId: id }]); }).catch(() => {});
  }

  function ungroupSelection() {
    const targets = selection.filter(isElementTarget);
    if (targets.length !== 1) return;
    const { pageId, elementId } = targets[0];
    void ungroupElements(pageId, elementId).then(ids => { if (ids.length) selectTargets(ids.map(id => ({ pageId, elementId: id }))); }).catch(() => {});
  }

  function nudgeElements(dx: number, dy: number) {
    const entries = elementEntries(selection);
    if (!entries.length) return;
    rememberRepeatMove(entries, { x: dx, y: dy });
    void Promise.resolve(moveElements(entries.map(entry => ({
      id: entry.id, to_page_id: entry.pageId, to_parent_id: entry.parentId, index: entry.index,
      rect: { ...entry.rect, x: entry.rect.x + dx, y: entry.rect.y + dy },
      size_id: entry.allSizes ? null : entry.sizeId, all_sizes: entry.allSizes,
    })))).catch(() => {});
  }

  function focusElement(direction: 'child' | 'parent') {
    const target = selection.find(isElementTarget);
    if (!target) return;
    if (direction === 'child') {
      const children = elementChildrenOf(project, target.pageId, target.elementId);
      if (children.length) selectTargets([{ pageId: target.pageId, elementId: children[0].id }]);
      return;
    }
    const parents = elementParents(project, target.pageId, target.elementId);
    if (parents.length) selectTargets([{ pageId: target.pageId, elementId: parents.at(-1)! }]);
    else selectBoundary(target.pageId, target.sizeId);
  }

  // Canvas-wide keys, so focus can sit in the Layers panel or Properties. The actions and keys
  // come from the same keymap the tool strip and keyboard help read.
  useEffect(() => {
    const handle = (event: KeyboardEvent) => {
      if (event.defaultPrevented || isTextField(event.target)) return;
      const action = shortcutFor(event, false);
      if (!action) return;
      const elementTargets = selection.filter(isElementTarget);
      switch (action) {
        case 'requestTool':
          event.preventDefault();
          if (!startRequestOnSelection()) onTool('request');
          return;
        case 'requestFromShape':
          if (elementTargets.length === 1) { event.preventDefault(); startRequestFromShape(); }
          return;
        case 'link': if (elementTargets.length) { event.preventDefault(); linkSelection(); } return;
        case 'unlink': if (elementTargets.length) { event.preventDefault(); unlinkSelection(); } return;
        case 'selectAll': event.preventDefault(); selectAllSiblings(); return;
        case 'copy': if (elementTargets.length) { event.preventDefault(); copyElements(); } return;
        // Plain paste is left to the paste event, which can also carry content from other apps.
        case 'paste': return;
        case 'pasteChoice': if (clipboard.current.length) { event.preventDefault(); pasteElements(true); } return;
        case 'duplicate': if (elementTargets.length) { event.preventDefault(); duplicateElements(); } return;
        case 'copyStyle': if (elementTargets.length) { event.preventDefault(); copyStyle(); } return;
        case 'pasteStyle': if (elementTargets.length && copiedStyle.current) { event.preventDefault(); pasteStyle(); } return;
        case 'paintFormat': if (elementTargets.length || copiedStyle.current) { event.preventDefault(); paintFormat(); } return;
        case 'group': if (elementTargets.length) { event.preventDefault(); groupSelection(); } return;
        case 'ungroup': if (elementTargets.length) { event.preventDefault(); ungroupSelection(); } return;
        case 'forward': case 'front': if (elementTargets.length) { event.preventDefault(); arrangeElements(true); } return;
        case 'backward': case 'back': if (elementTargets.length) { event.preventDefault(); arrangeElements(false); } return;
        case 'selectChild': if (elementTargets.length) { event.preventDefault(); focusElement('child'); } return;
        case 'selectParent': if (elementTargets.length) { event.preventDefault(); focusElement('parent'); } return;
        case 'nudge': case 'nudgeFar': {
          if (!elementTargets.length) return;
          event.preventDefault();
          const key = event.key;
          const step = action === 'nudgeFar' ? 10 : 1;
          nudgeElements(key === 'ArrowLeft' ? -step : key === 'ArrowRight' ? step : 0, key === 'ArrowUp' ? -step : key === 'ArrowDown' ? step : 0);
          return;
        }
        case 'minimap':
          event.preventDefault();
          setMinimapPreference(minimapVisible ? 'off' : 'on');
          return;
      }
    };
    window.addEventListener('keydown', handle);
    return () => window.removeEventListener('keydown', handle);
  }, [project, selection, focused, minimapVisible]);

  // The Image tool opens the platform file picker through the same picture store as reference.
  useEffect(() => {
    if (tool !== 'image' || pendingImage) return;
    imagePicker.current?.click();
  }, [tool, pendingImage]);
  useEffect(() => { if (tool !== 'image') setPendingImage(null); }, [tool]);

  function chooseImage(file: File | undefined) {
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
    return elementChildrenOf(project, pageId, parentId).length;
  }

  function elementRectAt(sizeId: string, parentId: string | null, parentOrigin: Point, boundary: Rect, rect: Rect): Rect {
    return elementLocalRect(rect, boundaryRect(sizeId) ?? boundary, parentOrigin);
  }

  async function placeElement(kind: ElementKind, pageId: string, sizeId: string, parentId: string | null, parentOrigin: Point, boundary: Rect, rect: Rect, picture?: string, rotation?: number) {
    const page = project.pages.find(item => item.id === pageId);
    const element = withPickedColours(forAppearance(kind === 'text' ? createTextElement(rect)
      : createElement(kind, rect, kind === 'image' ? { image: null } : {}), page?.appearance ?? 'light'));
    const local = elementRectAt(sizeId, parentId, parentOrigin, boundary, rect);
    element.rect = { ...local, width: rect.width, height: rect.height };
    if (kind === 'line' && rotation !== undefined) element.rotation = rotation;
    createdElement.current = element.id;
    const singleSize = !!page && hasManySizes(page) && !editingAllSizes(pageId);
    await insertElements(pageId, parentId, insertionPoint(pageId, parentId), [element], picture, singleSize ? sizeId : null);
    selectElement(pageId, element.id, singleSize ? sizeId : null);
    return element;
  }

  function drawElementDrag(event: ReactPointerEvent<HTMLDivElement>, point: Point, kind: ElementKind, line: boolean): DrawElementDrag | null {
    const boundary = boundaryUnder(point);
    if (!boundary) return null;
    const { parentId, parentOrigin } = boxParentAt(project, boundary.pageId, boundary.sizeId, event.target);
    event.preventDefault();
    beginPointerSession(event);
    const rect = { ...point, width: 0, height: line ? 0 : 0 };
    drag.current = { kind: 'draw-element', pointerId: event.pointerId, start: point, pageId: boundary.pageId, sizeId: boundary.sizeId, parentId, parentOrigin, boundary: boundary.rect, rect, elementKind: kind, line, rotation: 0, moved: false };
    setDraftElement({ rect, line, rotation: 0 });
    return drag.current;
  }

  function pointerDown(event: ReactPointerEvent<HTMLDivElement>) {
    if (busy.current) return;
    // Middle-button drag pans from any tool and never touches the selection.
    if (event.button === 1) { startPan(event, null); return; }
    if (pasteChoice || linkChoice) { setPasteChoice(null); setLinkChoice(null); }
    const target = event.target as HTMLElement;
    const boundaryNode = target.closest<HTMLElement>('[data-boundary]');
    const handle = target.closest<HTMLElement>('[data-boundary-handle]');
    const grip = target.closest<HTMLElement>('[data-reference-corner]');
    const itemNode = target.closest<HTMLElement>('[data-reference]');
    const elementNode = target.closest<HTMLElement>('[data-element]');
    const requestNode = target.closest<HTMLElement>('[data-request]');
    const point = worldPoint(event);
    lastPointer.current = point;
    // Z arms zooming: drag a rectangle to fit it, or click a zoom step at the pointer.
    if (event.button === 0 && zoom.current && !space.current) {
      event.preventDefault();
      beginPointerSession(event);
      drag.current = { kind: 'zoom-rect', pointerId: event.pointerId, start: point, current: point, zoomOut: event.altKey, moved: false };
      setZoomRect({ ...point, width: 0, height: 0 });
      return;
    }
    // The painter applies its style to the next clicked layer and then disarms.
    if (event.button === 0 && painterArmed && !space.current) {
      const hit = elementTargetAt(project, selection, target);
      if (hit) {
        event.preventDefault();
        applyPainterStyle(hit);
        surface.current?.focus({ preventScroll: true });
        return;
      }
      setPainterArmed(false);
    }
    if (requestNode && !space.current) {
      event.preventDefault();
      selectRequest(requestNode.dataset.request!);
      surface.current?.focus({ preventScroll: true });
      return;
    }
    if (playIsActive(latest.current.play) && !space.current) {
      // Play turns the boundary into a clickable surface: no selection, no dragging.
      event.preventDefault();
      const chain = elementChain(target);
      const clickedPage = chainPage(chain);
      if (clickedPage && latest.current.play.boundary === clickedPage) {
        // The innermost ancestor that carries a Play pointer is the one a click runs.
        const source = chain.find(entry => {
          const id = entry.dataset.element;
          return !!id && (project.pointers ?? []).some(pointer =>
            pointer.play && pointer.from.kind === 'element'
            && pointer.from.pageId === clickedPage && pointer.from.elementId === id);
        });
        const elementId = source?.dataset.element;
        if (elementId) latest.current.onPlayElement(clickedPage, elementId);
      }
      return;
    }
    const additive = event.shiftKey || event.ctrlKey || event.metaKey;
    if (event.button === 0 && space.current) { startPan(event, boundaryNode?.dataset.boundary ?? null); return; }
    if (event.button === 0 && tool === 'pan') { startPan(event, boundaryNode?.dataset.boundary ?? null); return; }
    if (event.button !== 0) return;

    if (tool === 'request') {
      event.preventDefault();
      if (requestShape === 'pick') {
        const target = requestTargetAt(point);
        if (!target) return;
        const shift = event.shiftKey || event.ctrlKey || event.metaKey;
        const existing = requestDraft?.targets ?? [];
        const key = requestTargetKey(target);
        const already = existing.some(item => requestTargetKey(item) === key);
        const targets = shift
          ? already ? existing.filter(item => requestTargetKey(item) !== key) : dedupeTargets([...existing, target])
          : already ? existing.filter(item => requestTargetKey(item) !== key) : [target];
        if (!targets.length) { setRequestDraft(null); return; }
        setRequestDraft({ targets, area: requestDraft?.area ?? null, screen: requestEditorScreen(targets, requestDraft?.area ?? null) });
        setPolygonDraft(null);
        setDraftArea(null);
        surface.current?.focus({ preventScroll: true });
        return;
      }
      if (requestShape === 'polygon') {
        const current = polygonDraft ?? [];
        const points = [...current, point];
        setPolygonDraft(points);
        setDraftArea({ kind: 'polygon', points });
        surface.current?.focus({ preventScroll: true });
        if (points.length >= 3 && Math.hypot(points[0].x - point.x, points[0].y - point.y) * camera.zoom < 12) closeRequestArea(points, 'polygon');
        return;
      }
      beginPointerSession(event);
      drag.current = { kind: 'draw-request-area', pointerId: event.pointerId, start: point, current: point, shape: requestShape, points: [point], moved: false };
      setPolygonDraft(null);
      setDraftArea(requestShape === 'ellipse' ? { kind: 'ellipse', rect: { ...point, width: 0, height: 0 } } : requestShape === 'freehand' ? { kind: 'freehand', points: [point] } : { kind: 'rect', rect: { ...point, width: 0, height: 0 } });
      return;
    }
    if (tool === 'note') {
      if (boundaryUnder(point)) return;
      event.preventDefault();
      beginPointerSession(event);
      drag.current = { kind: 'draw-note', pointerId: event.pointerId, start: point, rect: referenceNoteRect(point, point), moved: false };
      setDraftNote(drag.current.rect);
      return;
    }
    if (tool === 'boundary') {
      // Drawing over an existing boundary is allowed: the commit pushes neighbours apart, which
      // keeps boundaries from overlapping and keeps the tool from silently refusing a press.
      event.preventDefault();
      beginPointerSession(event);
      drag.current = { kind: 'draw-boundary', pointerId: event.pointerId, start: point, rect: boundaryDrawRect(point, point), moved: false };
      setDraft(drag.current.rect);
      return;
    }
    if (tool === 'pointer') {
      const source = pointerSourceAt(point);
      if (!source) { startPan(event, null); return; }
      event.preventDefault();
      beginPointerSession(event);
      drag.current = { kind: 'draw-pointer', pointerId: event.pointerId, start: point, source: source.endpoint, sourceRect: source.rect, current: point, moved: false };
      surface.current?.focus({ preventScroll: true });
      return;
    }
    if (tool === 'text') {
      const hit = elementTargetAt(project, selection, target);
      if (hit) {
        const element = findElementIn(pageFor(hit.pageId, hit.sizeId)?.elements ?? [], hit.elementId);
        if (element && ['text', 'button', 'link', 'input', 'textarea', 'select', 'checkbox', 'radio'].includes(element.kind)) {
          event.preventDefault();
          startEditingElement(hit.pageId, hit.elementId, hit.sizeId ?? editingSizeId(hit.pageId), false);
          return;
        }
      }
      if (drawElementDrag(event, point, 'text', false)) {
        drag.current = { ...(drag.current as DrawElementDrag), rect: { x: point.x, y: point.y, width: 0, height: 0 } };
        setDraftElement({ rect: { x: point.x, y: point.y, width: 0, height: 0 }, line: false, rotation: 0 });
      }
      return;
    }
    if (tool === 'image' || (tool === 'insert' && insertKind === 'image')) {
      if (!pendingImage) { imagePicker.current?.click(); return; }
      drawElementDrag(event, point, 'image', false);
      return;
    }
    if (tool === 'insert' && insertKind === 'text') {
      drawElementDrag(event, point, 'text', false);
      return;
    }
    if (tool === 'insert' && !insertKind) return;
    if (DRAW_TOOLS.includes(tool) || (tool === 'insert' && insertKind)) {
      const kind: ElementKind = tool === 'insert' ? insertKind! : tool === 'rect' ? 'box' : tool === 'ellipse' ? 'ellipse' : 'line';
      drawElementDrag(event, point, kind, kind === 'line');
      return;
    }
    if (grip) return;

    if (tool === 'select' && elementNode) {
      const hit = elementTargetAt(project, selection, elementNode);
      if (hit) {
        const key = `element:${hit.pageId}:${hit.elementId}:${hit.sizeId ?? 'all'}`;
        const current = selection.some(target => canvasTargetKey(target) === key);
        const next = additive
          ? selection.some(target => canvasTargetKey(target) === key) ? selection.filter(target => canvasTargetKey(target) !== key) : [...selection, hit]
          : current ? selection : [hit];
        if (additive) selectElement(hit.pageId, hit.elementId, hit.sizeId, true);
        else if (!current) selectElement(hit.pageId, hit.elementId, hit.sizeId, false);
        else selectBoundary(hit.pageId, hit.sizeId, true);
        const entries = elementEntries(next);
        if (!entries.length) return;
        event.preventDefault();
        beginPointerSession(event);
        drag.current = { kind: 'move-elements', pointerId: event.pointerId, start: point, entries, rects: {}, moved: false, duplicate: event.altKey, axisLocked: false };
        surface.current?.focus({ preventScroll: true });
        return;
      }
    }
    if (tool === 'select' && itemNode) {
      const itemId = itemNode.dataset.reference!;
      const next = nextSelectionFor({ kind: 'reference', itemId }, additive);
      selectReference(itemId, additive);
      const active = beginMove(event, point, next);
      if (!active) return;
      event.preventDefault();
      beginPointerSession(event);
      drag.current = active;
      surface.current?.focus({ preventScroll: true });
      return;
    }
    if (tool === 'select' && handle && boundaryNode) {
      const pageId = boundaryNode.dataset.boundary!;
      const sizeId = handle.dataset.boundaryHandle!;
      const boundary = boundaries.find(item => item.sizeId === sizeId);
      if (!boundary) return;
      event.preventDefault();
      beginPointerSession(event);
      drag.current = { kind: 'move-boundary', pointerId: event.pointerId, start: point, pageId, sizeId, origin: boundary.rect, rect: boundary.rect, moved: false };
      selectBoundary(pageId, sizeId);
      surface.current?.focus({ preventScroll: true });
      return;
    }
    if (tool === 'select') {
      const item = referenceItemAt(project.reference.map(entry => ({ itemId: entry.id, rect: shownReference(entry, movingItems) })), point);
      if (item) {
        const next = nextSelectionFor({ kind: 'reference', itemId: item.itemId }, additive);
        selectReference(item.itemId, additive);
        const active = beginMove(event, point, next);
        if (!active) return;
        event.preventDefault();
        beginPointerSession(event);
        drag.current = active;
        surface.current?.focus({ preventScroll: true });
        return;
      }
      const boundary = boundaryUnder(point);
      event.preventDefault();
      beginPointerSession(event);
      drag.current = { kind: 'marquee', pointerId: event.pointerId, start: point, current: point, pageId: boundary?.pageId ?? null, additive, moved: false };
      surface.current?.focus({ preventScroll: true });
      return;
    }
    startPan(event, null);
  }

  /**
   * Capture is best effort: the window listeners below carry the drag even when a platform
   * declines or drops the capture (WebKitGTK on Wayland does both), so drawing never depends on
   * one element keeping the pointer.
   */
  function beginPointerSession(event: { pointerId: number }) {
    try { surface.current?.setPointerCapture(event.pointerId); } catch { /* The window keeps the drag. */ }
    if (dragSession.current) return;
    const move = (native: PointerEvent) => { if (drag.current?.pointerId === native.pointerId) dragHandlers.current?.move(native); };
    const end = (native: PointerEvent) => { if (drag.current?.pointerId === native.pointerId) dragHandlers.current?.end(native); };
    window.addEventListener('pointermove', move, true);
    window.addEventListener('pointerup', end, true);
    window.addEventListener('pointercancel', end, true);
    window.addEventListener('blur', cancelDrag);
    dragSession.current = { move, end };
  }

  function endPointerSession() {
    const session = dragSession.current;
    if (!session) return;
    window.removeEventListener('pointermove', session.move, true);
    window.removeEventListener('pointerup', session.end, true);
    window.removeEventListener('pointercancel', session.end, true);
    window.removeEventListener('blur', cancelDrag);
    dragSession.current = null;
  }

  /** A drag that loses its window drops the draft instead of committing half a gesture. */
  function cancelDrag() {
    if (!drag.current) return;
    drag.current = null;
    endPointerSession();
    setDraft(null); setDraftArea(null); setPolygonDraft(null); setDraftNote(null); setDraftElement(null); setMarquee(null); setZoomRect(null);
    setPointerDraft(null); setMoving(null); setMovingElements({}); setMovingItems({}); setPanning(false);
  }

  useEffect(() => endPointerSession, []);

  function pointerMove(event: DragInput) {
    const active = drag.current;
    if (!active || active.pointerId !== event.pointerId) return;
    if (active.kind === 'pan') {
      const delta = { x: event.clientX - active.start.x, y: event.clientY - active.start.y };
      if (!active.moved && Math.abs(delta.x) < 2 && Math.abs(delta.y) < 2) return;
      active.moved = true;
      touched.current = true;
      setCamera(panCamera(active.camera, delta));
      return;
    }
    const point = worldPoint(event);
    lastPointer.current = point;
    if (!active.moved && Math.hypot(point.x - active.start.x, point.y - active.start.y) * camera.zoom < 2) return;
    active.moved = true;
    if (active.kind === 'move-boundary') {
      active.rect = { ...active.origin, x: Math.round(active.origin.x + point.x - active.start.x), y: Math.round(active.origin.y + point.y - active.start.y) };
      setMoving({ pageId: active.pageId, sizeId: active.sizeId, rect: active.rect });
    } else if (active.kind === 'draw-boundary') setDraft(active.rect = boundaryDrawRect(active.start, point, { square: event.shiftKey, fromCentre: event.altKey }));
    else if (active.kind === 'draw-request-area') {
      active.current = point;
      if (active.shape === 'freehand') {
        const last = active.points[active.points.length - 1];
        if (Math.hypot(point.x - last.x, point.y - last.y) * camera.zoom >= 2) active.points.push(point);
        setDraftArea({ kind: 'freehand', points: [...active.points, point] });
      } else {
        const rect = drawRectWithKeys(active.start, point, { square: event.shiftKey, fromCentre: event.altKey });
        setDraftArea({ kind: active.shape === 'ellipse' ? 'ellipse' : 'rect', rect });
      }
    }
    else if (active.kind === 'draw-note') setDraftNote(active.rect = referenceNoteRect(active.start, point));
    else if (active.kind === 'draw-element') {
      if (active.line) {
        // A drawn line's rotation snaps to 15 degree steps while Shift is held.
        const end = event.shiftKey ? snappedLineEnd(active.start, point, 15) : point;
        const length = Math.max(1, Math.hypot(end.x - active.start.x, end.y - active.start.y));
        active.rotation = Math.atan2(end.y - active.start.y, end.x - active.start.x) * 180 / Math.PI;
        active.rect = { x: (active.start.x + end.x) / 2 - length / 2, y: (active.start.y + end.y) / 2, width: length, height: 0 };
      } else if (active.elementKind === 'box' || active.elementKind === 'ellipse') {
        active.rect = drawRectWithKeys(active.start, point, { snap: true, square: event.shiftKey, fromCentre: event.altKey });
      } else {
        active.rect = rectFromPoints(active.start, event.altKey ? point : snapPoint(point));
      }
      setDraftElement({ rect: active.rect, line: active.line, rotation: active.rotation });
    } else if (active.kind === 'marquee') {
      active.current = point;
      setMarquee(rectFromPoints(active.start, point));
    } else if (active.kind === 'zoom-rect') {
      active.current = point;
      setZoomRect(rectFromPoints(active.start, point));
    } else if (active.kind === 'move-elements') {
      active.axisLocked = event.shiftKey;
      const delta = elementMoveDelta(active.entries, active.start, point, !event.altKey, active.axisLocked);
      const rects: Record<string, Rect> = {};
      for (const entry of active.entries) {
        const world = { ...entry.world, x: entry.world.x + delta.x, y: entry.world.y + delta.y };
        rects[entry.id] = elementLocalRect(world, entry.boundaryOrigin, entry.parentOrigin);
      }
      active.rects = rects;
      setMovingElements(rects);
    } else if (active.kind === 'resize-element') {
      const localPoint = elementLocalRect({ x: point.x, y: point.y, width: 0, height: 0 }, active.entry.boundaryOrigin, active.entry.parentOrigin);
      // Images keep their ratio by default and Shift frees it; Alt resizes from the centre.
      const keepRatio = active.entry.element.kind === 'image' ? !event.shiftKey : event.shiftKey;
      active.rect = resizedRectWithKeys(active.entry.rect, active.corner, localPoint, { snap: true, ratio: keepRatio, fromCentre: event.altKey });
      setMovingElements({ [active.entry.id]: active.rect });
    } else if (active.kind === 'draw-pointer') {
      active.current = point;
      const hit = endpointAt(point);
      const route = nearestEdgeAnchors(active.sourceRect, hit?.rect ?? { ...point, width: 0, height: 0 });
      setPointerDraft({ from: route.from, to: route.to });
    } else if (active.kind === 'reattach-pointer') {
      active.current = point;
      const hit = endpointAt(point);
      const targetRect = hit?.rect ?? { ...point, width: 0, height: 0 };
      const route = active.end === 'from'
        ? nearestEdgeAnchors(targetRect, { ...active.fixed, width: 0, height: 0 })
        : nearestEdgeAnchors({ ...active.fixed, width: 0, height: 0 }, targetRect);
      setPointerDraft({ from: route.from, to: route.to });
    } else if (active.kind === 'move-selection') {
      active.axisLocked = event.shiftKey;
      const travel = { x: point.x - active.start.x, y: point.y - active.start.y };
      const delta = active.axisLocked ? axisLockedDelta(travel) : travel;
      const items: Record<string, Rect> = {};
      for (const target of active.targets) {
        const moved = movedReferenceRect(target.origin, delta);
        // Keep the locked axis on its exact starting value, grid snapping included.
        const rect = active.axisLocked
          ? { ...moved, x: delta.x === 0 ? target.origin.x : moved.x, y: delta.y === 0 ? target.origin.y : moved.y }
          : moved;
        active.rects[target.itemId] = rect;
        items[target.itemId] = rect;
      }
      setMovingItems(items);
    } else if (active.kind === 'resize-reference') {
      active.rect = resizedReferenceRect(active.origin, active.corner, point);
      setMovingItems({ [active.itemId]: active.rect });
    }
  }

  function boxAtPoint(pageId: string, sizeId: string, point: Point, skip: Set<string>): { id: string; origin: Point } | null {
    const surfaceNode = surface.current;
    const boundary = boundaryRect(sizeId);
    if (!surfaceNode || !boundary) return null;
    const host = surfaceNode.getBoundingClientRect();
    const client = { x: host.left + point.x * camera.zoom + camera.x, y: host.top + point.y * camera.zoom + camera.y };
    let found: { id: string; origin: Point; area: number } | null = null;
    for (const node of surfaceNode.querySelectorAll<HTMLElement>('[data-element-kind="box"]')) {
      if (node.closest<HTMLElement>('[data-size]')?.dataset.size !== sizeId) continue;
      const id = node.dataset.element!;
      if (skip.has(id)) continue;
      const box = node.getBoundingClientRect();
      if (client.x < box.left || client.x > box.right || client.y < box.top || client.y > box.bottom) continue;
      const path = elementPath(project, pageId, id, sizeId);
      const origin = path?.[path.length - 1]?.origin;
      if (!origin) continue;
      const area = box.width * box.height;
      if (!found || area < found.area) found = { id, origin, area };
    }
    return found ? { id: found.id, origin: found.origin } : null;
  }

  function marqueeTargets(rect: Rect): ElementTarget[] {
    const surfaceNode = surface.current;
    if (!surfaceNode) return [];
    const host = surfaceNode.getBoundingClientRect();
    const candidates: (ElementTarget & { rect: Rect })[] = [];
    for (const node of surfaceNode.querySelectorAll<HTMLElement>('[data-element]')) {
      const box = node.getBoundingClientRect();
      const screen = { x: box.left - host.left, y: box.top - host.top, width: box.width, height: box.height };
      if (!intersects(screen, rect)) continue;
      const hit = elementTargetAt(project, [], node);
      if (!hit) continue;
      candidates.push({ ...hit, rect: screen });
    }
    const unique = new Map<string, ElementTarget>();
    for (const candidate of candidates) {
      const element = findElementIn(pageFor(candidate.pageId, candidate.sizeId)?.elements ?? [], candidate.elementId);
      if (!element) continue;
      const ancestor = candidates.some(other => other.pageId === candidate.pageId && other.elementId !== candidate.elementId
        && elementParents(project, candidate.pageId, other.elementId).includes(candidate.elementId) && element.kind !== 'group');
      const { rect: _rect, ...target } = candidate;
      if (!ancestor) unique.set(`${candidate.pageId}:${candidate.elementId}:${candidate.sizeId ?? 'all'}`, target);
    }
    return [...unique.values()];
  }

  /** A move of the layer just duplicated shifts the offset the next duplicate repeats. */
  function rememberRepeatMove(entries: ElementEntry[], delta: Point) {
    const repeat = duplicateRepeat.current;
    if (repeat && repeat.key === entryTargetKey(entries)) {
      duplicateRepeat.current = { key: repeat.key, offset: { x: repeat.offset.x + delta.x, y: repeat.offset.y + delta.y } };
    }
  }

  /** Alt+drag leaves the originals in place and drops a copy at the dragged position. */
  async function duplicateElementMove(active: MoveElementsDrag, point: Point, snap: boolean) {
    const delta = elementMoveDelta(active.entries, active.start, point, snap, active.axisLocked);
    const groups = new Map<string, { pageId: string; parentId: string | null; index: number; sizeId: string | null; allSizes: boolean; elements: Element[]; targets: { pageId: string; elementId: string; sizeId: string | null }[] }>();
    for (const entry of active.entries) {
      const key = `${entry.pageId}\u0000${entry.parentId ?? ''}\u0000${entry.sizeId}\u0000${entry.allSizes}`;
      const group = groups.get(key) ?? { pageId: entry.pageId, parentId: entry.parentId, index: entry.index + 1, sizeId: entry.allSizes ? null : entry.sizeId, allSizes: entry.allSizes, elements: [], targets: [] };
      const element = cloneElement(entry.element);
      element.rect = { ...entry.rect, x: entry.rect.x + delta.x, y: entry.rect.y + delta.y };
      group.elements.push(element);
      group.targets.push({ pageId: entry.pageId, elementId: element.id, sizeId: entry.allSizes ? null : entry.sizeId });
      groups.set(key, group);
    }
    const targets: { pageId: string; elementId: string; sizeId: string | null }[] = [];
    for (const group of groups.values()) {
      await Promise.resolve(insertElements(group.pageId, group.parentId, group.index, group.elements, undefined, group.sizeId)).catch(() => {});
      targets.push(...group.targets);
    }
    if (targets.length) {
      // An Alt-drag duplicate arms Ctrl/Cmd+D to repeat the same offset.
      duplicateRepeat.current = { key: targetKey(targets), offset: delta };
      selectTargets(targets);
    }
  }

  async function commitElementMove(active: MoveElementsDrag, point: Point, snap: boolean): Promise<boolean> {
    const targetBoundary = boundaryUnder(point);
    const skip = new Set(active.entries.map(entry => entry.id));
    for (const entry of active.entries) for (const id of elementTreeIds(entry.element.children)) skip.add(id);
    const moves: ElementMove[] = [];
    const delta = elementMoveDelta(active.entries, active.start, point, snap, active.axisLocked);
    rememberRepeatMove(active.entries, delta);
    let converted = false;
    for (const entry of active.entries) {
      const world = { ...entry.world, x: entry.world.x + delta.x, y: entry.world.y + delta.y };
      if (!targetBoundary && entry.parentId === null) {
        await elementToReference(entry.pageId, entry.id, world);
        converted = true;
        continue;
      }
      const pageId = targetBoundary?.pageId ?? entry.pageId;
      const pageChanged = pageId !== entry.pageId;
      // A drag inside one size moves that size's position; a drag across sizes belongs to the
      // size it landed in. Moving to another page changes the shared base position.
      const sizeId = pageChanged ? null : targetBoundary?.sizeId ?? entry.sizeId;
      const allSizes = !pageChanged && entry.allSizes && (!targetBoundary || targetBoundary.sizeId === entry.sizeId);
      // A layer dropped on another boundary takes the shared base position there, so measure
      // from that page's base boundary even when the pointer landed on a linked size.
      const baseBoundary = pageChanged
        ? boundaryRect(project.pages.find(page => page.id === pageId)?.sizes[0]?.id ?? '')
        : null;
      const boundary = pageChanged ? baseBoundary ?? targetBoundary?.rect ?? null : boundaryRect(sizeId ?? entry.sizeId);
      if (!boundary) continue;
      const box = targetBoundary && sizeId ? boxAtPoint(pageId, sizeId, point, skip) : null;
      const parentId = box?.id ?? (pageChanged ? null : entry.parentId);
      const parentOrigin = box && parentId !== entry.parentId ? box.origin : parentId === entry.parentId && !pageChanged ? entry.parentOrigin : { x: 0, y: 0 };
      const sameParent = pageId === entry.pageId && parentId === entry.parentId;
      const index = sameParent ? entry.index : elementChildrenOf(project, pageId, parentId).length;
      moves.push({
        id: entry.id, to_page_id: pageId, to_parent_id: parentId, index,
        rect: elementLocalRect(world, boundary, parentOrigin),
        size_id: allSizes || pageChanged ? null : sizeId, all_sizes: allSizes,
      });
    }
    if (moves.length) await Promise.resolve(moveElements(moves)).catch(() => {});
    if (converted) clearSelection();
    else selectTargets(moves.map(move => ({ pageId: move.to_page_id, elementId: move.id, sizeId: move.all_sizes ? null : move.size_id ?? null })));
    return true;
  }

  function pointerUp(event: DragInput) {
    const active = drag.current;
    if (!active || active.pointerId !== event.pointerId) return;
    drag.current = null;
    endPointerSession();
    if (active.kind === 'pan') {
      setPanning(false);
      if (!active.moved && !active.pageId && event.button === 0) selectBoundary(null);
      return;
    }
    if (active.kind === 'zoom-rect') {
      setZoomRect(null);
      const node = surface.current;
      if (!node) return;
      const box = node.getBoundingClientRect();
      touched.current = true;
      if (active.moved && Math.abs(active.current.x - active.start.x) * camera.zoom >= 4 && Math.abs(active.current.y - active.start.y) * camera.zoom >= 4) {
        setCamera(fitCamera(rectFromPoints(active.start, active.current), { width: box.width, height: box.height }, 0, MAX_ZOOM));
        return;
      }
      if (!active.moved) {
        const at = worldToScreen(active.start);
        setCamera(current => zoomCameraAt(current, at, steppedZoom(current.zoom, active.zoomOut ? -1 : 1)));
      }
      return;
    }
    if (active.kind === 'move-boundary') { setMoving(null); if (active.moved) void persistMove(active.sizeId, active.rect); return; }
    if (active.kind === 'draw-boundary') {
      setDraft(null);
      // A click places the tool's default boundary, as a click does with every other draw tool.
      if (!active.moved) { void persistDraw({ x: active.start.x, y: active.start.y, ...CLICK_BOUNDARY_SIZE }); return; }
      void persistDraw(active.rect);
      return;
    }
    if (active.kind === 'draw-request-area') {
      setDraftArea(null);
      const points = [...active.points, active.current];
      if (active.shape === 'freehand') { if (points.length >= 3) closeRequestArea(points, 'freehand'); return; }
      const rect = drawRectWithKeys(active.start, active.current, { square: event.shiftKey, fromCentre: event.altKey });
      if (rect.width < 4 || rect.height < 4) return;
      closeRequestArea(rect, active.shape);
      return;
    }
    if (active.kind === 'draw-note') {
      setDraftNote(null);
      const id = crypto.randomUUID();
      const rect = active.rect;
      busy.current = true;
      void addReference({ id, rect, kind: { kind: 'note', text: '' } }).then(() => { createdItem.current = id; selectReference(id); }).catch(() => {}).finally(() => { busy.current = false; });
      return;
    }
    if (active.kind === 'draw-element') {
      setDraftElement(null);
      const boundary = boundaryRect(active.sizeId);
      if (!boundary) return;
      let rect = active.rect;
      if (!active.moved) {
        const size = active.elementKind === 'text' ? { width: 0, height: 0 } : active.elementKind === 'line' ? { width: 160, height: 0 } : defaultSize(active.elementKind);
        rect = { x: active.start.x, y: active.start.y, ...size };
      }
      if (active.elementKind === 'image' && pendingImage) {
        const width = Math.max(1, rect.width || Math.min(480, pendingImage.width));
        rect = { ...rect, width, height: Math.round(width * pendingImage.height / Math.max(1, pendingImage.width)) };
      }
      busy.current = true;
      const picture = active.elementKind === 'image' ? pendingImage?.uri : undefined;
      void placeElement(active.elementKind, active.pageId, active.sizeId, active.parentId, active.parentOrigin, active.boundary, rect, picture, active.rotation).then(element => {
        const createdNow = createdElement.current;
        if (element && createdNow) {
          if (active.elementKind === 'text') setEditing({ pageId: active.pageId, sizeId: active.sizeId, elementId: createdNow, autoWidth: !active.moved, created: true });
          else { createdElement.current = null; setEditing(null); }
          if (active.elementKind === 'image') { setPendingImage(null); onTool('select'); }
        }
      }).catch(() => {}).finally(() => { busy.current = false; });
      return;
    }
    if (active.kind === 'move-elements') {
      setMovingElements({});
      if (active.moved) {
        if (active.duplicate) void duplicateElementMove(active, lastPointer.current ?? active.start, !event.altKey).catch(() => {});
        else void commitElementMove(active, lastPointer.current ?? active.start, !event.altKey).catch(() => {});
      }
      return;
    }
    if (active.kind === 'resize-element') {
      setMovingElements({});
      if (!active.moved) return;
      const boundary = boundaryRect(active.entry.sizeId);
      if (!boundary) return;
      const sizing = active.entry.allSizes ? { all_sizes: true } : { size_id: active.entry.sizeId };
      if (active.entry.element.kind === 'group' && active.entry.rect.width > 0 && active.entry.rect.height > 0) {
        const sx = active.rect.width / active.entry.rect.width, sy = active.rect.height / active.entry.rect.height;
        const patches: ElementPatch[] = [{ id: active.entry.id, rect: active.rect, ...sizing }];
        for (const child of elementTree(active.entry.element.children)) {
          if (child.id === active.entry.id) continue;
          patches.push({ id: child.id, rect: { x: child.rect.x * sx, y: child.rect.y * sy, width: child.rect.width * sx, height: child.rect.height * sy }, ...sizing });
        }
        void Promise.resolve(updateElements(active.entry.pageId, patches)).catch(() => {});
      } else {
        void Promise.resolve(moveElements([{ id: active.entry.id, to_page_id: active.entry.pageId, to_parent_id: active.entry.parentId, index: active.entry.index, rect: active.rect,
          size_id: active.entry.allSizes ? null : active.entry.sizeId, all_sizes: active.entry.allSizes }])).catch(() => {});
      }
      return;
    }
    if (active.kind === 'marquee') {
      setMarquee(null);
      if (!active.moved) {
        if (active.pageId) selectBoundary(active.pageId);
        else selectBoundary(null);
        return;
      }
      const rect = rectFromPoints(worldToScreen(active.start), worldToScreen(active.current));
      const hits = marqueeTargets(rect);
      if (active.additive) {
        const existing = new Set(selection.map(canvasTargetKey));
        for (const hit of hits) if (!existing.has(`element:${hit.pageId}:${hit.elementId}:${hit.sizeId ?? 'all'}`)) selectElement(hit.pageId, hit.elementId, hit.sizeId, true);
      } else selectTargets(hits);
      return;
    }
    if (active.kind === 'draw-pointer') {
      setPointerDraft(null);
      if (!active.moved) return;
      const hit = endpointAt(lastPointer.current ?? active.current);
      if (!hit || samePointerEndpoint(active.source, hit.endpoint)) return;
      const pointer: Pointer = { id: crypto.randomUUID(), from: active.source, to: hit.endpoint as PointerTarget, comment: '', wiring: null };
      busy.current = true;
      void addPointer(pointer).then(() => { createdPointer.current = pointer.id; setArmedPointer(null); selectPointer(pointer.id); }).catch(() => {}).finally(() => { busy.current = false; });
      return;
    }
    if (active.kind === 'reattach-pointer') {
      setPointerDraft(null);
      setArmedPointer(null);
      if (!active.moved) return;
      const hit = endpointAt(lastPointer.current ?? active.current);
      if (!hit || samePointerEndpoint(active.original, hit.endpoint)) return;
      if (active.end === 'from' && hit.endpoint.kind !== 'element' && hit.endpoint.kind !== 'boundary') return;
      void updatePointer(active.pointer, active.end === 'from' ? { from: hit.endpoint as PointerSource } : { to: hit.endpoint as PointerTarget }).catch(() => {});
      return;
    }
    if (active.kind === 'move-selection') { setMovingItems({}); if (active.moved) void commitMove(active.targets, active.rects); return; }
    if (active.kind === 'resize-reference') { setMovingItems({}); if (active.moved) updateReference(active.itemId, { rect: active.rect }); }
  }

  function worldToScreen(point: Point): Point {
    return { x: point.x * camera.zoom + camera.x, y: point.y * camera.zoom + camera.y };
  }

  function startElementResize(event: ReactPointerEvent<HTMLButtonElement>, elementId: string, corner: ElementCorner, sizeId: string | null) {
    if (!surface.current) return;
    const target = selection.find(isElementTarget);
    if (!target || target.elementId !== elementId) return;
    const page = project.pages.find(item => item.id === target.pageId);
    if (!page) return;
    const editSizeId = sizeId ?? target.sizeId ?? baseSizeId(page);
    const path = elementPath(project, target.pageId, elementId, editSizeId);
    const last = path?.[path.length - 1];
    const boundary = boundaryRect(editSizeId);
    if (!last || !boundary) return;
    const over = last.element.overrides?.[editSizeId];
    const rect = over?.rect ?? last.element.rect;
    const origin = { x: last.parentOrigin.x + rect.x, y: last.parentOrigin.y + rect.y };
    event.preventDefault();
    event.stopPropagation();
    beginPointerSession(event);
    drag.current = { kind: 'resize-element', pointerId: event.pointerId, start: worldPoint(event), corner, rect,
      entry: { id: elementId, element: last.element, pageId: target.pageId, sizeId: editSizeId, allSizes: target.sizeId === null, parentId: last.parentId, index: last.index, rect, parentOrigin: last.parentOrigin,
        boundaryOrigin: { x: boundary.x, y: boundary.y }, world: { x: boundary.x + origin.x, y: boundary.y + origin.y, width: rect.width, height: rect.height } }, moved: false };
    surface.current.focus({ preventScroll: true });
  }

  function startReferenceResize(event: ReactPointerEvent<HTMLButtonElement>, corner: FrameCorner, item: ReferenceItem) {
    if (!surface.current) return;
    event.preventDefault();
    event.stopPropagation();
    beginPointerSession(event);
    drag.current = { kind: 'resize-reference', pointerId: event.pointerId, start: worldPoint(event), corner, itemId: item.id, origin: shownReference(item, movingItems), rect: shownReference(item, movingItems), moved: false };
    surface.current.focus({ preventScroll: true });
  }

  function startReattach(event: ReactPointerEvent<HTMLButtonElement>, pointerId: string, end: 'from' | 'to') {
    if (!surface.current) return;
    const pointer = (project.pointers ?? []).find(item => item.id === pointerId);
    if (!pointer) return;
    const route = pointerRoute(pointer, pointerWorldNow());
    if (!route) return;
    event.preventDefault();
    event.stopPropagation();
    beginPointerSession(event);
    const point = worldPoint(event);
    drag.current = { kind: 'reattach-pointer', pointerId: event.pointerId, start: point, pointer: pointer.id, end,
      original: end === 'from' ? pointer.from : pointer.to, fixed: end === 'from' ? route.to : route.from, current: point, moved: false };
    surface.current.focus({ preventScroll: true });
  }

  async function persistMove(sizeId: string, rect: Rect) {
    const anchor = boundaries.find(item => item.sizeId === sizeId);
    if (!anchor) return;
    const before = boundaries.map(item => item.sizeId === sizeId ? { ...item, rect } : item);
    const next = pushApartBoundaries(before, sizeId);
    const boxes = referenceBoxes();
    const pushedItems = pushApartReference(boxes, next);
    await saveMoves(boundaryMoves(next, boundaries), referenceMoves(pushedItems, boxes));
  }

  async function persistDraw(rect: Rect) {
    busy.current = true;
    try {
      const anchor = '\u0000new';
      const pushed = pushApartBoundaries([...boundaries, { pageId: anchor, sizeId: anchor, label: 'Size', rect, kind: boundaryKind }], anchor);
      const boxes = referenceBoxes();
      const pushedItems = pushApartReference(boxes, pushed);
      const pageId = await createBoundary(rect, boundaryKind, boundaryMoves(pushed.filter(item => item.sizeId !== anchor), boundaries), referenceMoves(pushedItems, boxes));
      if (pageId) selectBoundary(pageId);
    } catch { /* App surfaces the failed command. */ }
    finally { busy.current = false; }
  }

  async function saveMoves(moves: BoundaryMove[], reference: ReferenceMove[]) {
    if (!moves.length && !reference.length) return;
    busy.current = true;
    try { await savePlacements(moves, reference); } catch { /* App surfaces the failed command. */ }
    finally { busy.current = false; }
  }

  async function commitMove(targets: MoveTarget[], rects: Record<string, Rect>) {
    busy.current = true;
    try {
      const boundaryList = boundaries.map(item => moving?.sizeId === item.sizeId ? { ...item, rect: moving.rect } : item);
      for (const target of targets) {
        const rect = rects[target.itemId];
        if (!rect) continue;
        const boundary = boundaryAt(boundaryList, centre(rect));
        const item = project.reference.find(entry => entry.id === target.itemId);
        if (boundary && item) {
          await referenceToElement(target.itemId, boundary.pageId, pageRectFromCanvas(referenceFrameRect(rect, boundary.rect), boundary.rect), boundary.sizeId);
        } else updateReference(target.itemId, { rect });
      }
    } catch { /* App surfaces the failed command. */ }
    finally { busy.current = false; }
  }

  function soleReferenceItem(): ReferenceItem | null {
    if (selection.length !== 1 || selection[0].kind !== 'reference') return null;
    const itemId = selection[0].itemId;
    return project.reference.find(item => item.id === itemId) ?? null;
  }

  function createReference(text: string, point: Point): boolean {
    if (/^#([0-9a-f]{3}|[0-9a-f]{6})$/i.test(text)) {
      const id = crypto.randomUUID();
      void addReference({ id, rect: { x: point.x, y: point.y, width: 120, height: 120 }, kind: { kind: 'swatch', color: text.toLowerCase() } })
        .then(() => selectReference(id)).catch(() => {});
      return true;
    }
    let url: URL;
    try { url = new URL(text); } catch { return false; }
    if (url.protocol !== 'http:' && url.protocol !== 'https:') return false;
    const id = crypto.randomUUID();
    void addReference({ id, rect: { x: point.x, y: point.y, width: 240, height: 100 }, kind: { kind: 'link', url: text, title: url.hostname } })
      .then(() => selectReference(id)).catch(() => {});
    return true;
  }

  /** Under the pointer when it is over the canvas, otherwise the middle of what is visible. */
  function referencePoint(): Point | null {
    if (lastPointer.current) return lastPointer.current;
    if (viewSize.width < 1 || viewSize.height < 1) return null;
    return { x: (viewSize.width / 2 - camera.x) / camera.zoom, y: (viewSize.height / 2 - camera.y) / camera.zoom };
  }

  /** Pasted text follows the bounds: a layer inside a boundary, reference outside every one. */
  async function pasteText(text: string, point: Point) {
    const boundary = boundaryUnder(point);
    if (!boundary) {
      if (createReference(text, point)) return;
      const id = crypto.randomUUID();
      await addReference({ id, rect: pastedNoteRect(point), kind: { kind: 'note', text: text.slice(0, 16384) } })
        .then(() => selectReference(id)).catch(() => {});
      return;
    }
    const page = project.pages.find(item => item.id === boundary.pageId);
    const local = elementRectAt(boundary.sizeId, null, { x: 0, y: 0 }, boundary.rect, { ...point, width: 0, height: 0 });
    const element = withPickedColours(forAppearance(createTextElement({ ...local, width: 0, height: 0 }, text), page?.appearance ?? 'light'));
    const singleSize = !!page && hasManySizes(page) && !editingAllSizes(boundary.pageId);
    busy.current = true;
    try {
      await insertElements(boundary.pageId, null, insertionPoint(boundary.pageId, null), [element], undefined, singleSize ? boundary.sizeId : null);
      selectElement(boundary.pageId, element.id, singleSize ? boundary.sizeId : null);
    } catch { /* App surfaces the failed command. */ }
    finally { busy.current = false; }
  }

  /** Pasted pictures take the Image tool's path, or the reference path outside every boundary. */
  async function pasteImage(files: File[]) {
    // One picture per paste keeps the whole paste a single undo step.
    const point = referencePoint();
    const image = await readPastedImage(files[0]);
    if (!image || !point) return;
    const rect = pastedImageRect(point, image.width, image.height);
    const boundary = boundaryUnder(point);
    if (boundary) {
      busy.current = true;
      try { await placeElement('image', boundary.pageId, boundary.sizeId, null, { x: 0, y: 0 }, boundary.rect, rect, image.uri); }
      catch { /* App surfaces the failed command. */ }
      finally { busy.current = false; createdElement.current = null; }
      return;
    }
    const id = crypto.randomUUID();
    await addReference({ id, rect, kind: { kind: 'image', picture: NIL_UUID, name: image.name.slice(0, 120) } }, image.uri)
      .then(() => selectReference(id)).catch(() => {});
  }

  /** One paste path for the surface and for focus that sits elsewhere: content, not the event. */
  function pasteClipboard(data: DataTransfer | null, target: EventTarget | null): boolean {
    if (!data || isTextField(target)) return false;
    const images = pastedImageFiles(data);
    if (images.length) { void pasteImage(images); return true; }
    const text = data.getData('text').trim();
    const internal = clipboard.current.length > 0;
    // Our marker on the clipboard means the layers copied here; unreadable text falls back to them.
    if (internal && (!clipboardMark.current || !text || text === clipboardMark.current)) { pasteElements(false); return true; }
    if (!text) return false;
    if (isClipboardMarker(text)) return true;
    const point = referencePoint();
    if (!point) return false;
    void pasteText(text, point);
    return true;
  }

  function paste(event: React.ClipboardEvent<HTMLDivElement>) {
    if (pasteClipboard(event.clipboardData, event.target)) event.preventDefault();
  }

  // Layers copied here, or content from another app, pasted while focus sits outside the surface
  // (the Layers panel, say) still lands on the canvas, like the other canvas-wide keys.
  useEffect(() => {
    const handle = (event: ClipboardEvent) => {
      if (event.defaultPrevented) return;
      if (surface.current && event.target instanceof Node && surface.current.contains(event.target)) return;
      if (pasteHandler.current(event.clipboardData, event.target)) event.preventDefault();
    };
    window.addEventListener('paste', handle);
    return () => window.removeEventListener('paste', handle);
  }, []);

  function dropText(event: ReactDragEvent<HTMLDivElement>) {
    if (isTextField(event.target) || event.dataTransfer.files.length) return;
    const text = (event.dataTransfer.getData('text/uri-list') || event.dataTransfer.getData('text/plain') || '').trim();
    if (!text) return;
    event.preventDefault();
    const point = worldPoint(event);
    if (boundaryAt(latest.current.boundaries, point)) {
      const element = elementAt(elementBoxesNow(latest.current.project, latest.current.boundaries), point);
      if (element) attachToRequest(element.elementId, [], [text]);
      return;
    }
    createReference(text, point);
  }

  function keyDown(event: ReactKeyboardEvent<HTMLDivElement>) {
    if (event.defaultPrevented) return;
    if (event.key === 'Enter' && polygonDraft && polygonDraft.length >= 3) { event.preventDefault(); closeRequestArea(polygonDraft, 'polygon'); return; }
    if (event.key === 'Enter' && focused && !selection.length) { event.preventDefault(); openPage(focused); }
    else if (event.key === 'Escape') { event.preventDefault(); escapeCanvas(); }
  }

  /** Double click with Select edits the layer in every linked size, not just this one. */
  function doubleClick(event: React.MouseEvent<HTMLDivElement>) {
    if (tool === 'request' && requestShape === 'polygon' && polygonDraft && polygonDraft.length >= 3) {
      event.preventDefault();
      event.stopPropagation();
      closeRequestArea(polygonDraft, 'polygon');
      return;
    }
    if (tool !== 'select') return;
    const hit = elementTargetAt(project, selection, event.target);
    if (!hit) return;
    event.preventDefault();
    event.stopPropagation();
    selectElement(hit.pageId, hit.elementId, null, false);
    surface.current?.focus({ preventScroll: true });
  }

  const dots = dotGrid(camera, project.showGrid);
  const dotStyle: CSSProperties | undefined = dots ? {
    backgroundImage: `radial-gradient(circle at 1px 1px, color-mix(in srgb, var(--canvas-dot) ${Math.round(dots.fade * 100)}%, transparent) 1px, transparent 1.5px)`,
    backgroundSize: `${dots.size}px ${dots.size}px`,
    backgroundPosition: `${dots.x}px ${dots.y}px`,
  } : undefined;
  const selectedItem = soleReferenceItem();
  const selectedItemScreen = selectedItem ? screenRect(shownReference(selectedItem, movingItems), camera) : null;
  const inverse = 1 / camera.zoom;
  const pointers = project.pointers ?? [];
  const pointerWorldNowValue = pointerWorld(project, boundaries, moving, {}, movingItems);
  const viewport: Rect = viewSize.width > 0 ? viewportWorldRect(camera, viewSize) : { x: -1e7, y: -1e7, width: 2e7, height: 2e7 };
  const selectedPointerIds = new Set(selectedPointers(selection));
  const entries = pointerEntries(pointers, pointerWorldNowValue, selectedPointerIds, viewport);
  const selectedPointerId = selection.length === 1 && selection[0].kind === 'pointer' ? selection[0].pointerId : null;
  const choosePointer = (pointerId: string) => { selectPointer(pointerId); surface.current?.focus({ preventScroll: true }); };
  const autoLinks = automaticLinks(project, boundaries, elementBoxes(project, boundaries, moving));
  const selectedElementTargets = selection.flatMap(target => target.kind === 'element' ? [target] : []);
  const editingElement = editing ? findElementIn(pageFor(editing.pageId, editing.sizeId)?.elements ?? [], editing.elementId) : null;
  const editingBoundary = editing ? boundaries.find(item => item.sizeId === editing.sizeId) ?? null : null;
  const editingScreen = editing && editingElement
    ? elementScreenRect(editing.elementId, editing.sizeId) ?? (editingBoundary ? textEditorScreen(project, editing.pageId, editingElement, editingBoundary, camera, editing.sizeId) : null)
    : null;

  function elementScreenRect(elementId: string, sizeId?: string | null): ScreenRect | null {
    const scope = sizeId ? `[data-size="${CSS.escape(sizeId)}"] ` : '';
    const node = surface.current?.querySelector<HTMLElement>(`${scope}[data-element="${CSS.escape(elementId)}"]`);
    if (!node || !surface.current) return null;
    const host = surface.current.getBoundingClientRect();
    const box = node.getBoundingClientRect();
    return { left: box.left - host.left, top: box.top - host.top, width: box.width, height: box.height };
  }

  async function commitEditingText(text: string, size?: { width: number; height: number }) {
    if (!editing) return;
    const element = findElementIn(pageFor(editing.pageId, editing.sizeId)?.elements ?? [], editing.elementId);
    if (!element) { setEditing(null); return; }
    // Text is shared across sizes; an auto width belongs to the size it was typed in.
    const patch: ElementPatch = { id: editing.elementId, text };
    if (size && editing.autoWidth) patch.rect = { ...element.rect, width: size.width, height: size.height };
    setEditing(null);
    createdElement.current = null;
    const page = project.pages.find(item => item.id === editing.pageId);
    const sized = size && page && hasManySizes(page) ? { size_id: editing.sizeId } : {};
    await Promise.resolve(updateElements(editing.pageId, [{ ...patch, ...(patch.rect ? sized : {}) }])).catch(() => {});
    selectElement(editing.pageId, editing.elementId, page && hasManySizes(page) ? editing.sizeId : null);
    surface.current?.focus({ preventScroll: true });
  }

  function cancelEditingText() {
    if (!editing) return;
    const current = editing;
    setEditing(null);
    if (current.created) removeElements([{ pageId: current.pageId, elementId: current.elementId }], false);
    else selectElement(current.pageId, current.elementId, current.sizeId);
  }

  const previewBoundary = preview ? boundaries.find(item => item.pageId === preview.pageId) ?? null : null;
  const previewScreen = previewBoundary ? screenRect(previewBoundary.rect, camera) : null;
  return <div ref={surface} id="project-canvas" className={`project-canvas tool-${tool}${tool === 'request' ? ` request-shape-${requestShape}` : ''}${panning ? ' panning' : ''}${spaceHeld ? ' pan-ready' : ''}${zoomHeld ? ' zoom-ready' : ''}${painterArmed ? ' paint-format' : ''}${playIsActive(play) ? ' playing' : ''}${dropHover && !dropHover.rect ? ' drop-bed' : ''}`} style={dotStyle} tabIndex={0}
    onPointerDown={pointerDown} onKeyDown={keyDown} onDoubleClickCapture={doubleClick} onPaste={paste}
    onDragOver={event => { if (!isTextField(event.target)) event.preventDefault(); }} onDrop={dropText}>
    <input ref={imagePicker} type="file" hidden accept="image/png,image/jpeg,image/webp,image/gif" onChange={event => { chooseImage(event.target.files?.[0]); event.target.value = ''; }} />
    <div className="project-canvas-layer" style={{ transform: `translate(${camera.x}px, ${camera.y}px) scale(${camera.zoom})` }}>
      <BoundaryLayer project={project} boundaries={boundaries} focused={focused} selection={selection}
        moving={moving} tool={tool} spaceHeld={spaceHeld} openPage={openPage} editingId={editing?.elementId ?? null}
        movingElements={movingElements} onPlaceholder={openPlaceholder} play={play} />
      {dropHover?.rect && <div className="canvas-drop-target" style={{ left: dropHover.rect.x, top: dropHover.rect.y, width: dropHover.rect.width, height: dropHover.rect.height, borderWidth: 2 * inverse }} />}
      <RequestLayer project={project} boundaries={boundaries} selection={selection} draft={requestDraft}
        draftArea={draftArea} inverse={inverse} selectRequest={selectRequest} />
      <ReferenceLayer project={project} movingItems={movingItems} selection={selection} draftNote={draftNote} />
      {draft && <div className={`boundary-draft${boundaryKind === 'screen' ? ' screen' : ''}`} style={{ left: draft.x, top: draft.y, width: draft.width, height: draft.height, borderWidth: 2 * inverse }} />}
      {draftElement && <div className={`element-draft${draftElement.line ? ' line' : ''}`} style={{ left: draftElement.rect.x, top: draftElement.rect.y, width: draftElement.rect.width, height: draftElement.rect.height, borderWidth: inverse, transform: draftElement.rotation ? `rotate(${draftElement.rotation}deg)` : undefined }} />}
      {marquee && <div className="element-marquee" style={{ left: marquee.x, top: marquee.y, width: marquee.width, height: marquee.height, borderWidth: inverse }} />}
      {zoomRect && <div className="zoom-marquee" style={{ left: zoomRect.x, top: zoomRect.y, width: zoomRect.width, height: zoomRect.height, borderWidth: inverse }} />}
      <PointerLines entries={entries} autoLinks={autoLinks} draft={pointerDraft} camera={camera} viewport={viewport}
        selected={selectedPointerIds} pointerStates={pointerStates} enabled={tool === 'select'} onSelect={choosePointer} />
    </div>
    <PointerComments entries={entries} camera={camera} selected={selectedPointerIds} pointerStates={pointerStates} interactive={tool === 'select'} onSelect={choosePointer}
      change={(pointerId, comment) => updatePointer(pointerId, { comment })}
      autoFocusId={createdPointer.current} close={() => { createdPointer.current = null; surface.current?.focus({ preventScroll: true }); }} />
    <BoundaryLabels project={project} boundaries={boundaries} moving={moving} focused={focused} selection={selection} camera={camera} openPage={openPage} />
    {minimapVisible && <Minimap boundaries={boundaries} reference={project.reference} camera={camera} viewSize={viewSize}
      move={next => { stopTransition(); touched.current = true; setCamera(next); }} />}
    <PointerHandles entries={entries} camera={camera} selectedId={selectedPointerId} armed={armedPointer} onStart={startReattach} />
    {!editing && <ElementSelectionOverlay surface={surface} project={project} selection={selectedElementTargets} camera={camera}
      version={Object.entries(movingElements).map(([id, rect]) => `${id}:${rect.x},${rect.y},${rect.width},${rect.height}`).join('|')} onResizeStart={startElementResize} />}
    {editing && editingElement && editingScreen && <ElementTextEditor key={`${editing.elementId}:${editing.created}`} element={editingElement} screen={editingScreen} zoom={camera.zoom}
      autoWidth={editing.autoWidth} autoFocus onCommit={commitEditingText} onCancel={cancelEditingText} />}
    {placeholderChoice && <PlaceholderChoiceRow screen={placeholderChoice.screen}
      include={() => resolvePlaceholder(placeholderChoice, true)} dismiss={() => resolvePlaceholder(placeholderChoice, false)}
      close={() => { setPlaceholderChoice(null); surface.current?.focus({ preventScroll: true }); }} />}
    {requestDraft && <RequestBoxEditor screen={requestDraft.screen} text="" submit={submitRequestDraft} close={cancelRequestDraft} />}
    {selectedItem && selectedItemScreen && selectedItem.kind.kind === 'note' && <ReferenceNoteEditor key={selectedItem.id} item={selectedItem}
      kind={selectedItem.kind} screen={selectedItemScreen} autoFocus={createdItem.current === selectedItem.id}
      change={text => updateReference(selectedItem.id, { text })} close={() => { createdItem.current = null; surface.current?.focus({ preventScroll: true }); }} />}
    {selectedItem && selectedItemScreen && <>
      {CORNERS.map(corner => <button key={corner} type="button" data-reference-corner={corner} className={`canvas-grip reference-grip ${corner}`} aria-label={`Resize reference item ${corner}`} title="Drag to resize"
        style={{ left: selectedItemScreen.left + (corner === 'ne' || corner === 'se' ? selectedItemScreen.width : 0) - 5, top: selectedItemScreen.top + (corner === 'sw' || corner === 'se' ? selectedItemScreen.height : 0) - 5 }}
        onPointerDown={event => startReferenceResize(event, corner, selectedItem)} />)}
    </>}
    {pasteChoice && <PasteChoiceRow screen={pasteChoice.screen} value={pasteChoice.value} useDefault={pasteChoice.useDefault}
      change={value => setPasteChoice(current => current ? { ...current, value } : current)}
      toggleDefault={value => setPasteChoice(current => current ? { ...current, useDefault: value } : current)}
      confirm={confirmPaste} cancel={() => { setPasteChoice(null); surface.current?.focus({ preventScroll: true }); }} />}
    {linkChoice && <LinkChoiceRow screen={linkChoice.screen} copies={linkChoice.copies} error={linkChoice.error}
      pick={id => linkCopies(linkChoice.copies.map(copy => copy.id), id)}
      cancel={() => { setLinkChoice(null); surface.current?.focus({ preventScroll: true }); }} />}
    {preview && previewScreen && <div className="preview-overlay" style={{ left: previewScreen.left, top: previewScreen.top, width: previewScreen.width, height: previewScreen.height }} onPointerDown={event => event.stopPropagation()}>
      <div className="preview-overlay-bar">
        <span className="preview-overlay-title">Preview</span>
        {preview.stale ? <><span className="preview-outdated">Preview is out of date</span><button type="button" className="dock-action" onClick={previewRefresh}>Refresh</button></> : null}
        <button type="button" className="dock-action" onClick={previewClose}>Close (Esc)</button>
      </div>
      <iframe className="preview-overlay-frame" title="Preview export" src={preview.url} sandbox="allow-scripts allow-forms allow-popups allow-modals" />
    </div>}
    {dropNotice && <Notice key={dropNotice.id} variant={dropNotice.variant} message={dropNotice.message} dismiss={() => setDropNotice(null)} />}
  </div>;
});

function elementMoveDelta(entries: ElementEntry[], start: Point, point: Point, snap: boolean, axisLock: boolean): Point {
  const travel = { x: point.x - start.x, y: point.y - start.y };
  if (!snap || !entries.length) return axisLock ? axisLockedDelta(travel) : travel;
  const first = entries[0].world;
  const delta = { x: snapValue(first.x + travel.x) - first.x, y: snapValue(first.y + travel.y) - first.y };
  if (!axisLock) return delta;
  // The locked axis stays exactly where it was; snapping only adjusts the axis being dragged.
  const axis = axisLockedDelta(travel);
  return { x: axis.x === 0 ? 0 : delta.x, y: axis.y === 0 ? 0 : delta.y };
}

/** Boundary drawing with Shift (square) and Alt (from the centre), on the boundary grid. */
function boundaryDrawRect(start: Point, point: Point, keys: { square?: boolean; fromCentre?: boolean } = {}): Rect {
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

const targetKey = (targets: { pageId: string; elementId: string }[]): string =>
  targets.map(target => `${target.pageId}:${target.elementId}`).sort().join('|');
const entryTargetKey = (entries: { pageId: string; id: string }[]): string => targetKey(entries.map(entry => ({ pageId: entry.pageId, elementId: entry.id })));

function elementTree(elements: Element[]): Element[] {
  return elements.flatMap(element => [element, ...elementTree(element.children)]);
}

function elementTreeIds(elements: Element[]): string[] {
  return elementTree(elements).map(element => element.id);
}

function defaultSize(kind: ElementKind): { width: number; height: number } {
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
