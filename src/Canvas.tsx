import { forwardRef, useImperativeHandle, useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type PointerEvent, type KeyboardEvent, type MouseEvent as ReactMouseEvent } from 'react';
import { useDraftText } from './DraftText';
import { ElementDialog, type ElementDialogMode } from './ElementTools';
import { ContextMenu, MenuButton } from './Menus';
import { PreviewFrame, type CanvasPreviewHandle, type ElementMover, type ElementResizer, type InlineText } from './Preview';
import { alignmentLines, elementResizeRect, elementToolbarPosition, clamp, clientToCanvas, DEFAULT_GRID, expandWorkspace, geometryForGesture, gridSteps, initialWorkspaceBounds, keyboardRectChange, percent, percentageTicks, pixelTicks, requestAt, requestPlacement, sameBounds, sameRect, WORLD_MAX, WORLD_MIN, MIN_ZOOM, wheelZoom, zoomOrigin, type CanvasGridSettings, type GeometryGesture, type Point, type RectChange, type WorkspaceBounds, type ResizeCorner } from './canvasGeometry';
import type { ElementBounds, ElementDetails, ElementEdit, ElementSelection, ElementTarget, Page, Rect, Region, Viewport } from './types';
import './canvas.css';

export type RequestPatch = { prompt?: string; rect?: RectChange | Partial<Rect> };
export type CanvasViewAction = { serial: number; kind: 'guide' | 'selection' | 'fit' };
type Gesture = GeometryGesture & { pointerId: number; region?: Region; active: boolean };
type Measurements = { left: number; top: number; width: number; height: number };
const emptyMeasurements: Measurements = { left: 0, top: 0, width: 0, height: 0 };
const isTextInput = (target: EventTarget | null) => target instanceof HTMLElement && (target.matches('input, textarea, select') || target.isContentEditable);

function viewStyle(rect: Rect, origin: Point, zoom: number): CSSProperties {
  return { left: (rect.x - origin.x) * zoom, top: (rect.y - origin.y) * zoom, width: rect.width * zoom, height: rect.height * zoom };
}

