import type { ProjectCanvasProps } from './interactionTypes';
import { type PointerEvent as ReactPointerEvent } from 'react';
import {
  canvasTargetKey, fitCamera, MAX_ZOOM, referenceItemAt, referenceNoteRect, movedReferenceRect, panCamera,
  resizedReferenceRect, samePointerEndpoint, zoomCameraAt, type Boundary, type CanvasCamera, type CanvasTarget,
  type ElementTarget, type Point,
  CLICK_NOTE_SIZE,
} from '../projectCanvasGeometry';
import { steppedZoom } from '../canvasGeometry';
import type { Element, ElementKind, ElementMove, ElementPatch, Page, Pointer, PointerSource, PointerTarget, Project, Rect, ReferenceItem, RequestArea, RequestTarget } from '../types';
import type { ProjectTool, RequestToolShape } from '../EditorTools';
import { dedupeTargets, requestTargetKey } from './requests';
import { shownReference } from './reference';
import { type ScreenRect } from './surface';
import { elementLocalRect, isTextKind } from './elementGeometry';
import { playIsActive } from './play';
import { behaviourFor, behaviourPointer } from './behaviours';
import {
  elementTree, findElementIn, axisLockedDelta, drawRectWithKeys, rectFromPoints, resizedRectWithKeys,
  snappedLineEnd, snapPoint,
} from './elementGeometry';
import {
  MoveTarget, MoveSelectionDrag, ElementEntry, PasteChoice, LinkDecision, MoveElementsDrag, DrawElementDrag,
  Drag, DRAW_TOOLS, DragInput, elementChain, chainPage, elementTargetAt, elementMoveDelta,
  boundaryDrawRect, defaultSize,
} from './interactionTypes';

