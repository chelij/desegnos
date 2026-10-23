import { ProblemBanner } from './ProblemBanner.js';
import { areaProblems, placeCallouts } from './generationProblems';
import type { GenerationController } from './useGeneration';
import { userError, runtimeError, generationError, generationPageError } from './userErrors.js';
import { isWholePage } from './wholePageScope';
import { forwardRef, useImperativeHandle, useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type PointerEvent, type KeyboardEvent, type MouseEvent as ReactMouseEvent } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { isTauri } from '@tauri-apps/api/core';
import { flushSync } from 'react-dom';
import { canonicalSelections, regionTargets, sameTarget, unionRect } from './elementTargets';
import { useDraftText } from './DraftText';
import { escapeHtml, imageFile } from './elementEditing';
import { StyleBar, type StyleChange } from './StyleBar';
import { askBeforeDelete, setAskBeforeDelete } from './deletePreference';
import { LayersPopover, LayerIcon } from './Layers';
import type { SiteLayer } from './siteLayers';
import { ElementDialog, type ElementDialogMode } from './ElementTools';
import { ContextMenu, type MenuItem } from './Menus';
import { PreviewFrame, type CanvasPreviewHandle, type ElementMover, type ElementResizer, type FrameView, type InlineText } from './Preview';
import { FrameOverlay, useLiveFrameSession } from './FrameOverlays';
import type { LiveHistoryAction } from './liveBar';
import { overlayPlacement, placeOverlays } from './previewFrames';
import { shownSelectionRect } from './previewMeasurement';
import { freehandArea, pointInRegion, alignmentLines, elementResizeRect, elementToolbarPosition, selectionLabelPosition, clamp, clientToCanvas, DEFAULT_GRID, expandWorkspace, geometryForGesture, gridSteps, workspaceWindow, keyboardRectChange, percent, percentageTicks, pixelTicks, requestAt, requestPlacement, sameBounds, sameRect, intersectsWorkspace, clipToWorkspace, validWorkspace, MIN_ZOOM, wheelZoom, zoomOrigin, type CanvasGridSettings, type GeometryGesture, type Point, type RectChange, type WorkspaceBounds, type ResizeCorner } from './canvasGeometry';
import type { ElementBounds, ElementDetails, ElementEdit, ElementSelection, ElementTarget, ElementStyleEdit, LayerTarget, LayerDirection, LayerPasteView, Page, Rect, Region, RegionShape, Viewport } from './types';
import { RegionOutline } from './RegionOutline';
import type { RequestShapeKind } from './CanvasTools';
import './canvas.css';

// A prompt function is read when the save runs, so queued keystrokes save the newest text once.
export type RequestPatch = { wholePage?: boolean; prompt?: string | (() => string); rect?: RectChange | Partial<Rect> };
export type CanvasViewAction = { serial: number; kind: 'guide' | 'selection' | 'fit' };
type Gesture = GeometryGesture & { pointerId: number; region?: Region; active: boolean; shapeKind: RequestShapeKind; points: Point[]; freehand?: RegionShape };
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

function requestPromptRect(rect: Rect, zoom: number): Rect {
  return { x: rect.x, y: rect.y + rect.height + 10 / zoom, width: Math.max(260 / zoom, Math.min(rect.width, 420 / zoom)), height: 112 / zoom };
}

