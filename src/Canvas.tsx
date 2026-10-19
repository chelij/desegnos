import { forwardRef, useImperativeHandle, useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type PointerEvent, type KeyboardEvent, type MouseEvent as ReactMouseEvent } from 'react';
import { flushSync } from 'react-dom';
import { useDraftText } from './DraftText';
import { LayersPopover, LayerIcon } from './Layers';
import type { SiteLayer } from './siteLayers';
import { ElementDialog, type ElementDialogMode } from './ElementTools';
import { ContextMenu, type MenuItem } from './Menus';
import { PreviewFrame, type CanvasPreviewHandle, type ElementMover, type ElementResizer, type InlineText } from './Preview';
import { alignmentLines, elementResizeRect, elementToolbarPosition, clamp, clientToCanvas, DEFAULT_GRID, expandWorkspace, geometryForGesture, gridSteps, workspaceWindow, keyboardRectChange, percent, percentageTicks, pixelTicks, requestAt, requestPlacement, sameBounds, sameRect, intersectsWorkspace, clipToWorkspace, validWorkspace, MIN_ZOOM, wheelZoom, zoomOrigin, type CanvasGridSettings, type GeometryGesture, type Point, type RectChange, type WorkspaceBounds, type ResizeCorner } from './canvasGeometry';
import type { ElementBounds, ElementDetails, ElementEdit, ElementSelection, ElementTarget, ElementStyleEdit, LayerTarget, LayerDirection, LayerPasteView, Page, Rect, Region, Viewport } from './types';
import './canvas.css';

export type RequestPatch = { prompt?: string; rect?: RectChange | Partial<Rect> };
export type CanvasViewAction = { serial: number; kind: 'guide' | 'selection' | 'fit' };
type Gesture = GeometryGesture & { pointerId: number; region?: Region; active: boolean };
type CanvasContext = { point: Point } & ({ kind: 'element'; selection: ElementSelection } | { kind: 'request'; regionId: string } | { kind: 'canvas'; at: Point });
type Measurements = { left: number; top: number; width: number; height: number };
const emptyMeasurements: Measurements = { left: 0, top: 0, width: 0, height: 0 };
const isTextInput = (target: EventTarget | null) => target instanceof HTMLElement && (target.matches('input, textarea, select') || target.isContentEditable);

function viewStyle(rect: Rect, origin: Point, zoom: number): CSSProperties {
  return { left: (rect.x - origin.x) * zoom, top: (rect.y - origin.y) * zoom, width: rect.width * zoom, height: rect.height * zoom };
}
function windowStyle(rect: Rect, bounds: WorkspaceBounds, zoom: number): CSSProperties {
  const origin = { x: bounds.left, y: bounds.top };
  return intersectsWorkspace(rect, bounds) ? viewStyle(rect, origin, zoom)
    : { ...viewStyle({ ...rect, x: origin.x, y: origin.y }, origin, zoom), visibility: 'hidden' };
}

function paintStyle(rect: Rect, bounds: WorkspaceBounds, zoom: number): CSSProperties {
  const clipped = clipToWorkspace(rect, bounds);
  return clipped ? viewStyle(clipped, { x: bounds.left, y: bounds.top }, zoom) : { visibility: 'hidden', left: 0, top: 0, width: 0, height: 0 };
}