type Context = {
  busy: import("react").RefObject<boolean>;
  startPan: (event: ReactPointerEvent<HTMLDivElement>, pageId: string | null) => void;
  pasteChoice: PasteChoice | null;
  linkChoice: LinkDecision | null;
  setPasteChoice: import("react").Dispatch<import("react").SetStateAction<PasteChoice | null>>;
  setLinkChoice: import("react").Dispatch<import("react").SetStateAction<LinkDecision | null>>;
  worldPoint: (event: { clientX: number; clientY: number; }) => Point;
  lastPointer: import("react").RefObject<Point | null>;
  held: import("react").RefObject<import("./surface").HeldKeys>;
  drag: { current: Drag | null; };
  painterArmed: boolean;
  project: Project;
  selection: CanvasTarget[];
  applyPainterStyle: (target: ElementTarget) => void;
  surface: import("react").RefObject<HTMLDivElement | null>;
  transitionPainter: (event: import("./keyboardActions").PainterEvent) => void;
  selectRequest: (requestId: string) => void;
  latest: import("react").RefObject<Pick<ProjectCanvasProps, 'play' | 'onPlayElement'>>;
  tool: import("../EditorTools").EditorTool;
  requestShape: RequestToolShape;
  requestTargetAt: (point: Point) => RequestTarget | null;
  requestDraft: { targets: RequestTarget[]; area: RequestArea | null; screen: ScreenRect; } | null;
  setRequestDraft: import("react").Dispatch<import("react").SetStateAction<{ targets: RequestTarget[]; area: RequestArea | null; screen: ScreenRect; } | null>>;
  requestEditorScreen: (targets: RequestTarget[], area: RequestArea | null) => ScreenRect;
  setPolygonDraft: (points: Point[] | null) => void;
  polygonDraft: Point[] | null;
  camera: CanvasCamera;
  closeRequestArea: (input: Rect | Point[], kind: 'rect' | 'ellipse' | 'polygon' | 'freehand') => void;
  boundaryUnder: (point: Point) => Boundary | null;
  boundaryNear: (point: Point) => Boundary | null;
  pointerSourceAt: (point: Point) => { endpoint: PointerSource; rect: Rect; } | null;
  pageFor: (pageId: string, sizeId?: string | null) => Page | null;
  startEditingElement: (pageId: string, elementId: string, sizeId: string, created: boolean) => void;
  editingSizeId: (pageId: string) => string;
  drawElementDrag: (event: ReactPointerEvent<HTMLDivElement>, point: Point, kind: ElementKind, line: boolean) => DrawElementDrag | null;
  insertKind: ElementKind | null;
  pendingImage: { uri: string; width: number; height: number; name: string; } | null;
  imagePicker: import("react").RefObject<HTMLInputElement | null>;
  selectElement: (pageId: string, elementId: string, sizeId: string | null, additive?: boolean) => void;
  selectBoundary: (pageId: string | null, sizeId?: string | null, additive?: boolean) => void;
  elementEntries: (targets: CanvasTarget[]) => ElementEntry[];
  nextSelectionFor: (target: CanvasTarget, additive: boolean) => CanvasTarget[];
  selectReference: (itemId: string, additive?: boolean) => void;
  beginMove: (event: ReactPointerEvent, point: Point, targets: CanvasTarget[]) => MoveSelectionDrag | null;
  boundaries: Boundary[];
  movingItems: Record<string, Rect>;
  dragSession: import("react").RefObject<{ move: (event: PointerEvent) => void; end: (event: PointerEvent) => void; } | null>;
  dragHandlers: import("react").RefObject<{ move: (event: DragInput) => void; end: (event: DragInput) => void; } | null>;
  setPanning: import("react").Dispatch<import("react").SetStateAction<boolean>>;
  touched: import("react").RefObject<boolean>;
  setCamera: import("react").Dispatch<import("react").SetStateAction<CanvasCamera>>;
  renderDrag: import("react").Dispatch<import("react").SetStateAction<number>>;
  worldToScreen: (point: Point) => Point;
  persistMove: (sizeId: string, rect: Rect) => Promise<void>;
  persistDraw: (rect: Rect, clicked?: boolean) => Promise<void>;
  addReference: (item: ReferenceItem, pictureDataUri?: string) => Promise<void>;
  createdItem: import("react").RefObject<string | null>;
  boundaryRect: (sizeId: string) => Rect | null;
  placeElement: (kind: ElementKind, pageId: string, sizeId: string, parentId: string | null, parentOrigin: Point, boundary: Rect, rect: Rect, picture?: string, rotation?: number) => Promise<Element>;
  createdElement: import("react").RefObject<string | null>;
  transitionText: (event: import("./drawing").TextEditingEvent) => void;
  setPendingImage: import("react").Dispatch<import("react").SetStateAction<{ uri: string; width: number; height: number; name: string; } | null>>;
  onTool: (tool: ProjectTool) => void;
  duplicateElementMove: (active: MoveElementsDrag, point: Point, snap: boolean) => Promise<void>;
  commitElementMove: (active: MoveElementsDrag, point: Point, snap: boolean) => Promise<boolean>;
  updateElements: (pageId: string, patches: ElementPatch[]) => Promise<unknown> | void;
  moveElements: (moves: ElementMove[]) => Promise<unknown> | void;
  marqueeTargets: (rect: Rect) => ElementTarget[];
  selectTargets: (targets: { pageId: string; elementId: string; sizeId?: string | null; }[]) => void;
  endpointAt: (point: Point) => { endpoint: PointerSource | PointerTarget; rect: Rect; } | null;
  addPointer: (pointer: Pointer) => Promise<void>;
  focusBehaviour: (source: PointerSource) => void;
  createdPointer: import("react").RefObject<string | null>;
  setArmedPointer: import("react").Dispatch<import("react").SetStateAction<{ pointerId: string; end: 'from' | 'to'; } | null>>;
  selectPointer: (pointerId: string, additive?: boolean) => void;
  updatePointer: (pointerId: string, patch: { from?: PointerSource; to?: PointerTarget; comment?: string; kind?: Pointer['kind']; }) => Promise<unknown>;
  commitMove: (targets: MoveTarget[], rects: Record<string, Rect>) => Promise<void>;
  updateReference: (itemId: string, patch: { rect?: Rect; text?: string; url?: string; title?: string; color?: string; }) => void;
};

