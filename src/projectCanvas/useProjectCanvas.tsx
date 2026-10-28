import { createRequestTool } from './requestTool';
import { createPointerTool } from './pointerTool';
import { createDrawingActions } from './drawing';
import { createMovingActions } from './moving';
import { createPasteDrop } from './pasteDrop';
import { createKeyboardActions } from './keyboardActions';
import { createCanvasInteraction } from './interaction';
import {
  ElementEntry, ClipboardEntry, PasteChoice, LinkDecision, Drag, NIL_UUID, DragInput, ProjectCanvasHandle,
  DuplicateRepeat, ProjectCanvasProps, isElementTarget, elementAt, targetKey,
} from './interactionTypes';
import { canvasView } from './canvasView';
import { useEffect, useImperativeHandle, useMemo, useRef, useState, type CSSProperties } from 'react';
import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { isTextField } from '../WorkspaceSurface';
import {
  automaticLinks, boundaryAt, canvasTargetKey, dotGrid, fitCamera, nearestEdgeAnchors, selectedPointers,
  viewportWorldRect, type Boundary, type CanvasTarget, type Point, type ReferenceBox, type RequestBox,
} from '../projectCanvasGeometry';
import type { Element, ElementPatch, NamedView, Page, Rect, ReferenceItem, RequestArea, RequestShape, RequestTarget } from '../types';
import { onEditScope, type EditScope } from './selectionBar';
import { elementBoxes, elementBoxesNow, pendingRequests, requestAreaContains } from './requests';
import { shownReference } from './reference';
import { pointerEntries, pointerWorld } from './pointers';
import { screenRect, useCanvasCamera, type NativeDrag, type ScreenRect } from './surface';
import { contentFitsViewport, overviewRects } from './minimap';
import { textEditorScreen } from './elements';
import { resolveElements } from './links';
import { usePasteDefault } from '../pastePreference';
import { baseSizeId, hasManySizes, resolveForSize, sizeAtWidth } from './sizes';
import { elementLocalRect } from './elementGeometry';
import { pastedImageRect, pastedNoteRect } from './clipboardPaste';
import { droppedDataUri, droppedFile, droppedFileKind, droppedImageSize, droppedText } from './fileDrop';
import { shortcutFor, viewSlot } from '../shortcuts';
import { playIsActive } from './play';
import {
  unionRects, createElement, createTextElement, forAppearance, withPickedColours, elementChildrenOf,
  elementPath, elementParents, findElementIn, drawRectWithKeys, rectFromPoints,
} from './elementGeometry';
import type { ForwardedRef } from 'react';

const EMPTY_RECTS: Record<string, Rect> = {};