type CanvasProps = {
  generation?: GenerationController;
  archiveReports?: Record<string, import('./types').ArchiveReport[]>; projectId?: string;
  pendingImagesChanged: (count: number) => void; canUndo: boolean; canRedo: boolean; undo: () => void; redo: () => void; toggleGrid: () => void; configureGrid: () => void;
  fixRuntimeErrors?: (errors: string[]) => void; aiDisabled?: boolean; observationsVersion?: number;
  projectImages?: import('./types').ProjectImage[]; page: Page; layersOpen: boolean; setLayersOpen: (open: boolean) => void; selectionChanged: (target: ElementTarget | null) => void; multiChanged?: (count: number) => void; repick?: (regionId: string, selection: ElementSelection) => Promise<void>; fixOnPhones?: (selection: ElementSelection) => void; attach?: (regionId: string, files: File[], links: string[]) => void; liveView?: { width: number; height: number } | null; detach?: (regionId: string, attachmentId: string) => void; onLiveLocation?: (path: string) => void;
  arrangeRequest: (id: string, direction: LayerDirection) => Promise<void>; copyNative: (target: LayerTarget, styles: ElementStyleEdit[]) => Promise<void>; cutNative: (target: LayerTarget, styles: ElementStyleEdit[]) => Promise<void>; pasteNative: (after: ElementTarget | null) => Promise<LayerPasteView>;
  requestShape?: RequestShapeKind; grid: boolean; gridSettings?: CanvasGridSettings; tool?: 'select' | 'request' | 'pan'; rulerUnit?: 'pixels' | 'percent'; zoom: number; selected: string | null; focusRequest: string | null; viewAction?: CanvasViewAction; changing?: { rect: Rect; shape?: RegionShape }[]; highlighted?: { rect: Rect; shape?: RegionShape }[];
  refreshElementBounds: (pageId: string, viewport: Viewport, bounds: ElementBounds[]) => Promise<void>;
  openSource?: (target: ElementTarget) => void;
  inspectElement: (target: ElementTarget) => Promise<ElementDetails>; editElement: (target: ElementTarget, change: ElementEdit) => Promise<ElementTarget | null>; undoElement?: (target: ElementTarget) => Promise<ElementTarget | null>;
  changeZoom: (zoom: number) => void; select: (id: string | null, cleanEmpty?: boolean) => void; add: (rect: Rect, shape?: RegionShape) => void; pickElement: (selection: ElementSelection[]) => void; deleteElements: (elements: ElementTarget[]) => Promise<void>; change: (id: string, patch: RequestPatch) => Promise<Region>; remove: (id: string) => void; setTargetRemoval: (regionId: string, allowed: boolean) => void;
};
export type CanvasHandle = { liveHistory: (action: LiveHistoryAction) => void; focusElement: (selector: string, tag: string, id?: string, expectedPageId?: string) => ElementSelection | null; synchronize: (requireBounds?: boolean) => Promise<void>; addRequest: () => void; focusRequest: (id: string) => void; askAI: () => void; undoElement: () => Promise<void>; copyLayer: () => Promise<void>; cutLayer: () => Promise<void>; pasteLayer: () => Promise<void>; arrangeLayer: (direction: LayerDirection) => Promise<void> };
export const Canvas = forwardRef<CanvasHandle, CanvasProps>(function Canvas({ generation, archiveReports = {}, fixRuntimeErrors, aiDisabled, observationsVersion, projectId, pendingImagesChanged, projectImages = [], canUndo, canRedo, undo, redo, toggleGrid, configureGrid, page, layersOpen, setLayersOpen, selectionChanged, multiChanged, repick, fixOnPhones, attach, detach, liveView = null, onLiveLocation, arrangeRequest, copyNative, cutNative, pasteNative, grid, gridSettings = DEFAULT_GRID, tool = 'select', requestShape = 'rect', rulerUnit = 'pixels', zoom, selected, focusRequest, viewAction, changing = [], highlighted = [], inspectElement, editElement, undoElement, refreshElementBounds, openSource, changeZoom, select, add, pickElement, deleteElements, change, remove, setTargetRemoval }, ref) {
  const [dismissedProblems, setDismissedProblems] = useState<string[]>([]);
  const [problemErrors, setProblemErrors] = useState<Record<string, string>>({});
  const [problemBusy, setProblemBusy] = useState(false);
  const [runtimeProblems, setRuntimeProblems] = useState<string[]>([]);
  useEffect(() => { setRuntimeProblems([]); }, [page.id]);
  const problems = areaProblems(generation?.job ?? null, page);
  const dismissProblem = (key: string) => setDismissedProblems(previous => [...previous, key]);
  async function applyProblem(regionId: string, draftId: string, key: string) {
    if (!generation?.job) return;
    setProblemBusy(true);
    try { await generation.draftAction(generation.job.jobId, draftId, 'apply', undefined, regionId); }
    catch (error) { setProblemErrors(previous => ({ ...previous, [key]: String(error) })); }
    finally { setProblemBusy(false); }
  }
  const toolbar = useRef<HTMLDivElement>(null);
  const measurement = useRef<HTMLDivElement>(null);
  const [measurementPosition, setMeasurementPosition] = useState<Point | null>(null);
  const [toolbarSize, setToolbarSize] = useState({ width: 160, height: 40 });
  const [context, setContext] = useState<CanvasContext | null>(null);
  const scroll = useRef<HTMLDivElement>(null);
  const site = useRef<CanvasPreviewHandle>(null);
  // The live page's own frame: its bridge owns navigation and history inside Live.
  const liveSite = useRef<CanvasPreviewHandle>(null);
  const [siteLayers, setSiteLayers] = useState<SiteLayer[]>([]);
  const [layersReady, setLayersReady] = useState(false);
  const [layerBusy, setLayerBusy] = useState(false);
  const [siteHeight, setSiteHeight] = useState(page.viewport.height);
  // Frames read from the promoted static document, and the one the user is playing.
  const [frameView, setFrameView] = useState<FrameView>({ pageUrl: null, frames: [] });
  const [interactingFrame, setInteractingFrame] = useState<string | null>(null);
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
  // The box the current edit aims at. A promoted frame is measured before its layout has
  // settled, so the canvas keeps drawing this box until the settled measurement arrives.
  const expectedSelection = useRef<{ element: ElementTarget; rect: Rect } | null>(null);
  function expectSelection(element: ElementTarget, rect: Rect) { expectedSelection.current = { element, rect }; }
  const pageRef = useRef(page); pageRef.current = page;
  const nudge = useRef({ x: 0, y: 0, promise: null as Promise<void> | null });
  const syncing = useRef<Promise<void> | null>(null);
  useEffect(() => {
    const guard = (event: DragEvent) => { if (event.dataTransfer?.types.includes('Files')) event.preventDefault(); };
    window.addEventListener('dragover', guard); window.addEventListener('drop', guard);
    return () => { window.removeEventListener('dragover', guard); window.removeEventListener('drop', guard); };
  }, []);
  const [placeholders, setPlaceholders] = useState<ElementSelection[]>([]);
  // When a side panel opens or closes, the canvas changes width. Shift by half the difference so
  // whatever was in the middle of the view stays there instead of sliding under the panel.
  useEffect(() => {
    const node = scroll.current; if (!node) return;
    let width = node.clientWidth;
    const observer = new ResizeObserver(() => {
      const next = node.clientWidth;
      if (next !== width && width > 0) node.scrollLeft += (width - next) / 2;
      width = next;
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, []);
  useEffect(() => { if (liveView) { deselectElement(); setExtras([]); } }, [!!liveView]);
  const [extras, setExtrasState] = useState<ElementSelection[]>([]);
  const extrasRef = useRef<ElementSelection[]>([]);
  // Element requests whose element can no longer be found on the rendered page.
  const [stale, setStale] = useState<Set<string>>(new Set());
  function refreshStaleTargets() {
    setStale(new Set(page.content && site.current?.source() ? page.regions.filter(region => !region.completed && regionTargets(region).some(target => !site.current?.contains(target))).map(region => region.id) : []));
  }
  useEffect(refreshStaleTargets, [page.id, page.content, page.regions]);
  const [repicking, setRepicking] = useState<string | null>(null);
  function setExtras(value: ElementSelection[]) { extrasRef.current = value; setExtrasState(value); }
  useEffect(() => { multiChanged?.(extras.length ? extras.length + 1 : 0); }, [extras.length, multiChanged]);
  const [dropTarget, setDropTarget] = useState<Rect | null>(null);
  const imagePicker = useRef<HTMLInputElement>(null);
  const nativeDrop = useRef<{ id: string; pageId: string; source: string | null; selection: ElementSelection | null; regionId?: string } | null>(null);
  const nativeDrag = useRef<(value: { phase: string; id: string; position: Point | null; picture: { name: string; mime: string; data: string } | null; files: { name: string; mime: string; data: string }[]; fileErrors: string[]; message: string | null }) => void>(() => {});
  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false; let stop: (() => void) | undefined;
    void getCurrentWindow().listen<Parameters<typeof nativeDrag.current>[0]>('desegnos://image-drag', event => nativeDrag.current(event.payload)).then(unlisten => { if (disposed) unlisten(); else stop = unlisten; });
    return () => { disposed = true; stop?.(); };
  }, []);
  const browseFor = useRef<ElementSelection | null>(null);
  const pendingImages = useRef<{ selection: ElementSelection; file: File; src: string }[]>([]);
  const imageReads = useRef(new Set<Promise<void>>());
  const imageFlush = useRef<Promise<void> | null>(null);
  async function stageImage(selection: ElementSelection, file: File) {
    const pageId = page.id;
    const work = (async () => {
      try {
        const src = await imageFile(file, false);
        if (pageRef.current.id !== pageId || !site.current?.previewImage(selection.element, src)) throw new Error('The picture target changed. Drop it again.');
        pendingImages.current = [...pendingImages.current.filter(item => !sameTarget(item.selection.element, selection.element)), { selection, file, src }];
        pendingImagesChanged(pendingImages.current.length);
        setPlaceholders(current => current.filter(item => !sameTarget(item.element, selection.element)));
        setElementError('');
      } catch (error) { setElementError(error instanceof Error ? error.message : String(error)); }
    })();
    imageReads.current.add(work);
    try { await work; } finally { imageReads.current.delete(work); }
  }
  async function flushImages() {
    if (imageFlush.current) return imageFlush.current;
    imageFlush.current = (async () => {
      await Promise.all(imageReads.current);
      if (pendingImages.current.length) await waitForSite(pageRef.current.id);
      while (pendingImages.current.length) {
        const item = pendingImages.current[0];
        // A picture deleted while its replacement was pending needs no update.
        if (site.current?.contains(item.selection.element)) await replaceImage(item.selection, item.file);
        pendingImages.current = pendingImages.current.filter(entry => entry !== item);
        pendingImagesChanged(pendingImages.current.length);
        await waitForSite(pageRef.current.id);
      }
    })().finally(() => { imageFlush.current = null; });
    return imageFlush.current;
  }
  // Fill a picture from a file: dropped from the file manager, or chosen with Browse.
  async function replaceImage(selection: ElementSelection, file: File) {
    if (!file.type.startsWith('image/')) { setElementError('Choose a picture file (PNG, JPEG, WebP or GIF).'); return; }
    try {
      const src = await imageFile(file);
      const details = await inspectElement(selection.element);
      const alt = details.attributes.alt ?? '';
      const description = /^image placeholder:?\s*/i.test(alt) ? alt.replace(/^image placeholder:?\s*/i, '') || file.name.replace(/\.[^.]+$/, '') : alt;
      // A placeholder carries a marker that ordinary attribute edits can't remove, so swap in a fresh <img> with the same styling.
      if (site.current?.placeholders().some(item => item.element.selector === selection.element.selector)) {
        const css = document.createElement('div').style; css.cssText = details.inlineStyle;
        // The edit form exposes only editable fields; authored dimensions and loading hints live in the full element HTML.
        const original = document.createElement('template'); original.innerHTML = details.html;
        const attributes = Array.from(original.content.firstElementChild?.attributes ?? []).filter(attribute => !['src', 'srcset', 'sizes', 'alt', 'style'].includes(attribute.name) && !attribute.name.startsWith('data-desegnos-'));
        await applyElement({ kind: 'replace', html: `<img ${attributes.map(attribute => `${attribute.name}="${escapeHtml(attribute.value)}"`).join(' ')} src="${escapeHtml(src)}" alt="${escapeHtml(description)}" style="${escapeHtml(css.cssText)}">`, keepStyle: true }, selection);
      } else await applyElement({ kind: 'update', texts: [], attributes: description === alt ? { src } : { src, alt: description }, style: null }, selection);
    } catch (error) { setElementError(error instanceof Error ? error.message : String(error)); throw error; }
  }
  function browseImage(selection: ElementSelection | null) { browseFor.current = selection; imagePicker.current?.click(); }
  function storeElement(value: ElementSelection | null) { elementRef.current = value; setElement(value); selectionChanged(value?.element ?? null); }
  function deselectElement() { setElementDialog(null); setConfirmMany(null); setExtras([]); setContext(null); inlineTicket.current++; selectionSerial.current++; nudge.current.x = 0; nudge.current.y = 0; pendingSelection.current = null; expectedSelection.current = null; storeElement(null); }
  useLayoutEffect(() => { deselectElement(); setRepicking(null); setStale(new Set()); setElementDialog(null); setElementError(''); }, [page.id]);
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
  const [draftShape, setDraftShape] = useState<RegionShape>({ kind: 'rect' });
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
  useLayoutEffect(() => { setContext(null); plainClicks.current = 0; lastPlainClick.current = null; if (elementDrag.current) cancel(); inlineTicket.current++; inlineTextRef.current = null; setInlineText(null); setInteractingFrame(null); setFrameView({ pageUrl: null, frames: [] }); }, [acceptedVersion]);

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
  function cancel(resetClicks = true, restoreElement = true) {
    if (resetClicks) { plainClicks.current = 0; lastPlainClick.current = null; }
    const pointerId = gesture.current?.pointerId ?? pan.current?.pointerId ?? elementDrag.current?.pointerId;
    if (elementDrag.current) {
      const drag = elementDrag.current;
      if (restoreElement) {
        drag.mover?.cancel(); drag.resizer?.cancel();
        expectedSelection.current = null;
        if (drag.pageId === pageRef.current.id && drag.version === acceptedVersionRef.current) storeElement(drag.original);
        else deselectElement();
      }
      elementDrag.current = null;
    }
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
    if (event.target instanceof Element && event.target.closest('.canvas-empty-hint button, .area-problem')) return;
    if (event.button === 1 || event.button === 0 && (panKey.current || tool === 'pan')) startPan(event, false);
  }
  function begin(event: PointerEvent, kind: Gesture['kind'], region?: Region) {
    if (event.button !== 0 || gesture.current || pan.current || panKey.current) return;
    event.preventDefault(); event.stopPropagation();
    const start = point({ x: event.clientX, y: event.clientY });
    gesture.current = { kind, start, initial: region?.rect ?? { x: start.x, y: start.y, width: 0, height: 0 }, pointerId: event.pointerId, region, active: false, shapeKind: region ? region.shape?.kind ?? 'rect' : requestShape, points: [start] };
    setDraftShape({ kind: requestShape === 'ellipse' ? 'ellipse' : 'rect' });
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
    if (!current.active) return null;
    if (current.kind === 'new' && current.shapeKind === 'freehand') {
      const last = current.points.at(-1)!;
      if (Math.hypot(now.x - last.x, now.y - last.y) * zoom >= 1) current.points.push(now);
      if (current.points.length > 4096) current.points = current.points.filter((_, index) => index % 2 === 0);
      const area = freehandArea(current.points, 1.5 / zoom);
      if (!area) return null;
      current.freehand = area.shape; setDraftShape(area.shape); return area.rect;
    }
    return geometryForGesture(current, now, page.viewport, gridSettings, current.shapeKind === 'rect' ? minimum : { x: 1, y: 1 });
  }
  function updateDrag(client: Point) {
    if (elementDrag.current) { updateElementDrag(client, elementDrag.current.fine); return; }
    const current = gesture.current;
    if (!current) return;
    const rect = currentRect(client, current);
    if (rect) setPreview(current.kind === 'new' && current.shapeKind === 'rect' ? requestPlacement(rect, zoom) : rect);
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
  // Extra elements selected with Shift/Ctrl/Cmd+click, alongside the primary selection.
  // Nested picks keep the outer element, since its descendants are in scope anyway.
  function toggleElement(hit: ElementSelection | null) {
    if (!hit || hit.element.selector === 'body') return;
    const primary = elementRef.current;
    if (!primary) { chooseElement(hit); return; }
    const all = [primary, ...extrasRef.current];
    const rest = all.some(item => sameTarget(item.element, hit.element))
      ? all.filter(item => !sameTarget(item.element, hit.element)) : canonicalSelections([...all, hit]);
    if (rest.length > 8) { setElementError('Select up to 8 elements for one request.'); return; }
    setElementError(''); select(null);
    storeElement(rest[0] ?? null); setExtras(rest.slice(1));
  }
  const [confirmMany, setConfirmMany] = useState<{ count: number; skip: boolean; targets: ElementTarget[]; pageId: string } | null>(null);
  useEffect(() => { setConfirmMany(null); }, [page.id, element?.element.id, extras.length]);
  useEffect(() => {
    if (!confirmMany) return;
    const cancel = (event: globalThis.KeyboardEvent) => { if (event.key === 'Escape') { event.preventDefault(); event.stopImmediatePropagation(); setConfirmMany(null); } };
    window.addEventListener('keydown', cancel, true);
    return () => window.removeEventListener('keydown', cancel, true);
  }, [!!confirmMany]);
  // Delete the selected element, asking first unless the user chose "Don't ask again".
  function deleteElement() {
    if (!elementRef.current || elementRef.current.element.selector === 'body') return;
    if (askBeforeDelete()) { setElementDialog('remove'); return; }
    void applyElement({ kind: 'remove' }).catch(() => {});
  }
  async function deleteSelected(confirmed = false, targets?: ElementTarget[]) {
    const all = targets ?? canonicalSelections([elementRef.current, ...extrasRef.current].flatMap(item => item ? [item] : [])).map(item => item.element);
    if (all.length < 2 || editingElement) return;
    if (!confirmed && askBeforeDelete()) { setConfirmMany({ count: all.length, skip: false, targets: all, pageId: page.id }); return; }
    setEditingElement(true);
    try { await deleteElements(all); deselectElement(); }
    finally { setEditingElement(false); }
  }
  function chooseElement(selection: ElementSelection | null) { deselectElement(); storeElement(selection); setElementError(''); select(null); }
  const [styleOpen, setStyleOpen] = useState(() => { try { return localStorage.getItem('desegnos.style-bar') === 'open'; } catch { return false; } });
  function toggleStyle() { setStyleOpen(value => { try { localStorage.setItem('desegnos.style-bar', value ? 'closed' : 'open'); } catch {} return !value; }); }
  // Quick style edits update the element's own inline style, the same way the Edit dialog does.
  async function applyStyles(change: StyleChange) {
    const current = elementRef.current; if (!current) return;
    try {
      const details = await inspectElement(current.element);
      const css = document.createElement('div').style; css.cssText = details.inlineStyle;
      for (const [name, value] of Object.entries(change)) {
        if (!CSS.supports(name, value)) throw new Error('That style value is not supported.');
        css.setProperty(name, value, 'important');
      }
      await applyElement({ kind: 'update', texts: [], attributes: {}, style: css.cssText }, current);
    } catch (error) { setElementError(error instanceof Error ? error.message : String(error)); }
  }
  async function applyElement(change: ElementEdit, selected = elementRef.current) {
    if (!selected) throw new Error('Select an element first.');
    const ticket = selectionSerial.current; const pageId = page.id;
    const ownsSelection = !!elementRef.current && sameTarget(elementRef.current.element, selected.element);
    setEditingElement(true); setElementError('');
    try {
      const target = await editElement(selected.element, change);
      if (ownsSelection && pageRef.current.id === pageId && selectionSerial.current === ticket) {
        // Keep the box the canvas is already drawing (a drag target or a nudged box)
        // instead of the rect measured before the edit.
        const latest = elementRef.current;
        const shown = latest && sameTarget(latest.element, selected.element) ? latest : selected;
        // The edit may report a new identity for the same element; the box the canvas is
        // drawing belongs to it either way.
        if (target && expectedSelection.current && sameTarget(expectedSelection.current.element, selected.element)) expectedSelection.current = { element: target, rect: expectedSelection.current.rect };
        pendingSelection.current = target;
        storeElement(target ? { ...shown, element: target } : null);
      }
    } catch (error) { if (pageRef.current.id === pageId && selectionSerial.current === ticket) setElementError(error instanceof Error ? error.message : String(error)); throw error; }
    finally { setEditingElement(false); }
  }
  // Steps only the selected element back to its previous version; other changes stay.
  async function stepBackElement(selected = elementRef.current) {
    if (!undoElement || !selected || selected.element.selector === 'body' || editBusy) return;
    const ticket = selectionSerial.current; const pageId = page.id;
    setEditingElement(true); setElementError('');
    try {
      const target = await undoElement(selected.element);
      if (pageRef.current.id === pageId && selectionSerial.current === ticket) {
        pendingSelection.current = target;
        storeElement(target ? { ...selected, element: target } : null);
      }
    } catch (error) { if (pageRef.current.id === pageId && selectionSerial.current === ticket) setElementError(error instanceof Error ? error.message : String(error)); }
    finally { setEditingElement(false); }
  }
  async function editTextAt(at: Point) {
    const text = site.current?.textAt(at.x, at.y);
    if (!text) { if (elementRef.current) setElementDialog('edit'); return; }
    chooseElement(text.selection);
    const ticket = ++inlineTicket.current; const pageId = page.id;
    try {
      const details = await inspectElement(text.selection.element);
      if (ticket !== inlineTicket.current || pageRef.current.id !== pageId || !site.current?.find(text.selection.element)) return;
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
    const css = mover.style(x, y);
    const source = site.current?.source(); const pageId = pageRef.current.id;
    const target = { ...selected.rect, x: selected.rect.x + x, y: selected.rect.y + y };
    // Move the box with the nudge and keep it as the target until the committed
    // preview settles; a measurement taken before then can describe another layout.
    expectSelection(selected.element, target);
    storeElement({ ...selected, rect: target });
    try {
      await applyElement({ kind: 'update', texts: [], attributes: {}, style: css }, selected);
      // Stable IDs also resolve in the retained old frame. Wait for the committed
      // document before another queued nudge reads its computed translation.
      await waitForSite(pageId, elementRef.current?.element, source);
    }
    finally { mover.cancel(); }
  }
  const moveElementRef = useRef(moveElement); moveElementRef.current = moveElement;
  async function waitForSite(pageId: string, target?: ElementTarget, previousSource?: string | null) {
    const deadline = performance.now() + 4000;
    do {
      if (pageRef.current.id !== pageId) throw new Error('The page changed. Select the item again.');
      const source = site.current?.source();
      if (source && source !== previousSource && (!target || site.current?.find(target))) return;
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
      if (!current.content || !current.regions.some(region => !region.completed)) { setStale(new Set()); return; }
      await waitForSite(current.id);
      if (JSON.stringify(pageRef.current.content) !== JSON.stringify(current.content) || pageRef.current.id !== current.id) throw new Error('The page changed while measuring its items. Try again.');
      const regions = pageRef.current.regions.filter(region => !region.completed);
      const bounds: ElementBounds[] = []; const missing = new Set<string>();
      const areaBounds = regions.some(region => !regionTargets(region).length) ? site.current!.measuredElements().map(item => ({ target: item.element, rect: item.rect })) : [];
      for (const region of regions) {
        const targets = regionTargets(region);
        if (!targets.length) {
          if (JSON.stringify(region.areaBounds ?? []) !== JSON.stringify(areaBounds)) bounds.push({ regionId: region.id, bounds: areaBounds });
          continue;
        }
        if (targets.some(target => !site.current?.contains(target))) { missing.add(region.id); continue; }
        const found = targets.map(target => site.current?.find(target) ?? null);
        // A hidden element still exists. Keep its last placement until it has visible bounds again.
        if (found.some(item => !item)) continue;
        const measured = found.filter((item): item is ElementSelection => !!item);
        if (!sameRect(unionRect(measured.map(item => item.rect)), region.rect)) bounds.push({ regionId: region.id, bounds: measured.map(item => ({ target: item.element, rect: item.rect })) });
      }
      setStale(missing);
      if (missing.size) throw new Error('This element was removed. Pick it again or remove its request before generating.');
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
    await flushImages();
    if (inlineSaving.current) await inlineSaving.current;
    const text = inlineTextRef.current;
    if (text) await finishText(stage.current?.querySelector<HTMLTextAreaElement>('.inline-text-editor')?.value ?? text.text);
    while (nudge.current.promise) await nudge.current.promise;
    if (elementDrag.current) cancel();
    const target = elementRef.current?.element;
    if (target) await waitForSite(pageRef.current.id, target);
  }
  async function copyLayer() {
    if (extrasRef.current.length) { setElementError('Select one item to copy it.'); return; }
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
    if (extrasRef.current.length) { setElementError('Select one item to cut it.'); return; }
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
    if (extrasRef.current.length) { setElementError('Select one item to arrange it.'); return; }
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
  function askAI() { const current = elementRef.current; if (current && !editingElement && !nudging) { setContext(null); pickElement(canonicalSelections([current, ...extrasRef.current])); } }
  // Select a website element by its structural selector (used by Page check), if the page has rendered.
  function focusElement(selector: string, tag: string, id?: string, expectedPageId?: string) {
    if (expectedPageId && pageRef.current.id !== expectedPageId) return null;
    const sourceSha256 = site.current?.source(); if (!sourceSha256) return null;
    const found = site.current?.find({ selector, tag, label: '', ...(id ? { id } : { sourceSha256 }) }) ?? null;
    if (found) { chooseElement(found); reveal(found.rect, false); }
    return found;
  }
  useImperativeHandle(ref, () => ({ liveHistory: action => liveSite.current?.liveHistory(action), focusElement, addRequest: addRequestInView, focusRequest: focusRegion, askAI, undoElement: stepBackElement, copyLayer, cutLayer, pasteLayer, arrangeLayer, async synchronize(requireBounds = false) {
    await flushImages();
    if (inlineSaving.current) await inlineSaving.current;
    const text = inlineTextRef.current;
    if (text) { const node = stage.current?.querySelector<HTMLTextAreaElement>('.inline-text-editor'); await finishText(node?.value ?? text.text); }
    while (nudge.current.promise) await nudge.current.promise;
    if (elementDrag.current) cancel();
    try { await synchronizeBounds(); } catch (error) { if (requireBounds) { setElementError(String(error)); throw error; } }
  } }));
  function beginWorkspace(event: PointerEvent) {
    if (event.button === 2) { contextMenu(event); return; }
    if (event.target !== event.currentTarget || event.button !== 0) return;
    if (interactingFrame) setInteractingFrame(null);
    if (panKey.current || tool === 'pan') { startPan(event, false); return; }
    // Live mode: the page itself takes clicks; the canvas only pans or places new requests.
    if (liveView) { if (tool === 'request' && liveRequests) begin(event, 'new'); else startPan(event, false); return; }
    const at = point({ x: event.clientX, y: event.clientY });
    const hit = page.content ? site.current?.pick(at.x, at.y) : null;
    if (tool === 'request' || event.shiftKey && (!hit || hit.element.tag === 'body')) { begin(event, 'new'); return; }
    if (event.shiftKey || event.ctrlKey || event.metaKey || repicking) { startPan(event, true); return; }
    const current = elementRef.current;
    if (!panKey.current && current && !extrasRef.current.length && current.element.selector !== 'body' && at.x >= current.rect.x && at.x <= current.rect.x + current.rect.width && at.y >= current.rect.y && at.y <= current.rect.y + current.rect.height) beginElementGesture(event);
    else startPan(event, !panKey.current);
  }
  function beginElementGesture(event: PointerEvent, corner?: ResizeCorner) {
    if (event.button !== 0 || !element || editingElement || nudging || gesture.current || pan.current || elementDrag.current) return;
    if (panKey.current || tool === 'pan') { startPan(event, false); return; }
    if (tool === 'request') { begin(event, 'new'); return; }
    if (extrasRef.current.length) return;
    if ((event.shiftKey || event.ctrlKey || event.metaKey || repicking) && !corner) { startPan(event, true); return; }
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
      expectSelection(moved.element, moved.rect);
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
  function refreshElement(settled = true, revision?: number | string) {
    // A measurement from an earlier revision describes a layout the canvas has already
    // replaced; the frame loading now owns the selection.
    if (revision !== undefined && revision !== acceptedVersionRef.current) return;
    refreshLayers();
    setFrameView(page.content ? site.current?.frameOverlays() ?? { pageUrl: null, frames: [] } : { pageUrl: null, frames: [] });
    setExtras(extrasRef.current.flatMap(item => { const found = site.current?.find(item.element); return found ? [{ ...found, rect: settled ? found.rect : item.rect }] : []; }));
    for (const item of pendingImages.current) site.current?.previewImage(item.selection.element, item.src);
    setPlaceholders(page.content ? (site.current?.placeholders() ?? []).filter(item => !pendingImages.current.some(pending => sameTarget(pending.selection.element, item.element))) : []);
    refreshStaleTargets();
    const current = elementRef.current;
    if (current) {
      const found = site.current?.find(current.element);
      if (found) {
        pendingSelection.current = null;
        // The edit's own target box, or the box already drawn when there is none.
        const aimed = expectedSelection.current && sameTarget(expectedSelection.current.element, found.element) ? expectedSelection.current.rect : current.rect;
        if (settled) expectedSelection.current = null;
        storeElement({ ...found, rect: shownSelectionRect(found.rect, aimed, settled) });
      }
      else if (!dialogRef.current && (pendingSelection.current !== current.element || (site.current?.source() && !site.current?.find(current.element)))) deselectElement();
    }
    // Missing targets already have an actionable request banner. The Generate
    // path requires valid bounds and reports an error if any remain unresolved.
    void synchronizeBounds().catch(() => {});
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
    lastPointer.current = client; gesture.current.proportional = gesture.current.shapeKind === 'ellipse' && event.shiftKey; updateDrag(client);
  }
  function finish(event: PointerEvent) {
    if (elementDrag.current?.pointerId === event.pointerId) {
      updateElementDrag({ x: event.clientX, y: event.clientY }, event.altKey);
      const drag = elementDrag.current; if (!drag) return;
      let css: string | null = null;
      try {
        if (drag.active && (drag.resizer ? !sameRect(drag.rect, drag.original.rect) : drag.x || drag.y)) css = drag.resizer ? drag.resizer.style(drag.rect) : drag.mover!.style(drag.x, drag.y);
      } catch (error) { setElementError(String(error)); }
      // Retain the gesture's final pixels and bounds until the committed preview swaps in.
      const placed = elementRef.current;
      cancel(drag.active || !!drag.corner, css === null);
      if (css !== null) void applyElement({ kind: 'update', texts: [], attributes: {}, style: css }, placed ?? drag.original).catch(() => { drag.mover?.cancel(); drag.resizer?.cancel(); refreshElement(); });
      else if (!drag.active && !drag.corner) {
        finishPlainClick({ x: event.clientX, y: event.clientY }, !event.shiftKey && !event.ctrlKey && !event.metaKey && !event.altKey, event.shiftKey || event.ctrlKey || event.metaKey);
      }
      return;
    }
    if (pan.current?.pointerId === event.pointerId) {
      const current = pan.current;
      if (current.active) seek({ x: current.origin.x + (current.start.x - event.clientX) / zoom, y: current.origin.y + (current.start.y - event.clientY) / zoom });
      cancel(false);
      if (!current.active && current.click) finishPlainClick({ x: event.clientX, y: event.clientY }, !event.shiftKey && !event.ctrlKey && !event.metaKey && !event.altKey, event.shiftKey || event.ctrlKey || event.metaKey);
      else { plainClicks.current = 0; lastPlainClick.current = null; }
      return;
    }
    const current = gesture.current;
    if (!current || current.pointerId !== event.pointerId) return;
    current.proportional = current.shapeKind === 'ellipse' && event.shiftKey;
    const rect = currentRect({ x: event.clientX, y: event.clientY }, current);
    cancel();
    if (current.kind === 'new') {
      if (current.shapeKind === 'freehand') { if (current.active && rect && current.freehand) add(rect, current.freehand); }
      else if (current.active && rect && rect.width >= 1 && rect.height >= 1) add(current.shapeKind === 'rect' ? requestPlacement(rect, zoom) : rect, { kind: current.shapeKind });
      else if (!current.active && tool === 'request') { const at = requestAt(current.start, zoom); add(current.shapeKind === 'ellipse' ? { ...at, height: at.width } : at, { kind: current.shapeKind }); }
      else if (tool === 'select' && page.content) { const at = point({ x: event.clientX, y: event.clientY }); toggleElement(site.current?.pick(at.x, at.y) ?? null); }
      return;
    }
    if (current.region && rect && !sameRect(rect, current.initial)) void change(current.region.id, { rect }).catch(() => {});
  }
  function finishPlainClick(client: Point, plain: boolean, additive = false) {
    if (repicking && page.content) {
      const at = point(client); const hit = site.current?.pick(at.x, at.y) ?? null;
      if (hit && hit.element.selector !== 'body' && repick) { const id = repicking; setRepicking(null); void repick(id, hit).catch(error => setElementError(error instanceof Error ? error.message : String(error))); }
      return;
    }
    if (additive && page.content) { const at = point(client); toggleElement(site.current?.pick(at.x, at.y) ?? null); plainClicks.current = 0; lastPlainClick.current = null; return; }
    const at = point(client);
    const area = [...page.regions].sort((a,b) => b.zIndex-a.zIndex).find(region => !region.completed && region.shape && region.shape.kind !== 'rect' && pointInRegion(at, region.rect, region.shape));
    if (area) { deselectElement(); select(area.id); return; }
    chooseElement(page.content ? site.current?.pick(at.x, at.y) ?? null : null);
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
    if (page.content) {
      const hit = site.current?.pick(at.x, at.y);
      if (hit && hit.element.tag !== 'body') {
        // The second click on an already selected frame starts interacting with it,
        // the same place the toolbar's Interact button goes.
        const current = elementRef.current;
        if (hit.element.tag === 'iframe' && current && sameTarget(current.element, hit.element)) {
          const frame = liveOverlays.find(item => item.selector === hit.element.selector);
          if (frame) { setInteractingFrame(value => value === frame.key ? null : frame.key); return; }
        }
        chooseElement(hit); void editTextAt(at); return;
      }
    }
    const snapped = geometryForGesture({ kind: 'new', start: at, initial: { ...at, width: 0, height: 0 } }, at, page.viewport, gridSettings);
    add(requestAt({ x: snapped.x, y: snapped.y }, zoom));
  }
  function activate(event: ReactMouseEvent) {
    if (liveView) return;
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
    if (event.key === 'Escape') { setRepicking(null); setInteractingFrame(null); event.preventDefault(); event.stopPropagation(); panKey.current = false; setPanReady(false); if (!gesture.current && !pan.current && !elementDrag.current) { select(null); deselectElement(); } cancel(); return; }
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
      if (extrasRef.current.length) { if (event.key === 'Delete' || event.key === 'Backspace') { event.preventDefault(); void deleteSelected().catch(error => setElementError(String(error))); } return; }
      if (event.key === 'F2' && !nudge.current.promise && !editingElement) { event.preventDefault(); setElementDialog('edit'); return; }
      if ((event.key === 'Delete' || event.key === 'Backspace') && !nudge.current.promise && !editingElement) { event.preventDefault(); deleteElement(); return; }
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
    setNudging(true);
    void change(region.id, { rect: keyboardRectChange(direction, page.viewport, gridSettings, resizing, event.altKey, region.shape && region.shape.kind !== 'rect' ? { x: 1, y: 1 } : minimum) }).then(updated => reveal(updated.rect, resizing)).catch(() => {}).finally(() => setNudging(false));
  }

  useLayoutEffect(() => {
    const node = toolbar.current; if (!node) return;
    const measure = () => { const rect = node.getBoundingClientRect(); if (!rect.width || !rect.height) return; setToolbarSize(current => current.width === rect.width && current.height === rect.height ? current : { width: rect.width, height: rect.height }); };
    measure(); const observer = new ResizeObserver(measure); observer.observe(node); return () => observer.disconnect();
  }, [!!element, !!selected, !!inlineText]);
  const visible = { x: bounds.left + measurements.left / zoom, y: bounds.top + measurements.top / zoom };
  // Frames on screen get a live overlay; the rest wait until the canvas is scrolled to them.
  const frameViewport = { x: visible.x, y: visible.y, width: measurements.width / zoom, height: measurements.height / zoom };
  const visibleFrames = useMemo(() => page.content && !liveView ? placeOverlays(frameView.frames, frameViewport) : [],
    [frameView, frameViewport.x, frameViewport.y, frameViewport.width, frameViewport.height, liveView, page.content]);
  const frameSession = useLiveFrameSession(page.id, acceptedVersion, visibleFrames.length > 0);
  const liveOverlays = frameSession ? visibleFrames : [];
  const overlaySelection = element?.element.tag === 'iframe' && !extras.length ? liveOverlays.find(frame => frame.selector === element.element.selector) ?? null : null;
  const interacting = !!overlaySelection && interactingFrame === overlaySelection.key;
  const overlayKeys = liveOverlays.map(frame => frame.key).join('|');
  useEffect(() => { if (interactingFrame && !(tool === 'select' && interacting)) setInteractingFrame(null); }, [interactingFrame, tool, interacting]);
  // Hide the still copy of every frame the live overlay covers, and show it again when
  // the overlay goes away (live mode, a new revision, or the frame leaves the page).
  useEffect(() => {
    site.current?.hideStaticFrames(liveOverlays.map(frame => frame.key));
  }, [frameView, frameSession?.url, overlayKeys, acceptedVersion]);
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
  const selectedRequest = page.regions.find(item => item.id === selected && !item.completed);
  const shapedPrompt = selectedRequest?.shape && selectedRequest.shape.kind !== 'rect' ? inViewport({ ...requestPromptRect(preview ?? selectedRequest.rect, zoom), height: Math.max(160, measurements.height * .55) / zoom }) : null;
  const occupiedInViewport = [...occupied.map(inViewport), ...(shapedPrompt ? [shapedPrompt] : [])];
  const toolbarRect = element ? unionRect([element.rect, ...extras.map(item => item.rect)]) : selectedRequest?.rect;
  const toolbarPoint = toolbarRect ? elementToolbarPosition(inViewport(toolbarRect), { x: toolbarSize.width, y: toolbarSize.height }, { x: inset.x, y: inset.y, width: measurements.width, height: measurements.height }, occupiedInViewport) : null;
  const toolbarPosition = toolbarPoint ? { left: toolbarPoint.x, top: toolbarPoint.y } : {};
  const measuring = !!gesture.current?.active || !!elementDrag.current?.active || nudging;
  // Read actual text-box, handle, menu and label bounds after layout, including detached
  // prompts and expanded text. Toolbar placement permits collisions; labels never do.
  useLayoutEffect(() => {
    let position: Point | null = null;
    if (measuring && selectedRect && measurement.current && scroll.current) {
      const root = scroll.current.getBoundingClientRect();
      const local = (node: Element): Rect => {
        const rect = node.getBoundingClientRect();
        return { x: rect.left - root.left + inset.x, y: rect.top - root.top + inset.y, width: rect.width, height: rect.height };
      };
      const label = local(measurement.current);
      const obstacles = [...scroll.current.querySelectorAll('.annotation, .annotation-grip, .element-resize-corner'), ...(toolbar.current ? [toolbar.current] : [])]
        .filter(node => node.getClientRects().length && getComputedStyle(node).visibility !== 'hidden').map(local);
      position = selectionLabelPosition(inViewport(selectedRect), { x: label.width, y: label.height },
        { x: inset.x, y: inset.y, width: measurements.width, height: measurements.height }, obstacles);
    }
    setMeasurementPosition(previous => previous?.x === position?.x && previous?.y === position?.y ? previous : position);
  });
  const px = (value: number) => `${Math.round(value * 10) / 10}`;

  const editBusy = layerBusy || editingElement || nudging;
  // Requests line up with the design only at the page's own width, so they show (and can be added) there.
  const liveRequests = !liveView || liveView.width === page.viewport.width;
  const liveHeight = liveView ? Math.max(liveView.height, liveView.width === page.viewport.width ? previewHeight : liveView.height) : 0;
  nativeDrag.current = value => {
    if (value.phase === 'leave') { setDropTarget(null); return; }
    if (value.phase === 'drop') {
      const pending = nativeDrop.current; if (!pending || pending.id !== value.id) return;
      nativeDrop.current = null; setDropTarget(null);
      if (pending.pageId !== pageRef.current.id || pending.source !== site.current?.source()) return;
      if (pending.regionId && attach) {
        if (value.files.length) attach(pending.regionId, value.files.map(file => new File([Uint8Array.from(atob(file.data), ch => ch.charCodeAt(0))], file.name, { type: file.mime })), []);
        if (value.fileErrors.length) setElementError(value.fileErrors.join(' '));
        else if (!value.files.length) setElementError(value.message ?? 'The files could not be attached.');
        return;
      }
      if (value.message) { setElementError(value.message); return; }
      if (!pending.selection) { setElementError('Drop the picture onto an image on the page.'); return; }
      if (value.picture) {
        const picture = value.picture; const bytes = Uint8Array.from(atob(picture.data), character => character.charCodeAt(0));
        chooseElement(pending.selection); void stageImage(pending.selection, new File([bytes], picture.name, { type: picture.mime }));
      }
      return;
    }
    const at = value.position; const visible = scroll.current?.getBoundingClientRect();
    const allowed = at && visible && at.x >= visible.left && at.x <= visible.right && at.y >= visible.top && at.y <= visible.bottom && !editBusy;
    const world = allowed ? point(at) : null;
    const hit = world && page.content ? site.current?.imageAt(world.x, world.y) ?? null : null;
    setDropTarget(hit?.rect ?? null);
    if (value.phase === 'pending') nativeDrop.current = { id: value.id, pageId: page.id, source: site.current?.source() ?? null, selection: hit, regionId: allowed ? document.elementFromPoint(at!.x, at!.y)?.closest<HTMLElement>('[data-region-id]')?.dataset.regionId : undefined };
  };

  const foundUrl = element ? site.current?.foundSource(element.element) ?? null : null;
  const foundHost = (() => { try { return foundUrl ? new URL(foundUrl).hostname.replace(/^www\./, '') : ''; } catch { return ''; } })();
  const arrangeItems: MenuItem[] = (['front', 'forward', 'backward', 'back'] as const).map((direction, index) => ({ id: direction, label: ['Bring to front', 'Bring forward', 'Send backward', 'Send to back'][index], disabled: editBusy, action: () => { void arrangeLayer(direction); } }));
  const contextItems: MenuItem[] = context ? [
    ...(context.kind === 'element' ? [
      { id: 'edit', label: 'Edit', disabled: editBusy, action: () => setElementDialog('edit') },
      { id: 'generate', label: 'Ask AI', disabled: editBusy, action: askAI },
      ...(fixOnPhones && elementRef.current && elementRef.current.element.selector !== 'body' ? [{ id: 'phones', label: 'Fix on phones', disabled: editBusy, action: () => { const current = elementRef.current; if (current) fixOnPhones(current); } }] : []),
      { id: 'replace', label: 'Replace', disabled: editBusy, action: () => setElementDialog('replace') },
      ...(undoElement && elementRef.current && elementRef.current.element.selector !== 'body' ? [{ id: 'undo-element', label: 'Undo changes to this element', shortcut: 'Alt+Z', disabled: editBusy, action: () => { void stepBackElement(); } }] : []),
    ] : context.kind === 'request' ? [{ id: 'edit', label: 'Edit request', disabled: editBusy, action: () => focusRegion(context.regionId) }] : [{ id: 'add', label: 'Add request here', action: () => { deselectElement(); add(requestAt(context.at, zoom)); } }]),
    ...(context.kind === 'request' ? [{ id: 'whole-page', label: 'Whole page', checked: page.regions.some(item => item.id === context.regionId && isWholePage(item, page)), action: () => { const region = page.regions.find(item => item.id === context.regionId); if (region) void change(region.id, { wholePage: !isWholePage(region, page) }).catch(() => {}); } }] : []),
    ...(context.kind === 'request' && regionTargets(page.regions.find(item => item.id === context.regionId)).length ? [{ id: 'allow-target-removal', label: 'Allow AI to remove selected items', checked: !!page.regions.find(item => item.id === context.regionId)?.allowTargetRemoval, action: () => setTargetRemoval(context.regionId, !page.regions.find(item => item.id === context.regionId)?.allowTargetRemoval) }] : []),
    ...(context.kind !== 'canvas' ? [
      { id: 'copy', label: 'Copy', disabled: editBusy, shortcut: 'Ctrl/Cmd+C', action: () => { void copyLayer(); } },
      { id: 'cut', label: 'Cut', disabled: editBusy, shortcut: 'Ctrl/Cmd+X', action: () => { void cutLayer(); } },
    ] : []),
    { id: 'paste', label: 'Paste', disabled: editBusy, shortcut: 'Ctrl/Cmd+V', action: () => { void pasteLayer(); } },
    ...(context.kind !== 'canvas' ? [
      { id: 'arrange', label: 'Arrange', disabled: editBusy, children: arrangeItems },
      ...(context.kind === 'element' ? [{ id: 'parent', label: 'Select parent', action: () => chooseElement(site.current?.next(true, elementRef.current?.element) ?? null) }] : []),
      { id: 'delete', label: 'Delete', disabled: editBusy, action: () => { if (context.kind === 'element') deleteElement(); else if (context.kind === 'request') remove(context.regionId); } },
    ] : [
      { id: 'fit', label: 'Fit to screen', action: () => show('fit') },
      { id: 'grid', label: 'Show grid', checked: grid, action: toggleGrid },
    ]),
    { id: 'layers', label: 'Layers', action: () => setLayersOpen(true) },
  ] : [];
  return <div className={`free-workspace ${page.appearance} tool-${tool} ${gridSettings.rulers && !liveView ? 'with-rulers' : ''} ${panReady ? 'pan-ready' : ''} ${panning ? 'panning' : ''} ${repicking ? 'repicking' : ''} ${liveView ? 'live' : ''}`}
    onKeyDownCapture={event => { if (!(event.target instanceof Element && event.target.closest('.workspace-surface')) && event.code === 'Space' && !isTextInput(event.target) && !(event.target instanceof HTMLElement && event.target.closest('button, a, [role=button], .element-toolbar, .editor-menu')) && !event.ctrlKey && !event.metaKey && !event.altKey) { event.preventDefault(); panKey.current = true; setPanReady(true); } }}>
    <p className="sr-only" id="canvas-keyboard-help">Click a website element to select it. Shift, Control or Command click adds or removes a selection, up to 8 elements. Drag the selected element to move it; drag a corner to resize it. Double-click text to edit it in place. Enter saves text; Escape cancels. Ask AI in the right-click menu, or Ctrl or Command K, opens a change request. Double-click empty canvas to add a request. Select, Add request and Pan tools are in the floating canvas toolbar, with V, R and H shortcuts. In Add request, click to place a text box or drag to draw an area. Left-drag on empty space pans; Space or middle drag pans anywhere. Shift+drag draws an area. Ctrl or Command with the wheel zooms under the pointer. F2 opens local element settings. Arrow keys move an element one pixel; Shift moves ten. Enter selects the next element; Alt+Enter selects its parent. Shift+Enter adds a request in view. Escape from a request focuses its move grip. Arrow keys on grips move or resize by the grid setting. Hold Alt for one-pixel changes. Delete on a grip removes its request. Home returns to the screen guide. Escape cancels a drag.</p>
    {(() => {
      const key = `page:${generation?.job?.jobId ?? 'start'}:${page.id}`;
      const failed = (!problems.length && generation?.error) || (generation?.job && ['failed', 'unknown', 'blocked'].includes(generation.job.status) && !problems.length && !generation.job.drafts?.some(d => d.applied || d.dismissed) ? generationPageError(generation.job.message) : '');
      if (runtimeProblems.length && !dismissedProblems.includes(`runtime:${key}`)) return <ProblemBanner message={runtimeError(runtimeProblems[0])} action={{ label: 'Fix with AI', disabled: aiDisabled, onClick: () => fixRuntimeErrors?.(runtimeProblems) }} dismiss={() => dismissProblem(`runtime:${key}`)} />;
      if (!failed || dismissedProblems.includes(key)) return null;
      return <ProblemBanner message={userError(failed)} action={{ label: 'Try again', disabled: aiDisabled, onClick: () => { void generation?.prepare().catch(() => {}); } }} dismiss={() => dismissProblem(key)} />;
    })()}
    {gridSettings.rulers && !liveView && <><div className="ruler-corner" aria-hidden="true">{rulerUnit === 'percent' ? '%' : 'px'}</div>
      <Ruler axis="x" unit={rulerUnit} start={visible.x} length={measurements.width} reference={page.viewport.width} zoom={zoom} cursor={cursor?.x} selection={selectedRect ? [selectedRect.x, selectedRect.x + selectedRect.width] : undefined} />
      <Ruler axis="y" unit={rulerUnit} start={visible.y} length={measurements.height} reference={page.viewport.height} zoom={zoom} cursor={cursor?.y} selection={selectedRect ? [selectedRect.y, selectedRect.y + selectedRect.height] : undefined} /></>}
    <div ref={scroll} className="workspace-scroll" data-testid="workspace-scroll" onScroll={scrolled} onPointerDownCapture={beginPan}>
      <div className="canvas-size" style={{ width: (bounds.right - bounds.left) * zoom, height: (bounds.bottom - bounds.top) * zoom }}>
        <div id="page-canvas" ref={stage} data-testid="canvas" data-world-left={bounds.left} data-world-top={bounds.top} data-zoom={zoom} className={`canvas ${page.appearance} ${overElement ? 'element-hover' : ''} ${elementDrag.current ? 'element-transforming' : ''}`} tabIndex={0} role="group" aria-label="Page workspace. Click to select, double-click text to edit. Double-click empty canvas to add a request. Drag selected elements to move; drag corners to resize." aria-describedby="canvas-keyboard-help"
          style={{ width: (bounds.right - bounds.left) * zoom, height: (bounds.bottom - bounds.top) * zoom }}
          onPointerDown={beginWorkspace} onPointerMove={move} onPointerUp={finish} onDoubleClick={activate} onContextMenu={contextMenu}
          onDragOver={event => {
            if (!event.dataTransfer.types.includes('Files')) return;
            event.preventDefault();
            const at = point({ x: event.clientX, y: event.clientY });
            const hit = page.content && !editBusy ? site.current?.imageAt(at.x, at.y) ?? null : null;
            event.dataTransfer.dropEffect = hit ? 'copy' : 'none';
            setDropTarget(current => hit && current && sameRect(current, hit.rect) ? current : hit?.rect ?? null);
          }}
          onDragLeave={event => { if (!(event.relatedTarget instanceof Node && event.currentTarget.contains(event.relatedTarget))) setDropTarget(null); }}
          onDrop={event => {
            if (!event.dataTransfer.files.length) return;
            event.preventDefault(); setDropTarget(null);
            const file = Array.from(event.dataTransfer.files).find(item => item.type.startsWith('image/'));
            const at = point({ x: event.clientX, y: event.clientY });
            const hit = page.content ? site.current?.imageAt(at.x, at.y) ?? null : null;
            if (!file) setElementError('Drop a picture file (PNG, JPEG, WebP or GIF).');
            else if (!hit) setElementError('Drop the picture onto an image on the page.');
            else { chooseElement(hit); void stageImage(hit, file); }
          }}
          onPointerLeave={() => { if (!gesture.current) setCursor(null); }} onPointerCancel={event => { if (gesture.current?.pointerId === event.pointerId || pan.current?.pointerId === event.pointerId || elementDrag.current?.pointerId === event.pointerId) cancel(); }}
          onLostPointerCapture={event => { if (gesture.current?.pointerId === event.pointerId || pan.current?.pointerId === event.pointerId || elementDrag.current?.pointerId === event.pointerId) cancel(); }}
          onKeyDown={event => { if (event.target === event.currentTarget || event.key === 'Escape') keyboard(event); }}>
          {page.content && <div className="accepted-canvas-preview" data-testid="accepted-canvas-preview" style={positioned({ x: 0, y: 0, width: page.viewport.width, height: previewHeight })} aria-hidden="true">
            <div className="accepted-canvas-content" style={{ width: page.viewport.width, height: previewHeight, transform: `scale(${zoom})` }}>
              <PreviewFrame ref={site} content={page.content} referenceHeight={page.viewport.height} onHeight={setSiteHeight} onReady={(settled, revision) => refreshElement(settled, revision)} pageId={page.id} revision={acceptedVersion} title={`${page.name} accepted static preview`} staticView />
            </div>
          </div>}
          {liveView && page.content && <div className={`live-canvas-page${tool === 'request' || panReady || panning ? ' passive' : ''}`} data-testid="live-canvas-page" style={positioned({ x: 0, y: 0, width: liveView.width, height: liveHeight })}>
            <div className="live-canvas-content" style={{ width: liveView.width, height: liveHeight, transform: `scale(${zoom})` }}>
              <PreviewFrame ref={liveSite} onLiveLocation={onLiveLocation} onRuntimeProblems={setRuntimeProblems} fixRuntimeErrors={fixRuntimeErrors} aiDisabled={aiDisabled} observationsVersion={observationsVersion} pageId={page.id} revision={`live:${acceptedVersion}`} runScripts title={`${page.name} live`} />
            </div>
          </div>}
          {!liveView && frameSession && liveOverlays.map(frame => <FrameOverlay key={frame.key} frame={frame} live={frameSession} pageId={page.id} staticPageUrl={frameView.pageUrl} zoom={zoom}
            interactive={tool === 'select' && interactingFrame === frame.key && !!element && element.element.selector === frame.selector}
            placement={{ ...overlayPlacement(frame.rect, origin, zoom), zIndex: 1 }} title={`${page.name} interactive frame`} stop={() => setInteractingFrame(null)} />)}
          <div className={`screen-guide ${page.content ? 'has-preview' : ''}`} data-testid="screen-guide" style={positioned({ x: 0, y: 0, ...page.viewport })} aria-hidden="true">
            <span className="screen-guide-label">Screen · {page.viewport.width} × {page.viewport.height}</span>
          </div>
          {grid && !liveView && <div className="canvas-grid" aria-hidden="true" style={{ ...positioned({ x: 0, y: 0, width: page.viewport.width, height: Math.max(page.viewport.height, page.content ? previewHeight : 0) }), backgroundSize: `${displayStep.x * zoom}px ${displayStep.y * zoom}px`, backgroundPosition: '0 0' }} />}
          {!page.content && !page.regions.some(region => !region.completed) && <div className="canvas-empty-hint" style={positioned({ x: 0, y: 0, ...page.viewport })}>
            <strong>This page is empty</strong><button type="button" className="primary" onClick={addRequestInView}>Add request</button>
          </div>}
          {liveRequests && page.regions.map((region, index) => {
            if (region.completed) return null;
            const rect = gesture.current?.region?.id === region.id && preview ? preview : region.rect;
            return <RequestBox reports={archiveReports[`${projectId}:${region.id}`] ?? []} key={region.id} wholePage={isWholePage(region, page)} region={region} number={index + 1} rect={rect} bounds={bounds} zoom={zoom} maxHeight={Math.max(160, measurements.height * 0.55)} selected={selected === region.id} focus={focusRequest === region.id} focusToken={requestFocus.id === region.id ? requestFocus.serial : 0}
              layer={requestRanks.get(region.id)!} controlsLayer={overlayLayer + 1} stale={stale.has(region.id)} repickNow={() => { deselectElement(); setRepicking(region.id); }} removeNow={() => remove(region.id)} attach={attach ? (files, links) => attach(region.id, files, links) : undefined} detach={detach ? id => detach(region.id, id) : undefined} select={() => { deselectElement(); select(region.id); }} begin={begin} keyboard={keyboard} change={patch => change(region.id, patch)} />;
          })}
          {element && !liveView && <div className="element-selection direct-selection" data-testid="direct-element-selection" data-selector={element.element.selector} data-element-id={element.element.id} data-source-hash={element.element.sourceSha256} style={{ ...paintStyle(element.rect, bounds, zoom), zIndex: overlayLayer + 1 }} role="group" aria-label="Selected element bounds">
            <span className="element-type-tag">{elementName(element.element.tag)}{foundUrl && <span className="found-tag" title={foundUrl}> · Found online · {foundHost}</span>}</span>
            {!extras.length && element.element.selector !== 'body' && (['nw','ne','sw','se'] as const).map(corner => <button key={corner} type="button" className={`element-resize-corner corner-${corner}`} data-corner={corner} aria-label={`Resize ${corner.includes('n') ? 'top' : 'bottom'}-${corner.includes('w') ? 'left' : 'right'}`} title="Drag to resize; press Enter for size settings" disabled={editingElement || nudging} onPointerDown={event => beginElementGesture(event, corner)} onClick={event => { if (event.detail === 0) setElementDialog('edit'); }} />)}
          </div>}
          {extras.map(item => <div key={item.element.id ?? item.element.selector} data-testid="multi-element-selection" data-element-id={item.element.id} className="element-selection multi-selection" style={{ ...paintStyle(item.rect, bounds, zoom), zIndex: overlayLayer + 1 }} aria-hidden="true" />)}
          {inlineText && <InlineTextEditor key={`${inlineText.selection.element.sourceSha256}:${inlineText.index}`} text={inlineText} origin={origin} zoom={zoom} layer={overlayLayer + 4} disabled={editingElement} finish={finishText} />}
          {placeCallouts(problems, page.regions.map(region => region.rect), origin, zoom, dismissedProblems).map(position => {
            const problem = problems.find(item => item.key === position.key)!;
            const error = problemErrors[problem.key];
            const busy = problemBusy || !!generation?.operation || generation?.canStop;
            return <div key={problem.key} role="alert" className="area-problem" style={{ left: position.x, top: position.y, width: position.width, height: position.height, zIndex: overlayLayer + 5 }} onPointerDown={event => event.stopPropagation()}>
              <span>{error ? generationError(error, page.regions.find(r => r.id === problem.regionId)?.prompt) : problem.message}</span>
              <div><button disabled={busy || !problem.draftId || !!error} onClick={() => { void applyProblem(problem.regionId, problem.draftId!, problem.key); }}>Apply anyway</button>
              <button disabled={busy || aiDisabled} onClick={() => { void generation?.prepare(undefined, [problem.regionId]).catch(() => {}); }}>Try again</button>
              {!problem.stale && !error && <button disabled={busy} onClick={() => { select(problem.regionId); setRequestFocus(previous => ({ id: problem.regionId, serial: previous.serial + 1 })); }}>Edit request</button>}
              <button aria-label="Dismiss problem" onClick={() => dismissProblem(problem.key)}>×</button></div>
            </div>;
          })}
          {changing.map(({ rect, shape }, index) => shape && shape.kind !== 'rect' ? <RegionOutline key={index} shape={shape} rect={rect} bounds={bounds} zoom={zoom} layer={overlayLayer + 2} draft /> : <div key={index} className="generation-blur" data-testid="generation-blur" style={{ ...paintStyle(rect, bounds, zoom), zIndex: overlayLayer + 2 }} aria-hidden="true" />)}
          {!editBusy && !liveView && placeholders.map(item => <button key={item.element.selector} type="button" className="image-drop-target" style={{ ...paintStyle(item.rect, bounds, zoom), zIndex: overlayLayer + 1 }}
            onPointerDown={event => event.stopPropagation()} onClick={event => { if (event.shiftKey || event.ctrlKey || event.metaKey) toggleElement(item); else if (repicking && repick) { const id = repicking; setRepicking(null); void repick(id, item).catch(error => setElementError(String(error))); } else { chooseElement(item); browseImage(item); } }} title="Drop a picture here, or click to choose one">
            <strong>Drop a picture here</strong><span>or click to browse</span></button>)}
          {dropTarget && <div className="image-drop-highlight" style={{ ...paintStyle(dropTarget, bounds, zoom), zIndex: overlayLayer + 3 }} aria-hidden="true"><span>Drop to use this picture</span></div>}
          {highlighted.map(({ rect, shape }, index) => shape && shape.kind !== 'rect' ? <RegionOutline key={index} shape={shape} rect={rect} bounds={bounds} zoom={zoom} layer={overlayLayer + 2} selected /> : <div key={index} className="generation-highlight" data-testid="generation-highlight" style={{ ...paintStyle(rect, bounds, zoom), zIndex: overlayLayer + 2 }} aria-hidden="true" />)}
          {guides.x.map(x => <div key={`x${x}`} className="alignment-guide alignment-x" aria-hidden="true" style={{ left: (x - bounds.left) * zoom, zIndex: overlayLayer + 1 }} />)}
          {guides.y.map(y => <div key={`y${y}`} className="alignment-guide alignment-y" aria-hidden="true" style={{ top: (y - bounds.top) * zoom, zIndex: overlayLayer + 1 }} />)}
          {preview && gesture.current?.kind === 'new' && (gesture.current.shapeKind === 'rect' ? <div className="annotation-draft" style={{ ...paintStyle(preview, bounds, zoom), zIndex: overlayLayer }} /> : <RegionOutline shape={draftShape} rect={preview} bounds={bounds} zoom={zoom} layer={overlayLayer} draft />)}
        </div>
      </div>
    </div>
    {toolbarRect && !inlineText && !(liveView && element) && <div ref={toolbar} className="element-toolbar" role="toolbar" aria-label={element ? 'Selected element' : 'Selected request'} style={toolbarPosition}>
      {confirmMany ? <div className="delete-inline" role="group" aria-label="Delete items">
        <span>Delete {confirmMany.count} items?</span>
        <button type="button" className="danger" onClick={() => { if (confirmMany.skip) setAskBeforeDelete(false); setConfirmMany(null); void (confirmMany.pageId === page.id ? deleteSelected(true, confirmMany.targets) : Promise.resolve()).catch(error => setElementError(error instanceof Error ? error.message : String(error))); }}>Delete</button>
        <button type="button" onClick={() => setConfirmMany(null)}>Cancel</button>
        <details><summary>Options</summary><label className="check-label"><input type="checkbox" checked={confirmMany.skip} onChange={event => setConfirmMany({ ...confirmMany, skip: event.target.checked })} />Don’t ask again</label></details>
      </div> : <>
      {!element && selectedRequest && <button className="ask-ai" aria-pressed={isWholePage(selectedRequest, page)} disabled={editBusy} title="Allow this request to change the page layout, CSS and JavaScript" onClick={() => { void change(selectedRequest.id, { wholePage: !isWholePage(selectedRequest, page) }).catch(() => {}); }}>Whole page</button>}
      {extras.length > 0 ? <>
        <span className="multi-count">{extras.length + 1} selected</span>
        <button className="ask-ai" disabled={editBusy} onClick={askAI}>Ask AI</button>
        <button aria-label="Delete selected items" title="Delete all selected items" disabled={editBusy} onClick={() => { void deleteSelected().catch(error => setElementError(error instanceof Error ? error.message : String(error))); }}><QuickIcon name="delete" /></button>
      </> : <>
      {element?.element.tag === 'img' ? <>
        <button className="ask-ai" title="Choose a picture from your computer, or drop one onto the image" disabled={editBusy} onClick={() => browseImage(elementRef.current)}>Replace image</button>
        <button className="ask-ai" title="Describe a change for AI · Ctrl/⌘ K" disabled={editBusy} onClick={askAI}>Ask AI</button>
      </> : element && <>
        {element.element.tag === 'iframe' && overlaySelection && <button className="ask-ai" aria-pressed={interacting} title={interacting ? 'Stop playing this frame' : 'Play this frame in place'}
          onClick={() => setInteractingFrame(value => value === overlaySelection.key ? null : overlaySelection.key)}>{interacting ? 'Done' : 'Interact'}</button>}
        <button className="ask-ai" title="Change text, links and style · F2" disabled={editBusy} onClick={() => setElementDialog('edit')}>Edit</button>
        <button className="ask-ai" title="Describe a change for AI · Ctrl/⌘ K" disabled={editBusy} onClick={askAI}>Ask AI</button>
        {element.element.selector !== 'body' && <button className="ask-ai" aria-pressed={styleOpen} title="Show or hide quick style controls" onClick={toggleStyle}>Style</button>}
      </>}
      {element && foundUrl && openSource && <button className="ask-ai" title={`Open where this came from: ${foundUrl}`} onClick={() => openSource(element.element)}>Found online{foundHost ? ` · ${foundHost}` : ''} ↗</button>}
      {element && undoElement && element.element.selector !== 'body' && <button aria-label="Undo changes to this element" title="Step this element back to its previous version · Alt+Z" disabled={editBusy} onClick={() => { void stepBackElement(); }}><QuickIcon name="undo" /></button>}
      <button aria-label="Delete layer" title="Delete" disabled={editBusy || element?.element.selector === 'body'} onClick={() => { if (element) deleteElement(); else if (selected) remove(selected); }}><QuickIcon name="delete" /></button>
      {element && styleOpen && !layersOpen && element.element.tag !== 'img' && element.element.selector !== 'body' && <StyleBar key={element.element.selector} visual={site.current?.visual(element.element) ?? null} busy={editBusy} apply={change => { void applyStyles(change); }} />}
      </>}
      </>}
    </div>}
    {context && <ContextMenu key={`${context.kind}:${context.point.x}:${context.point.y}`} label={context.kind === 'element' ? 'Element actions' : context.kind === 'request' ? 'Request actions' : 'Canvas actions'} point={context.point} close={closeContext} items={contextItems} />}
    <LayersPopover open={layersOpen} page={page} siteLayers={siteLayers} selectedElement={element?.element.selector ?? null} selectedRequest={selected} busy={layerBusy || editingElement || nudging} ready={layersReady} anchor={toolbarPoint ? { x: toolbarPoint.x, y: toolbarPoint.y, height: toolbarSize.height } : null} contextElement={openElementContext} contextRequest={openRequestContext}
      chooseElement={item => { chooseElement(item); reveal(item.rect, false); }} chooseRequest={item => { deselectElement(); select(item.id); reveal(item.rect, false); }}
      arrange={direction => { void arrangeLayer(direction); }} copy={() => { void copyLayer(); }} paste={() => { void pasteLayer(); }} close={() => setLayersOpen(false)} />
    <input ref={imagePicker} type="file" hidden accept="image/png,image/jpeg,image/webp,image/gif" onChange={event => { const file = event.target.files?.[0]; const target = browseFor.current; event.target.value = ''; if (file && target) void stageImage(target, file); }} />
    {repicking && <div className="repick-hint" role="status">Click the element this request is for. <button type="button" onClick={() => setRepicking(null)}>Cancel</button></div>}

    {elementError && <ProblemBanner className="element-edit-error" message={userError(elementError)} dismiss={() => setElementError('')} />}
    {elementDialog && element && <ElementDialog projectImages={projectImages} mode={elementDialog} target={element.element} visual={site.current?.visual(element.element) ?? null} inspect={inspectElement} apply={change => applyElement(change)} move={moveElement} close={() => { setElementDialog(null); if (elementRef.current && !site.current?.find(elementRef.current.element) && !pendingSelection.current) deselectElement(); }} />}
    {measuring && selectedRect && coordinate && gridSettings.rulers && !liveView && <div ref={measurement} className="workspace-measurement" aria-label="Placement in pixels" title={`${percent(coordinate.x, page.viewport.width)} · ${percent(coordinate.y, page.viewport.height)} · ${percent(selectedRect.width, page.viewport.width)} × ${percent(selectedRect.height, page.viewport.height)}`}
      style={{ left: measurementPosition?.x ?? 0, top: measurementPosition?.y ?? 0, visibility: measurementPosition ? undefined : 'hidden' }}>
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

function RequestBox({ reports, wholePage, region, number, rect, bounds, zoom, maxHeight, selected, focus, focusToken, layer, controlsLayer, select, begin, keyboard, change, stale = false, repickNow, removeNow, attach, detach }: {
  reports: import('./types').ArchiveReport[]; wholePage: boolean;
  stale?: boolean; repickNow?: () => void; removeNow?: () => void; attach?: (files: File[], links: string[]) => void; detach?: (attachmentId: string) => void;
  region: Region; number: number; rect: Rect; bounds: WorkspaceBounds; zoom: number; maxHeight: number; selected: boolean; focus: boolean; focusToken: number; layer: number; controlsLayer: number; select: () => void;
  begin: (event: PointerEvent, kind: Gesture['kind'], region?: Region) => void;
  keyboard: (event: KeyboardEvent, region?: Region, resize?: boolean) => void; change: (patch: RequestPatch) => Promise<Region>;
}) {
  const text = useRef<HTMLTextAreaElement>(null);
  const move = useRef<HTMLButtonElement>(null);
  const [draft, changeDraft] = useDraftText(region.prompt, (_value, latest) => change({ prompt: latest }).then(updated => updated.prompt));
  const [expanded, setExpanded] = useState(false);
  const [dropping, setDropping] = useState(false);
  const [attachmentsExpanded, setAttachmentsExpanded] = useState(false);
  const attachInput = useRef<HTMLInputElement>(null);
  const [naturalHeight, setNaturalHeight] = useState(0);
  const focusedToken = useRef(0);
  useLayoutEffect(() => {
    const requested = focus || focusToken !== focusedToken.current && focusToken > 0; focusedToken.current = focusToken;
    if (!requested) return;
    text.current?.focus({ preventScroll: true });
  }, [focus, focusToken]);
  const attachments = region.attachments ?? [];
  const targets = regionTargets(region);
  const target = targets.length > 0;
  const shaped = !target && !!region.shape && region.shape.kind !== 'rect';
  const promptRect = requestPlacement(target || shaped ? requestPromptRect(rect, zoom) : rect, zoom);
  useLayoutEffect(() => {
    const node = text.current; if (!node) return;
    const old = node.style.height; node.style.height = '0px'; const style = getComputedStyle(node);
    const height = node.scrollHeight + (Number.parseFloat(style.borderTopWidth) || 0) + (Number.parseFloat(style.borderBottomWidth) || 0); node.style.height = old;
    setNaturalHeight(current => current === height ? current : height);
  }, [draft, promptRect.width, zoom]);
  const minimumHeight = promptRect.height * zoom;
  // Text keeps its own height; attachments and actions add normal-flow rows below it.
  const collapseHeight = target || shaped ? Math.max(minimumHeight, maxHeight) : minimumHeight;
  const fullHeight = Math.max(minimumHeight, naturalHeight);
  const hasMore = fullHeight > collapseHeight;
  useLayoutEffect(() => { if (target && hasMore && document.activeElement === text.current) setExpanded(true); }, [target, hasMore, draft]);
  const displayHeight = expanded ? fullHeight : Math.min(collapseHeight, fullHeight);
  const shown = { ...promptRect, height: (displayHeight + (hasMore ? 30 : 0)) / zoom };
  // Only placement controls are clipped to the workspace. The card grows below
  // its text without changing the area's geometry or clipping wrapped attachments.
  const painted = clipToWorkspace(shown, bounds);
  const at = (x: number, y: number) => x >= bounds.left && x <= bounds.right && y >= bounds.top && y <= bounds.bottom;
  const moveVisible = at(rect.x, rect.y);
  const resizeVisible = at(rect.x + rect.width, rect.y + rect.height);
  return <>
  {shaped && <RegionOutline shape={region.shape!} rect={rect} bounds={bounds} zoom={zoom} layer={layer} selected={selected} regionId={region.id} onPointerDown={event => { if (event.button !== 0) return; event.preventDefault(); event.stopPropagation(); select(); }} />}
  {selected && !target && !shaped && <div className="element-selection request-placement" style={{ ...paintStyle(rect, bounds, zoom), zIndex: controlsLayer }} aria-hidden="true" />}
  {target && <div className="element-selection" data-testid="element-selection" style={{ ...paintStyle(rect, bounds, zoom), zIndex: layer }} aria-hidden="true" />}
  <div data-region-id={region.id} className={`annotation ${target || shaped ? 'element-prompt' : ''} ${selected ? 'selected' : ''} ${expanded ? 'expanded' : ''}`} style={{ ...windowStyle(shown, bounds, zoom), height: 'auto', zIndex: layer }}
    onPointerDown={event => { if (event.target === event.currentTarget) { event.preventDefault(); event.stopPropagation(); text.current?.focus({ preventScroll: true }); } }}
    onFocusCapture={select}
    onDragOver={event => { if (!attach || !(event.dataTransfer.types.includes('Files') || event.dataTransfer.types.includes('text/uri-list'))) return; event.preventDefault(); event.stopPropagation(); event.dataTransfer.dropEffect = 'copy'; setDropping(true); }}
    onDragLeave={event => { if (!(event.relatedTarget instanceof Node && event.currentTarget.contains(event.relatedTarget))) setDropping(false); }}
    onDrop={event => {
      if (!attach) return;
      event.preventDefault(); event.stopPropagation(); setDropping(false);
      const links = event.dataTransfer.files.length ? [] : (event.dataTransfer.getData('text/uri-list') || event.dataTransfer.getData('text/plain')).split(/\r?\n/).map(line => line.trim()).filter(line => /^https?:\/\//i.test(line));
      attach(Array.from(event.dataTransfer.files), links);
    }}>
    <div className="request-frame" aria-hidden="true" />
    {dropping && <div className="request-drop-hint" aria-hidden="true">Drop to attach to this request</div>}
    {stale && <ProblemBanner className="request-stale" message="This element was removed." action={repickNow ? { label: 'Pick again', onClick: repickNow } : undefined} secondaryAction={removeNow ? { label: 'Remove request', onClick: removeNow } : undefined} />}
    {!target && <span className="request-number" aria-hidden="true">{number}</span>}
    {wholePage && <span className="request-phone-tag" title={region.wholePage ? "Whole page" : "Whole page (automatically detected). Turn off in request controls."}>Whole page</span>}
    {region.mobileOnly && <span className="request-phone-tag">Phone fix</span>}
    <textarea ref={text} aria-label={`Request ${number}`} aria-describedby="canvas-keyboard-help" maxLength={16384} value={draft} placeholder={targets.length === 1 && targets[0].tag === 'img' ? 'Describe the picture…' : target ? 'Describe the change…' : 'Describe what goes here…'}
      style={{ height: displayHeight, borderWidth: 0, outline: 'none' }} onFocus={() => { if (target && hasMore) setExpanded(true); }} onChange={event => { changeDraft(event.target.value); if (target && hasMore) setExpanded(true); }} onPointerDown={event => event.stopPropagation()}
      onKeyDown={event => { if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); (moveVisible ? move.current : document.getElementById('page-canvas'))?.focus({ preventScroll: true }); } }} />
    {hasMore && <button type="button" className="request-expand" aria-expanded={expanded} onClick={() => setExpanded(value => !value)}>{expanded ? 'Less' : '… More'}</button>}
    {(attachments.length > 0 || reports.length > 0) && <div className="request-attachment-group">
    {attachments.length > 0 && <ul className="request-attachments" aria-label="Attachments" onPointerDown={event => event.stopPropagation()}>{(attachmentsExpanded ? attachments : attachments.slice(0, 3)).map(item => <li key={item.id} title={item.kind === 'link' ? item.url : item.name}>
      {item.kind === 'image' && item.dataUri ? <img src={item.preview ?? item.dataUri} alt="" /> : <span className="attachment-icon" aria-hidden="true">{item.kind === 'link' ? '🔗' : '¶'}</span>}
      <span className="attachment-name">{item.name}</span>{detach && <button type="button" aria-label={`Remove ${item.name}`} onClick={() => detach(item.id)}>×</button>}
    </li>)}
      {attachments.length > 3 && <li><button type="button" aria-expanded={attachmentsExpanded} aria-label={attachmentsExpanded ? 'Show fewer attachments' : `Show ${attachments.length - 3} more attachments`} onClick={() => setAttachmentsExpanded(value => !value)}>{attachmentsExpanded ? 'Less' : `+${attachments.length - 3}`}</button></li>}
    </ul>}
    {reports.map((report, index) => <details className="request-archive-report" key={index} onPointerDown={event => event.stopPropagation()}>
      <summary>From {report.name}: {report.added} added, {report.skipped.length} skipped{report.skipped.length ? ` (${new Set(report.skipped.map(item => item.reason)).size === 1 ? userError(report.skipped[0].reason) : 'multiple reasons'})` : ''}</summary>
      {!!report.skipped.length && <ul>{report.skipped.map((item, i) => <li key={i}>{item.name}: <ProblemBanner message={userError(item.reason)} /></li>)}</ul>}
    </details>)}
    </div>}
    {selected && attach && <div className="request-attach-row">
      <button type="button" className="request-attach" title="Attach pictures, text files or ZIPs (or drop them on the box)" aria-label={`Attach files to request ${number}`} onPointerDown={event => event.stopPropagation()} onClick={() => attachInput.current?.click()}>📎</button>
      <input ref={attachInput} type="file" hidden multiple accept="image/png,image/jpeg,image/webp,image/gif,.txt,.md,.csv,.json,.html,.zip,application/zip,text/plain" onChange={event => { const files = Array.from(event.target.files ?? []); event.target.value = ''; if (files.length) attach(files, []); }} />
    </div>}
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
  // Native text-control selection stays within its value. A transient contenteditable
  // can leave WebKitGTK's accessibility caret outside its cached Unicode text.
  const editor = useRef<HTMLTextAreaElement>(null);
  const done = useRef(false);
  function complete(value: string | null, restoreFocus = false) {
    if (done.current) return;
    done.current = true;
    editor.current?.setSelectionRange(0, 0);
    void finish(value, restoreFocus).catch(() => { done.current = false; editor.current?.focus({ preventScroll: true }); });
  }
  useLayoutEffect(() => {
    const node = editor.current; if (!node) return;
    node.value = text.text; node.focus({ preventScroll: true }); node.select();
    node.style.height = `${Math.max(text.rect.height, node.scrollHeight)}px`;
  }, []);
  return <textarea ref={editor} className="inline-text-editor" role="textbox" aria-label="Edit text in place" aria-multiline="true" aria-busy={disabled} readOnly={disabled} spellCheck rows={1}
    style={{ ...text.style, position: 'absolute', left: (text.rect.x - origin.x) * zoom, top: (text.rect.y - origin.y) * zoom, width: Math.max(24 / zoom, text.rect.width), minHeight: text.rect.height, transform: `scale(${zoom})`, transformOrigin: 'top left', zIndex: layer, resize: 'none', overflow: 'hidden' }}
    onPointerDown={event => event.stopPropagation()} onDoubleClick={event => event.stopPropagation()} onContextMenu={event => event.stopPropagation()}
    onInput={event => { const node = event.currentTarget; node.style.height = 'auto'; node.style.height = `${Math.max(text.rect.height, node.scrollHeight)}px`; }}
    onBlur={() => complete(editor.current?.value ?? text.text)}
    onKeyDown={event => {
      if (!event.ctrlKey && !event.metaKey) event.stopPropagation();
      if (event.key === 'Escape') { event.preventDefault(); complete(null, true); }
      if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) { event.preventDefault(); complete(editor.current?.value ?? text.text, true); }
    }} />;
}

function elementName(tag: string): string {
  return ({ a: 'Link', button: 'Button', img: 'Image', input: 'Input', textarea: 'Text area', select: 'Dropdown', h1: 'Heading', h2: 'Heading', h3: 'Heading', h4: 'Heading', h5: 'Heading', h6: 'Heading', p: 'Paragraph', span: 'Text', div: 'Container', section: 'Section', nav: 'Navigation', header: 'Header', footer: 'Footer', main: 'Main', body: 'Page', ul: 'List', ol: 'List', li: 'List item', form: 'Form' } as Record<string, string>)[tag] ?? tag.charAt(0).toUpperCase() + tag.slice(1);
}

function QuickIcon({ name }: { name: 'copy' | 'cut' | 'delete' | 'undo' }) {
  return <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    {name === 'undo' ? <path d="M7 4L3 8L7 12M3 8H12A5 5 0 0 1 12 18H8" /> : name === 'copy' ? <path d="M7 7H17V17H7ZM3 13V3H13" /> : name === 'cut' ? <><circle cx="5" cy="5" r="2.5"/><circle cx="5" cy="15" r="2.5"/><path d="M7 6.5L17 17M7 13.5L17 3M10 10L7 7"/></> : <><path d="M3 5H17M7 5V2H13V5M5 5L6 18H14L15 5M8 8V15M12 8V15"/></>}
  </svg>;
}