export function createCanvasInteraction(context: () => Context) {
  function pointerDown(event: ReactPointerEvent<HTMLDivElement>) {
    const { busy, startPan, pasteChoice, linkChoice, setPasteChoice, setLinkChoice, worldPoint, lastPointer, held, drag, painterArmed, project, selection, applyPainterStyle, surface, transitionPainter, selectRequest, latest, tool, requestShape, requestTargetAt, requestDraft, setRequestDraft, requestEditorScreen, setPolygonDraft, polygonDraft, camera, closeRequestArea, boundaryUnder, boundaryNear, pointerSourceAt, pageFor, startEditingElement, editingSizeId, drawElementDrag, insertKind, pendingImage, imagePicker, selectElement, selectBoundary, elementEntries, nextSelectionFor, selectReference, beginMove, boundaries, movingItems } = context();
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
    const spaceHeld = held.current === 'space' || held.current === 'space-and-zoom';
    // Z arms zooming: drag a rectangle to fit it, or click a zoom step at the pointer.
    if (event.button === 0 && held.current === 'zoom') {
      event.preventDefault();
      beginPointerSession(event);
      drag.current = { kind: 'zoom-rect', pointerId: event.pointerId, start: point, current: point, zoomOut: event.altKey, moved: false };
      return;
    }
    // The painter applies its style to the next clicked layer and then disarms.
    if (event.button === 0 && painterArmed && !spaceHeld) {
      const hit = elementTargetAt(project, selection, target);
      if (hit) {
        event.preventDefault();
        applyPainterStyle(hit);
        surface.current?.focus({ preventScroll: true });
        return;
      }
      transitionPainter({ kind: 'disarm' });
    }
    if (requestNode && !spaceHeld) {
      event.preventDefault();
      selectRequest(requestNode.dataset.request!);
      surface.current?.focus({ preventScroll: true });
      return;
    }
    if (playIsActive(latest.current.play) && !spaceHeld) {
      // Play turns the boundary into a clickable surface: no selection, no dragging.
      event.preventDefault();
      const chain = elementChain(target);
      const clickedPage = chainPage(chain);
      if (clickedPage && latest.current.play.boundary === clickedPage) {
        // Play resolves the innermost Play pointer or page link from the clicked layer outward.
        const elementId = chain.find(entry => entry.dataset.element)?.dataset.element;
        if (elementId) latest.current.onPlayElement(clickedPage, elementId);
      }
      return;
    }
    const additive = event.shiftKey || event.ctrlKey || event.metaKey;
    if (event.button === 0 && spaceHeld) { startPan(event, boundaryNode?.dataset.boundary ?? null); return; }
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
        surface.current?.focus({ preventScroll: true });
        return;
      }
      if (requestShape === 'polygon') {
        const current = polygonDraft ?? [];
        const points = [...current, point];
        setPolygonDraft(points);
        surface.current?.focus({ preventScroll: true });
        if (points.length >= 3 && Math.hypot(points[0].x - point.x, points[0].y - point.y) * camera.zoom < 12) closeRequestArea(points, 'polygon');
        return;
      }
      beginPointerSession(event);
      drag.current = { kind: 'draw-request-area', pointerId: event.pointerId, start: point, current: point, shape: requestShape, points: [point], moved: false };
      setPolygonDraft(null);
      return;
    }
    // Text just outside a boundary still belongs to it; a note pressed inside one becomes text,
    // the mirror of text outside every boundary becoming a note.
    if (tool === 'note' && boundaryUnder(point)) {
      if (drawElementDrag(event, point, 'text', false)) {
        drag.current = { ...(drag.current as DrawElementDrag), rect: { x: point.x, y: point.y, width: 0, height: 0 } };
      }
      return;
    }
    if (tool === 'note' || (tool === 'text' && !boundaryUnder(point) && !boundaryNear(point))) {
      event.preventDefault();
      beginPointerSession(event);
      drag.current = { kind: 'draw-note', pointerId: event.pointerId, start: point, rect: referenceNoteRect(point, point), moved: false };
      return;
    }
    if (tool === 'boundary') {
      // Drawing over an existing boundary is allowed: the commit pushes neighbours apart, which
      // keeps boundaries from overlapping and keeps the tool from silently refusing a press.
      event.preventDefault();
      beginPointerSession(event);
      drag.current = { kind: 'draw-boundary', pointerId: event.pointerId, start: point, rect: boundaryDrawRect(point, point), moved: false };
      return;
    }
    if (tool === 'pointer') {
      // A press that just misses a boundary starts from its edge instead of panning.
      const near = boundaryUnder(point) ? null : boundaryNear(point);
      const from = near ? { x: Math.min(Math.max(point.x, near.rect.x), near.rect.x + near.rect.width), y: Math.min(Math.max(point.y, near.rect.y), near.rect.y + near.rect.height) } : point;
      const source = pointerSourceAt(from);
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

  function beginPointerSession(event: { pointerId: number }) {
    const { surface, dragSession, drag, dragHandlers } = context();
    try { surface.current?.setPointerCapture(event.pointerId); } catch { /* The window keeps the drag. */ }
    if (dragSession.current) return;
    const move = (native: PointerEvent) => { if (drag.current?.pointerId === native.pointerId) dragHandlers.current?.move(native); };
    const end = (native: PointerEvent) => { if (drag.current?.pointerId === native.pointerId) { if (native.type === 'pointercancel') cancelDrag(); else dragHandlers.current?.end(native); } };
    window.addEventListener('pointermove', move, true);
    window.addEventListener('pointerup', end, true);
    window.addEventListener('pointercancel', end, true);
    window.addEventListener('blur', cancelDrag);
    dragSession.current = { move, end };
  }

  function endPointerSession() {
    const { dragSession } = context();
    const session = dragSession.current;
    if (!session) return;
    window.removeEventListener('pointermove', session.move, true);
    window.removeEventListener('pointerup', session.end, true);
    window.removeEventListener('pointercancel', session.end, true);
    window.removeEventListener('blur', cancelDrag);
    dragSession.current = null;
  }

  function cancelDrag() {
    const { drag, setPolygonDraft, setPanning } = context();
    if (!drag.current) return;
    drag.current = null;
    endPointerSession(); setPolygonDraft(null); setPanning(false);
  }

  function pointerMove(event: DragInput) {
    const { drag, touched, setCamera, worldPoint, lastPointer, camera, renderDrag, project } = context();
    const active = drag.current;
    if (!active || active.kind === 'polygon-request' || active.pointerId !== event.pointerId) return;
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
    const pointerStart = active.kind === 'draw-element' ? active.pointerStart : active.start;
    // A click that wobbles a few pixels is still a click, not a drag.
    if (!active.moved && Math.hypot(point.x - pointerStart.x, point.y - pointerStart.y) * camera.zoom < 4) return;
    active.moved = true;
    if (active.kind === 'move-boundary') {
      active.rect = { ...active.origin, x: Math.round(active.origin.x + point.x - active.start.x), y: Math.round(active.origin.y + point.y - active.start.y) };
    } else if (active.kind === 'draw-boundary') active.rect = boundaryDrawRect(active.start, point, { square: event.shiftKey, fromCentre: event.altKey });
    else if (active.kind === 'draw-request-area') {
      active.current = point;
      active.keys = { square: event.shiftKey, fromCentre: event.altKey };
      if (active.shape === 'freehand') {
        const last = active.points[active.points.length - 1];
        if (Math.hypot(point.x - last.x, point.y - last.y) * camera.zoom >= 2) active.points.push(point);
      }
    }
    else if (active.kind === 'draw-note') active.rect = referenceNoteRect(active.start, point);
    else if (active.kind === 'draw-element') {
      const endPoint = project.gridSettings.snap && !event.altKey ? snapPoint(point) : point;
      if (active.line) {
        // A drawn line's rotation snaps to 15 degree steps while Shift is held.
        const end = event.shiftKey ? snappedLineEnd(active.start, endPoint, 15) : endPoint;
        const length = Math.max(1, Math.hypot(end.x - active.start.x, end.y - active.start.y));
        active.rotation = Math.atan2(end.y - active.start.y, end.x - active.start.x) * 180 / Math.PI;
        active.rect = { x: (active.start.x + end.x) / 2 - length / 2, y: (active.start.y + end.y) / 2, width: length, height: 0 };
      } else if (active.elementKind === 'box' || active.elementKind === 'ellipse') {
        active.rect = drawRectWithKeys(active.start, endPoint, { square: event.shiftKey, fromCentre: event.altKey });
      } else {
        active.rect = rectFromPoints(active.start, endPoint);
      }
    } else if (active.kind === 'marquee') {
      active.current = point;
    } else if (active.kind === 'zoom-rect') {
      active.current = point;
    } else if (active.kind === 'move-elements') {
      active.axisLocked = event.shiftKey;
      const delta = elementMoveDelta(active.entries, active.start, point, project.gridSettings.snap, active.axisLocked);
      const rects: Record<string, Rect> = {};
      for (const entry of active.entries) {
        const world = { ...entry.world, x: entry.world.x + delta.x, y: entry.world.y + delta.y };
        rects[entry.id] = elementLocalRect(world, entry.boundaryOrigin, entry.parentOrigin);
      }
      active.rects = rects;
    } else if (active.kind === 'resize-element') {
      const localPoint = elementLocalRect({ x: point.x, y: point.y, width: 0, height: 0 }, active.entry.boundaryOrigin, active.entry.parentOrigin);
      // Images keep their ratio by default and Shift frees it; Alt resizes from the centre.
      const keepRatio = active.entry.element.kind === 'image' ? !event.shiftKey : event.shiftKey;
      active.rect = resizedRectWithKeys(active.entry.rect, active.corner, localPoint, { snap: true, ratio: keepRatio, fromCentre: event.altKey });
    } else if (active.kind === 'draw-pointer') {
      active.current = point;
    } else if (active.kind === 'reattach-pointer') {
      active.current = point;
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
    } else if (active.kind === 'resize-reference') {
      active.rect = resizedReferenceRect(active.origin, active.corner, point);
    }
    renderDrag(version => version + 1);
  }

  function pointerUp(event: DragInput) {
    const { project, drag, setPanning, selectBoundary, surface, touched, camera, setCamera, worldToScreen, persistMove, persistDraw, closeRequestArea, busy, addReference, createdItem, selectReference, boundaryRect, pendingImage, placeElement, createdElement, transitionText, setPendingImage, onTool, duplicateElementMove, lastPointer, commitElementMove, updateElements, moveElements, marqueeTargets, selection, selectElement, selectTargets, endpointAt, addPointer, createdPointer, setArmedPointer, selectPointer, updatePointer, commitMove, updateReference } = context();
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
    if (active.kind === 'move-boundary') { if (active.moved) void persistMove(active.sizeId, active.rect); return; }
    if (active.kind === 'draw-boundary') {
      // A click places the tool's default boundary, as a click does with every other draw tool.
      if (!active.moved) { void persistDraw(active.rect, true); return; }
      void persistDraw(active.rect);
      return;
    }
    if (active.kind === 'draw-request-area') {
      const points = [...active.points, active.current];
      if (active.shape === 'freehand') { if (points.length >= 3) closeRequestArea(points, 'freehand'); return; }
      const rect = drawRectWithKeys(active.start, active.current, { square: event.shiftKey, fromCentre: event.altKey });
      if (rect.width < 4 || rect.height < 4) return;
      closeRequestArea(rect, active.shape);
      return;
    }
    if (active.kind === 'draw-note') {
      const id = crypto.randomUUID();
      const rect = active.moved ? active.rect : { x: active.rect.x, y: active.rect.y, ...CLICK_NOTE_SIZE };
      busy.current = true;
      void addReference({ id, rect, kind: { kind: 'note', text: '' } }).then(() => { createdItem.current = id; selectReference(id); onTool('select'); }).catch(() => {}).finally(() => { busy.current = false; });
      return;
    }
    if (active.kind === 'draw-element') {
      const boundary = boundaryRect(active.sizeId);
      if (!boundary) return;
      let rect = active.rect;
      // A drag too small to see places the default size, as a click does.
      if (!active.moved || Math.max(rect.width, rect.height) * camera.zoom < 6) {
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
          if (active.elementKind === 'text') transitionText({ kind: 'open', session: { pageId: active.pageId, sizeId: active.sizeId, elementId: createdNow, autoWidth: element.style.sizing?.width === 'fit', created: true } });
          else { createdElement.current = null; transitionText({ kind: 'open', session: null }); }
          if (active.elementKind === 'image') setPendingImage(null);
          onTool('select');
        }
      }).catch(() => {}).finally(() => { busy.current = false; });
      return;
    }
    if (active.kind === 'move-elements') {
      if (active.moved) {
        if (active.duplicate) void duplicateElementMove(active, lastPointer.current ?? active.start, project.gridSettings.snap).catch(() => {});
        else void commitElementMove(active, lastPointer.current ?? active.start, project.gridSettings.snap).catch(() => {});
      }
      return;
    }
    if (active.kind === 'resize-element') {
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
      if (!active.moved) return;
      const hit = endpointAt(lastPointer.current ?? active.current);
      if (!hit) return;
      if (samePointerEndpoint(active.source, hit.endpoint)) {
        // Dropping a pointer back on its own layer or boundary describes that layer's behaviour.
        const { project, addPointer, focusBehaviour, busy } = context();
        const source = active.source;
        const focus = () => focusBehaviour(source);
        if (behaviourFor(project, source)) { focus(); return; }
        busy.current = true;
        void addPointer(behaviourPointer(source, '')).then(focus).catch(() => {}).finally(() => { busy.current = false; });
        return;
      }
      const pointer: Pointer = { id: crypto.randomUUID(), from: active.source, to: hit.endpoint as PointerTarget, comment: '', kind: 'reference', wiring: null };
      busy.current = true;
      void addPointer(pointer).then(() => { createdPointer.current = pointer.id; setArmedPointer(null); selectPointer(pointer.id); }).catch(() => {}).finally(() => { busy.current = false; });
      return;
    }
    if (active.kind === 'reattach-pointer') {
      setArmedPointer(null);
      if (!active.moved) return;
      const hit = endpointAt(lastPointer.current ?? active.current);
      if (!hit || samePointerEndpoint(active.original, hit.endpoint)) return;
      if (active.end === 'from' && hit.endpoint.kind !== 'element' && hit.endpoint.kind !== 'boundary') return;
      void updatePointer(active.pointer, active.end === 'from' ? { from: hit.endpoint as PointerSource } : { to: hit.endpoint as PointerTarget }).catch(() => {});
      return;
    }
    if (active.kind === 'move-selection') { if (active.moved) void commitMove(active.targets, active.rects); return; }
    if (active.kind === 'resize-reference') { if (active.moved) updateReference(active.itemId, { rect: active.rect }); }
  }

  function doubleClick(event: React.MouseEvent<HTMLDivElement>) {
    const { tool, requestShape, polygonDraft, closeRequestArea, project, selection, pageFor, selectElement, startEditingElement, editingSizeId, surface } = context();
    if (tool === 'request' && requestShape === 'polygon' && polygonDraft && polygonDraft.length >= 3) {
      event.preventDefault();
      event.stopPropagation();
      closeRequestArea(polygonDraft, 'polygon');
      return;
    }
    if (tool !== 'select') return;
    // The first click captured the pointer, so the browser aims the double-click at the canvas
    // itself; find the layer under the pointer instead.
    const target = (event.target as HTMLElement).closest('[data-element]') ? event.target as HTMLElement
      : document.elementsFromPoint(event.clientX, event.clientY).find(node => node.closest('[data-element]')) as HTMLElement | undefined;
    if (!target) return;
    const hit = elementTargetAt(project, selection, target);
    if (!hit) return;
    event.preventDefault();
    event.stopPropagation();
    const element = findElementIn(pageFor(hit.pageId, hit.sizeId)?.elements ?? [], hit.elementId);
    const inside = elementChain(target)[0]?.dataset.element ?? null;
    if (element?.kind === 'group' && inside && inside !== hit.elementId) selectElement(hit.pageId, inside, hit.sizeId, false);
    else if (element && isTextKind(element.kind)) startEditingElement(hit.pageId, hit.elementId, hit.sizeId ?? editingSizeId(hit.pageId), false);
    else selectElement(hit.pageId, hit.elementId, hit.sizeId, false);
    surface.current?.focus({ preventScroll: true });
  }

  return { pointerDown, beginPointerSession, endPointerSession, cancelDrag, pointerMove, pointerUp, doubleClick };
}