type CanvasProps = {
  canUndo: boolean; canRedo: boolean; undo: () => void; redo: () => void; toggleGrid: () => void; configureGrid: () => void;
  page: Page; layersOpen: boolean; setLayersOpen: (open: boolean) => void; selectionChanged: (target: ElementTarget | null) => void;
  arrangeRequest: (id: string, direction: LayerDirection) => Promise<void>; copyNative: (target: LayerTarget, styles: ElementStyleEdit[]) => Promise<void>; cutNative: (target: LayerTarget, styles: ElementStyleEdit[]) => Promise<void>; pasteNative: (after: ElementTarget | null) => Promise<LayerPasteView>;
  grid: boolean; gridSettings?: CanvasGridSettings; tool?: 'select' | 'request' | 'pan'; rulerUnit?: 'pixels' | 'percent'; zoom: number; selected: string | null; focusRequest: string | null; viewAction?: CanvasViewAction; changing?: Rect[]; highlighted?: Rect[];
  refreshElementBounds: (pageId: string, viewport: Viewport, bounds: ElementBounds[]) => Promise<void>;
  inspectElement: (target: ElementTarget) => Promise<ElementDetails>; editElement: (target: ElementTarget, change: ElementEdit) => Promise<ElementTarget | null>;
  changeZoom: (zoom: number) => void; select: (id: string | null, cleanEmpty?: boolean) => void; add: (rect: Rect) => void; pickElement: (selection: ElementSelection) => void; change: (id: string, patch: RequestPatch) => Promise<Region>; remove: (id: string) => void;
};
export type CanvasHandle = { synchronize: (requireBounds?: boolean) => Promise<void>; addRequest: () => void; focusRequest: (id: string) => void; askAI: () => void; copyLayer: () => Promise<void>; cutLayer: () => Promise<void>; pasteLayer: () => Promise<void>; arrangeLayer: (direction: LayerDirection) => Promise<void> };
export const Canvas = forwardRef<CanvasHandle, CanvasProps>(function Canvas({ canUndo, canRedo, undo, redo, toggleGrid, configureGrid, page, layersOpen, setLayersOpen, selectionChanged, arrangeRequest, copyNative, cutNative, pasteNative, grid, gridSettings = DEFAULT_GRID, tool = 'select', rulerUnit = 'pixels', zoom, selected, focusRequest, viewAction, changing = [], highlighted = [], inspectElement, editElement, refreshElementBounds, changeZoom, select, add, pickElement, change, remove }, ref) {
  const toolbar = useRef<HTMLDivElement>(null);
  const [toolbarSize, setToolbarSize] = useState({ width: 160, height: 40 });
  const [context, setContext] = useState<CanvasContext | null>(null);
  const scroll = useRef<HTMLDivElement>(null);
  const site = useRef<CanvasPreviewHandle>(null);
  const [siteLayers, setSiteLayers] = useState<SiteLayer[]>([]);
  const [layersReady, setLayersReady] = useState(false);
  const [layerBusy, setLayerBusy] = useState(false);
  const [siteHeight, setSiteHeight] = useState(page.viewport.height);
  useEffect(() => setSiteHeight(page.viewport.height), [page.id, page.content?.html, page.content?.css, page.content?.javascript, page.viewport.width, page.viewport.height]);
  const stage = useRef<HTMLDivElement>(null);
  const gesture = useRef<Gesture | null>(null);
  const pan = useRef<{ pointerId: number; origin: Point; start: Point; click: boolean; active: boolean } | null>(null);
  const plainClicks = useRef(0);
  const lastPlainClick = useRef<{ at: number; point: Point } | null>(null);
  const [element, setElement] = useState<ElementSelection | null>(null);
  const [elementDialog, setElementDialog] = useState<ElementDialogMode | null>(null);
  const [elementError, setElementError] = useState('');
  const [inlineText, setInlineText] = useState<InlineText | null>(null);
  const inlineTextRef = useRef(inlineText); inlineTextRef.current = inlineText;
  const inlineTicket = useRef(0);
  const inlineSaving = useRef<Promise<void> | null>(null);
  const [requestFocus, setRequestFocus] = useState({ id: '', serial: 0 });
  const elementDrag = useRef<{ pointerId: number; pageId: string; version: string; start: Point; world: Point; original: ElementSelection; mover: ElementMover | null; resizer: ElementResizer | null; corner?: ResizeCorner; active: boolean; fine: boolean; x: number; y: number; rect: Rect } | null>(null);
  const [editingElement, setEditingElement] = useState(false);
  const [nudging, setNudging] = useState(false);
  const selectionSerial = useRef(0);
  const elementRef = useRef(element); elementRef.current = element;
  const dialogRef = useRef(elementDialog); dialogRef.current = elementDialog;
  const pendingSelection = useRef<ElementTarget | null>(null);
  const pageRef = useRef(page); pageRef.current = page;
  const nudge = useRef({ x: 0, y: 0, promise: null as Promise<void> | null });
  const syncing = useRef<Promise<void> | null>(null);
  function storeElement(value: ElementSelection | null) { elementRef.current = value; setElement(value); selectionChanged(value?.element ?? null); }
  function deselectElement() { setContext(null); inlineTicket.current++; selectionSerial.current++; nudge.current.x = 0; nudge.current.y = 0; pendingSelection.current = null; storeElement(null); }
  useLayoutEffect(() => { deselectElement(); setElementDialog(null); setElementError(''); }, [page.id]);
  const zoomIntent = useRef<{ pageId: string; zoom: number; origin: Point } | null>(null);
  const live = useRef({ zoom, changeZoom, pageId: page.id });
  live.current = { zoom, changeZoom, pageId: page.id };
  const panKey = useRef(false);
  const lastPointer = useRef<Point | null>(null);
  const savedViews = useRef(new Map<string, { center: Point }>());
  const [workspace, setWorkspace] = useState(() => ({ pageId: page.id, bounds: workspaceWindow({ x: 0, y: 0 }, { x: 1200 / zoom, y: 800 / zoom }) }));
  const bounds = workspace.bounds;
  const origin = { x: bounds.left, y: bounds.top };
  function positioned(rect: Rect): CSSProperties {
    return windowStyle(rect, bounds, zoom);
  }
  const boundsRef = useRef(bounds);
  boundsRef.current = bounds;
  const previous = useRef({ pageId: '', zoom, bounds });
  const pendingOrigin = useRef<{ origin: Point; bounds: WorkspaceBounds; pageId: string; zoom: number } | null>(null);
  const measurementsRef = useRef(emptyMeasurements);
  const [measurements, setMeasurements] = useState(emptyMeasurements);
  const [preview, setPreview] = useState<Rect | null>(null);
  const [cursor, setCursor] = useState<Point | null>(null);
  const [panReady, setPanReady] = useState(false);
  const [panning, setPanning] = useState(false);
  const [dragging, setDragging] = useState(false);
  const requestRanks = new Map(page.regions.map((region, index) => ({ region, index })).filter(item => !item.region.completed)
    .sort((a, b) => a.region.zIndex - b.region.zIndex || a.index - b.index).map((item, index) => [item.region.id, index + 2]));
  const overlayLayer = requestRanks.size + 3;
  const minimum = { x: 160 / zoom, y: 80 / zoom };
  const previewHeight = Math.max(page.viewport.height, siteHeight);
  const acceptedVersion = useMemo(() => JSON.stringify([page.content, page.viewport, page.appearance]),
    [page.content?.html, page.content?.css, page.content?.javascript, page.viewport.width, page.viewport.height, page.appearance]);
  const acceptedVersionRef = useRef(acceptedVersion); acceptedVersionRef.current = acceptedVersion;
  useLayoutEffect(() => { setContext(null); plainClicks.current = 0; lastPlainClick.current = null; if (elementDrag.current) cancel(); inlineTicket.current++; inlineTextRef.current = null; setInlineText(null); }, [acceptedVersion]);

  function measure() {
    const node = scroll.current;
    if (!node) return;
    const value = { left: node.scrollLeft, top: node.scrollTop, width: node.clientWidth, height: node.clientHeight };
    measurementsRef.current = value;
    setMeasurements(current => current.left === value.left && current.top === value.top && current.width === value.width && current.height === value.height ? current : value);
  }
  function visibleBounds(origin: Point, size: Point): WorkspaceBounds {
    return { left: origin.x, top: origin.y, right: origin.x + size.x, bottom: origin.y + size.y };
  }
  function seek(origin: Point) {
    const node = scroll.current;
    if (!node) return;
    const size = { x: node.clientWidth / zoom, y: node.clientHeight / zoom };
    const next = workspaceWindow(origin, size);
    if (!validWorkspace(next, size)) { setElementError('That position is too distant to display accurately.'); return; }
    pendingOrigin.current = { origin, bounds: next, pageId: page.id, zoom };
    setWorkspace({ pageId: page.id, bounds: next });
  }
  function show(kind: CanvasViewAction['kind']) {
    const node = scroll.current;
    if (!node) return;
    if (kind === 'fit') {
      const target = Math.max(MIN_ZOOM, Math.min(4, Math.max(1, node.clientWidth - 96) / page.viewport.width, Math.max(1, node.clientHeight - 160) / page.viewport.height));
      const origin = { x: page.viewport.width / 2 - node.clientWidth / target / 2, y: page.viewport.height / 2 - (node.clientHeight - 64) / target / 2 };
      if (Math.abs(target - zoom) < 1e-8) seek(origin);
      else { zoomIntent.current = { pageId: page.id, zoom: target, origin }; changeZoom(target); }
      return;
    }
    const region = kind === 'selection' ? page.regions.find(region => region.id === selected) : null;
    seek(region ? { x: region.rect.x + region.rect.width / 2 - node.clientWidth / zoom / 2, y: region.rect.y + region.rect.height / 2 - node.clientHeight / zoom / 2 }
      : { x: page.viewport.width / 2 - node.clientWidth / zoom / 2, y: -64 / zoom });
  }
  function point(client: Point): Point {
    return clientToCanvas(client, stage.current!.getBoundingClientRect(), zoom, { x: boundsRef.current.left, y: boundsRef.current.top });
  }
  function cancel(resetClicks = true) {
    if (resetClicks) { plainClicks.current = 0; lastPlainClick.current = null; }
    const pointerId = gesture.current?.pointerId ?? pan.current?.pointerId ?? elementDrag.current?.pointerId;
    if (elementDrag.current) { elementDrag.current.mover?.cancel(); elementDrag.current.resizer?.cancel(); if (elementDrag.current.pageId === pageRef.current.id && elementDrag.current.version === acceptedVersionRef.current) storeElement(elementDrag.current.original); else deselectElement(); elementDrag.current = null; }
    gesture.current = null; pan.current = null; lastPointer.current = null;
    if (pointerId !== undefined && stage.current?.hasPointerCapture(pointerId)) stage.current.releasePointerCapture(pointerId);
    setPreview(null); setDragging(false); setPanning(false);
  }

  useLayoutEffect(() => {
    const node = scroll.current;
    if (!node) return;
    const prior = previous.current;
    const measured = measurementsRef.current;
    if (prior.pageId !== page.id) {
      if (prior.pageId) savedViews.current.set(prior.pageId, { center: { x: prior.bounds.left + (measured.left + measured.width / 2) / prior.zoom, y: prior.bounds.top + (measured.top + measured.height / 2) / prior.zoom } });
      cancel(); zoomIntent.current = null; setCursor(null);
      const saved = savedViews.current.get(page.id);
      previous.current = { pageId: page.id, zoom, bounds };
      seek(saved ? { x: saved.center.x - node.clientWidth / zoom / 2, y: saved.center.y - node.clientHeight / zoom / 2 }
        : { x: page.viewport.width / 2 - node.clientWidth / zoom / 2, y: -64 / zoom });
    } else if (prior.zoom !== zoom) {
      cancel(); setCursor(null);
      const center = { x: prior.bounds.left + (measured.left + measured.width / 2) / prior.zoom, y: prior.bounds.top + (measured.top + measured.height / 2) / prior.zoom };
      previous.current = { ...prior, zoom };
      const intent = zoomIntent.current;
      zoomIntent.current = null;
      seek(intent?.pageId === page.id && intent.zoom === zoom ? intent.origin
        : { x: center.x - node.clientWidth / zoom / 2, y: center.y - node.clientHeight / zoom / 2 });
    }
  }, [page.id, zoom]);
  useLayoutEffect(() => {
    const node = scroll.current;
    if (!node || workspace.pageId !== page.id) return;
    const pending = pendingOrigin.current;
    if (pending && (pending.pageId !== workspace.pageId || pending.zoom !== zoom || !sameBounds(pending.bounds, bounds))) return;
    pendingOrigin.current = null;
    if (pending) { node.scrollLeft = (pending.origin.x - bounds.left) * zoom; node.scrollTop = (pending.origin.y - bounds.top) * zoom; }
    previous.current = { pageId: page.id, zoom, bounds };
    measure();
  }, [workspace, zoom]);
  useEffect(() => {
    const node = scroll.current;
    if (!node) return;
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    function wheel(event: WheelEvent) {
      if (event.ctrlKey || event.metaKey) {
        event.preventDefault(); event.stopPropagation();
        if (gesture.current || pan.current || elementDrag.current || event.deltaY === 0) return;
        const current = live.current;
        const intent = zoomIntent.current?.pageId === current.pageId ? zoomIntent.current : null;
        const from = intent?.zoom ?? current.zoom;
        const delta = event.deltaY * (event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? node!.clientHeight : 1);
        const target = wheelZoom(from, delta);
        if (target === from) return;
        const rect = node!.getBoundingClientRect();
        const pointer = { x: clamp(event.clientX - rect.left, 0, node!.clientWidth), y: clamp(event.clientY - rect.top, 0, node!.clientHeight) };
        const origin = intent?.origin ?? { x: boundsRef.current.left + node!.scrollLeft / current.zoom, y: boundsRef.current.top + node!.scrollTop / current.zoom };
        zoomIntent.current = { pageId: current.pageId, zoom: target, origin: zoomOrigin(origin, pointer, from, target) };
        current.changeZoom(target); return;
      }
      if (!event.shiftKey || event.ctrlKey || event.metaKey || event.deltaX !== 0) return;
      event.preventDefault();
      const multiplier = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? node!.clientWidth : 1;
      node!.scrollLeft += event.deltaY * multiplier;
    }
    function release(event: globalThis.KeyboardEvent) { if (event.code === 'Space') { panKey.current = false; setPanReady(false); } }
    function escape(event: globalThis.KeyboardEvent) { if (event.key === 'Escape' && pan.current) { event.preventDefault(); event.stopPropagation(); cancel(); } }
    function blur() { panKey.current = false; setPanReady(false); cancel(); }
    node.addEventListener('wheel', wheel, { passive: false });
    window.addEventListener('keyup', release); window.addEventListener('keydown', escape, true); window.addEventListener('blur', blur);
    return () => { observer.disconnect(); node.removeEventListener('wheel', wheel); window.removeEventListener('keyup', release); window.removeEventListener('keydown', escape, true); window.removeEventListener('blur', blur); };
  }, []);
  const lastViewAction = useRef<number | null>(viewAction?.serial ?? null);
  useLayoutEffect(() => {
    if (!viewAction || viewAction.serial === lastViewAction.current) return;
    lastViewAction.current = viewAction.serial;
    show(viewAction.kind);
  }, [viewAction]);

  function scrolled() {
    plainClicks.current = 0; lastPlainClick.current = null;
    measure();
    if (pendingOrigin.current) return;
    const node = scroll.current!;
    const current = boundsRef.current;
    const origin = { x: current.left + node.scrollLeft / zoom, y: current.top + node.scrollTop / zoom };
    const size = { x: node.clientWidth / zoom, y: node.clientHeight / zoom };
    const next = expandWorkspace(current, visibleBounds(origin, size), { x: Math.max(600, size.x), y: Math.max(600, size.y) });
    if (!sameBounds(current, next)) {
      // Commit a scroll rebase before another wheel or drag-edge delta arrives.
      pendingOrigin.current = { origin, bounds: next, pageId: page.id, zoom };
      flushSync(() => setWorkspace({ pageId: page.id, bounds: next }));
    }
  }
  function startPan(event: PointerEvent, click: boolean) {
    if (gesture.current || pan.current || elementDrag.current) return;
    event.preventDefault(); event.stopPropagation();
    if (!click) plainClicks.current = 0;
    const client = { x: event.clientX, y: event.clientY };
    pan.current = { pointerId: event.pointerId, origin: { x: boundsRef.current.left + scroll.current!.scrollLeft / zoom, y: boundsRef.current.top + scroll.current!.scrollTop / zoom }, start: client, click, active: false };
    stage.current!.focus({ preventScroll: true });
    stage.current!.setPointerCapture(event.pointerId); setCursor(null);
  }
  function beginPan(event: PointerEvent) {
    if (event.target instanceof Element && event.target.closest('.canvas-empty-hint button')) return;
    if (event.button === 1 || event.button === 0 && (panKey.current || tool === 'pan')) startPan(event, false);
  }
  function begin(event: PointerEvent, kind: Gesture['kind'], region?: Region) {
    if (event.button !== 0 || gesture.current || pan.current || panKey.current) return;
    event.preventDefault(); event.stopPropagation();
    const start = point({ x: event.clientX, y: event.clientY });
    gesture.current = { kind, start, initial: region?.rect ?? { x: start.x, y: start.y, width: 0, height: 0 }, pointerId: event.pointerId, region, active: false };
    lastPointer.current = { x: event.clientX, y: event.clientY };
    plainClicks.current = 0;
    if (region) select(region.id);
    (event.currentTarget as HTMLElement).focus({ preventScroll: true });
    stage.current!.setPointerCapture(event.pointerId);
    setPreview(gesture.current.initial); setDragging(true);
  }
  function currentRect(client: Point, current: Gesture): Rect | null {
    const now = point(client);
    setCursor(now);
    if (!current.active) current.active = Math.hypot((now.x - current.start.x) * zoom, (now.y - current.start.y) * zoom) >= 3;
    return current.active ? geometryForGesture(current, now, page.viewport, gridSettings, minimum) : null;
  }
  function updateDrag(client: Point) {
    if (elementDrag.current) { updateElementDrag(client, elementDrag.current.fine); return; }
    const current = gesture.current;
    if (!current) return;
    const rect = currentRect(client, current);
    if (rect) setPreview(current.kind === 'new' ? requestPlacement(rect, zoom) : rect);
  }
  const updateDragRef = useRef(updateDrag);
  updateDragRef.current = updateDrag;
  useEffect(() => {
    if (!dragging) return;
    let frame = 0;
    function tick() {
      const node = scroll.current;
      const client = lastPointer.current;
      if (!node || !client || (!gesture.current && !elementDrag.current)) return;
      const rect = node.getBoundingClientRect();
      const speed = (value: number, start: number, end: number) => value < start + 32 ? -Math.min(18, (start + 32 - value) / 3) : value > end - 32 ? Math.min(18, (value - end + 32) / 3) : 0;
      const dx = speed(client.x, rect.left, rect.right); const dy = speed(client.y, rect.top, rect.bottom);
      if ((dx || dy) && (gesture.current?.active || elementDrag.current?.active)) { node.scrollBy(dx, dy); updateDragRef.current(client); }
      frame = requestAnimationFrame(tick);
    }
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [dragging]);
  function chooseElement(selection: ElementSelection | null) { deselectElement(); storeElement(selection); setElementError(''); select(null); }
  async function applyElement(change: ElementEdit, selected = elementRef.current) {
    if (!selected) throw new Error('Select an element first.');
    const ticket = selectionSerial.current; const pageId = page.id;
    const ownsSelection = elementRef.current?.element.selector === selected.element.selector && elementRef.current?.element.sourceSha256 === selected.element.sourceSha256;
    setEditingElement(true); setElementError('');
    try {
      const target = await editElement(selected.element, change);
      if (ownsSelection && pageRef.current.id === pageId && selectionSerial.current === ticket) {
        pendingSelection.current = target;
        storeElement(target ? { ...selected, element: target } : null);
      }
    } catch (error) { if (pageRef.current.id === pageId && selectionSerial.current === ticket) setElementError(error instanceof Error ? error.message : String(error)); throw error; }
    finally { setEditingElement(false); }
  }
  async function editTextAt(at: Point) {
    const text = site.current?.textAt(at.x, at.y);
    if (!text) { if (elementRef.current) setElementDialog('edit'); return; }
    chooseElement(text.selection);
    const ticket = ++inlineTicket.current; const pageId = page.id;
    try {
      const details = await inspectElement(text.selection.element);
      if (ticket !== inlineTicket.current || pageRef.current.id !== pageId || site.current?.source() !== text.selection.element.sourceSha256) return;
      if (!details.texts.some(item => item.index === text.index && item.text === text.text)) { setElementDialog('edit'); return; }
      setInlineText(text);
    } catch (error) { if (ticket === inlineTicket.current) setElementError(String(error)); }
  }
  function finishText(value: string | null, restoreFocus = false): Promise<void> {
    if (inlineSaving.current) return inlineSaving.current;
    const text = inlineTextRef.current; if (!text) return Promise.resolve();
    inlineTicket.current++;
    const work = async () => {
      if (value !== null && value !== text.text) await applyElement({ kind: 'update', texts: [{ index: text.index, text: value }], attributes: {}, style: null }, text.selection);
      if (inlineTextRef.current === text) { inlineTextRef.current = null; setInlineText(null); }
      if (restoreFocus) stage.current?.focus({ preventScroll: true });
    };
    const pending = work().finally(() => { if (inlineSaving.current === pending) inlineSaving.current = null; });
    inlineSaving.current = pending; return pending;
  }
  async function moveElement(x: number, y: number) {
    const selected = elementRef.current;
    if (!selected || !x && !y) return;
    const mover = site.current?.beginMove(selected.element);
    if (!mover) throw new Error('This item cannot be moved here. Select it again.');
    try { await applyElement({ kind: 'update', texts: [], attributes: {}, style: mover.style(x, y) }, selected); }
    finally { mover.cancel(); }
  }
  const moveElementRef = useRef(moveElement); moveElementRef.current = moveElement;
  async function waitForSite(pageId: string, target?: ElementTarget) {
    const deadline = performance.now() + 4000;
    do {
      if (pageRef.current.id !== pageId) throw new Error('The page changed. Select the item again.');
      if (site.current?.source() && (!target || site.current.find(target))) return;
      await new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
    } while (performance.now() < deadline);
    throw new Error('The editable page is still loading. Try again when it appears.');
  }
  function nudgeElement(x: number, y: number) {
    nudge.current.x += x; nudge.current.y += y;
    if (nudge.current.promise) return;
    setNudging(true);
    const ticket = selectionSerial.current; const pageId = pageRef.current.id;
    const work = async () => {
      await new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
      while (nudge.current.x || nudge.current.y) {
        if (ticket !== selectionSerial.current || pageRef.current.id !== pageId || !elementRef.current) return;
        await waitForSite(pageId, elementRef.current.element);
        if (ticket !== selectionSerial.current) return;
        const { x, y } = nudge.current; nudge.current.x = 0; nudge.current.y = 0;
        await moveElementRef.current(x, y);
      }
    };
    nudge.current.promise = work().catch(error => {
      if (ticket === selectionSerial.current) { nudge.current.x = 0; nudge.current.y = 0; setElementError(String(error)); }
    }).finally(() => {
      nudge.current.promise = null;
      if (nudge.current.x || nudge.current.y) nudgeElement(0, 0); else setNudging(false);
    });
  }
  function synchronizeBounds(): Promise<void> {
    if (syncing.current) return syncing.current;
    const work = async () => {
      const current = pageRef.current;
      const regions = current.regions.filter(region => !region.completed && region.element);
      if (!regions.length) return;
      await waitForSite(current.id);
      if (JSON.stringify(pageRef.current.content) !== JSON.stringify(current.content) || pageRef.current.id !== current.id) throw new Error('The page changed while measuring its items. Try again.');
      const bounds: ElementBounds[] = [];
      for (const region of regions) {
        const found = site.current?.find(region.element!);
        if (!found) throw new Error('An item with a request changed. Select it again before generating.');
        if (!sameRect(found.rect, region.rect)) bounds.push({ regionId: region.id, target: region.element!, rect: found.rect });
      }
      if (bounds.length) await refreshElementBounds(current.id, current.viewport, bounds);
    };
    syncing.current = work().finally(() => { syncing.current = null; });
    return syncing.current;
  }
  function addRequestInView() {
    const node = scroll.current; if (!node) return;
    const current = boundsRef.current;
    const start = { x: current.left + node.scrollLeft / zoom, y: current.top + node.scrollTop / zoom };
    const end = { x: start.x + node.clientWidth / zoom, y: start.y + node.clientHeight / zoom };
    const withinGuide = end.x > 0 && start.x < page.viewport.width && end.y > 0 && start.y < page.viewport.height;
    const center = withinGuide ? { x: (Math.max(0, start.x) + Math.min(page.viewport.width, end.x)) / 2, y: (Math.max(0, start.y) + Math.min(page.viewport.height, end.y)) / 2 }
      : { x: (start.x + end.x) / 2, y: (start.y + end.y) / 2 };
    deselectElement(); add(requestAt({ x: center.x - 160 / zoom, y: center.y - 80 / zoom }, zoom));
  }
  function focusRegion(id: string) {
    const region = pageRef.current.regions.find(item => item.id === id && !item.completed); if (!region) return;
    const node = scroll.current; if (!node) return;
    deselectElement(); select(id);
    seek({ x: region.rect.width * zoom > node.clientWidth - 96 ? region.rect.x - 48 / zoom : region.rect.x + region.rect.width / 2 - node.clientWidth / zoom / 2,
      y: region.rect.height * zoom > node.clientHeight - 96 ? region.rect.y - 48 / zoom : region.rect.y + region.rect.height / 2 - node.clientHeight / zoom / 2 });
    setRequestFocus(current => ({ id, serial: current.serial + 1 }));
  }
  function refreshLayers() { const ready = !!site.current?.source(); setLayersReady(ready); setSiteLayers(ready ? site.current!.layers() : []); }
  useEffect(() => { if (layersOpen) refreshLayers(); }, [layersOpen, page.id, acceptedVersion]);
  async function finishLayerDraft() {
    if (inlineSaving.current) await inlineSaving.current;
    const text = inlineTextRef.current;
    if (text) await finishText(stage.current?.querySelector<HTMLElement>('.inline-text-editor')?.innerText ?? text.text);
    while (nudge.current.promise) await nudge.current.promise;
    if (elementDrag.current) cancel();
    const target = elementRef.current?.element;
    if (target) await waitForSite(pageRef.current.id, target);
  }
  async function copyLayer() {
    if (layerBusy) return;
    setLayerBusy(true); setElementError('');
    try {
      await finishLayerDraft();
      const region = pageRef.current.regions.find(item => item.id === selected && !item.completed);
      if (region) await copyNative({ kind: 'request', regionId: region.id }, []);
      else if (elementRef.current && elementRef.current.element.selector !== 'body') {
        const target = elementRef.current.element;
        await copyNative({ kind: 'element', target }, site.current!.copyStyles(target));
      } else throw new Error('Select a layer to copy.');
    } catch (error) { setElementError(error instanceof Error ? error.message : String(error)); }
    finally { setLayerBusy(false); }
  }
  async function cutLayer() {
    if (layerBusy || editingElement || nudging) return;
    setLayerBusy(true); setElementError('');
    try {
      await finishLayerDraft();
      const ticket = selectionSerial.current, pageId = pageRef.current.id;
      const region = pageRef.current.regions.find(item => item.id === selected && !item.completed);
      if (region) await cutNative({ kind: 'request', regionId: region.id }, []);
      else if (elementRef.current && elementRef.current.element.selector !== 'body') {
        const target = elementRef.current.element;
        await cutNative({ kind: 'element', target }, site.current!.copyStyles(target));
      } else throw new Error('Select a layer to cut.');
      if (pageRef.current.id === pageId && selectionSerial.current === ticket) { deselectElement(); select(null, false); }
      refreshLayers();
    } catch (error) { setElementError(error instanceof Error ? error.message : String(error)); }
    finally { setLayerBusy(false); }
  }
  async function pasteLayer() {
    if (layerBusy) return;
    setLayerBusy(true); setElementError('');
    let ownsSelection = () => true;
    try {
      await finishLayerDraft();
      const pageId = pageRef.current.id, ticket = selectionSerial.current;
      ownsSelection = () => pageRef.current.id === pageId && selectionSerial.current === ticket;
      const result = await pasteNative(elementRef.current?.element ?? null);
      if (!ownsSelection()) return;
      const pastedPage = result.workspace.project?.pages.find(item => item.id === pageId);
      if (!pastedPage) return;
      if (result.selection.element) {
        const target = result.selection.element;
        pendingSelection.current = target;
        storeElement(null);
        const expected = JSON.stringify([pastedPage.content, pastedPage.viewport, pastedPage.appearance]);
        const deadline = performance.now() + 4000;
        let rendered = false;
        do {
          await new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
          if (!ownsSelection()) return;
          if (acceptedVersionRef.current === expected) rendered = true;
          else if (rendered) return; // Undo or another edit owns the new page now.
          const item = rendered ? site.current?.find(target) : null;
          if (item) { deselectElement(); storeElement(item); select(null, false); reveal(item.rect, false); refreshLayers(); return; }
        } while (performance.now() < deadline);
        if (acceptedVersionRef.current !== expected) return;
        throw new Error('The pasted layer is saved, but the editable page is still loading. Select it in Layers when it appears.');
      } else if (result.selection.regionId) {
        deselectElement();
        select(result.selection.regionId, false);
        const item = pastedPage.regions.find(item => item.id === result.selection.regionId);
        if (item) reveal(item.rect, false);
      }
      refreshLayers();
    } catch (error) { if (ownsSelection()) setElementError(error instanceof Error ? error.message : String(error)); }
    finally { setLayerBusy(false); }
  }
  async function arrangeLayer(direction: LayerDirection) {
    if (layerBusy || editingElement || nudging) return;
    setLayerBusy(true); setElementError('');
    try {
      await finishLayerDraft();
      const region = pageRef.current.regions.find(item => item.id === selected && !item.completed);
      if (region) await arrangeRequest(region.id, direction);
      else if (elementRef.current) {
        const styles = site.current!.layerStyles(elementRef.current.element, direction);
        if (styles) await applyElement({ kind: 'layerStyles', styles });
      } else throw new Error('Select a layer to arrange.');
      refreshLayers();
    } catch (error) { setElementError(error instanceof Error ? error.message : String(error)); }
    finally { setLayerBusy(false); }
  }
  function askAI() { const current = elementRef.current; if (current && !editingElement && !nudging) { setContext(null); pickElement(current); } }
  useImperativeHandle(ref, () => ({ addRequest: addRequestInView, focusRequest: focusRegion, askAI, copyLayer, cutLayer, pasteLayer, arrangeLayer, async synchronize(requireBounds = false) {
    if (inlineSaving.current) await inlineSaving.current;
    const text = inlineTextRef.current;
    if (text) { const node = stage.current?.querySelector<HTMLElement>('.inline-text-editor'); await finishText(node?.innerText ?? text.text); }
    while (nudge.current.promise) await nudge.current.promise;
    if (elementDrag.current) cancel();
    try { await synchronizeBounds(); } catch (error) { setElementError(String(error)); if (requireBounds) throw error; }
  } }));
  function beginWorkspace(event: PointerEvent) {
    if (event.button === 2) { contextMenu(event); return; }
    if (event.target !== event.currentTarget || event.button !== 0) return;
    if (panKey.current || tool === 'pan') { startPan(event, false); return; }
    if (event.shiftKey || tool === 'request') { begin(event, 'new'); return; }
    const at = point({ x: event.clientX, y: event.clientY }); const current = elementRef.current;
    if (!panKey.current && current && current.element.selector !== 'body' && at.x >= current.rect.x && at.x <= current.rect.x + current.rect.width && at.y >= current.rect.y && at.y <= current.rect.y + current.rect.height) beginElementGesture(event);
    else startPan(event, !panKey.current);
  }
  function beginElementGesture(event: PointerEvent, corner?: ResizeCorner) {
    if (event.button !== 0 || !element || editingElement || nudging || gesture.current || pan.current || elementDrag.current) return;
    if (panKey.current || tool === 'pan') { startPan(event, false); return; }
    if (tool === 'request') { begin(event, 'new'); return; }
    if (event.shiftKey && !corner) { begin(event, 'new'); return; }
    event.preventDefault(); event.stopPropagation(); setContext(null);
    const mover = corner ? null : site.current?.beginMove(element.element) ?? null;
    const resizer = corner ? site.current?.beginResize(element.element, corner) ?? null : null;
    if (!mover && !resizer) { setElementError('Choose a container to change this item’s layout.'); return; }
    const client = { x: event.clientX, y: event.clientY };
    elementDrag.current = { pointerId: event.pointerId, pageId: page.id, version: acceptedVersion, start: client, world: point(client), original: element, mover, resizer, corner, active: false, fine: event.altKey, x: 0, y: 0, rect: element.rect };
    lastPointer.current = client; setDragging(true);
    stage.current!.setPointerCapture(event.pointerId); stage.current!.focus({ preventScroll: true });
  }
  function updateElementDrag(client: Point, fine: boolean) {
    const drag = elementDrag.current; if (!drag) return;
    if (!drag.active && Math.hypot(client.x - drag.start.x, client.y - drag.start.y) < 3) return;
    drag.active = true; drag.fine = fine; plainClicks.current = 0;
    const step = gridSteps(page.viewport, gridSettings); const now = point(client);
    const x = now.x - drag.world.x; const y = now.y - drag.world.y;
    try {
      let moved: ElementSelection | null;
      if (drag.resizer && drag.corner) {
        drag.rect = elementResizeRect(drag.original.rect, { x, y }, drag.corner, step, gridSettings.snap && !fine);
        moved = drag.resizer.preview(drag.rect);
      } else {
        drag.x = gridSettings.snap && !fine ? Math.round((drag.original.rect.x + x) / step.x) * step.x - drag.original.rect.x : x;
        drag.y = gridSettings.snap && !fine ? Math.round((drag.original.rect.y + y) / step.y) * step.y - drag.original.rect.y : y;
        moved = drag.mover!.preview(drag.x, drag.y);
      }
      if (!moved) throw new Error('This item left its editable page.');
      storeElement(moved);
    } catch (error) { cancel(); setElementError(error instanceof Error ? error.message : String(error)); }
  }
  function closeContext(restore = true) { setContext(null); if (restore) stage.current?.focus({ preventScroll: true }); }
  function openElementContext(selection: ElementSelection, client: Point) {
    cancel(); chooseElement(selection); setContext({ kind: 'element', selection, point: client });
  }
  function openRequestContext(regionId: string, client: Point) {
    cancel(); deselectElement(); select(regionId); setContext({ kind: 'request', regionId, point: client });
  }
  function contextMenu(event: ReactMouseEvent) {
    const node = event.target;
    if (!(node instanceof Element) || node.closest('.element-toolbar, .editor-menu, .inline-text-editor')) return;
    // Selected text keeps the operating system's text editing menu.
    if (node instanceof HTMLTextAreaElement && node.selectionStart !== node.selectionEnd) return;
    const client = { x: event.clientX, y: event.clientY }, at = point(client);
    event.preventDefault(); event.stopPropagation();
    const request = node.closest<HTMLElement>('[data-region-id]')?.dataset.regionId;
    if (request && pageRef.current.regions.some(item => item.id === request && !item.completed)) { openRequestContext(request, client); return; }
    const selection = node.closest('[data-corner]') ? elementRef.current : page.content ? site.current?.pick(at.x, at.y) : null;
    if (selection && selection.element.selector !== 'body') openElementContext(selection, client);
    else { cancel(); chooseElement(null); setContext({ kind: 'canvas', at, point: client }); }
  }
  function refreshElement() {
    refreshLayers();
    const current = elementRef.current;
    if (current) {
      const found = site.current?.find(current.element);
      if (found) { pendingSelection.current = null; storeElement(found); }
      else if (!dialogRef.current && (pendingSelection.current !== current.element || (site.current?.source() && site.current.source() !== current.element.sourceSha256))) deselectElement();
    }
    void synchronizeBounds().catch(error => setElementError(String(error)));
  }
  function move(event: PointerEvent) {
    const client = { x: event.clientX, y: event.clientY };
    const drag = elementDrag.current;
    if (drag?.pointerId === event.pointerId) { lastPointer.current = client; updateElementDrag(client, event.altKey); return; }
    const currentPan = pan.current;
    if (currentPan?.pointerId === event.pointerId) {
      if (!currentPan.active) {
        if (Math.hypot(client.x - currentPan.start.x, client.y - currentPan.start.y) < 3) return;
        currentPan.active = true; plainClicks.current = 0; setPanning(true);
      }
      seek({ x: currentPan.origin.x + (currentPan.start.x - client.x) / zoom, y: currentPan.origin.y + (currentPan.start.y - client.y) / zoom });
      return;
    }
    if (!gesture.current) { if (!panReady) setCursor(point(client)); return; }
    if (gesture.current.pointerId !== event.pointerId) return;
    lastPointer.current = client; updateDrag(client);
  }
  function finish(event: PointerEvent) {
    if (elementDrag.current?.pointerId === event.pointerId) {
      updateElementDrag({ x: event.clientX, y: event.clientY }, event.altKey);
      const drag = elementDrag.current; if (!drag) return;
      let css: string | null = null;
      try {
        if (drag.active && (drag.resizer ? !sameRect(drag.rect, drag.original.rect) : drag.x || drag.y)) css = drag.resizer ? drag.resizer.style(drag.rect) : drag.mover!.style(drag.x, drag.y);
      } catch (error) { setElementError(String(error)); }
      cancel(drag.active || !!drag.corner);
      if (css !== null) void applyElement({ kind: 'update', texts: [], attributes: {}, style: css }, drag.original).catch(() => {});
      else if (!drag.active && !drag.corner) {
        finishPlainClick({ x: event.clientX, y: event.clientY }, !event.shiftKey && !event.ctrlKey && !event.metaKey && !event.altKey);
      }
      return;
    }
    if (pan.current?.pointerId === event.pointerId) {
      const current = pan.current;
      if (current.active) seek({ x: current.origin.x + (current.start.x - event.clientX) / zoom, y: current.origin.y + (current.start.y - event.clientY) / zoom });
      cancel(false);
      if (!current.active && current.click) finishPlainClick({ x: event.clientX, y: event.clientY }, !event.shiftKey && !event.ctrlKey && !event.metaKey && !event.altKey);
      else { plainClicks.current = 0; lastPlainClick.current = null; }
      return;
    }
    const current = gesture.current;
    if (!current || current.pointerId !== event.pointerId) return;
    const rect = currentRect({ x: event.clientX, y: event.clientY }, current);
    cancel();
    if (current.kind === 'new') {
      if (current.active && rect) add(requestPlacement(rect, zoom));
      else if (tool === 'request') add(requestAt(current.start, zoom));
      return;
    }
    if (current.region && rect && !sameRect(rect, current.initial)) void change(current.region.id, { rect }).catch(() => {});
  }
  function finishPlainClick(client: Point, plain: boolean) {
    const at = point(client); chooseElement(page.content ? site.current?.pick(at.x, at.y) ?? null : null);
    if (!plain) { plainClicks.current = 0; lastPlainClick.current = null; return; }
    const previous = lastPlainClick.current; const now = performance.now();
    plainClicks.current = Math.min(2, plainClicks.current + 1);
    lastPlainClick.current = { at: now, point: client };
    // WebKit can omit dblclick after the selected-element pointer-capture path.
    // Two completed unmoved clicks still represent the same user gesture.
    if (tool === 'select' && previous && now - previous.at <= 500 && Math.hypot(client.x - previous.point.x, client.y - previous.point.y) <= 4) {
      plainClicks.current = 0; lastPlainClick.current = null; activateAt(client);
    }
  }
  useEffect(() => {
    const reset = () => { lastPlainClick.current = null; plainClicks.current = 0; };
    const outside = (event: globalThis.PointerEvent) => { if (!(event.target instanceof Node) || !stage.current?.contains(event.target)) reset(); };
    reset(); document.addEventListener('pointerdown', outside, true); window.addEventListener('keydown', reset, true);
    return () => { document.removeEventListener('pointerdown', outside, true); window.removeEventListener('keydown', reset, true); };
  }, [tool, page.id]);
  function activateAt(client: Point) {
    const at = point(client);
    if (page.content) { const hit = site.current?.pick(at.x, at.y); if (hit && hit.element.tag !== 'body') { chooseElement(hit); void editTextAt(at); return; } }
    const snapped = geometryForGesture({ kind: 'new', start: at, initial: { ...at, width: 0, height: 0 } }, at, page.viewport, gridSettings);
    add(requestAt({ x: snapped.x, y: snapped.y }, zoom));
  }
  function activate(event: ReactMouseEvent) {
    if (tool !== 'select' || event.target !== event.currentTarget || event.button !== 0 || event.shiftKey || event.ctrlKey || event.metaKey || event.altKey || plainClicks.current < 2) return;
    event.preventDefault(); event.stopPropagation(); plainClicks.current = 0; lastPlainClick.current = null;
    activateAt({ x: event.clientX, y: event.clientY });
  }
  function reveal(rect: Rect, resize: boolean) {
    const node = scroll.current;
    if (!node || previous.current.pageId !== page.id) return;
    const current = boundsRef.current;
    const start = { x: current.left + node.scrollLeft / zoom, y: current.top + node.scrollTop / zoom };
    const target = { x: rect.x + (resize ? rect.width : 0), y: rect.y + (resize ? rect.height : 0) };
    const padding = 48 / zoom;
    const next = { x: target.x < start.x + padding ? target.x - padding : target.x > start.x + node.clientWidth / zoom - padding ? target.x - node.clientWidth / zoom + padding : start.x,
      y: target.y < start.y + padding ? target.y - padding : target.y > start.y + node.clientHeight / zoom - padding ? target.y - node.clientHeight / zoom + padding : start.y };
    if (next.x !== start.x || next.y !== start.y) seek(next);
  }
  function keyboard(event: KeyboardEvent, region?: Region, resize = false) {
    if ((event.ctrlKey || event.metaKey) && event.key !== 'Escape') return;
    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); panKey.current = false; setPanReady(false); if (!gesture.current && !pan.current && !elementDrag.current) { select(null); deselectElement(); } cancel(); return; }
    if (event.key === 'ContextMenu' || event.key === 'F10' && event.shiftKey) {
      event.preventDefault();
      const box = stage.current!.getBoundingClientRect();
      const request = region ?? page.regions.find(item => item.id === selected && !item.completed);
      const rect = request?.rect ?? element?.rect;
      const at = rect ? { x: box.left + (rect.x - bounds.left) * zoom, y: box.top + (rect.y + rect.height - bounds.top) * zoom }
        : { x: measurements.width / 2 + inset.x, y: measurements.height / 2 + inset.y };
      if (request) openRequestContext(request.id, at);
      else if (element) openElementContext(element, at);
      else setContext({ kind: 'canvas', point: at, at: point(at) });
      return;
    }
    if (!region && event.key === 'Enter') {
      event.preventDefault();
      if (page.content && !event.shiftKey) { chooseElement(site.current?.next(event.altKey, elementRef.current?.element) ?? null); return; }
      const node = scroll.current!;
      const at = { x: bounds.left + (node.scrollLeft + node.clientWidth / 2 - 160) / zoom, y: bounds.top + (node.scrollTop + node.clientHeight / 2 - 80) / zoom };
      const snapped = geometryForGesture({ kind: 'new', start: at, initial: { ...at, width: 0, height: 0 } }, at, page.viewport, gridSettings);
      add(requestAt({ x: snapped.x, y: snapped.y }, zoom)); return;
    }
    if (!region && element) {
      if (event.key === 'F2' && !nudge.current.promise && !editingElement) { event.preventDefault(); setElementDialog('edit'); return; }
      if ((event.key === 'Delete' || event.key === 'Backspace') && !nudge.current.promise && !editingElement) { event.preventDefault(); setElementDialog('remove'); return; }
      if (['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(event.key)) {
        event.preventDefault(); const step = event.shiftKey ? 10 : 1;
        nudgeElement(event.key === 'ArrowLeft' ? -step : event.key === 'ArrowRight' ? step : 0, event.key === 'ArrowUp' ? -step : event.key === 'ArrowDown' ? step : 0); return;
      }
    }
    if (!region && event.key === 'Home') { event.preventDefault(); show('guide'); return; }
    if (!region && (event.key === 'PageDown' || event.key === 'PageUp')) { event.preventDefault(); scroll.current!.scrollTop += scroll.current!.clientHeight * (event.key === 'PageDown' ? 0.85 : -0.85); return; }
    if (region && (event.key === 'Delete' || event.key === 'Backspace')) { event.preventDefault(); remove(region.id); return; }
    if (!['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(event.key)) return;
    event.preventDefault(); event.stopPropagation();
    const direction = { x: event.key === 'ArrowLeft' ? -1 : event.key === 'ArrowRight' ? 1 : 0, y: event.key === 'ArrowUp' ? -1 : event.key === 'ArrowDown' ? 1 : 0 };
    if (!region) { scroll.current!.scrollBy(direction.x * 80, direction.y * 80); return; }
    const resizing = event.shiftKey || resize;
    void change(region.id, { rect: keyboardRectChange(direction, page.viewport, gridSettings, resizing, event.altKey, minimum) }).then(updated => reveal(updated.rect, resizing)).catch(() => {});
  }

  useLayoutEffect(() => {
    const node = toolbar.current; if (!node) return;
    const measure = () => { const rect = node.getBoundingClientRect(); if (!rect.width || !rect.height) return; setToolbarSize(current => current.width === rect.width && current.height === rect.height ? current : { width: rect.width, height: rect.height }); };
    measure(); const observer = new ResizeObserver(measure); observer.observe(node); return () => observer.disconnect();
  }, [!!element, !!selected, !!inlineText]);
  const visible = { x: bounds.left + measurements.left / zoom, y: bounds.top + measurements.top / zoom };
  const selectedRect = preview ?? element?.rect ?? page.regions.find(region => region.id === selected)?.rect;
  const aligningRegion = gesture.current?.region?.id ?? selected;
  const guides = alignmentLines(selectedRect, page.viewport, page.regions.filter(region => region.id !== aligningRegion).map(region => region.rect), zoom);
  const gridStep = gridSteps(page.viewport, gridSettings);
  const displayStep = { x: gridStep.x * Math.max(1, Math.ceil(6 / (gridStep.x * zoom))), y: gridStep.y * Math.max(1, Math.ceil(6 / (gridStep.y * zoom))) };
  const coordinate = selectedRect ?? cursor;
  const overElement = !!element && element.element.selector !== 'body' && !!cursor && cursor.x >= element.rect.x && cursor.x <= element.rect.x + element.rect.width && cursor.y >= element.rect.y && cursor.y <= element.rect.y + element.rect.height;
  const inset = { x: scroll.current?.offsetLeft ?? 0, y: scroll.current?.offsetTop ?? 0 };
  const occupied = useMemo(() => element ? site.current?.occupied(element.element) ?? [] : [], [element, acceptedVersion]);
  const inViewport = (rect: Rect): Rect => ({ x: (rect.x - bounds.left) * zoom - measurements.left + inset.x, y: (rect.y - bounds.top) * zoom - measurements.top + inset.y, width: rect.width * zoom, height: rect.height * zoom });
  const toolbarRect = element?.rect ?? page.regions.find(item => item.id === selected && !item.completed)?.rect;
  const toolbarPoint = toolbarRect ? elementToolbarPosition(inViewport(toolbarRect), { x: toolbarSize.width, y: toolbarSize.height }, { x: inset.x, y: inset.y, width: measurements.width, height: measurements.height }, occupied.map(inViewport)) : null;
  const toolbarPosition = toolbarPoint ? { left: toolbarPoint.x, top: toolbarPoint.y } : {};
  const measurementRect = coordinate ? inViewport(selectedRect ?? { ...coordinate, width: 0, height: 0 }) : null;
  const px = (value: number) => `${Math.round(value * 10) / 10}`;

  const editBusy = layerBusy || editingElement || nudging;
  const arrangeItems: MenuItem[] = (['front', 'forward', 'backward', 'back'] as const).map((direction, index) => ({ id: direction, label: ['Bring to front', 'Bring forward', 'Send backward', 'Send to back'][index], disabled: editBusy, action: () => { void arrangeLayer(direction); } }));
  const historyItems: MenuItem[] = [{ id: 'history-divider', label: '', separator: true }, { id: 'undo', label: 'Undo', disabled: !canUndo || editBusy, shortcut: 'Ctrl/Cmd+Z', action: undo }, { id: 'redo', label: 'Redo', disabled: !canRedo || editBusy, shortcut: 'Ctrl/Cmd+Shift+Z', action: redo }];
  const contextItems: MenuItem[] = context ? [
    ...(context.kind === 'element' ? [
      { id: 'edit', label: 'Edit', disabled: editBusy, action: () => setElementDialog('edit') },
      { id: 'generate', label: 'Ask AI', disabled: editBusy, action: askAI },
      { id: 'replace', label: 'Replace', disabled: editBusy, action: () => setElementDialog('replace') },
      { id: 'position', label: 'Move or resize', disabled: editBusy, children: [{ id: 'move', label: 'Move', action: () => setElementDialog('position') }, { id: 'resize', label: 'Resize', action: () => setElementDialog('edit') }] },
    ] : context.kind === 'request' ? [{ id: 'edit', label: 'Edit request', disabled: editBusy, action: () => focusRegion(context.regionId) }] : [{ id: 'add', label: 'Add request here', action: () => { deselectElement(); add(requestAt(context.at, zoom)); } }]),
    ...(context.kind !== 'canvas' ? [
      { id: 'copy', label: 'Copy', disabled: editBusy, shortcut: 'Ctrl/Cmd+C', action: () => { void copyLayer(); } },
      { id: 'cut', label: 'Cut', disabled: editBusy, shortcut: 'Ctrl/Cmd+X', action: () => { void cutLayer(); } },
    ] : []),
    { id: 'paste', label: 'Paste', disabled: editBusy, shortcut: 'Ctrl/Cmd+V', action: () => { void pasteLayer(); } },
    ...(context.kind !== 'canvas' ? [
      { id: 'arrange', label: 'Arrange', disabled: editBusy, children: arrangeItems },
      ...(context.kind === 'element' ? [{ id: 'parent', label: 'Select parent', action: () => chooseElement(site.current?.next(true, elementRef.current?.element) ?? null) }] : []),
      { id: 'delete', label: 'Delete', disabled: editBusy, action: () => { if (context.kind === 'element') setElementDialog('remove'); else if (context.kind === 'request') remove(context.regionId); } },
    ] : [
      { id: 'fit', label: 'Fit screen guide', action: () => show('fit') },
      { id: 'center', label: 'Center screen guide', action: () => show('guide') },
      { id: 'grid', label: 'Show grid', checked: grid, action: toggleGrid },
      { id: 'grid-settings', label: 'Grid and rulers', action: configureGrid },
    ]),
    { id: 'layers', label: 'Layers', action: () => setLayersOpen(true) },
    ...historyItems,
  ] : [];
  return <div className={`free-workspace ${page.appearance} tool-${tool} ${gridSettings.rulers ? 'with-rulers' : ''} ${panReady ? 'pan-ready' : ''} ${panning ? 'panning' : ''}`}
    onKeyDownCapture={event => { if (event.code === 'Space' && !isTextInput(event.target) && !(event.target instanceof HTMLElement && event.target.closest('button, a, [role=button], .element-toolbar, .editor-menu')) && !event.ctrlKey && !event.metaKey && !event.altKey) { event.preventDefault(); panKey.current = true; setPanReady(true); } }}>
    <p className="sr-only" id="canvas-keyboard-help">Click a website element to select it. Drag the selected element to move it; drag a corner to resize it. Double-click text to edit it in place. Enter saves text; Escape cancels. Ask AI in the right-click menu, or Ctrl or Command K, opens a change request. Double-click empty canvas to add a request. Select, Add request and Pan tools are in the floating canvas toolbar, with V, R and H shortcuts. In Add request, click to place a text box or drag to draw an area. Left-drag on empty space pans; Space or middle drag pans anywhere. Shift+drag draws an area. Ctrl or Command with the wheel zooms under the pointer. F2 opens local element settings. Arrow keys move an element one pixel; Shift moves ten. Enter selects the next element; Alt+Enter selects its parent. Shift+Enter adds a request in view. Escape from a request focuses its move grip. Arrow keys on grips move or resize by the grid setting. Hold Alt for one-pixel changes. Delete on a grip removes its request. Home returns to the screen guide. Escape cancels a drag.</p>
    {gridSettings.rulers && <><div className="ruler-corner" aria-hidden="true">{rulerUnit === 'percent' ? '%' : 'px'}</div>
      <Ruler axis="x" unit={rulerUnit} start={visible.x} length={measurements.width} reference={page.viewport.width} zoom={zoom} cursor={cursor?.x} selection={selectedRect ? [selectedRect.x, selectedRect.x + selectedRect.width] : undefined} />
      <Ruler axis="y" unit={rulerUnit} start={visible.y} length={measurements.height} reference={page.viewport.height} zoom={zoom} cursor={cursor?.y} selection={selectedRect ? [selectedRect.y, selectedRect.y + selectedRect.height] : undefined} /></>}
    <div ref={scroll} className="workspace-scroll" data-testid="workspace-scroll" onScroll={scrolled} onPointerDownCapture={beginPan}>
      <div className="canvas-size" style={{ width: (bounds.right - bounds.left) * zoom, height: (bounds.bottom - bounds.top) * zoom }}>
        <div id="page-canvas" ref={stage} data-testid="canvas" data-world-left={bounds.left} data-world-top={bounds.top} data-zoom={zoom} className={`canvas ${page.appearance} ${overElement ? 'element-hover' : ''} ${elementDrag.current ? 'element-transforming' : ''}`} tabIndex={0} role="group" aria-label="Page workspace. Click to select, double-click text to edit. Double-click empty canvas to add a request. Drag selected elements to move; drag corners to resize." aria-describedby="canvas-keyboard-help"
          style={{ width: (bounds.right - bounds.left) * zoom, height: (bounds.bottom - bounds.top) * zoom }}
          onPointerDown={beginWorkspace} onPointerMove={move} onPointerUp={finish} onDoubleClick={activate} onContextMenu={contextMenu}
          onPointerLeave={() => { if (!gesture.current) setCursor(null); }} onPointerCancel={event => { if (gesture.current?.pointerId === event.pointerId || pan.current?.pointerId === event.pointerId || elementDrag.current?.pointerId === event.pointerId) cancel(); }}
          onLostPointerCapture={event => { if (gesture.current?.pointerId === event.pointerId || pan.current?.pointerId === event.pointerId || elementDrag.current?.pointerId === event.pointerId) cancel(); }}
          onKeyDown={event => { if (event.target === event.currentTarget || event.key === 'Escape') keyboard(event); }}>
          {page.content && <div className="accepted-canvas-preview" data-testid="accepted-canvas-preview" style={positioned({ x: 0, y: 0, width: page.viewport.width, height: previewHeight })} aria-hidden="true">
            <div className="accepted-canvas-content" style={{ width: page.viewport.width, height: previewHeight, transform: `scale(${zoom})` }}>
              <PreviewFrame ref={site} content={page.content} referenceHeight={page.viewport.height} onHeight={setSiteHeight} onReady={refreshElement} pageId={page.id} revision={acceptedVersion} title={`${page.name} accepted static preview`} staticView />
            </div>
          </div>}
          <div className={`screen-guide ${page.content ? 'has-preview' : ''}`} data-testid="screen-guide" style={positioned({ x: 0, y: 0, ...page.viewport })} aria-hidden="true">
            <span className="screen-guide-label">Screen · {page.viewport.width} × {page.viewport.height}</span>
          </div>
          {grid && <div className="canvas-grid" aria-hidden="true" style={{ backgroundSize: `${displayStep.x * zoom}px ${displayStep.y * zoom}px`, backgroundPosition: `${(-bounds.left % displayStep.x) * zoom}px ${(-bounds.top % displayStep.y) * zoom}px` }} />}
          {!page.content && !page.regions.some(region => !region.completed) && <div className="canvas-empty-hint" style={positioned({ x: 0, y: 0, ...page.viewport })}>
            <p>Double-click anywhere to describe what goes here.</p><button type="button" onClick={addRequestInView}>Add request</button>
          </div>}
          {page.regions.map((region, index) => {
            if (region.completed) return null;
            const rect = gesture.current?.region?.id === region.id && preview ? preview : region.rect;
            return <RequestBox key={region.id} region={region} number={index + 1} rect={rect} bounds={bounds} zoom={zoom} maxHeight={Math.max(160, measurements.height * 0.55)} selected={selected === region.id} focus={focusRequest === region.id} focusToken={requestFocus.id === region.id ? requestFocus.serial : 0}
              layer={requestRanks.get(region.id)!} controlsLayer={overlayLayer + 1} select={() => { deselectElement(); select(region.id); }} begin={begin} keyboard={keyboard} change={patch => change(region.id, patch)} />;
          })}
          {element && <div className="element-selection direct-selection" data-testid="direct-element-selection" data-selector={element.element.selector} data-source-hash={element.element.sourceSha256} style={{ ...paintStyle(element.rect, bounds, zoom), zIndex: overlayLayer + 1 }} role="group" aria-label="Selected element bounds">
            <span className="element-type-tag">{elementName(element.element.tag)}</span>
            {element.element.selector !== 'body' && (['nw','ne','sw','se'] as const).map(corner => <button key={corner} type="button" className={`element-resize-corner corner-${corner}`} data-corner={corner} aria-label={`Resize ${corner.includes('n') ? 'top' : 'bottom'}-${corner.includes('w') ? 'left' : 'right'}`} title="Drag to resize; press Enter for size settings" disabled={editingElement || nudging} onPointerDown={event => beginElementGesture(event, corner)} onClick={event => { if (event.detail === 0) setElementDialog('edit'); }} />)}
          </div>}
          {inlineText && <InlineTextEditor key={`${inlineText.selection.element.sourceSha256}:${inlineText.index}`} text={inlineText} origin={origin} zoom={zoom} layer={overlayLayer + 4} disabled={editingElement} finish={finishText} />}
          {changing.map((rect, index) => <div key={index} className="generation-blur" data-testid="generation-blur" style={{ ...paintStyle(rect, bounds, zoom), zIndex: overlayLayer + 2 }} aria-hidden="true" />)}
          {highlighted.map((rect, index) => <div key={index} className="generation-highlight" data-testid="generation-highlight" style={{ ...paintStyle(rect, bounds, zoom), zIndex: overlayLayer + 2 }} aria-hidden="true" />)}
          {guides.x.map(x => <div key={`x${x}`} className="alignment-guide alignment-x" aria-hidden="true" style={{ left: (x - bounds.left) * zoom, zIndex: overlayLayer + 1 }} />)}
          {guides.y.map(y => <div key={`y${y}`} className="alignment-guide alignment-y" aria-hidden="true" style={{ top: (y - bounds.top) * zoom, zIndex: overlayLayer + 1 }} />)}
          {preview && gesture.current?.kind === 'new' && <div className="annotation-draft" style={{ ...paintStyle(preview, bounds, zoom), zIndex: overlayLayer }} />}
        </div>
      </div>
    </div>
    {toolbarRect && !inlineText && <div ref={toolbar} className="element-toolbar" role="toolbar" aria-label={element ? 'Selected element' : 'Selected request'} style={toolbarPosition}>
      <button data-layers-trigger aria-label="Layers" title="Layers" aria-haspopup="dialog" aria-expanded={layersOpen} onClick={() => setLayersOpen(!layersOpen)}><LayerIcon /></button>
      <button aria-label="Copy layer" title="Copy · Ctrl/⌘ C" disabled={layerBusy || editingElement || nudging || element?.element.selector === 'body'} onClick={() => { void copyLayer(); }}><QuickIcon name="copy" /></button>
      <button aria-label="Cut layer" title="Cut · Ctrl/⌘ X" disabled={layerBusy || editingElement || nudging || element?.element.selector === 'body'} onClick={() => { void cutLayer(); }}><QuickIcon name="cut" /></button>
      <button aria-label="Delete layer" title="Delete" disabled={layerBusy || editingElement || nudging || element?.element.selector === 'body'} onClick={() => { if (element) setElementDialog('remove'); else if (selected) remove(selected); }}><QuickIcon name="delete" /></button>
    </div>}
    {context && <ContextMenu key={`${context.kind}:${context.point.x}:${context.point.y}`} label={context.kind === 'element' ? 'Element actions' : context.kind === 'request' ? 'Request actions' : 'Canvas actions'} point={context.point} close={closeContext} items={contextItems} />}
    <LayersPopover open={layersOpen} page={page} siteLayers={siteLayers} selectedElement={element?.element.selector ?? null} selectedRequest={selected} busy={layerBusy || editingElement || nudging} ready={layersReady} anchor={toolbarPoint ? { x: toolbarPoint.x, y: toolbarPoint.y, height: toolbarSize.height } : null} contextElement={openElementContext} contextRequest={openRequestContext}
      chooseElement={item => { chooseElement(item); reveal(item.rect, false); }} chooseRequest={item => { deselectElement(); select(item.id); reveal(item.rect, false); }}
      arrange={direction => { void arrangeLayer(direction); }} copy={() => { void copyLayer(); }} paste={() => { void pasteLayer(); }} close={() => setLayersOpen(false)} />
    {elementError && <div className="element-edit-error" role="alert">{elementError}<button onClick={() => setElementError('')}>Dismiss</button></div>}
    {elementDialog && element && <ElementDialog mode={elementDialog} target={element.element} visual={site.current?.visual(element.element) ?? null} inspect={inspectElement} apply={change => applyElement(change)} move={moveElement} close={() => { setElementDialog(null); if (elementRef.current && !site.current?.find(elementRef.current.element) && !pendingSelection.current) deselectElement(); }} />}
    {selectedRect && coordinate && measurementRect && gridSettings.rulers && <div className="workspace-measurement" aria-label="Placement in pixels" title={`${percent(coordinate.x, page.viewport.width)} · ${percent(coordinate.y, page.viewport.height)} · ${percent(selectedRect.width, page.viewport.width)} × ${percent(selectedRect.height, page.viewport.height)}`}
      style={(() => { const position = elementToolbarPosition(measurementRect, { x: 245, y: 28 }, { x: inset.x, y: inset.y, width: measurements.width, height: measurements.height }, [...occupied.map(inViewport), ...(toolbarPoint ? [{ x: toolbarPoint.x, y: toolbarPoint.y, width: toolbarSize.width, height: toolbarSize.height }] : [])]); return { left: position.x, top: position.y }; })()}>
      {px(coordinate.x)}, {px(coordinate.y)} px{selectedRect && <span>{px(selectedRect.width)} × {px(selectedRect.height)} px</span>}</div>}
  </div>;
});

function Ruler({ axis, unit, start, length, reference, zoom, cursor, selection }: { axis: 'x' | 'y'; unit: 'pixels' | 'percent'; start: number; length: number; reference: number; zoom: number; cursor?: number; selection?: [number, number] }) {
  const offset = (value: number) => (value - start) * zoom;
  const style = (position: number, size?: number): CSSProperties => axis === 'x' ? { left: position, width: size } : { top: position, height: size };
  const band = (from: number, to: number) => { const first = clamp(offset(from), 0, length); return style(first, Math.max(0, clamp(offset(to), 0, length) - first)); };
  return <div className={`workspace-ruler ruler-${axis}`} aria-hidden="true">
    <div className="ruler-screen-band" style={band(0, reference)} />
    {selection && <div className="ruler-selection-band" style={band(selection[0], selection[1])} />}
    {(unit === 'percent' ? percentageTicks(start, length, reference, zoom) : pixelTicks(start, length, zoom)).map(tick => <span key={tick.value} className="ruler-tick" style={style(tick.position)}><span className="ruler-label">{unit === 'percent' ? percent(tick.value, reference) : tick.value}</span></span>)}
    {cursor !== undefined && offset(cursor) >= 0 && offset(cursor) <= length && <span className="ruler-cursor" style={style(offset(cursor))} />}
  </div>;
}

function RequestBox({ region, number, rect, bounds, zoom, maxHeight, selected, focus, focusToken, layer, controlsLayer, select, begin, keyboard, change }: {
  region: Region; number: number; rect: Rect; bounds: WorkspaceBounds; zoom: number; maxHeight: number; selected: boolean; focus: boolean; focusToken: number; layer: number; controlsLayer: number; select: () => void;
  begin: (event: PointerEvent, kind: Gesture['kind'], region?: Region) => void;
  keyboard: (event: KeyboardEvent, region?: Region, resize?: boolean) => void; change: (patch: RequestPatch) => Promise<Region>;
}) {
  const text = useRef<HTMLTextAreaElement>(null);
  const move = useRef<HTMLButtonElement>(null);
  const [draft, changeDraft] = useDraftText(region.prompt, value => change({ prompt: value }).then(updated => updated.prompt));
  const [expanded, setExpanded] = useState(false);
  const [naturalHeight, setNaturalHeight] = useState(0);
  const focusedToken = useRef(0);
  useLayoutEffect(() => {
    const requested = focus || focusToken !== focusedToken.current && focusToken > 0; focusedToken.current = focusToken;
    if (!requested) return;
    text.current?.focus({ preventScroll: true });
  }, [focus, focusToken]);
  const target = !!region.element;
  const promptRect = target ? { x: rect.x, y: rect.y + rect.height + 10 / zoom, width: Math.max(260 / zoom, Math.min(rect.width, 420 / zoom)), height: 112 / zoom } : rect;
  useLayoutEffect(() => {
    const node = text.current; if (!node) return;
    const old = node.style.height; node.style.height = '0px'; const style = getComputedStyle(node);
    const height = node.scrollHeight + (Number.parseFloat(style.borderTopWidth) || 0) + (Number.parseFloat(style.borderBottomWidth) || 0); node.style.height = old;
    setNaturalHeight(current => current === height ? current : height);
  }, [draft, promptRect.width, zoom]);
  const minimumHeight = promptRect.height * zoom;
  const collapseHeight = Math.max(minimumHeight, maxHeight);
  const fullHeight = Math.max(minimumHeight, naturalHeight);
  const hasMore = fullHeight > collapseHeight;
  useLayoutEffect(() => { if (hasMore && document.activeElement === text.current) setExpanded(true); }, [hasMore, draft]);
  const displayHeight = expanded ? fullHeight : Math.min(collapseHeight, fullHeight);
  const shown = { ...promptRect, height: (displayHeight + (hasMore ? 30 : 0)) / zoom };
  // Bound the painted area while keeping the text and grips anchored to their
  // original world positions. Expanding text never changes placement intent.
  const painted = clipToWorkspace(shown, bounds);
  const at = (x: number, y: number) => x >= bounds.left && x <= bounds.right && y >= bounds.top && y <= bounds.bottom;
  const moveVisible = at(rect.x, rect.y);
  const resizeVisible = at(rect.x + rect.width, rect.y + rect.height);
  return <>
  {selected && !target && <div className="element-selection request-placement" style={{ ...paintStyle(rect, bounds, zoom), zIndex: controlsLayer }} aria-hidden="true" />}
  {target && <div className="element-selection" data-testid="element-selection" style={{ ...paintStyle(rect, bounds, zoom), zIndex: layer }} aria-hidden="true" />}
  <div data-region-id={region.id} className={`annotation ${target ? 'element-prompt' : ''} ${selected ? 'selected' : ''} ${expanded ? 'expanded' : ''}`} style={{ ...paintStyle(shown, bounds, zoom), zIndex: layer, background: 'var(--annotation-surface)' }}
    onPointerDown={event => { if (event.target === event.currentTarget) { event.preventDefault(); event.stopPropagation(); text.current?.focus({ preventScroll: true }); } }}
    onFocusCapture={select}>
    <div className="request-frame" aria-hidden="true" />
    <textarea ref={text} aria-label={`Request ${number}`} aria-describedby="canvas-keyboard-help" maxLength={16384} value={draft} placeholder={target ? "Describe the change…" : "Describe what belongs here…"}
      style={{ position: 'absolute', left: (shown.x - (painted?.x ?? shown.x)) * zoom, top: (shown.y - (painted?.y ?? shown.y)) * zoom, width: shown.width * zoom, height: displayHeight, borderWidth: 0, background: 'transparent', outline: 'none' }} onFocus={() => { if (hasMore) setExpanded(true); }} onChange={event => { changeDraft(event.target.value); if (hasMore) setExpanded(true); }} onPointerDown={event => event.stopPropagation()}
      onKeyDown={event => { if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); (moveVisible ? move.current : document.getElementById('page-canvas'))?.focus({ preventScroll: true }); } }} />
    {hasMore && <button type="button" className="request-expand" style={{ position: 'absolute', left: (shown.x - (painted?.x ?? shown.x)) * zoom, top: (shown.y - (painted?.y ?? shown.y)) * zoom + displayHeight, width: shown.width * zoom }} aria-expanded={expanded} onClick={() => setExpanded(value => !value)}>{expanded ? 'Less' : '… More'}</button>}
  </div>
    {selected && !target && <div className="request-controls" data-region-id={region.id} style={{ ...paintStyle(shown, bounds, zoom), zIndex: controlsLayer }} onFocusCapture={select}><button ref={move} className="annotation-grip move-grip" aria-label={`Move request ${number}`} aria-describedby="canvas-keyboard-help" title="Drag to move. Arrow keys change placement."
      style={{ visibility: moveVisible ? undefined : 'hidden', left: (rect.x - (painted?.x ?? rect.x)) * zoom - 28, top: (rect.y - (painted?.y ?? rect.y)) * zoom }}
      onPointerDown={event => begin(event, 'move', region)} onKeyDown={event => keyboard(event, region)}><svg viewBox="0 0 16 16" aria-hidden="true"><circle cx="5" cy="4" r="1"/><circle cx="11" cy="4" r="1"/><circle cx="5" cy="8" r="1"/><circle cx="11" cy="8" r="1"/><circle cx="5" cy="12" r="1"/><circle cx="11" cy="12" r="1"/></svg></button>
      <button className="annotation-grip resize-grip" aria-label={`Resize request ${number}`} aria-describedby="canvas-keyboard-help" title="Drag or use arrow keys to resize the placement hint."
        style={{ visibility: resizeVisible ? undefined : 'hidden', right: 'auto', bottom: 'auto', left: (rect.x + rect.width - (painted?.x ?? rect.x)) * zoom, top: (rect.y + rect.height - (painted?.y ?? rect.y)) * zoom - 28 }}
        onPointerDown={event => begin(event, 'resize', region)} onKeyDown={event => keyboard(event, region, true)}><svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4 12L12 4M8 12H12V8M4 8V4H8" fill="none" stroke="currentColor" strokeWidth="1.5"/></svg></button></div>}
  </>;
}

function InlineTextEditor({ text, origin, zoom, layer, disabled, finish }: { text: InlineText; origin: Point; zoom: number; layer: number; disabled: boolean; finish: (value: string | null, restoreFocus?: boolean) => Promise<void> }) {
  const editor = useRef<HTMLDivElement>(null);
  const done = useRef(false);
  function complete(value: string | null, restoreFocus = false) { if (!done.current) { done.current = true; void finish(value, restoreFocus).catch(() => { done.current = false; editor.current?.focus({ preventScroll: true }); }); } }
  useLayoutEffect(() => {
    const node = editor.current; if (!node) return;
    node.textContent = text.text; node.focus({ preventScroll: true });
    const range = document.createRange(); range.selectNodeContents(node);
    const selection = window.getSelection(); selection?.removeAllRanges(); selection?.addRange(range);
  }, []);
  return <div ref={editor} className="inline-text-editor" role="textbox" aria-label="Edit text in place" aria-multiline="true" aria-busy={disabled} contentEditable={!disabled} suppressContentEditableWarning spellCheck
    style={{ ...text.style, position: 'absolute', left: (text.rect.x - origin.x) * zoom, top: (text.rect.y - origin.y) * zoom, width: Math.max(24 / zoom, text.rect.width), minHeight: text.rect.height, transform: `scale(${zoom})`, transformOrigin: 'top left', zIndex: layer }}
    onPointerDown={event => event.stopPropagation()} onDoubleClick={event => event.stopPropagation()} onContextMenu={event => event.stopPropagation()}
    onBlur={() => complete(editor.current?.innerText ?? text.text)}
    onKeyDown={event => {
      if (!event.ctrlKey && !event.metaKey) event.stopPropagation();
      if (event.key === 'Escape') { event.preventDefault(); complete(null, true); }
      if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) { event.preventDefault(); complete(editor.current?.innerText ?? text.text, true); }
    }}
    onPaste={event => {
      event.preventDefault(); const value = event.clipboardData.getData('text/plain');
      const selection = window.getSelection(); if (!selection?.rangeCount || !editor.current?.contains(selection.anchorNode)) return;
      const range = selection.getRangeAt(0); range.deleteContents(); const node = document.createTextNode(value); range.insertNode(node); range.setStartAfter(node); range.collapse(true); selection.removeAllRanges(); selection.addRange(range);
    }} />;
}

function elementName(tag: string): string {
  return ({ a: 'Link', button: 'Button', img: 'Image', input: 'Input', textarea: 'Text area', select: 'Dropdown', h1: 'Heading', h2: 'Heading', h3: 'Heading', h4: 'Heading', h5: 'Heading', h6: 'Heading', p: 'Paragraph', span: 'Text', div: 'Container', section: 'Section', nav: 'Navigation', header: 'Header', footer: 'Footer', main: 'Main', body: 'Page', ul: 'List', ol: 'List', li: 'List item', form: 'Form' } as Record<string, string>)[tag] ?? tag.charAt(0).toUpperCase() + tag.slice(1);
}

function QuickIcon({ name }: { name: 'copy' | 'cut' | 'delete' }) {
  return <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    {name === 'copy' ? <path d="M7 7H17V17H7ZM3 13V3H13" /> : name === 'cut' ? <><circle cx="5" cy="5" r="2.5"/><circle cx="5" cy="15" r="2.5"/><path d="M7 6.5L17 17M7 13.5L17 3M10 10L7 7"/></> : <><path d="M3 5H17M7 5V2H13V5M5 5L6 18H14L15 5M8 8V15M12 8V15"/></>}
  </svg>;
}