type CanvasProps = {
  page: Page; grid: boolean; gridSettings?: CanvasGridSettings; tool?: 'select' | 'request' | 'pan'; rulerUnit?: 'pixels' | 'percent'; zoom: number; selected: string | null; focusRequest: string | null; viewAction?: CanvasViewAction; changing?: Rect[]; highlighted?: Rect[];
  refreshElementBounds: (pageId: string, viewport: Viewport, bounds: ElementBounds[]) => Promise<void>;
  inspectElement: (target: ElementTarget) => Promise<ElementDetails>; editElement: (target: ElementTarget, change: ElementEdit) => Promise<ElementTarget | null>;
  changeZoom: (zoom: number) => void; select: (id: string | null) => void; add: (rect: Rect) => void; pickElement: (selection: ElementSelection) => void; change: (id: string, patch: RequestPatch) => Promise<Region>; remove: (id: string) => void;
};
export type CanvasHandle = { synchronize: (requireBounds?: boolean) => Promise<void>; addRequest: () => void; focusRequest: (id: string) => void; askAI: () => void };
export const Canvas = forwardRef<CanvasHandle, CanvasProps>(function Canvas({ page, grid, gridSettings = DEFAULT_GRID, tool = 'select', rulerUnit = 'pixels', zoom, selected, focusRequest, viewAction, changing = [], highlighted = [], inspectElement, editElement, refreshElementBounds, changeZoom, select, add, pickElement, change, remove }, ref) {
  const toolbar = useRef<HTMLDivElement>(null);
  const [toolbarSize, setToolbarSize] = useState({ width: 160, height: 40 });
  const [context, setContext] = useState<{ selection: ElementSelection; point: Point } | null>(null);
  const scroll = useRef<HTMLDivElement>(null);
  const site = useRef<CanvasPreviewHandle>(null);
  const [siteHeight, setSiteHeight] = useState(page.viewport.height);
  useEffect(() => setSiteHeight(page.viewport.height), [page.id, page.content?.html, page.content?.css, page.content?.javascript, page.viewport.width, page.viewport.height]);
  const stage = useRef<HTMLDivElement>(null);
  const gesture = useRef<Gesture | null>(null);
  const pan = useRef<{ pointerId: number; last: Point; start: Point; click: boolean; active: boolean } | null>(null);
  const plainClicks = useRef(0);
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
  function storeElement(value: ElementSelection | null) { elementRef.current = value; setElement(value); }
  function deselectElement() { setContext(null); inlineTicket.current++; selectionSerial.current++; nudge.current.x = 0; nudge.current.y = 0; pendingSelection.current = null; storeElement(null); }
  useLayoutEffect(() => { deselectElement(); setElementDialog(null); setElementError(''); }, [page.id]);
  const zoomIntent = useRef<{ pageId: string; zoom: number; origin: Point } | null>(null);
  const live = useRef({ zoom, changeZoom, pageId: page.id });
  live.current = { zoom, changeZoom, pageId: page.id };
  const panKey = useRef(false);
  const lastPointer = useRef<Point | null>(null);
  const savedViews = useRef(new Map<string, { bounds: WorkspaceBounds; center: Point }>());
  const [workspace, setWorkspace] = useState(() => ({ pageId: page.id, bounds: initialWorkspaceBounds(page.viewport, page.regions.map(region => region.rect), { x: 1200 / zoom, y: 800 / zoom }) }));
  const bounds = workspace.bounds;
  const origin = { x: bounds.left, y: bounds.top };
  const boundsRef = useRef(bounds);
  boundsRef.current = bounds;
  const previous = useRef({ pageId: '', zoom, bounds });
  const pendingOrigin = useRef<{ origin: Point; bounds: WorkspaceBounds; pageId: string; zoom: number } | null>(null);
  const measurementsRef = useRef(emptyMeasurements);
  const [measurements, setMeasurements] = useState(emptyMeasurements);
  const [preview, setPreview] = useState<Rect | null>(null);
  const [cursor, setCursor] = useState<Point | null>(null);
  const [focused, setFocused] = useState<string | null>(null);
  const [panReady, setPanReady] = useState(false);
  const [panning, setPanning] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [atEdge, setAtEdge] = useState(false);
  const overlayLayer = Math.max(0, ...page.regions.map(region => region.zIndex)) + 3;
  const minimum = { x: 160 / zoom, y: 80 / zoom };
  const previewHeight = Math.min(WORLD_MAX, Math.max(page.viewport.height, siteHeight, ...page.regions.map(region => region.rect.y + region.rect.height + 64)));
  const acceptedVersion = useMemo(() => JSON.stringify([page.content, page.viewport, page.appearance]),
    [page.content?.html, page.content?.css, page.content?.javascript, page.viewport.width, page.viewport.height, page.appearance]);
  const acceptedVersionRef = useRef(acceptedVersion); acceptedVersionRef.current = acceptedVersion;
  useLayoutEffect(() => { if (elementDrag.current) cancel(); inlineTicket.current++; inlineTextRef.current = null; setInlineText(null); }, [acceptedVersion]);

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
  function seek(origin: Point, available = boundsRef.current) {
    const node = scroll.current;
    if (!node) return;
    const size = { x: node.clientWidth / zoom, y: node.clientHeight / zoom };
    const start = { x: clamp(origin.x, WORLD_MIN, WORLD_MAX - size.x), y: clamp(origin.y, WORLD_MIN, WORLD_MAX - size.y) };
    const next = expandWorkspace(available, visibleBounds(start, size), { x: Math.max(600, size.x), y: Math.max(600, size.y) });
    pendingOrigin.current = { origin: start, bounds: next, pageId: page.id, zoom };
    setWorkspace({ pageId: page.id, bounds: next });
  }
  function show(kind: CanvasViewAction['kind']) {
    const node = scroll.current;
    if (!node) return;
    if (kind === 'fit') {
      const target = Math.max(MIN_ZOOM, Math.min(4, Math.max(1, node.clientWidth - 96) / page.viewport.width, Math.max(1, node.clientHeight - 96) / page.viewport.height));
      const origin = { x: page.viewport.width / 2 - node.clientWidth / target / 2, y: page.viewport.height / 2 - node.clientHeight / target / 2 };
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
    if (resetClicks) plainClicks.current = 0;
    const pointerId = gesture.current?.pointerId ?? pan.current?.pointerId ?? elementDrag.current?.pointerId;
    if (elementDrag.current) { elementDrag.current.mover?.cancel(); elementDrag.current.resizer?.cancel(); if (elementDrag.current.pageId === pageRef.current.id && elementDrag.current.version === acceptedVersionRef.current) storeElement(elementDrag.current.original); else deselectElement(); elementDrag.current = null; }
    gesture.current = null; pan.current = null; lastPointer.current = null;
    if (pointerId !== undefined && stage.current?.hasPointerCapture(pointerId)) stage.current.releasePointerCapture(pointerId);
    setPreview(null); setDragging(false); setPanning(false); setAtEdge(false);
  }

  useLayoutEffect(() => {
    const node = scroll.current;
    if (!node) return;
    const prior = previous.current;
    const measured = measurementsRef.current;
    if (prior.pageId !== page.id) {
      if (prior.pageId) savedViews.current.set(prior.pageId, { bounds: prior.bounds,
        center: { x: prior.bounds.left + (measured.left + measured.width / 2) / prior.zoom, y: prior.bounds.top + (measured.top + measured.height / 2) / prior.zoom } });
      cancel(); zoomIntent.current = null; setCursor(null); setFocused(null);
      const saved = savedViews.current.get(page.id);
      const content = initialWorkspaceBounds(page.viewport, page.regions.map(region => region.rect), { x: node.clientWidth / zoom, y: node.clientHeight / zoom });
      const available = saved ? { left: Math.min(content.left, saved.bounds.left), top: Math.min(content.top, saved.bounds.top), right: Math.max(content.right, saved.bounds.right), bottom: Math.max(content.bottom, saved.bounds.bottom) } : content;
      previous.current = { pageId: page.id, zoom, bounds: available };
      seek(saved ? { x: saved.center.x - node.clientWidth / zoom / 2, y: saved.center.y - node.clientHeight / zoom / 2 }
        : { x: page.viewport.width / 2 - node.clientWidth / zoom / 2, y: -64 / zoom }, available);
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
  useLayoutEffect(() => {
    if (workspace.pageId !== page.id || pendingOrigin.current) return;
    const content = initialWorkspaceBounds(page.viewport, page.regions.map(region => region.rect), { x: 0, y: 0 });
    const next = { left: Math.min(bounds.left, content.left), top: Math.min(bounds.top, content.top), right: Math.max(bounds.right, content.right), bottom: Math.max(bounds.bottom, content.bottom) };
    if (!sameBounds(bounds, next)) {
      const measured = measurementsRef.current;
      seek({ x: bounds.left + measured.left / zoom, y: bounds.top + measured.top / zoom }, next);
    }
  }, [page.regions, page.viewport.width, page.viewport.height, workspace.pageId]);
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
    measure();
    if (pendingOrigin.current) return;
    const node = scroll.current!;
    const current = boundsRef.current;
    const origin = { x: current.left + node.scrollLeft / zoom, y: current.top + node.scrollTop / zoom };
    const size = { x: node.clientWidth / zoom, y: node.clientHeight / zoom };
    const next = expandWorkspace(current, visibleBounds(origin, size), { x: Math.max(600, size.x), y: Math.max(600, size.y) });
    if (!sameBounds(current, next)) { pendingOrigin.current = { origin, bounds: next, pageId: page.id, zoom }; setWorkspace({ pageId: page.id, bounds: next }); }
  }
  function startPan(event: PointerEvent, click: boolean) {
    if (gesture.current || pan.current || elementDrag.current) return;
    event.preventDefault(); event.stopPropagation();
    if (!click) plainClicks.current = 0;
    const client = { x: event.clientX, y: event.clientY };
    pan.current = { pointerId: event.pointerId, last: client, start: client, click, active: false };
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
    const raw = point({ x: event.clientX, y: event.clientY });
    const start = { x: clamp(raw.x, WORLD_MIN, WORLD_MAX), y: clamp(raw.y, WORLD_MIN, WORLD_MAX) };
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
    setAtEdge(now.x < WORLD_MIN || now.x > WORLD_MAX || now.y < WORLD_MIN || now.y > WORLD_MAX);
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
    seek({ x: region.rect.x + region.rect.width / 2 - node.clientWidth / zoom / 2, y: region.rect.y + region.rect.height / 2 - node.clientHeight / zoom / 2 });
    setRequestFocus(current => ({ id, serial: current.serial + 1 }));
  }
  function makeRoom(rect: Rect) {
    const current = boundsRef.current;
    const next = { ...current, right: Math.min(WORLD_MAX, Math.max(current.right, rect.x + rect.width + 64 / zoom)), bottom: Math.min(WORLD_MAX, Math.max(current.bottom, rect.y + rect.height + 64 / zoom)) };
    if (!sameBounds(current, next)) {
      const node = scroll.current; if (node) seek({ x: current.left + node.scrollLeft / zoom, y: current.top + node.scrollTop / zoom }, next);
    }
  }
  function askAI() { const current = elementRef.current; if (current && !editingElement && !nudging) { setContext(null); pickElement(current); } }
  useImperativeHandle(ref, () => ({ addRequest: addRequestInView, focusRequest: focusRegion, askAI, async synchronize(requireBounds = false) {
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
  function contextMenu(event: ReactMouseEvent) {
    if (event.target instanceof HTMLElement && event.target.closest('.annotation, .element-toolbar, .editor-menu')) return;
    const at = point({ x: event.clientX, y: event.clientY });
    const selection = event.target instanceof Element && event.target.closest('[data-corner]') ? elementRef.current : page.content ? site.current?.pick(at.x, at.y) : null;
    event.preventDefault(); event.stopPropagation(); cancel(); chooseElement(selection ?? null);
    if (!selection) return;
    setContext({ selection, point: { x: event.clientX, y: event.clientY } });
  }
  function refreshElement() {
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
      scroll.current!.scrollBy(currentPan.last.x - client.x, currentPan.last.y - client.y);
      currentPan.last = client; return;
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
        plainClicks.current = Math.min(2, plainClicks.current + 1);
        const at = point({ x: event.clientX, y: event.clientY }); chooseElement(site.current?.pick(at.x, at.y) ?? null);
      }
      return;
    }
    if (pan.current?.pointerId === event.pointerId) {
      const current = pan.current; cancel(false);
      plainClicks.current = !current.active && current.click ? Math.min(2, plainClicks.current + 1) : 0;
      if (!current.active && current.click) { const at = point({ x: event.clientX, y: event.clientY }); chooseElement(page.content ? site.current?.pick(at.x, at.y) ?? null : null); }
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
  function activate(event: ReactMouseEvent) {
    if (tool !== 'select' || event.target !== event.currentTarget || event.button !== 0 || event.shiftKey || event.ctrlKey || event.metaKey || event.altKey || plainClicks.current < 2) return;
    event.preventDefault(); event.stopPropagation(); plainClicks.current = 0;
    const at = point({ x: event.clientX, y: event.clientY });
    if (page.content) { const hit = site.current?.pick(at.x, at.y); if (hit && hit.element.tag !== 'body') { chooseElement(hit); void editTextAt(at); return; } }
    const snapped = geometryForGesture({ kind: 'new', start: at, initial: { ...at, width: 0, height: 0 } }, at, page.viewport, gridSettings);
    add(requestAt({ x: snapped.x, y: snapped.y }, zoom));
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
    if (!region && element && (event.key === 'ContextMenu' || event.key === 'F10' && event.shiftKey)) {
      event.preventDefault();
      const box = stage.current!.getBoundingClientRect();
      setContext({ selection: element, point: { x: box.left + (element.rect.x - bounds.left) * zoom, y: box.top + (element.rect.y + element.rect.height - bounds.top) * zoom } }); return;
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
    const measure = () => { const rect = node.getBoundingClientRect(); setToolbarSize(current => current.width === rect.width && current.height === rect.height ? current : { width: rect.width, height: rect.height }); };
    measure(); const observer = new ResizeObserver(measure); observer.observe(node); return () => observer.disconnect();
  }, [!!element]);
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
  const toolbarPoint = element ? elementToolbarPosition(inViewport(element.rect), { x: toolbarSize.width, y: toolbarSize.height }, { x: inset.x, y: inset.y, width: measurements.width, height: measurements.height }, occupied.map(inViewport)) : null;
  const toolbarPosition = toolbarPoint ? { left: toolbarPoint.x, top: toolbarPoint.y } : {};
  const measurementRect = coordinate ? inViewport(selectedRect ?? { ...coordinate, width: 0, height: 0 }) : null;
  const px = (value: number) => `${Math.round(value * 10) / 10}`;

  return <div className={`free-workspace ${page.appearance} tool-${tool} ${gridSettings.rulers ? 'with-rulers' : ''} ${panReady ? 'pan-ready' : ''} ${panning ? 'panning' : ''}`}
    onKeyDownCapture={event => { if (event.code === 'Space' && !isTextInput(event.target) && !(event.target instanceof HTMLElement && event.target.closest('button, a, [role=button], .element-toolbar, .editor-menu')) && !event.ctrlKey && !event.metaKey && !event.altKey) { event.preventDefault(); panKey.current = true; setPanReady(true); } }}>
    <p className="sr-only" id="canvas-keyboard-help">Click a website element to select it. Drag the selected element to move it; drag a corner to resize it. Double-click text to edit it in place. Enter saves text; Escape cancels. Ask AI in the element toolbar, or Ctrl or Command K, opens a change request. Double-click empty canvas to add a request. Select, Add request and Pan tools are in the floating canvas toolbar, with V, R and H shortcuts. In Add request, click to place a text box or drag to draw an area. Left-drag on empty space pans; Space or middle drag pans anywhere. Shift+drag draws an area. Ctrl or Command with the wheel zooms under the pointer. F2 opens local element settings. Arrow keys move an element one pixel; Shift moves ten. Enter selects the next element; Alt+Enter selects its parent. Shift+Enter adds a request in view. Escape from a request focuses its move grip. Arrow keys on grips move or resize by the grid setting. Hold Alt for one-pixel changes. Delete on a grip removes its request. Home returns to the screen guide. Escape cancels a drag.</p>
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
          {page.content && <div className="accepted-canvas-preview" data-testid="accepted-canvas-preview" style={viewStyle({ x: 0, y: 0, width: page.viewport.width, height: previewHeight }, origin, zoom)} aria-hidden="true">
            <div className="accepted-canvas-content" style={{ width: page.viewport.width, height: previewHeight, transform: `scale(${zoom})` }}>
              <PreviewFrame ref={site} content={page.content} referenceHeight={page.viewport.height} onHeight={setSiteHeight} onReady={refreshElement} pageId={page.id} revision={acceptedVersion} title={`${page.name} accepted static preview`} staticView />
            </div>
          </div>}
          <div className={`screen-guide ${page.content ? 'has-preview' : ''}`} data-testid="screen-guide" style={viewStyle({ x: 0, y: 0, ...page.viewport }, origin, zoom)} aria-hidden="true">
            <span className="screen-guide-label">Screen · {page.viewport.width} × {page.viewport.height}</span>
          </div>
          {grid && <div className="canvas-grid" aria-hidden="true" style={{ backgroundSize: `${displayStep.x * zoom}px ${displayStep.y * zoom}px`, backgroundPosition: `${-bounds.left * zoom}px ${-bounds.top * zoom}px` }} />}
          {!page.content && !page.regions.some(region => !region.completed) && <div className="canvas-empty-hint" style={viewStyle({ x: 0, y: 0, ...page.viewport }, origin, zoom)}>
            <p>Double-click anywhere to describe what goes here.</p><button type="button" onClick={addRequestInView}>Add request</button>
          </div>}
          {page.regions.map((region, index) => {
            if (region.completed) return null;
            const rect = gesture.current?.region?.id === region.id && preview ? preview : region.rect;
            return <RequestBox key={region.id} region={region} number={index + 1} rect={rect} origin={origin} zoom={zoom} maxHeight={Math.max(160, measurements.height * 0.55)} selected={selected === region.id} focus={focusRequest === region.id} focusToken={requestFocus.id === region.id ? requestFocus.serial : 0}
              layer={focused === region.id ? overlayLayer : region.zIndex + 2} select={() => { deselectElement(); setFocused(region.id); select(region.id); }} blur={() => setFocused(null)} begin={begin} keyboard={keyboard} extent={makeRoom} change={patch => change(region.id, patch)} />;
          })}
          {element && <div className="element-selection direct-selection" data-testid="direct-element-selection" data-selector={element.element.selector} data-source-hash={element.element.sourceSha256} style={{ ...viewStyle(element.rect, origin, zoom), zIndex: overlayLayer + 1 }} role="group" aria-label="Selected element bounds">
            <span className="element-type-tag">{elementName(element.element.tag)}</span>
            {element.element.selector !== 'body' && (['nw','ne','sw','se'] as const).map(corner => <button key={corner} type="button" className={`element-resize-corner corner-${corner}`} data-corner={corner} aria-label={`Resize ${corner.includes('n') ? 'top' : 'bottom'}-${corner.includes('w') ? 'left' : 'right'}`} title="Drag to resize; press Enter for size settings" disabled={editingElement || nudging} onPointerDown={event => beginElementGesture(event, corner)} onClick={event => { if (event.detail === 0) setElementDialog('edit'); }} />)}
          </div>}
          {inlineText && <InlineTextEditor key={`${inlineText.selection.element.sourceSha256}:${inlineText.index}`} text={inlineText} origin={origin} zoom={zoom} layer={overlayLayer + 4} disabled={editingElement} finish={finishText} />}
          {changing.map((rect, index) => <div key={index} className="generation-blur" data-testid="generation-blur" style={{ ...viewStyle(rect, origin, zoom), zIndex: overlayLayer + 2 }} aria-hidden="true" />)}
          {highlighted.map((rect, index) => <div key={index} className="generation-highlight" data-testid="generation-highlight" style={{ ...viewStyle(rect, origin, zoom), zIndex: overlayLayer + 2 }} aria-hidden="true" />)}
          {guides.x.map(x => <div key={`x${x}`} className="alignment-guide alignment-x" aria-hidden="true" style={{ left: (x - bounds.left) * zoom, zIndex: overlayLayer + 1 }} />)}
          {guides.y.map(y => <div key={`y${y}`} className="alignment-guide alignment-y" aria-hidden="true" style={{ top: (y - bounds.top) * zoom, zIndex: overlayLayer + 1 }} />)}
          {preview && gesture.current?.kind === 'new' && <div className="annotation-draft" style={{ ...viewStyle(preview, origin, zoom), zIndex: overlayLayer }} />}
        </div>
      </div>
    </div>
    {element && !inlineText && <div ref={toolbar} className="element-toolbar" role="toolbar" aria-label="Selected element" style={toolbarPosition}>
      <button className="ask-ai" title="Describe a change with AI · Ctrl/⌘ K" disabled={editingElement || nudging} onClick={askAI}>Ask AI</button>
      <button aria-label="Edit" title="Edit element · F2" disabled={editingElement || nudging} onClick={() => setElementDialog('edit')}><ElementIcon name="edit" /></button>
      <button aria-label="Replace" title="Replace element" disabled={editingElement || nudging || element.element.selector === 'body'} onClick={() => setElementDialog('replace')}><ElementIcon name="replace" /></button>
      <button aria-label="Move" title="Position offsets" disabled={editingElement || nudging || element.element.selector === 'body'} onClick={() => setElementDialog('position')}><ElementIcon name="move" /></button>
      <MenuButton label={<ElementIcon name="more" />} ariaLabel="Element options" title="More element actions" showChevron={false} items={[
        { id: 'before', label: 'Move earlier', disabled: editingElement || nudging || element.element.selector === 'body', action: () => { void applyElement({ kind: 'reorder', direction: 'before' }).catch(() => {}); } },
        { id: 'after', label: 'Move later', disabled: editingElement || nudging || element.element.selector === 'body', action: () => { void applyElement({ kind: 'reorder', direction: 'after' }).catch(() => {}); } },
        { id: 'parent', label: 'Select parent', action: () => chooseElement(site.current?.next(true, elementRef.current?.element) ?? null) },
        { id: 'delete', label: 'Delete element', disabled: editingElement || nudging || element.element.selector === 'body', action: () => setElementDialog('remove') },
        { id: 'deselect', label: 'Deselect', action: deselectElement },
      ]} />
    </div>}
    {context && <ContextMenu key={`${context.point.x}:${context.point.y}:${context.selection.element.selector}`} label="Element actions" point={context.point} close={closeContext} items={[
      { id: 'edit', label: 'Edit', disabled: editingElement || nudging, action: () => setElementDialog('edit') },
      { id: 'generate', label: 'Ask AI', disabled: editingElement || nudging, action: () => pickElement(context.selection) },
    ]} />}
    {elementError && <div className="element-edit-error" role="alert">{elementError}<button onClick={() => setElementError('')}>Dismiss</button></div>}
    {elementDialog && element && <ElementDialog mode={elementDialog} target={element.element} visual={site.current?.visual(element.element) ?? null} inspect={inspectElement} apply={change => applyElement(change)} move={moveElement} close={() => { setElementDialog(null); if (elementRef.current && !site.current?.find(elementRef.current.element) && !pendingSelection.current) deselectElement(); }} />}
    {coordinate && measurementRect && gridSettings.rulers && <div className="workspace-measurement" aria-label="Placement in pixels" title={`${percent(coordinate.x, page.viewport.width)} · ${percent(coordinate.y, page.viewport.height)}${selectedRect ? ` · ${percent(selectedRect.width, page.viewport.width)} × ${percent(selectedRect.height, page.viewport.height)}` : ''}`}
      style={{ left: clamp(measurementRect.x, inset.x + 8, Math.max(inset.x + 8, inset.x + measurements.width - 260)), top: clamp(measurementRect.y + measurementRect.height + 8, inset.y + 8, Math.max(inset.y + 8, inset.y + measurements.height - 30)) }}>
      {px(coordinate.x)}, {px(coordinate.y)} px{selectedRect && <span>{px(selectedRect.width)} × {px(selectedRect.height)} px</span>}</div>}
    {atEdge && <div className="workspace-edge" role="status">Workspace edge reached</div>}
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

function RequestBox({ region, number, rect, origin, zoom, maxHeight, selected, focus, focusToken, layer, select, blur, begin, keyboard, extent, change }: {
  region: Region; number: number; rect: Rect; origin: Point; zoom: number; maxHeight: number; selected: boolean; focus: boolean; focusToken: number; layer: number; select: () => void; blur: () => void;
  begin: (event: PointerEvent, kind: Gesture['kind'], region?: Region) => void;
  keyboard: (event: KeyboardEvent, region?: Region, resize?: boolean) => void; extent: (rect: Rect) => void; change: (patch: RequestPatch) => Promise<Region>;
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
    text.current?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  }, [focus, focusToken]);
  const target = !!region.element;
  const promptRect = target ? { x: rect.x, y: rect.y + rect.height + 10 / zoom, width: Math.max(260 / zoom, Math.min(rect.width, 420 / zoom)), height: 112 / zoom } : rect;
  useLayoutEffect(() => {
    const node = text.current; if (!node) return;
    const old = node.style.height; node.style.height = '0px'; const style = getComputedStyle(node);
    const height = node.scrollHeight + (Number.parseFloat(style.borderTopWidth) || 0) + (Number.parseFloat(style.borderBottomWidth) || 0); node.style.height = old;
    setNaturalHeight(current => current === height ? current : height);
  }, [draft, promptRect.width, zoom]);
  const minimumHeight = Math.min(maxHeight, promptRect.height * zoom);
  const fullHeight = Math.max(minimumHeight, naturalHeight);
  const hasMore = fullHeight > maxHeight;
  useLayoutEffect(() => { if (hasMore && document.activeElement === text.current) setExpanded(true); }, [hasMore, draft]);
  const displayHeight = expanded ? fullHeight : Math.min(maxHeight, fullHeight);
  const shown = { ...promptRect, height: (displayHeight + (hasMore ? 30 : 0)) / zoom };
  useLayoutEffect(() => extent(shown), [shown.x, shown.y, shown.width, shown.height]);
  return <>
  {target && <div className="element-selection" data-testid="element-selection" style={{ ...viewStyle(rect, origin, zoom), zIndex: layer }} aria-hidden="true" />}
  <div data-region-id={region.id} className={`annotation ${target ? 'element-prompt' : ''} ${selected ? 'selected' : ''} ${expanded ? 'expanded' : ''}`} style={{ ...viewStyle(shown, origin, zoom), zIndex: layer }}
    onFocusCapture={select} onBlurCapture={event => { if (!(event.relatedTarget instanceof Node) || !event.currentTarget.contains(event.relatedTarget)) blur(); }}>
    <textarea ref={text} aria-label={`Request ${number}`} aria-describedby="canvas-keyboard-help" maxLength={16384} value={draft} placeholder={target ? "Describe the change…" : "Describe what belongs here…"}
      style={{ height: displayHeight }} onFocus={() => { if (hasMore) setExpanded(true); }} onChange={event => { changeDraft(event.target.value); if (hasMore) setExpanded(true); }} onPointerDown={event => event.stopPropagation()}
      onKeyDown={event => { if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); (move.current ?? document.getElementById('page-canvas'))?.focus({ preventScroll: true }); } }} />
    {hasMore && <button type="button" className="request-expand" aria-expanded={expanded} onClick={() => { setExpanded(value => !value); if (expanded) text.current?.scrollIntoView({ block: 'nearest', inline: 'nearest' }); }}>{expanded ? 'Less' : '… More'}</button>}
    {selected && !target && <><button ref={move} className="annotation-grip move-grip" aria-label={`Move request ${number}`} aria-describedby="canvas-keyboard-help" title="Drag to move. Arrow keys change placement."
      onPointerDown={event => begin(event, 'move', region)} onKeyDown={event => keyboard(event, region)}><svg viewBox="0 0 16 16" aria-hidden="true"><circle cx="5" cy="4" r="1"/><circle cx="11" cy="4" r="1"/><circle cx="5" cy="8" r="1"/><circle cx="11" cy="8" r="1"/><circle cx="5" cy="12" r="1"/><circle cx="11" cy="12" r="1"/></svg></button>
      <button className="annotation-grip resize-grip" aria-label={`Resize request ${number}`} aria-describedby="canvas-keyboard-help" title="Drag or use arrow keys to resize the placement hint."
        onPointerDown={event => begin(event, 'resize', region)} onKeyDown={event => keyboard(event, region, true)}><svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4 12L12 4M8 12H12V8M4 8V4H8" fill="none" stroke="currentColor" strokeWidth="1.5"/></svg></button></>}
  </div></>;
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
      event.stopPropagation();
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

function ElementIcon({ name }: { name: 'edit' | 'replace' | 'move' | 'more' }) {
  return <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" focusable="false">
    {name === 'edit' ? <><path d="m15 4 5 5M4 20l4-1 12-12a2 2 0 0 0-5-5L3 14l-1 6Z" /><path d="M13 20h9" /></> : name === 'replace' ? <><path d="M4 8h15l-4-4m4 12H4l4 4" /><path d="M20 4v6M4 14v6" /></> : name === 'move' ? <><path d="M12 3v18M3 12h18m-12-6 3-3 3 3m-6 12 3 3 3-3M6 9l-3 3 3 3m12-6 3 3-3 3" /></> : <><circle cx="5" cy="12" r="1" /><circle cx="12" cy="12" r="1" /><circle cx="19" cy="12" r="1" /></>}
  </svg>;
}
