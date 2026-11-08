import { shortcutFor } from '../shortcuts';
import { selectionEntries as resolveSelectionEntries } from './selectionGeometry';
import { createRequestTool } from './requestTool';
import { createPointerTool } from './pointerTool';
import { createDrawingActions, type TextEditing, type TextEditingEvent } from './drawing';
import { createMovingActions } from './moving';
import { createPasteDrop } from './pasteDrop';
import { createKeyboardActions, type Painter, type PainterEvent } from './keyboardActions';
import { createCanvasInteraction } from './interaction';
import { ElementEntry, ClipboardEntry, PasteChoice, LinkDecision, Drag, DragInput, ProjectCanvasHandle, DuplicateRepeat, type CanvasHookProps, isElementTarget, targetKey } from './interactionTypes';
import { canvasView } from './canvasView';
import { useNativeDrop } from './useNativeDrop';
import { useEffect, useImperativeHandle, useLayoutEffect, useMemo, useRef, useState, type CSSProperties } from 'react';
import { isTextField } from '../textField';
import { automaticLinks } from './autoLinks';
import { boundaryAt, boundaryNear, type Boundary } from './boundaryLayout';
import { canvasTargetKey, selectedPointers, type CanvasTarget, type RequestBox } from './canvasTargets';
import { dotGrid, fitCamera, textEditingCamera, unionBounds, viewportWorldRect, type CanvasCamera, type Point } from './camera';
import { nearestEdgeAnchors } from './pointerGeometry';
import { type ReferenceBox } from './referenceGeometry';
import type { Element, NamedView, Page, Project, Rect, ReferenceItem, RequestArea, RequestShape, RequestTarget } from '../types';
import { onEditScope, type EditScope } from './selectionBar';
import { elementBoxes, pointerWorld } from './pointerLayout';
import { shownReference } from './reference';
import { pointerEntries } from './pointers';
import { behaviourEntries } from './behaviours';
import { screenRect, useCanvasCamera, type ScreenRect } from './surface';
import { contentFitsViewport, overviewRects } from './minimap';
import { textEditorScreen } from './elements';
import { linkedDefinition, resolveElements } from './links';
import { usePasteDefault } from '../pastePreference';
import { baseSizeId, hasManySizes, resolveForSize, sizeAtWidth } from './sizes';
import { isPresenting, playIsActive } from './play';
import { drawRectWithKeys, rectFromPoints } from './elementGeometry';
import { findElementIn } from './elementFacts';
import type { ForwardedRef } from 'react';

const EMPTY_RECTS: Record<string, Rect> = {};