export function useProjectCanvas({
  project, openPage, createBoundary, savePlacements, saveView, selection, focused, selectBoundary, selectReference, selectPointer, selectElement, clearSelection,
  removeSelection, removeElements, deleteBoundary, addReference, updateReference, elementToReference, referenceToElement, attachToRequest,
  addRequest, updateRequest, selectRequest,
  insertElements, updateElements, moveElements, groupElements, ungroupElements, linkElements, unlinkElements, pasteLinked,
  addPointer, updatePointer, pointerStates, tool, insertKind, onTool, boundaryKind, requestShape, onRequestShape, focusedSize, onZoom, onLayout,
  play, onPlayElement, onPlayExit, preview, previewClose, previewRefresh,
}: ProjectCanvasProps, ref: ForwardedRef<ProjectCanvasHandle>) {

  const {
    requestTargetAt, requestEditorScreen, closeRequestArea, targetForSelection, startRequestOnSelection,
    startRequestFromShape, submitRequestDraft, cancelRequestDraft,
  } = createRequestTool(() => ({
    requestBoxes, project, movingItems, pointerWorldNow, camera, viewSize, boundaryUnder, elementScreenRect,
    boundaries, setPolygonDraft, setRequestDraft, surface, selection, focused, focusedSize, elementEntries,
    requestDraft, addRequest, selectRequest,
  }));
  const {
    armPointer, endpointAt, pointerSourceAt, startReattach,
  } = createPointerTool(() => ({
    latest, setArmedPointer, surface, requestBoxes, project, movingItems, boundaryUnder, pointerWorldNow,
    beginPointerSession, worldPoint, drag,
  }));
  const {
    chooseImage, insertionPoint, elementRectAt, placeElement, drawElementDrag, persistDraw,
  } = createDrawingActions(() => ({
    setPendingImage, project, boundaryRect, createdElement, editingAllSizes, insertElements, selectElement,
    boundaryUnder, beginPointerSession, drag, busy, boundaries, boundaryKind, referenceBoxes, createBoundary,
    selectBoundary,
  }));
  const {
    startPan, beginMove, boxAtPoint, marqueeTargets, rememberRepeatMove, duplicateElementMove,
    commitElementMove, startElementResize, startReferenceResize, persistMove, saveMoves, commitMove,
  } = createMovingActions(() => ({
    beginPointerSession, drag, setPanning, project, surface, boundaryRect, camera, pageFor, duplicateRepeat,
    insertElements, selectTargets, boundaryUnder, elementToReference, moveElements, clearSelection, selection,
    worldPoint, movingItems, boundaries, referenceBoxes, busy, savePlacements, moving, referenceToElement,
    updateReference,
  }));
  const {
    copyElements, pasteTarget, pasteScreen, pasteEntries, pasteElements, confirmPaste, cloneChildren,
    duplicateElements, createReference, referencePoint, pasteText, pasteImage, pasteClipboard, paste, dropText,
  } = createPasteDrop(() => ({
    elementEntries, selection, clipboard, pasteCount, setPasteChoice, clipboardMark, project, boundaryRect,
    lastPointer, viewSize, camera, boundaryUnder, editingAllSizes, pasteLinked, selectTargets, insertElements,
    setLinkChoice, pasteMode, pasteChoice, duplicateRepeat, addReference, selectReference, elementRectAt, busy,
    insertionPoint, selectElement, placeElement, createdElement, worldPoint, latest, attachToRequest,
  }));
  const {
    escapeCanvas, selectAllSiblings, arrangeElements, copyStyle, pasteStyle, paintFormat, applyPainterStyle,
    alignSelection, setSelectionFill, deleteSelectedLayers, linkSelection, linkCopies, unlinkSelection,
    groupSelection, ungroupSelection, nudgeElements, focusElement, canRunCanvasAction, runCanvasAction,
    keyDown,
  } = createKeyboardActions(() => ({
    drag, cancelDrag, painterArmed, setPainterArmed, latest, onPlayExit, requestDraft, polygonDraft, draftArea,
    cancelRequestDraft, editing, cancelEditingText, selection, selectBoundary, focused, project, selectTargets,
    pageFor, elementEntries, moveElements, copiedStyle, updateElements, surface, boundaryRect, removeSelection,
    elementScreenRect, setLinkChoice, linkElements, unlinkElements, groupElements, ungroupElements,
    rememberRepeatMove, clipboard, pasteTarget, deleteBoundary, focusedSize, startRequestOnSelection, onTool,
    startRequestFromShape, copyElements, pasteElements, duplicateElements, setMinimapPreference,
    minimapVisible, saveView, currentCamera, goToView, closeRequestArea, openPage,
  }));
  const {
    pointerDown, beginPointerSession, endPointerSession, cancelDrag, pointerMove, pointerUp, doubleClick,
  } = createCanvasInteraction(() => ({
    busy, startPan, pasteChoice, linkChoice, setPasteChoice, setLinkChoice, worldPoint, lastPointer, zoom,
    space, drag, painterArmed, project, selection, applyPainterStyle, surface, setPainterArmed, selectRequest,
    latest, tool, requestShape, requestTargetAt, requestDraft, setRequestDraft, requestEditorScreen,
    setPolygonDraft, polygonDraft, camera, closeRequestArea, boundaryUnder, pointerSourceAt, pageFor,
    startEditingElement, editingSizeId, drawElementDrag, insertKind, pendingImage, imagePicker, selectElement,
    selectBoundary, elementEntries, nextSelectionFor, selectReference, beginMove, boundaries, movingItems,
    dragSession, dragHandlers, setPanning, touched, setCamera, renderDrag, worldToScreen, persistMove,
    persistDraw, addReference, createdItem, boundaryRect, placeElement, createdElement, setEditing,
    setPendingImage, onTool, duplicateElementMove, commitElementMove, updateElements, moveElements,
    marqueeTargets, selectTargets, endpointAt, addPointer, createdPointer, setArmedPointer, selectPointer,
    updatePointer, commitMove, updateReference,
  }));

  const surface = useRef<HTMLDivElement>(null);
  const [, renderDrag] = useState(0);
  const dragValue = useRef<Drag | null>(null);
  const drag = useMemo(() => ({
    get current() { return dragValue.current; },
    set current(value: Drag | null) { dragValue.current = value; renderDrag(version => version + 1); },
  }), []);
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
  const { boundaries } = useMemo(() => canvasView(project), [project, project.revision]);
  const [widthPreview, setWidthPreview] = useState<{ sizeId: string; width: number } | null>(null);
  const scrubbed = useMemo(() => {
    if (!widthPreview) return { project, boundaries };
    const previewProject = structuredClone(project);
    const page = previewProject.pages.find(page => page.sizes.some(size => size.id === widthPreview.sizeId));
    if (!page) return { project, boundaries };
    const previewElements = resolveForSize(project, page, sizeAtWidth(page, widthPreview.width).id);
    const desired = new Map<string, Element>();
    const collect = (elements: Element[]) => { for (const element of elements) { desired.set(element.id, element); collect(element.children); } };
    collect(previewElements);
    page.elements = structuredClone(resolveElements(project, page.elements));
    const override = (elements: Element[]) => { for (const element of elements) {
      const preview = desired.get(element.id)!;
      element.link = null;
      element.overrides = { ...element.overrides, [widthPreview.sizeId]: { rect: preview.rect, style: preview.style, layout: preview.layout, hidden: !preview.visible, dismissed: false } };
      override(element.children);
    } };
    override(page.elements);
    const size = page.sizes.find(size => size.id === widthPreview.sizeId)!;
    size.width = widthPreview.width;
    return { project: previewProject, boundaries: boundaries.map(boundary => boundary.sizeId === size.id ? { ...boundary, rect: { ...boundary.rect, width: size.width } } : boundary) };
  }, [project, boundaries, widthPreview]);
  const { camera, setCamera, panning, setPanning, spaceHeld, space, zoomHeld, zoom, touched, fit, zoomStep, zoomTo, worldPoint, currentCamera, animateTo, stopTransition } = useCanvasCamera(surface, boundaries, project.id);
  // The minimap hides itself while the whole canvas fits; the shortcut can force it on or off.
  const [minimapPreference, setMinimapPreference] = useState<'auto' | 'on' | 'off'>('auto');

  const [requestDraft, setRequestDraft] = useState<{ targets: RequestTarget[]; area: RequestArea | null; screen: ScreenRect } | null>(null);
  const activeDrag = drag.current;
  const polygonDraft = activeDrag?.kind === 'polygon-request' ? activeDrag.points : null;
  function setPolygonDraft(points: Point[] | null) {
    if (points) drag.current = { kind: 'polygon-request', pointerId: -1, points };
    else if (drag.current?.kind === 'polygon-request') drag.current = null;
  }
  const moving = activeDrag?.kind === 'move-boundary' && activeDrag.moved ? activeDrag : null;
  const draft = activeDrag?.kind === 'draw-boundary' ? activeDrag.rect : null;
  const draftNote = activeDrag?.kind === 'draw-note' ? activeDrag.rect : null;
  const draftElement = activeDrag?.kind === 'draw-element' ? activeDrag : null;
  const marquee = activeDrag?.kind === 'marquee' && activeDrag.moved ? rectFromPoints(activeDrag.start, activeDrag.current) : null;
  const zoomRect = activeDrag?.kind === 'zoom-rect' ? rectFromPoints(activeDrag.start, activeDrag.current) : null;
  const movingElements: Record<string, Rect> = activeDrag?.kind === 'move-elements' ? activeDrag.rects
    : activeDrag?.kind === 'resize-element' && activeDrag.moved ? { [activeDrag.entry.id]: activeDrag.rect } : EMPTY_RECTS;
  const movingItems: Record<string, Rect> = activeDrag?.kind === 'move-selection' ? activeDrag.rects
    : activeDrag?.kind === 'resize-reference' && activeDrag.moved ? { [activeDrag.itemId]: activeDrag.rect } : EMPTY_RECTS;
  const draftArea: RequestShape | null = activeDrag?.kind === 'polygon-request' ? { kind: 'polygon', points: activeDrag.points }
    : activeDrag?.kind === 'draw-request-area' ? activeDrag.shape === 'freehand'
      ? { kind: 'freehand', points: [...activeDrag.points, activeDrag.current] }
      : { kind: activeDrag.shape === 'ellipse' ? 'ellipse' : 'rect', rect: drawRectWithKeys(activeDrag.start, activeDrag.current, activeDrag.keys) } : null;
  // Resolve the pointer preview below, after the model hit-testing helpers exist.

  const [armedPointer, setArmedPointer] = useState<{ pointerId: string; end: 'from' | 'to' } | null>(null);

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

  useEffect(() => { onLayout(boundaries); }, [boundaries, onLayout]);
  useEffect(() => { onZoom(camera.zoom); }, [camera.zoom, onZoom]);
  useEffect(() => { setPasteChoice(null); setLinkChoice(null); setPlaceholderChoice(null); }, [project.id]);
  // A repeat duplicate ends as soon as the selection changes any other way.
  const selectedElementKey = targetKey(selection.filter(isElementTarget));
  useEffect(() => {
    if (duplicateRepeat.current && duplicateRepeat.current.key !== selectedElementKey) duplicateRepeat.current = null;
  }, [selectedElementKey]);
  useEffect(() => { if (tool !== 'request') { setRequestDraft(null); if (drag.current?.kind === 'draw-request-area' || drag.current?.kind === 'polygon-request') cancelDrag(); } }, [tool]);
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

  // Delete and Escape follow the canvas selection even when focus sits outside the surface.
  useEffect(() => {
    const handle = (event: KeyboardEvent) => {
      if (event.defaultPrevented || isTextField(event.target)) return;
      // A choice row owns the keyboard until it closes.
      if (pasteChoice || linkChoice) return;
      if (event.key === 'Escape') {
        if (!drag.current && !selection.length && !focused && !editing && !painterArmed && !requestDraft) return;
        event.preventDefault();
        escapeCanvas();
        return;
      }
      if (shortcutFor(event, false) !== 'delete') return;
      event.preventDefault();
      runCanvasAction('delete');
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

  useImperativeHandle(ref, () => ({ focusCanvas: () => surface.current?.focus({ preventScroll: true }), fit, fitSelection, zoomStep, zoomTo, focusBoundary, armPointer, linkSelection, unlinkSelection, copyStyle, pasteStyle, paintFormat, currentCamera, goToView, runCanvasAction, canRunCanvasAction }));

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

  // Format painter: the style is the only thing that travels, never position, size or text.

  /** Ctrl/Cmd+D follows the paste default; each copy stays beside its original.
   *  A repeat duplicate keeps the offset of the copy that was just moved, Figma-style. */

  /** One edge of the selection bar: layers align to each other inside their boundary, or a lone
   *  layer to its parent. Layers in different boundaries align within their own boundary. */

  /**
   * Whether an action would change the canvas right now. The palette greys out the rows that
   * would not, and null says the canvas does not own the action at all.
   */

  /**
   * One path for every canvas key: its keydown listeners and the command palette both call this,
   * so a palette row runs exactly what its shortcut runs. True means the canvas took the action,
   * which is also when a key event may skip the browser's default.
   */

  // Canvas-wide keys, so focus can sit in the Layers panel or Properties. The actions and keys
  // come from the same keymap the tool strip and keyboard help read.
  useEffect(() => {
    const handle = (event: KeyboardEvent) => {
      if (event.defaultPrevented || isTextField(event.target)) return;
      const action = shortcutFor(event, false);
      // Plain paste is left to the paste event, which can also carry content from other apps.
      if (!action || action === 'paste') return;
      if (runCanvasAction(action, { key: event.key, slot: viewSlot(event) ?? undefined, repeat: event.repeat })) event.preventDefault();
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

  /**
   * Capture is best effort: the window listeners below carry the drag even when a platform
   * declines or drops the capture (WebKitGTK on Wayland does both), so drawing never depends on
   * one element keeping the pointer.
   */

  useEffect(() => endPointerSession, []);

  function worldToScreen(point: Point): Point {
    return { x: point.x * camera.zoom + camera.x, y: point.y * camera.zoom + camera.y };
  }

  function soleReferenceItem(): ReferenceItem | null {
    if (selection.length !== 1 || selection[0].kind !== 'reference') return null;
    const itemId = selection[0].itemId;
    return project.reference.find(item => item.id === itemId) ?? null;
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

  let pointerDraft: { from: Point; to: Point } | null = null;
  if (activeDrag && activeDrag.kind !== 'polygon-request' && activeDrag.moved && (activeDrag.kind === 'draw-pointer' || activeDrag.kind === 'reattach-pointer')) {
    const point = activeDrag.current;
    const hit = endpointAt(point);
    const target = hit?.rect ?? { ...point, width: 0, height: 0 };
    pointerDraft = activeDrag.kind === 'draw-pointer'
      ? nearestEdgeAnchors(activeDrag.sourceRect, target)
      : activeDrag.end === 'from' ? nearestEdgeAnchors(target, { ...activeDrag.fixed, width: 0, height: 0 })
        : nearestEdgeAnchors({ ...activeDrag.fixed, width: 0, height: 0 }, target);
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
  const pointerWorldNowValue = useMemo(() => pointerWorld(project, boundaries, moving, {}, movingItems), [project, boundaries, moving, movingItems]);
  const viewport: Rect = viewSize.width > 0 ? viewportWorldRect(camera, viewSize) : { x: -1e7, y: -1e7, width: 2e7, height: 2e7 };
  const selectedPointerIds = new Set(selectedPointers(selection));
  const entries = pointerEntries(pointers, pointerWorldNowValue, selectedPointerIds, viewport);
  const selectedPointerId = selection.length === 1 && selection[0].kind === 'pointer' ? selection[0].pointerId : null;
  const choosePointer = (pointerId: string) => { selectPointer(pointerId); surface.current?.focus({ preventScroll: true }); };
  const autoLinks = useMemo(() => automaticLinks(project, boundaries, elementBoxes(project, boundaries, moving)), [project, boundaries, moving]);
  const selectedElementTargets = selection.flatMap(target => target.kind === 'element' ? [target] : []);
  // The contextual bar stays out of every canvas gesture and every inline editor.
  const selectionBarVisible = tool === 'select' && !playIsActive(play) && !panning && !editing && !preview
    && !moving && !draft && !draftElement && !draftArea && !draftNote && !marquee && !zoomRect && !pointerDraft
    && !requestDraft && !pasteChoice && !linkChoice && !placeholderChoice
    && !Object.keys(movingElements).length && !Object.keys(movingItems).length;
  const selectionEntries = selectionBarVisible ? elementEntries(selectedElementTargets) : [];
  const selectionScreen = selectionEntries.length ? screenRect(unionRects(selectionEntries.map(entry => entry.world)), camera) : null;
  const selectionFills = [...new Set(selectionEntries.map(entry => entry.element.style.fill ?? ''))];
  const selectionFill = selectionFills.length === 1 ? selectionFills[0] : '';
  const selectionLinked = selectionEntries.length > 0 && selectionEntries.every(entry => !!entry.element.link);
  const selectionLinkable = selectionEntries.length > 0 && selectionEntries.every(entry => !entry.element.link);
  // This size / All sizes appears only for a pure layer selection on one boundary with
  // more than one linked size; the selection itself shows which scope edits use.
  const scopePage = selectedElementTargets.length > 0 && selection.every(target => target.kind === 'element')
    && selectedElementTargets.every(target => target.pageId === selectedElementTargets[0].pageId)
    ? project.pages.find(page => page.id === selectedElementTargets[0].pageId) ?? null
    : null;
  const selectionScope: EditScope | null = scopePage && hasManySizes(scopePage)
    ? (selectedElementTargets.every(target => target.sizeId === null) ? 'all' : 'this') : null;
  /** Move the selection into the size scope the control chose. */
  function setSelectionScope(next: EditScope) {
    if (!scopePage) return;
    const sizeId = next === 'all' ? null
      : selectedElementTargets.find(target => target.sizeId !== null)?.sizeId ?? focusedSize ?? baseSizeId(scopePage);
    selectedElementTargets.forEach((target, index) => selectElement(target.pageId, target.elementId, sizeId, index > 0));
  }
  useEffect(() => onEditScope(setSelectionScope));
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
    if (current.created) removeElements([{ pageId: current.pageId, elementId: current.elementId }]);
    else selectElement(current.pageId, current.elementId, current.sizeId);
  }

  const previewBoundary = preview ? boundaries.find(item => item.pageId === preview.pageId) ?? null : null;
  const previewScreen = previewBoundary ? screenRect(previewBoundary.rect, camera) : null;

  return {
    surface, tool, requestShape, panning, spaceHeld, zoomHeld, painterArmed, play, dropHover, dotStyle,
    pointerDown, keyDown, doubleClick, paste, dropText, imagePicker, chooseImage, camera, scrubbed, focused,
    selection, moving, openPage, editing, movingElements, openPlaceholder, viewport, inverse, project,
    boundaries, requestDraft, draftArea, selectRequest, movingItems, draftNote, draft, boundaryKind,
    draftElement, marquee, zoomRect, entries, autoLinks, pointerDraft, selectedPointerIds, pointerStates,
    choosePointer, updatePointer, createdPointer, widthPreview, setWidthPreview, selectionScreen, viewSize,
    selectionFill, selectionLinked, selectionLinkable, selectionScope, setSelectionFill, alignSelection,
    setSelectionScope, startRequestOnSelection, linkSelection, unlinkSelection, duplicateElements,
    deleteSelectedLayers, minimapVisible, stopTransition, touched, setCamera, selectedPointerId, armedPointer,
    startReattach, selectedElementTargets, startElementResize, editingElement, editingScreen,
    commitEditingText, cancelEditingText, placeholderChoice, resolvePlaceholder, setPlaceholderChoice,
    submitRequestDraft, cancelRequestDraft, selectedItem, selectedItemScreen, createdItem, updateReference,
    startReferenceResize, pasteChoice, setPasteChoice, confirmPaste, linkChoice, linkCopies, setLinkChoice,
    preview, previewScreen, previewRefresh, previewClose, dropNotice, setDropNotice,
  };
}