export function useProjectCanvas({
  editProject, project, openPage, createBoundary, savePlacements, saveView, selection, focused, selectBoundary, selectReference, selectPointer, selectElement, clearSelection,
  removeSelection, removeElements, deleteBoundary, addReference, updateReference, elementToReference, referenceToElement, attachToRequest,
  selectRequest,
  insertElements, updateElements, moveElements, groupElements, ungroupElements, linkElements, unlinkElements, pasteLinked,
  addPointer, updatePointer, pointerStates, tool, insertKind, onTool, boundaryKind, requestShape, focusedSize, widthPreview = null, onZoom,
  play, onPlayBoundary, onPlayElement, onPlayExit, preview, previewClose, previewRefresh, focusBehaviour, generate,
}: CanvasHookProps, ref: ForwardedRef<ProjectCanvasHandle>) {

  const {
    requestTargetAt, requestEditorScreen, closeRequestArea, startRequestOnSelection,
    startRequestFromShape, submitRequestDraft, askRequestDraft, cancelRequestDraft,
  } = createRequestTool(() => ({
    requestBoxes, project, movingItems, pointerWorldNow, camera, viewSize, boundaryUnder, elementScreenRect,
    boundaries, setPolygonDraft, setRequestDraft, surface, selection, focused, focusedSize, elementEntries,
    requestDraft, editProject, generate, selectRequest,
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
    setPendingImage, project, boundaryRect, createdElement, editingAllSizes, insertElements, selectElement, transitionText,
    boundaryUnder, boundaryNear: boundaryNearPress, beginPointerSession, drag, busy, boundaryKind, createBoundary, onTool,
    selectBoundary,
  }));
  const {
    startPan, beginMove, marqueeTargets, rememberRepeatMove, duplicateElementMove,
    commitElementMove, startElementResize, startReferenceResize, persistMove, commitMove,
  } = createMovingActions(() => ({
    beginPointerSession, drag, setPanning, project, surface, boundaryRect, camera, pageFor, duplicateRepeat,
    insertElements, selectTargets, boundaryUnder, elementToReference, moveElements, clearSelection, selection,
    worldPoint, movingItems, boundaries, referenceBoxes, busy, savePlacements, moving, referenceToElement,
    updateReference,
  }));
  const {
    copyElements, pasteTarget, pasteElements, confirmPaste,
    duplicateElements, pasteClipboard, paste, dropText,
  } = createPasteDrop(() => ({
    elementEntries, selection, clipboard, pasteCount, setPasteChoice, clipboardMark, project, boundaryRect,
    lastPointer, viewSize, camera, boundaryUnder, editingAllSizes, pasteLinked, selectTargets, insertElements,
    setLinkChoice, pasteMode, pasteChoice, duplicateRepeat, duplicateBurst, addReference, selectReference, elementRectAt, busy,
    insertionPoint, selectElement, placeElement, createdElement, worldPoint, latest, attachToRequest,
  }));
  const {
    escapeCanvas, copyStyle, pasteStyle, paintFormat, applyPainterStyle,
    alignSelection, distributeSelection, setSelectionFill, deleteSelectedLayers, linkSelection, linkCopies, unlinkSelection,
    canvasAction,
    keyDown,
  } = createKeyboardActions(() => ({
    editProject, createBoundary, drag, cancelDrag, painterArmed, transitionPainter, latest, onPlayExit, requestDraft, polygonDraft, draftArea,
    cancelRequestDraft, editing, finishEditingText, selection, selectBoundary, focused, project, selectTargets,
    pageFor, elementEntries, moveElements, painter, updateElements, surface, boundaryRect, removeSelection,
    elementScreenRect, setLinkChoice, linkElements, unlinkElements, groupElements, ungroupElements,
    rememberRepeatMove, clipboard, pasteTarget, deleteBoundary, focusedSize, startRequestOnSelection, onTool,
    startRequestFromShape, copyElements, pasteElements, duplicateElements, setMinimapPreference,
    minimapVisible, saveView, currentCamera, goToView, closeRequestArea, openPage,
  }));
  const {
    pointerDown, beginPointerSession, endPointerSession, cancelDrag, pointerMove, pointerUp, doubleClick,
  } = createCanvasInteraction(() => ({
    busy, startPan, pasteChoice, linkChoice, setPasteChoice, setLinkChoice, worldPoint, lastPointer, held,
    drag, painterArmed, project, selection, applyPainterStyle, surface, transitionPainter, selectRequest,
    latest, tool, requestShape, requestTargetAt, requestDraft, setRequestDraft, requestEditorScreen,
    setPolygonDraft, polygonDraft, camera, closeRequestArea, boundaryUnder, boundaryNear: boundaryNearPress, pointerSourceAt, pageFor,
    startEditingElement, editingSizeId, drawElementDrag, insertKind, pendingImage, imagePicker, selectElement,
    selectBoundary, elementEntries, nextSelectionFor, selectReference, beginMove, boundaries, movingItems,
    dragSession, dragHandlers, setPanning, touched, setCamera, renderDrag, worldToScreen, persistMove,
    persistDraw, addReference, createdItem, boundaryRect, placeElement, createdElement, transitionText,
    setPendingImage, onTool, duplicateElementMove, commitElementMove, updateElements, moveElements,
    marqueeTargets, selectTargets, endpointAt, addPointer, createdPointer, setArmedPointer, selectPointer,
    updatePointer, commitMove, updateReference, focusBehaviour,
  }));

  const surface = useRef<HTMLDivElement>(null);
  const [, renderDrag] = useState(0);
  const dragValue = useRef<Drag | null>(null);
  const drag = useMemo(() => ({
    get current() { return dragValue.current; },
    set current(value: Drag | null) { dragValue.current = value; renderDrag(version => version + 1); },
  }), []);
  const busy = useRef(false);
  // Independent autofocus markers: closing one editor does not clear the others.
  const createdItem = useRef<string | null>(null);
  const createdPointer = useRef<string | null>(null);
  const createdElement = useRef<string | null>(null);
  const lastPointer = useRef<Point | null>(null);
  const imagePicker = useRef<HTMLInputElement>(null);
  const clipboard = useRef<ClipboardEntry[]>([]);
  // The marker these layers left on the system clipboard; null when it could not be written.
  const clipboardMark = useRef<string | null>(null);
  const pasteCount = useRef(0);
  const pasteMode = usePasteDefault();
  // A copied style survives disarming, but an armed painter always has a style.
  const painter = useRef<Painter>({ kind: 'empty' });
  const painterArmed = painter.current.kind === 'armed';
  function transitionPainter(event: PainterEvent) {
    const current = painter.current;
    if (event.kind === 'copy') painter.current = { kind: current.kind === 'armed' ? 'armed' : 'ready', style: event.style };
    else if (current.kind !== 'empty') painter.current = { kind: event.kind === 'arm' ? 'armed' : 'ready', style: current.style };
    else return;
    if ((current.kind === 'armed') !== (painter.current.kind === 'armed')) renderDrag(version => version + 1);
  }
  // Smart duplicate: what the last duplicate was, so Ctrl/Cmd+D can repeat its offset.
  const duplicateRepeat = useRef<DuplicateRepeat | null>(null);
  const duplicateBurst = useRef<{ key: string; at: number; count: number; offset: Point } | null>(null);
  const [pasteChoice, setPasteChoice] = useState<PasteChoice | null>(null);
  const [placeholderChoice, setPlaceholderChoice] = useState<{ pageId: string; sizeId: string; elementId: string; screen: ScreenRect } | null>(null);
  const [linkChoice, setLinkChoice] = useState<LinkDecision | null>(null);
  const { boundaries } = useMemo(() => canvasView(project), [project, project.revision]);
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
  const { holdTool, heldKeyUp, camera, setCamera, panning, setPanning, spaceHeld, held, zoomHeld, touched, fit, zoomStep, zoomTo, worldPoint, currentCamera, animateTo, stopTransition } = useCanvasCamera(surface, boundaries, project.id);
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
      : { kind: activeDrag.shape === 'ellipse' ? 'ellipse' : 'rect', rect: drawRectWithKeys(activeDrag.start, activeDrag.current, { ...activeDrag.keys, spacing: project.gridSettings.spacing }) } : null;
  // Resolve the pointer preview below, after the model hit-testing helpers exist.

  const [armedPointer, setArmedPointer] = useState<{ pointerId: string; end: 'from' | 'to' } | null>(null);

  // A drag outlives a single pointer event. The window owns motion and release, so a failed
  // pointer capture, a retargeted event or an overlay layer cannot strand an active drag.
  const dragSession = useRef<{ move: (event: PointerEvent) => void; end: (event: PointerEvent) => void } | null>(null);
  const dragHandlers = useRef<{ move: (event: DragInput) => void; end: (event: DragInput) => void } | null>(null);
  const pasteHandler = useRef<(data: DataTransfer | null, target: EventTarget | null) => boolean>(() => false);
  const textEditing = useRef<TextEditing>({ kind: 'idle' });
  const textState = textEditing.current;
  const editing = textState.kind === 'existing' || textState.kind === 'draft' ? textState.session : null;
  const textDraft = textState.kind === 'pending' || textState.kind === 'draft' ? textState.draft : null;
  const textBounds = textState.kind === 'existing' || textState.kind === 'draft' ? textState.bounds : null;
  function transitionText(event: TextEditingEvent) {
    const current = textEditing.current;
    const draft = current.kind === 'pending' || current.kind === 'draft' ? current.draft : null;
    switch (event.kind) {
      case 'prepare':
        textEditing.current = current.kind === 'existing' || current.kind === 'draft'
          ? { ...current, kind: 'draft', draft: event.draft }
          : { kind: 'pending', draft: event.draft };
        return;
      case 'open':
        textEditing.current = event.session
          ? draft ? { kind: 'draft', session: event.session, draft, bounds: null }
            : { kind: 'existing', session: event.session, bounds: null }
          : draft ? { kind: 'pending', draft } : { kind: 'idle' };
        break;
      case 'measure':
        if ((current.kind !== 'existing' && current.kind !== 'draft') || current.session !== event.session) return;
        textEditing.current = { ...current, bounds: event.bounds };
        break;
      case 'finish': textEditing.current = { kind: 'idle' }; break;
      case 'retry': textEditing.current = { kind: 'draft', session: event.session, draft: event.draft, bounds: null }; break;
    }
    renderDrag(version => version + 1);
  }
  const [pendingImage, setPendingImage] = useState<{ uri: string; width: number; height: number; name: string } | null>(null);
  const [viewSize, setViewSize] = useState({ width: 0, height: 0 });
  const minimapRects = overviewRects(boundaries, project.reference);
  const minimapFits = viewSize.width >= 1 && contentFitsViewport(minimapRects, viewportWorldRect(camera, viewSize));
  const minimapVisible = viewSize.width >= 1 && (minimapPreference === 'on' || (minimapPreference === 'auto' && !minimapFits));
  // The boundary (or the canvas bed) under a native file drag, and the last drop message.

  // Drop and bridge listeners need the latest canvas state without re-registering on every pan.
  const latest = useRef({ camera, boundaries, project, selection, focused, addReference, attachToRequest, selectPointer, selectReference, selectElement, clearSelection, removeElements, insertElements, updateElements, moveElements, groupElements, ungroupElements, tool, insertKind, editing, play, onPlayElement });
  latest.current = { camera, boundaries, project, selection, focused, addReference, attachToRequest, selectPointer, selectReference, selectElement, clearSelection, removeElements, insertElements, updateElements, moveElements, groupElements, ungroupElements, tool, insertKind, editing, play, onPlayElement };
  dragHandlers.current = { move: pointerMove, end: pointerUp };
  pasteHandler.current = pasteClipboard;

  useEffect(() => { onZoom(camera.zoom); }, [camera.zoom, onZoom]);
  useEffect(() => { setPasteChoice(null); setLinkChoice(null); setPlaceholderChoice(null); }, [project.id]);
  // A repeat duplicate ends as soon as the selection changes any other way.
  const selectedElementKey = targetKey(selection.filter(isElementTarget));
  useEffect(() => {
    if (duplicateRepeat.current && duplicateRepeat.current.key !== selectedElementKey) duplicateRepeat.current = null;
  }, [selectedElementKey]);
  useEffect(() => { if (tool !== 'request') { setRequestDraft(null); if (drag.current?.kind === 'draw-request-area' || drag.current?.kind === 'polygon-request') cancelDrag(); } }, [tool]);
  useEffect(() => { if (tool !== 'select') transitionPainter({ kind: 'disarm' }); }, [tool]);
  useEffect(() => { if (requestShape === 'pick') setPolygonDraft(null); }, [requestShape]);

  const { dropHover, dropNotice, setDropNotice } = useNativeDrop(surface, latest);

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

  function goToView(view: NamedView) {
    animateTo({ x: view.x, y: view.y, zoom: view.zoom });
    surface.current?.focus({ preventScroll: true });
  }

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
      const boundary = boundaries.find(item => item.pageId === focused && item.sizeId === focusedSize)
        ?? boundaries.find(item => item.pageId === focused);
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

  function handleKeyDown(event: KeyboardEvent): boolean {
    if (pasteChoice || linkChoice || event.isComposing) return false;
    const presenting = isPresenting(project, play);
    const presentationAction = presenting ? shortcutFor(event, false, false, 'presentation') : null;
    if (presentationAction) {
      event.preventDefault();
      if (presentationAction === 'exitPresentation') onPlayExit();
      else {
        const index = Math.max(0, project.pages.findIndex(page => page.id === play.boundary));
        const delta = presentationAction === 'nextSlide' ? 1 : -1;
        onPlayBoundary(project.pages[Math.max(0, Math.min(project.pages.length - 1, index + delta))].id);
      }
      return true;
    }
    if (isTextField(event.target)) return false;
    if (event.key === 'Escape') {
      event.preventDefault();
      if (preview) previewClose();
      else escapeCanvas();
      return true;
    }
    const heldAction = shortcutFor(event, false, false, 'held');
    if (heldAction !== 'holdPan' && heldAction !== 'holdZoom') return false;
    event.preventDefault(); holdTool(heldAction); return true;
  }

  useImperativeHandle(ref, () => ({ containsTarget: target => target instanceof Node && !!surface.current?.contains(target), handleKeyDown, handleKeyUp: heldKeyUp, focusCanvas: () => surface.current?.focus({ preventScroll: true }), fit, fitSelection, zoomStep, zoomTo, focusBoundary, armPointer, linkSelection, unlinkSelection, copyStyle, pasteStyle, paintFormat, currentCamera, goToView, canvasAction, startRequestOnSelection }));

  /** Double-click editing is the all-sizes mode: a layer drawn then belongs to every size. */
  const editingAllSizes = (pageId: string): boolean =>
    selection.some(target => target.kind === 'element' && target.pageId === pageId && target.sizeId === null);
  const editingSizeId = (pageId: string): string => {
    const page = project.pages.find(item => item.id === pageId);
    return page ? baseSizeId(page) : '';
  };
  function openPlaceholder(pageId: string, sizeId: string, elementId: string) {
    const screen = elementScreenRect(elementId, sizeId) ?? { left: 24, top: 24, width: 0, height: 0 };
    setPlaceholderChoice({ pageId, sizeId, elementId, screen });
    surface.current?.focus({ preventScroll: true });
  }
  async function resolvePlaceholder(choice: { pageId: string; sizeId: string; elementId: string }, include: boolean) {
    setPlaceholderChoice(null);
    await updateElements(choice.pageId, [{ id: choice.elementId, visible: include, size_id: choice.sizeId, dismissed: !include }]);
    selectElement(choice.pageId, choice.elementId, choice.sizeId);
  }
  const boundaryRect = (sizeId: string): Rect | null => {
    const boundary = boundaries.find(item => item.sizeId === sizeId);
    if (!boundary) return null;
    return moving?.sizeId === sizeId ? moving.rect : boundary.rect;
  };
  const pageFor = (pageId: string, sizeId?: string | null): Page | null => {
    const page = project.pages.find(item => item.id === pageId);
    return page ? { ...page, elements: resolveForSize(project, page, sizeId ?? baseSizeId(page)) } : null;
  };
  const boundaryUnder = (point: Point): Boundary | null => boundaryAt(boundaries.map(item => moving?.sizeId === item.sizeId ? { ...item, rect: moving.rect } : item), point);
  // Drawing forgives a press that misses the edge by a few screen pixels, as at a corner.
  const boundaryNearPress = (point: Point): Boundary | null => boundaryNear(boundaries, point, 16 / camera.zoom);
  const referenceBoxes = (): ReferenceBox[] => (project.reference ?? []).map(item => ({ itemId: item.id, rect: movingItems[item.id] ?? item.rect }));
  const requestBoxes = (): RequestBox[] => elementBoxes(project, boundaries, moving);
  const pointerWorldNow = () => pointerWorld(project, boundaries, moving, {}, movingItems);

  function nextSelectionFor(target: CanvasTarget, additive: boolean): CanvasTarget[] {
    const key = canvasTargetKey(target);
    if (!additive) return [target];
    return selection.some(item => canvasTargetKey(item) === key) ? selection.filter(item => canvasTargetKey(item) !== key) : [...selection, target];
  }

  function elementEntries(targets: CanvasTarget[], source: Project = project): ElementEntry[] {
    return resolveSelectionEntries(source, targets, boundaryRect);
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
    transitionText({ kind: 'open', session: { pageId, sizeId, elementId, autoWidth: element.style.sizing?.width === 'fit' || rect.width === 0, created } });
    selectElement(pageId, elementId, sizeId);
    surface.current?.focus({ preventScroll: true });
  }

  // The Image tool opens the platform file picker through the same picture store as reference.
  useEffect(() => {
    if (tool !== 'image' || pendingImage) return;
    imagePicker.current?.click();
  }, [tool, pendingImage]);
  useEffect(() => { if (tool !== 'image') setPendingImage(null); }, [tool]);
  // A chosen picture appears at once, centred in the boundary in view, rather than waiting
  // for a click nobody is told to make. Clicking or dragging can still place it instead.
  useEffect(() => {
    if (tool !== 'image' || !pendingImage || viewSize.width < 1 || busy.current) return;
    const view = viewportWorldRect(camera, viewSize);
    const centre = { x: view.x + view.width / 2, y: view.y + view.height / 2 };
    const distance = (rect: Rect) => Math.hypot(Math.max(rect.x - centre.x, 0, centre.x - rect.x - rect.width), Math.max(rect.y - centre.y, 0, centre.y - rect.y - rect.height));
    const boundary = boundaryUnder(centre) ?? [...boundaries].sort((a, b) => distance(a.rect) - distance(b.rect))[0];
    if (!boundary) return;
    const shown = {
      x: Math.max(boundary.rect.x, view.x), y: Math.max(boundary.rect.y, view.y),
      right: Math.min(boundary.rect.x + boundary.rect.width, view.x + view.width), bottom: Math.min(boundary.rect.y + boundary.rect.height, view.y + view.height),
    };
    const width = Math.max(1, Math.round(Math.min(pendingImage.width, 480, boundary.rect.width * 0.6)));
    const height = Math.max(1, Math.round(width * pendingImage.height / Math.max(1, pendingImage.width)));
    const middle = shown.right > shown.x && shown.bottom > shown.y
      ? { x: (shown.x + shown.right) / 2, y: (shown.y + shown.bottom) / 2 }
      : { x: boundary.rect.x + boundary.rect.width / 2, y: boundary.rect.y + boundary.rect.height / 2 };
    const rect = { x: Math.round(middle.x - width / 2), y: Math.round(middle.y - height / 2), width, height };
    const picture = pendingImage.uri;
    // Leave the Image tool first: clearing the picture under it would open the picker again.
    onTool('select');
    setPendingImage(null);
    busy.current = true;
    void placeElement('image', boundary.pageId, boundary.sizeId, null, { x: 0, y: 0 }, boundary.rect, rect, picture)
      .then(() => { createdElement.current = null; })
      .finally(() => { busy.current = false; });
  }, [pendingImage]);

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
  const dots = dotGrid(camera, project.showGrid, project.gridSettings.spacing);
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
  const behaviours = useMemo(() => behaviourEntries(pointers, pointerWorldNowValue), [pointers, pointerWorldNowValue]);
  const selectedPointerId = selection.length === 1 && selection[0].kind === 'pointer' ? selection[0].pointerId : null;
  const choosePointer = (pointerId: string) => { selectPointer(pointerId); surface.current?.focus({ preventScroll: true }); };
  const autoLinks = useMemo(() => automaticLinks(project, boundaries, elementBoxes(project, boundaries, moving)), [project, boundaries, moving]);
  const selectedElementTargets = useMemo(() => selection.filter(isElementTarget), [selection]);
  // The contextual bar stays out of every canvas gesture and every inline editor.
  const selectionBarVisible = tool === 'select' && !playIsActive(play) && !panning && !editing && !preview
    && !moving && !draft && !draftElement && !draftArea && !draftNote && !marquee && !zoomRect && !pointerDraft
    && !requestDraft && !pasteChoice && !linkChoice && !placeholderChoice
    && !Object.keys(movingElements).length && !Object.keys(movingItems).length;
  const selectionEntries = useMemo(() => selectionBarVisible ? elementEntries(selectedElementTargets) : [],
    [project, project.revision, boundaries, selectedElementTargets, selectionBarVisible]);
  const selectionScreen = selectionEntries.length ? screenRect(unionBounds(selectionEntries.map(entry => entry.world)), camera) : null;
  const selectionFills = [...new Set(selectionEntries.map(entry => entry.element.style.fill ?? ''))];
  const selectionFill = selectionFills.length === 1 ? selectionFills[0] : '';
  const selectionLinked = selectionEntries.length > 0 && selectionEntries.every(entry => !!linkedDefinition(project, entry.element));
  const selectionLinkable = selectionEntries.length > 0 && selectionEntries.every(entry => !linkedDefinition(project, entry.element));
  // Text, boxes and buttons can carry a hyperlink; the field lives in Properties.
  const selectionHyperlink = selectionEntries.length === 1
    && ['text', 'box', 'button'].includes(selectionEntries[0].element.kind);
  function focusElementLink() {
    document.querySelector<HTMLInputElement>('[data-field="element-link"]')?.focus({ preventScroll: true });
  }
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
  const editingElement = editing ? textDraft?.element ?? findElementIn(pageFor(editing.pageId, editing.sizeId)?.elements ?? [], editing.elementId) : null;
  const editingBoundary = editing ? boundaries.find(item => item.sizeId === editing.sizeId) ?? null : null;
  const editingScreen = editing && editingElement
    ? (textBounds ? screenRect(textBounds, camera)
      : textDraft ? screenRect(textDraft.worldRect, camera) : elementScreenRect(editing.elementId, editing.sizeId))
      ?? (editingBoundary ? textEditorScreen(project, editing.pageId, editingElement, editingBoundary, camera, editing.sizeId) : null)
    : null;

  // Both pending Text-tool drafts and double-click edits arrive here. Adjust
  // once on entry, before painting, so typing and later manual zoom stay put.
  useLayoutEffect(() => {
    if (!editing || !editingElement || !surface.current) return;
    const current = currentCamera();
    const screen = (textDraft ? screenRect(textDraft.worldRect, current) : elementScreenRect(editing.elementId, editing.sizeId))
      ?? (editingBoundary ? textEditorScreen(project, editing.pageId, editingElement, editingBoundary, current, editing.sizeId) : null);
    if (!screen) return;
    const font = editingElement.style.textStyle;
    const bounds = {
      x: (screen.left - current.x) / current.zoom,
      y: (screen.top - current.y) / current.zoom,
      width: screen.width / current.zoom,
      height: screen.height / current.zoom,
    };
    // Project the measured world bounds on later camera changes: reading the DOM
    // during render would still see the previous camera's transform.
    transitionText({ kind: 'measure', session: editing, bounds });
    // The dock is beside this surface; the floating selection bar hides while editing.
    const next = textEditingCamera(current, { ...bounds, width: Math.max(bounds.width, font.size),
      height: Math.max(bounds.height, font.size * font.lineHeight) }, font.size, surface.current.getBoundingClientRect());
    if (next === current) return;
    stopTransition();
    touched.current = true;
    textZoomReturn.current = { before: current, editing: next };
    setCamera(next);
  }, [editing]);

  // The editing zoom is temporary: when editing ends, glide back to where the user was,
  // unless they moved the view themselves while typing.
  const textZoomReturn = useRef<{ before: CanvasCamera; editing: CanvasCamera } | null>(null);
  useEffect(() => {
    const pending = textZoomReturn.current;
    if (editing || !pending) return;
    textZoomReturn.current = null;
    const now = currentCamera();
    const unmoved = Math.abs(now.zoom - pending.editing.zoom) < 1e-6
      && Math.abs(now.x - pending.editing.x) < 1 && Math.abs(now.y - pending.editing.y) < 1;
    if (unmoved) animateTo(pending.before);
  }, [editing]);

  function elementScreenRect(elementId: string, sizeId?: string | null): ScreenRect | null {
    const scope = sizeId ? `[data-size="${CSS.escape(sizeId)}"] ` : '';
    const node = surface.current?.querySelector<HTMLElement>(`${scope}[data-element="${CSS.escape(elementId)}"]`);
    if (!node || !surface.current) return null;
    const host = surface.current.getBoundingClientRect();
    const box = node.getBoundingClientRect();
    return { left: box.left - host.left, top: box.top - host.top, width: box.width, height: box.height };
  }

  async function commitEditingText(text: string) {
    if (!editing) return;
    const current = editing;
    const retainFocus = document.activeElement === surface.current?.querySelector('.element-text-editor');
    const state = textEditing.current;
    const pending = state.kind === 'pending' || state.kind === 'draft' ? state.draft : null;
    transitionText({ kind: 'finish' });
    createdElement.current = null;
    if (!text.trim() && (pending || editingElement?.kind === 'text')) {
      if (!pending) removeElements([{ pageId: current.pageId, elementId: current.elementId }]);
      if (retainFocus) surface.current?.focus({ preventScroll: true });
      return;
    }
    if (pending) {
      try {
        await insertElements(pending.pageId, pending.parentId, pending.index, [{ ...pending.element, text }], undefined, pending.sizeId);
      } catch {
        transitionText({ kind: 'retry', session: current, draft: { ...pending, element: { ...pending.element, text } } });
        return;
      }
    } else {
      await updateElements(current.pageId, [{ id: current.elementId, text }]);
    }
    const page = project.pages.find(item => item.id === current.pageId);
    if (retainFocus || pending) selectElement(current.pageId, current.elementId, page && hasManySizes(page) ? current.sizeId : null);
    if (retainFocus) surface.current?.focus({ preventScroll: true });
  }

  function finishEditingText() {
    // Blurring uses the editor's current draft, including when Escape originates on the canvas.
    surface.current?.querySelector<HTMLTextAreaElement>('.element-text-editor')?.blur();
  }

  return {
    surface, tool, requestShape, panning, spaceHeld, zoomHeld, painterArmed, play, dropHover, dotStyle,
    pointerDown, keyDown, doubleClick, paste, dropText, imagePicker, chooseImage, camera, scrubbed, focused,
    selection, moving, openPage, editing, movingElements, openPlaceholder, viewport, inverse, project,
    boundaries, requestDraft, draftArea, selectRequest, movingItems, draftNote, draft, boundaryKind,
    draftElement, marquee, zoomRect, entries, autoLinks, behaviours, pointerDraft, selectedPointerIds, pointerStates,
    choosePointer, updatePointer, createdPointer, selectionScreen, viewSize,
    selectionFill, selectionLinked, selectionLinkable, selectionHyperlink, focusElementLink, selectionScope, setSelectionFill, alignSelection, distributeSelection,
    setSelectionScope, startRequestOnSelection, linkSelection, unlinkSelection, duplicateElements,
    deleteSelectedLayers, minimapVisible, stopTransition, touched, setCamera, selectedPointerId, armedPointer,
    startReattach, selectedElementTargets, startElementResize, editingElement, editingScreen,
    commitEditingText, finishEditingText, placeholderChoice, resolvePlaceholder, setPlaceholderChoice,
    submitRequestDraft, askRequestDraft, cancelRequestDraft, selectedItem, selectedItemScreen, createdItem, updateReference,
    startReferenceResize, pasteChoice, setPasteChoice, confirmPaste, linkChoice, linkCopies, setLinkChoice,
    preview, previewRefresh, previewClose, dropNotice, setDropNotice,
  };
}
