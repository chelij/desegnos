import { ProblemBanner } from './ProblemBanner.js';
import { areaProblems, declaredConflict, fixLabel, jobOutcome, placeCallouts, type OutcomeFix } from './generationProblems';
import type { GenerationController } from './useGeneration';
import { userError, runtimeError, generationError, generationPageError } from './userErrors.js';
import { isWholePage } from './wholePageScope';
import { forwardRef, memo, useCallback, useImperativeHandle, useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type PointerEvent, type KeyboardEvent, type MouseEvent as ReactMouseEvent, type RefObject } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { isTauri } from '@tauri-apps/api/core';
import { flushSync } from 'react-dom';
import { canonicalSelections, documentKey, regionTargets, sameDocument, sameTarget, targetKey, unionRect } from './elementTargets';
import { MAX_SELECTION, marqueeSelections } from './elementSelection';
import { acrossPagesFor, acrossPagesLabel, chosenAcrossPages, setAcrossPages, toggleAcrossPage, type AcrossPagesState, ACROSS_PAGES_OFF } from './acrossPages';
import { adoptRequestFrame, frameTargetShown, pageRectToInnerDocument, remapFramedRequest, requestFrame } from './frameDocuments';
import { adoptsFrameTarget } from './frameAdoption';
import { useDraftText } from './DraftText';
import { escapeHtml, imageFile } from './elementEditing';
import { StyleBar, type StyleChange } from './StyleBar';
import { askBeforeDelete, setAskBeforeDelete } from './deletePreference';
import { LayersPopover, LayerIcon } from './Layers';
import type { SiteLayer } from './siteLayers';
import { ElementDialog, type ElementDialogMode } from './ElementTools';
import { ContextMenu, type MenuItem } from './Menus';
import { requestEditorOpen } from './visibleActions';
import { PreviewFrame, type CanvasPreviewHandle, type ElementMover, type ElementResizer, type FrameView, type InlineText } from './Preview';
import { FrameOverlay, useLiveFrameSession, type LiveFrameSession } from './FrameOverlays';
import type { LiveHistoryAction } from './liveBar';
import { liveFrame, liveVisibility, type LiveDevice } from './liveFrame';
import { overlayPlacement, placeOverlays, type CollectedFrame } from './previewFrames';
import { shownSelectionRects } from './previewMeasurement';
import { freehandArea, pointInRegion, alignmentLines, elementResizeRect, elementToolbarPosition, selectionLabelPosition, attachedRequestRect, clamp, clientToCanvas, DEFAULT_GRID, expandWorkspace, geometryForGesture, gridSteps, workspaceWindow, keyboardRectChange, percent, percentageTicks, pixelTicks, requestAt, requestPlacement, sameBounds, sameRect, intersectsWorkspace, clipToWorkspace, validWorkspace, MIN_ZOOM, wheelZoom, zoomOrigin, type CanvasGridSettings, type GeometryGesture, type Point, type RectChange, type WorkspaceBounds, type ResizeCorner } from './canvasGeometry';
import type { AcrossPagesChange, AcrossPagesMatch, DocumentTarget, ElementBounds, ElementDetails, ElementEdit, ElementSelection, ElementTarget, ElementStyleEdit, ElementVisual, FrameDocumentView, FrameTarget, LayerTarget, LayerDirection, LayerPasteView, Page, PageSelection, Rect, Region, RegionShape, Viewport } from './types';
import { RegionOutline } from './RegionOutline';
import { FloatingBar } from './FloatingBar';
import { Icon } from './icons';
import { withShortcut } from './shortcuts';
import { RequestPages } from './RequestPages';
import type { RequestShapeKind } from './CanvasTools';
import './canvas.css';

// A prompt function is read when the save runs, so queued keystrokes save the newest text once.
export type RequestPatch = { wholePage?: boolean; pages?: PageSelection[]; prompt?: string | (() => string); rect?: RectChange | Partial<Rect> };
export type CanvasViewAction = { serial: number; kind: 'guide' | 'selection' | 'fit' };
type Gesture = GeometryGesture & { pointerId: number; region?: Region; active: boolean; shapeKind: RequestShapeKind; points: Point[]; freehand?: RegionShape };
type BatchChange = { selection: ElementSelection; change: ElementEdit };
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

// The live layer depends on the page's code and viewport, never on its requests. Typing a
// prompt re-renders React requests without touching the live page or its iframe.
const LiveCanvasLayer = memo(function LiveCanvasLayer({ pageId, pageName, revision, width, height, bounds, zoom, passive, aiDisabled, observationsVersion, frameRef, onLiveLocation, onRuntimeProblems, fixRuntimeErrors }: {
  pageId: string; pageName: string; revision: string; width: number; height: number; bounds: WorkspaceBounds; zoom: number; passive: boolean;
  aiDisabled: boolean; observationsVersion?: number; frameRef: RefObject<CanvasPreviewHandle | null>; onLiveLocation?: (path: string) => void;
  onRuntimeProblems: (errors: string[]) => void; fixRuntimeErrors?: (errors: string[]) => void;
}) {
  return <div className={`live-canvas-page${passive ? ' passive' : ''}`} data-testid="live-canvas-page" style={windowStyle({ x: 0, y: 0, width, height }, bounds, zoom)}>
    <div className="live-canvas-content" style={{ width, height, transform: `scale(${zoom})` }}>
      <PreviewFrame ref={frameRef} onLiveLocation={onLiveLocation} onRuntimeProblems={onRuntimeProblems} fixRuntimeErrors={fixRuntimeErrors} aiDisabled={aiDisabled} observationsVersion={observationsVersion} pageId={pageId} revision={revision} runScripts title={`${pageName} live`} />
    </div>
  </div>;
});

// Live copies of frames are anchored to their still copies. A prompt edit changes neither,
// so the overlays stay mounted and their sessions survive.
const FrameLayer = memo(function FrameLayer({ frames, session, pageId, pageName, staticPageUrl, bounds, zoom, interactingFrame, interactiveSelector, stop }: {
  frames: CollectedFrame[]; session: LiveFrameSession; pageId: string; pageName: string; staticPageUrl: string | null;
  bounds: WorkspaceBounds; zoom: number; interactingFrame: string | null; interactiveSelector: string | null; stop: () => void;
}) {
  return <>{frames.map(frame => <FrameOverlay key={frame.key} frame={frame} live={session} pageId={pageId} staticPageUrl={staticPageUrl} zoom={zoom}
    interactive={interactiveSelector !== null && interactingFrame === frame.key && interactiveSelector === frame.selector}
    placement={{ ...overlayPlacement(frame.rect, { x: bounds.left, y: bounds.top }, zoom), zIndex: 1 }} title={`${pageName} interactive frame`} stop={stop} />)}</>;
});

type CanvasProps = {
  generation?: GenerationController;
  archiveReports?: Record<string, import('./types').ArchiveReport[]>; projectId?: string;
  pendingImagesChanged: (count: number) => void; canUndo: boolean; canRedo: boolean; undo: () => void; redo: () => void; toggleGrid: () => void; configureGrid: () => void;
  fixRuntimeErrors?: (errors: string[]) => void; aiDisabled?: boolean; observationsVersion?: number;
  projectImages?: import('./types').ProjectImage[]; page: Page; pages: Page[]; layersOpen: boolean; setLayersOpen: (open: boolean) => void; selectionChanged: (target: ElementTarget | null) => void; multiChanged?: (count: number) => void; repick?: (regionId: string, selection: ElementSelection) => Promise<void>; fixOnPhones?: (selection: ElementSelection) => void; attach?: (regionId: string, files: File[], links: string[]) => void; liveDevice?: LiveDevice | null; detach?: (regionId: string, attachmentId: string) => void; onLiveLocation?: (path: string) => void;
  arrangeRequest: (id: string, direction: LayerDirection) => Promise<void>; copyNative: (target: LayerTarget, styles: ElementStyleEdit[]) => Promise<void>; cutNative: (target: LayerTarget, styles: ElementStyleEdit[]) => Promise<void>; pasteNative: (after: ElementTarget | null) => Promise<LayerPasteView>;
  requestShape?: RequestShapeKind; grid: boolean; gridSettings?: CanvasGridSettings; tool?: 'select' | 'request' | 'pan'; rulerUnit?: 'pixels' | 'percent'; zoom: number; selected: string | null; focusRequest: string | null; viewAction?: CanvasViewAction; changing?: { rect: Rect; shape?: RegionShape }[]; highlighted?: { rect: Rect; shape?: RegionShape }[];
  frameDocuments?: FrameDocumentView[]; ensureElementId: (target: ElementTarget) => Promise<ElementTarget | null>; setFrameTarget: (regionId: string, frameTarget: FrameTarget | null) => Promise<void>;
  refreshElementBounds: (pageId: string, viewport: Viewport, bounds: ElementBounds[], fingerprint: string) => Promise<void>;
  openSource?: (target: ElementTarget) => void;
  resolveGenerationFix?: (fix: OutcomeFix, regionId: string) => void;
  inspectElement: (target: ElementTarget) => Promise<ElementDetails>; editElement: (target: ElementTarget, change: ElementEdit) => Promise<ElementTarget | null>; undoElement?: (target: ElementTarget) => Promise<ElementTarget | null>;
  editElements: (edits: { target: ElementTarget; change: ElementEdit }[]) => Promise<(ElementTarget | null)[]>; copyElements: (targets: ElementTarget[], styles: ElementStyleEdit[][]) => Promise<void>;
  findAcrossPages: (target: ElementTarget) => Promise<AcrossPagesMatch[]>; editAcrossPages: (target: ElementTarget, change: ElementEdit, documents: DocumentTarget[]) => Promise<AcrossPagesChange[]>;
  allowWholePage?: (regionId: string) => Promise<void>;
  changeZoom: (zoom: number) => void; select: (id: string | null, cleanEmpty?: boolean) => void; add: (rect: Rect, shape?: RegionShape) => Promise<Region | null>; pickElement: (selection: ElementSelection[], pages?: PageSelection[]) => Promise<Region | null>; deleteElements: (elements: ElementTarget[]) => Promise<void>; change: (id: string, patch: RequestPatch) => Promise<Region>; remove: (id: string) => void; setTargetRemoval: (regionId: string, allowed: boolean) => void;
};
export type CanvasHandle = { liveHistory: (action: LiveHistoryAction) => void; selectSimilar: () => void; focusElement: (selector: string, tag: string, id?: string, expectedPageId?: string, document?: ElementTarget['document']) => ElementSelection | null; synchronize: (requireBounds?: boolean) => Promise<void>; addRequest: () => void; focusRequest: (id: string) => void; askAI: () => void; undoElement: () => Promise<void>; copyLayer: () => Promise<void>; cutLayer: () => Promise<void>; pasteLayer: () => Promise<void>; arrangeLayer: (direction: LayerDirection) => Promise<void> };
export const Canvas = forwardRef<CanvasHandle, CanvasProps>(function Canvas({ generation, archiveReports = {}, fixRuntimeErrors, aiDisabled, observationsVersion, projectId, allowWholePage, pendingImagesChanged, projectImages = [], canUndo, canRedo, undo, redo, toggleGrid, configureGrid, page, pages, layersOpen, setLayersOpen, selectionChanged, multiChanged, repick, fixOnPhones, attach, detach, liveDevice = null, onLiveLocation, arrangeRequest, copyNative, cutNative, pasteNative, grid, gridSettings = DEFAULT_GRID, tool = 'select', requestShape = 'rect', rulerUnit = 'pixels', zoom, selected, focusRequest, viewAction, changing = [], highlighted = [], frameDocuments = [], ensureElementId, setFrameTarget, inspectElement, editElement, undoElement, editElements, copyElements, findAcrossPages, editAcrossPages, refreshElementBounds, openSource, resolveGenerationFix, changeZoom, select, add, pickElement, deleteElements, change, remove, setTargetRemoval }, ref) {
  const [dismissedProblems, setDismissedProblems] = useState<string[]>([]);
  // Pages chosen for the next Ask AI on an element; the request box has its own picker.
  const [requestPages, setRequestPages] = useState<PageSelection[]>([]);
  const [problemErrors, setProblemErrors] = useState<Record<string, string>>({});
  const [problemBusy, setProblemBusy] = useState(false);
  const [runtimeProblems, setRuntimeProblems] = useState<string[]>([]);
  useEffect(() => { setRuntimeProblems([]); }, [page.id]);
  // A restored job keeps its problems in the AI conversation; only this session's job calls out.
  const announcedJob = generation?.owns ? generation.job ?? null : null;
  const problems = areaProblems(announcedJob, page);
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
  // Live replaces the page frame: desktop keeps the page's width and whole scroll height, a
  // device shows its own viewport and scrolls inside it.
  const liveView = useMemo(() => liveDevice ? liveFrame(liveDevice, page.viewport, siteHeight) : null,
    [liveDevice, page.viewport.width, page.viewport.height, siteHeight]);
  const liveShown = useMemo(() => liveVisibility(liveView, page.viewport.width), [liveView?.width, liveView?.height, page.viewport.width]);
  // Frames read from the promoted static document, and the one the user is playing.
  const [frameView, setFrameView] = useState<FrameView>({ pageUrl: null, frames: [] });
  const [interactingFrame, setInteractingFrame] = useState<string | null>(null);
  useEffect(() => setSiteHeight(page.viewport.height), [page.id, page.content?.html, page.content?.css, page.content?.javascript, page.viewport.width, page.viewport.height]);
  const stage = useRef<HTMLDivElement>(null);
  const gesture = useRef<Gesture | null>(null);
  const pan = useRef<{ pointerId: number; origin: Point; start: Point; click: boolean; active: boolean } | null>(null);
  const marquee = useRef<{ pointerId: number; clientStart: Point; start: Point; base: ElementSelection[]; active: boolean } | null>(null);
  const [marqueeRect, setMarqueeRect] = useState<Rect | null>(null);
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
  const elementDrag = useRef<{ pointerId: number; pageId: string; version: string; start: Point; world: Point; original: ElementSelection; mover: ElementMover | null; resizer: ElementResizer | null; group: { selection: ElementSelection; mover: ElementMover }[] | null; corner?: ResizeCorner; active: boolean; fine: boolean; x: number; y: number; rect: Rect } | null>(null);
  const [editingElement, setEditingElement] = useState(false);
  const [nudging, setNudging] = useState(false);
  const selectionSerial = useRef(0);
  const elementRef = useRef(element); elementRef.current = element;
  const dialogRef = useRef(elementDialog); dialogRef.current = elementDialog;
  const pendingSelection = useRef<ElementTarget | null>(null);
  // The boxes the current edit aims at, the primary and every extra. A promoted frame is
  // measured before its layout has settled, so the canvas keeps drawing these boxes until
  // the settled measurement arrives.
  const expectedSelections = useRef<ElementSelection[]>([]);
  // The revision whose settled measurement has arrived; request bounds are only recorded from it.
  const settledVersion = useRef<string | null>(null);
  function expectSelections(items: ElementSelection[]) {
    for (const item of items) expectedSelections.current = [...expectedSelections.current.filter(held => !sameTarget(held.element, item.element)), item];
  }
  function expectedRect(element: ElementTarget): Rect | undefined {
    return expectedSelections.current.find(held => sameTarget(held.element, element))?.rect;
  }
  // An edit may report a new identity for the same element; the held box belongs to it either way.
  function retargetExpected(from: ElementTarget, to: ElementTarget) {
    expectedSelections.current = expectedSelections.current.map(held => sameTarget(held.element, from) ? { ...held, element: to } : held);
  }
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
  // Every operation below works on this canonical list: the primary selection first.
  function selectionsNow(): ElementSelection[] {
    return canonicalSelections([elementRef.current, ...extrasRef.current].flatMap(item => item ? [item] : []));
  }
  function commonDocument(selections: ElementSelection[]): ElementTarget['document'] {
    const document = selections[0]?.element.document;
    if (selections.some(item => !sameDocument(item.element.document, document))) throw new Error('Select items in one document to change them together.');
    return document;
  }
  function applySelection(items: ElementSelection[]) {
    const unique = canonicalSelections(items);
    setElementError('');
    if (unique.length > MAX_SELECTION) setElementError(`Select up to ${MAX_SELECTION} elements at a time.`);
    select(null);
    const next = unique.slice(0, MAX_SELECTION);
    pendingSelection.current = next[0]?.element ?? null;
    storeElement(next[0] ?? null); setExtras(next.slice(1));
  }
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
  function deselectElement() { setElementDialog(null); setConfirmMany(null); setExtras([]); setContext(null); inlineTicket.current++; selectionSerial.current++; nudge.current.x = 0; nudge.current.y = 0; pendingSelection.current = null; expectedSelections.current = []; storeElement(null); }
  useLayoutEffect(() => { deselectElement(); settledVersion.current = null; setRepicking(null); setStale(new Set()); setElementDialog(null); setElementError(''); }, [page.id]);
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
    // Live frames the page itself, so fit and centre aim at what Live shows.
    const framed = liveView ?? page.viewport;
    if (kind === 'fit') {
      const target = Math.max(MIN_ZOOM, Math.min(4, Math.max(1, node.clientWidth - 96) / framed.width, Math.max(1, node.clientHeight - 160) / framed.height));
      const origin = { x: framed.width / 2 - node.clientWidth / target / 2, y: framed.height / 2 - (node.clientHeight - 64) / target / 2 };
      if (Math.abs(target - zoom) < 1e-8) seek(origin);
      else { zoomIntent.current = { pageId: page.id, zoom: target, origin }; changeZoom(target); }
      return;
    }
    const region = kind === 'selection' ? page.regions.find(region => region.id === selected) : null;
    seek(region ? { x: region.rect.x + region.rect.width / 2 - node.clientWidth / zoom / 2, y: region.rect.y + region.rect.height / 2 - node.clientHeight / zoom / 2 }
      : { x: framed.width / 2 - node.clientWidth / zoom / 2, y: -64 / zoom });
  }
  function point(client: Point): Point {
    return clientToCanvas(client, stage.current!.getBoundingClientRect(), zoom, { x: boundsRef.current.left, y: boundsRef.current.top });
  }
  function beginMarquee(event: PointerEvent, additive: boolean) {
    if (gesture.current || pan.current || elementDrag.current || marquee.current) return;
    event.preventDefault(); event.stopPropagation(); setContext(null);
    plainClicks.current = 0; lastPlainClick.current = null;
    const client = { x: event.clientX, y: event.clientY };
    marquee.current = { pointerId: event.pointerId, clientStart: client, start: point(client), base: additive ? selectionsNow() : [], active: false };
    stage.current!.focus({ preventScroll: true }); stage.current!.setPointerCapture(event.pointerId); setCursor(null);
  }
  function cancel(resetClicks = true, restoreElement = true) {
    if (resetClicks) { plainClicks.current = 0; lastPlainClick.current = null; }
    const pointerId = gesture.current?.pointerId ?? pan.current?.pointerId ?? elementDrag.current?.pointerId ?? marquee.current?.pointerId;
    marquee.current = null; setMarqueeRect(null);
    if (elementDrag.current) {
      const drag = elementDrag.current;
      if (restoreElement) {
        drag.mover?.cancel(); drag.resizer?.cancel();
        expectedSelections.current = [];
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
    const all = selectionsNow();
    if (!all.length) { chooseElement(hit); return; }
    const rest = all.some(item => sameTarget(item.element, hit.element))
      ? all.filter(item => !sameTarget(item.element, hit.element)) : [...all, hit];
    applySelection(rest);
  }
  // Elements matching the reference's tag and class list, or its structural role.
  function selectSimilar() {
    const current = elementRef.current;
    if (!current || editingElement || current.element.selector === 'body') return;
    try {
      const items = site.current?.similar(current.element) ?? [];
      if (items.length) applySelection(items);
    } catch (error) { setElementError(error instanceof Error ? error.message : String(error)); }
  }
  // The toggle belongs to one element: a new selection starts with it off.
  const [acrossMatches, setAcrossMatches] = useState<AcrossPagesMatch[]>([]);
  const [across, setAcross] = useState<AcrossPagesState>(ACROSS_PAGES_OFF);
  const acrossLookup = useRef(0);
  const acrossSelection = element && !extras.length && !liveView && element.element.selector !== 'body' ? element.element : null;
  const acrossKey = acrossSelection ? `${documentKey(acrossSelection.document)}|${acrossSelection.id || acrossSelection.selector}` : '';
  useEffect(() => { setAcross(current => acrossPagesFor(element && !extras.length && !liveView ? element.element : null, current)); }, [acrossKey, extras.length, liveView]);
  useEffect(() => {
    const target = acrossSelection;
    const ticket = ++acrossLookup.current;
    if (!target) { setAcrossMatches([]); return; }
    void findAcrossPages(target).then(matches => { if (acrossLookup.current === ticket) setAcrossMatches(matches); }, () => { if (acrossLookup.current === ticket) setAcrossMatches([]); });
  }, [acrossKey, extras.length, page.id]);
  // One native change carries the edit to the chosen pages; the toggle only reaches a single element.
  function acrossDocuments(selected: ElementSelection | null | undefined): DocumentTarget[] | null {
    if (!across.enabled || extrasRef.current.length || !selected || !elementRef.current || !sameTarget(selected.element, elementRef.current.element)) return null;
    const documents = chosenAcrossPages(across, acrossMatches);
    return documents.length ? documents : null;
  }
  async function applyAcross(selected: ElementSelection, change: ElementEdit, documents: DocumentTarget[]): Promise<AcrossPagesChange[]> {
    const ticket = selectionSerial.current; const pageId = page.id;
    setEditingElement(true); setElementError('');
    try {
      const changes = await editAcrossPages(selected.element, change, documents);
      if (pageRef.current.id === pageId && selectionSerial.current === ticket) {
        if (change.kind === 'remove') deselectElement();
        else {
          // Keep the selection on this page's element, including any identity the edit gave it.
          const reference = selected.element.document ?? { kind: 'page' as const, pageId };
          const changed = changes.find(item => documentKey(item.document) === documentKey(reference));
          const next = changed ? (selected.element.document ? { ...changed.target, document: selected.element.document } : changed.target) : null;
          if (next) {
            const latest = elementRef.current;
            const shown = latest && sameTarget(latest.element, selected.element) ? latest : selected;
            if (expectedRect(selected.element)) retargetExpected(selected.element, next);
            pendingSelection.current = next;
            storeElement({ ...shown, element: next });
          } else {
            pendingSelection.current = selected.element;
            const found = site.current?.find(selected.element);
            if (found) storeElement({ ...selected, rect: found.rect });
          }
        }
      }
      return changes;
    } catch (error) {
      if (pageRef.current.id === pageId && selectionSerial.current === ticket) setElementError(error instanceof Error ? error.message : String(error));
      throw error;
    } finally { setEditingElement(false); }
  }
  // One native batch edit for the whole selection: one revision, one undo step.
  async function applyBatch(changes: BatchChange[]) {
    if (!changes.length) return [];
    const selections = changes.map(item => item.selection);
    commonDocument(selections);
    const ticket = selectionSerial.current; const pageId = page.id;
    setEditingElement(true); setElementError('');
    try {
      const results = await editElements(changes.map(item => ({ target: item.selection.element, change: item.change })));
      if (pageRef.current.id === pageId && selectionSerial.current === ticket) {
        // Every element the batch edited keeps its aimed box until the settled measurement,
        // including any new identity the batch gave it.
        results.forEach((element, index) => { if (element) retargetExpected(selections[index].element, element); });
        const next = results.flatMap((element, index) => element ? [{ element, rect: expectedRect(element) ?? selections[index].rect }] : []);
        if (next.length) {
          pendingSelection.current = next[0].element;
          storeElement(next[0]); setExtras(next.slice(1));
        } else deselectElement();
      }
      return results;
    } catch (error) {
      if (pageRef.current.id === pageId && selectionSerial.current === ticket) setElementError(error instanceof Error ? error.message : String(error));
      throw error;
    } finally { setEditingElement(false); }
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
    const all = targets ?? selectionsNow().map(item => item.element);
    if (all.length < 2 || editingElement) return;
    if (new Set(all.map(target => documentKey(target.document))).size > 1) { setElementError('Select items in one document to delete them together.'); return; }
    if (!confirmed && askBeforeDelete()) { setConfirmMany({ count: all.length, skip: false, targets: all, pageId: page.id }); return; }
    setEditingElement(true);
    try { await deleteElements(all); deselectElement(); }
    finally { setEditingElement(false); }
  }
  function chooseElement(selection: ElementSelection | null) { deselectElement(); storeElement(selection); setElementError(''); select(null); }
  const [styleOpen, setStyleOpen] = useState(() => { try { return localStorage.getItem('desegnos.style-bar') === 'open'; } catch { return false; } });
  function toggleStyle() { setStyleOpen(value => { try { localStorage.setItem('desegnos.style-bar', value ? 'closed' : 'open'); } catch {} return !value; }); }
  // Quick style edits update each element's own inline style, the same way the Edit dialog does.
  async function applyStyles(change: StyleChange) {
    const selections = selectionsNow(); if (!selections.length) return;
    try {
      if (selections.length === 1) {
        const details = await inspectElement(selections[0].element);
        const css = document.createElement('div').style; css.cssText = details.inlineStyle;
        for (const [name, value] of Object.entries(change)) {
          if (!CSS.supports(name, value)) throw new Error('That style value is not supported.');
          css.setProperty(name, value, 'important');
        }
        await applyElement({ kind: 'update', texts: [], attributes: {}, style: css.cssText }, selections[0]);
        return;
      }
      const details = await Promise.all(selections.map(selection => inspectElement(selection.element)));
      const changes = selections.map((selection, index) => {
        const css = document.createElement('div').style; css.cssText = details[index].inlineStyle;
        for (const [name, value] of Object.entries(change)) {
          if (!CSS.supports(name, value)) throw new Error('That style value is not supported.');
          css.setProperty(name, value, 'important');
        }
        return { selection, change: { kind: 'update', texts: [], attributes: {}, style: css.cssText } as ElementEdit };
      });
      await applyBatch(changes);
    } catch (error) { setElementError(error instanceof Error ? error.message : String(error)); }
  }
  // Give every selected item the first item's width or height, one edit for the batch.
  async function resizeSelection(axis: 'width' | 'height') {
    const selections = selectionsNow(); if (selections.length < 2 || editBusy) return;
    try {
      commonDocument(selections);
      const size = axis === 'width' ? selections[0].rect.width : selections[0].rect.height;
      const aimed = selections.map(selection => ({ ...selection, rect: { ...selection.rect, [axis]: size } }));
      const changes = aimed.map(selection => {
        const resizer = site.current?.beginResize(selection.element, 'se');
        if (!resizer) throw new Error('This item cannot be resized in its current layout.');
        try { return { selection, change: { kind: 'update', texts: [], attributes: {}, style: resizer.style(selection.rect) } as ElementEdit }; }
        finally { resizer.cancel(); }
      });
      // Hold every resized box at its aimed size until the committed preview settles.
      expectSelections(aimed);
      await applyBatch(changes);
    } catch (error) { setElementError(error instanceof Error ? error.message : String(error)); }
  }
  async function applyElement(change: ElementEdit, selected = elementRef.current) {
    if (!selected) throw new Error('Select an element first.');
    const documents = change.kind === 'layerStyles' ? null : acrossDocuments(selected);
    if (documents) { await applyAcross(selected, change, documents); return; }
    const ticket = selectionSerial.current; const pageId = page.id;
    const ownsSelection = !!elementRef.current && sameTarget(elementRef.current.element, selected.element);
    setEditingElement(true); setElementError('');
    try {
      const target = await editElement(selected.element, change);
      // Native reports identities without their document; the selection still lives in it.
      const next = target && selected.element.document ? { ...target, document: selected.element.document } : target;
      if (ownsSelection && pageRef.current.id === pageId && selectionSerial.current === ticket) {
        // Keep the box the canvas is already drawing (a drag target or a nudged box)
        // instead of the rect measured before the edit.
        const latest = elementRef.current;
        const shown = latest && sameTarget(latest.element, selected.element) ? latest : selected;
        // The edit may report a new identity for the same element; the box the canvas is
        // drawing belongs to it either way.
        if (next) retargetExpected(selected.element, next);
        pendingSelection.current = next;
        storeElement(next ? { ...shown, element: next } : null);
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
      const next = target && selected.element.document ? { ...target, document: selected.element.document } : target;
      if (pageRef.current.id === pageId && selectionSerial.current === ticket) {
        pendingSelection.current = next;
        storeElement(next ? { ...selected, element: next } : null);
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
    const selections = selectionsNow();
    if (!selections.length || !x && !y) return;
    commonDocument(selections);
    const movers = selections.map(selection => ({ selection, mover: site.current?.beginMove(selection.element) ?? null }));
    if (movers.some(item => !item.mover)) {
      movers.forEach(item => item.mover?.cancel());
      throw new Error('Some selected items cannot be moved here. Select them again.');
    }
    const source = site.current?.sourceOf(selections[0].element); const pageId = pageRef.current.id;
    // Move the boxes with the nudge and keep them as targets until the committed
    // preview settles; a measurement taken before then can describe another layout.
    const boxes = selections.map(selection => ({ ...selection, rect: { ...selection.rect, x: selection.rect.x + x, y: selection.rect.y + y } }));
    expectSelections(boxes);
    storeElement(boxes[0]); setExtras(boxes.slice(1));
    try {
      if (selections.length === 1) await applyElement({ kind: 'update', texts: [], attributes: {}, style: movers[0].mover!.style(x, y) }, selections[0]);
      else await applyBatch(movers.map(item => ({ selection: item.selection, change: { kind: 'update', texts: [], attributes: {}, style: item.mover!.style(x, y) } as ElementEdit })));
      // Stable IDs also resolve in the retained old frame. Wait for the committed
      // document before another queued nudge reads its computed translation.
      await waitForSite(pageId, elementRef.current?.element, source);
    }
    finally { movers.forEach(item => item.mover?.cancel()); }
  }
  const moveElementRef = useRef(moveElement); moveElementRef.current = moveElement;
  async function waitForSite(pageId: string, target?: ElementTarget, previousSource?: string | null) {
    // A framed document is swapped behind its visible copy, so it may take longer than a page reload.
    const deadline = performance.now() + (target?.document ? 10_000 : 4000);
    do {
      if (pageRef.current.id !== pageId) throw new Error('The page changed. Select the item again.');
      const source = target ? site.current?.sourceOf(target) : site.current?.source();
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
  // A box drawn over an editable frame belongs to the document inside it. Boxes created before
  // framed editing existed, or whose frame changed, gain their framed document here, before
  // generation measures and freezes the request.
  async function adoptFrameTargets(): Promise<boolean> {
    const current = pageRef.current;
    if (liveView || !current.content) return false;
    const frames = site.current?.frames() ?? [];
    // Resolved frame documents do not need the static preview to be measurable; they keep a
    // hidden frame's request from being cleared while its tab is closed.
    const known = [
      ...frames.map(item => ({ iframeId: item.iframe.id, document: item.document })),
      ...frameDocuments.map(item => ({ iframeId: item.iframeId, document: item.document })),
    ];
    if (!known.length) return false;
    let changed = false;
    // A whole-page request stays on the host page; core keeps the same rule at prepare.
    for (const region of current.regions.filter(adoptsFrameTarget)) {
      const existing = region.frameTarget;
      if (existing) {
        const valid = frameTargetShown(frames, existing) || known.some(item => item.iframeId === existing.iframeId && sameDocument(item.document, existing.document));
        if (valid) continue;
        await setFrameTarget(region.id, null);
        changed = true;
        continue;
      }
      // A new target needs the measured frame box; the backend repeats this rule at prepare.
      if (!frames.length) continue;
      const framed = adoptRequestFrame(frames, region.rect);
      if (!framed) continue;
      let iframe: ElementTarget | null = framed.frame.iframe;
      if (!iframe.id) {
        try { iframe = await ensureElementId({ ...iframe }); }
        catch { iframe = null; }
      }
      if (!iframe?.id) continue;
      await setFrameTarget(region.id, { pageId: current.id, iframeId: iframe.id, document: framed.frame.document, rect: framed.rect });
      changed = true;
    }
    return changed;
  }
  // Request boxes describe the frame the canvas is showing, so their bounds are only read
  // once that frame's layout has settled. A promotion-time or replaced reading would record
  // positions the user never sees, on every element the request targets.
  async function settledFrame(): Promise<boolean> {
    const deadline = performance.now() + 2000;
    do {
      if (elementDrag.current) return false;
      if (settledVersion.current === acceptedVersionRef.current) return true;
      await new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
    } while (performance.now() < deadline);
    return false;
  }
  function synchronizeBounds(): Promise<void> {
    if (syncing.current) return syncing.current;
    const work = async () => {
      const current = pageRef.current;
      if (!current.content || !current.regions.some(region => !region.completed)) { setStale(new Set()); return; }
      await waitForSite(current.id);
      if (!await settledFrame()) return;
      // Frame boxes are only trustworthy once the page has settled: right after an undo or a new
      // version, an iframe can briefly sit over another request and would wrongly adopt it.
      if (await adoptFrameTargets()) await new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
      const version = acceptedVersionRef.current;
      if (JSON.stringify(pageRef.current.content) !== JSON.stringify(current.content) || pageRef.current.id !== current.id) throw new Error('The page changed while measuring its items. Try again.');
      const regions = pageRef.current.regions.filter(region => !region.completed && !region.frameTarget);
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
      if (bounds.length) await refreshElementBounds(current.id, current.viewport, bounds, version);
    };
    syncing.current = work().finally(() => { syncing.current = null; });
    return syncing.current;
  }
  // --- Framed requests -------------------------------------------------------------
  // A box drawn over an editable frame belongs to the document inside it: the region keeps
  // the host-page rect for the canvas, and a frame target with the rect mapped inside.
  async function createRequest(rect: Rect, shape?: RegionShape): Promise<Region | null> {
    // Live navigation leaves the still geometry behind, so a live box stays a page request.
    const frames = liveView ? [] : site.current?.frames() ?? [];
    const framed = requestFrame(frames, rect);
    let iframe = framed?.frame.iframe ?? null;
    if (iframe && !iframe.id) {
      try { iframe = await ensureElementId({ ...iframe }); }
      catch (error) { setElementError(error instanceof Error ? error.message : String(error)); }
    }
    const region = await add(rect, shape);
    if (region && framed && iframe?.id) {
      try {
        await setFrameTarget(region.id, { pageId: pageRef.current.id, iframeId: iframe.id, document: framed.frame.document, rect: framed.rect });
      } catch (error) { setElementError(error instanceof Error ? error.message : String(error)); }
    }
    return region;
  }
  function createRequestInBackground(rect: Rect, shape?: RegionShape) {
    void createRequest(rect, shape).catch(error => setElementError(error instanceof Error ? error.message : String(error)));
  }
  // A framed request keeps its mapping while it moves inside the frame; leaving the frame
  // turns it back into a host-page request.
  async function changeRequest(id: string, patch: RequestPatch): Promise<Region> {
    const region = await change(id, patch);
    const framed = region.frameTarget;
    if (patch.rect && framed) {
      const frames = site.current?.frames() ?? [];
      const rect = frames.length ? remapFramedRequest(frames, framed, region.rect) : framed.rect;
      if (rect && (rect.x !== framed.rect.x || rect.y !== framed.rect.y || rect.width !== framed.rect.width || rect.height !== framed.rect.height)) {
        await setFrameTarget(id, { ...framed, rect });
      } else if (!rect && frames.length) {
        await setFrameTarget(id, null);
      }
    }
    return region;
  }
  function addRequestInView() {
    const node = scroll.current; if (!node) return;
    const current = boundsRef.current;
    const start = { x: current.left + node.scrollLeft / zoom, y: current.top + node.scrollTop / zoom };
    const end = { x: start.x + node.clientWidth / zoom, y: start.y + node.clientHeight / zoom };
    const withinGuide = end.x > 0 && start.x < page.viewport.width && end.y > 0 && start.y < page.viewport.height;
    const center = withinGuide ? { x: (Math.max(0, start.x) + Math.min(page.viewport.width, end.x)) / 2, y: (Math.max(0, start.y) + Math.min(page.viewport.height, end.y)) / 2 }
      : { x: (start.x + end.x) / 2, y: (start.y + end.y) / 2 };
    deselectElement(); createRequestInBackground(requestAt({ x: center.x - 160 / zoom, y: center.y - 80 / zoom }, zoom));
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
  async function copySelection() {
    const selections = selectionsNow();
    commonDocument(selections);
    await copyElements(selections.map(item => item.element), selections.map(item => site.current!.copyStyles(item.element)));
  }
  async function copyLayer() {
    if (layerBusy) return;
    setLayerBusy(true); setElementError('');
    try {
      await finishLayerDraft();
      if (selectionsNow().length > 1) { await copySelection(); return; }
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
      if (selectionsNow().length > 1) {
        const selections = selectionsNow();
        await copySelection();
        await deleteElements(selections.map(item => item.element));
      } else {
        const region = pageRef.current.regions.find(item => item.id === selected && !item.completed);
        if (region) await cutNative({ kind: 'request', regionId: region.id }, []);
        else if (elementRef.current && elementRef.current.element.selector !== 'body') {
          const target = elementRef.current.element;
          await cutNative({ kind: 'element', target }, site.current!.copyStyles(target));
        } else throw new Error('Select a layer to cut.');
      }
      if (pageRef.current.id === pageId && selectionSerial.current === ticket) { deselectElement(); select(null, false); }
      refreshLayers();
    } catch (error) { setElementError(error instanceof Error ? error.message : String(error)); }
    finally { setLayerBusy(false); }
  }
  // Copy the selection, then paste it once beside the anchor: one undo step for the copies.
  async function duplicateSelection() {
    if (layerBusy || editingElement || nudging) return;
    const current = elementRef.current;
    const documents = current ? acrossDocuments(current) : null;
    if (current && documents) {
      setLayerBusy(true);
      try { await applyAcross(current, { kind: 'duplicate' }, documents); }
      catch { /* applyAcross reports the error */ }
      finally { setLayerBusy(false); }
      return;
    }
    setLayerBusy(true); setElementError('');
    try {
      await finishLayerDraft();
      await copySelection();
    } catch (error) { setElementError(error instanceof Error ? error.message : String(error)); return; }
    finally { setLayerBusy(false); }
    await pasteLayer();
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
      const pasted = result.selection.elements?.length ? result.selection.elements : result.selection.element ? [result.selection.element] : [];
      if (pasted.length) {
        const document = elementRef.current?.element.document ?? pageRef.current.regions.find(region => region.id === selected)?.frameTarget?.document ?? null;
        const targets = pasted.map(element => document ? { ...element, document } : element);
        pendingSelection.current = targets[0];
        storeElement(null);
        const expected = JSON.stringify([pastedPage.content, pastedPage.viewport, pastedPage.appearance]);
        const deadline = performance.now() + 4000;
        let rendered = false;
        do {
          await new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
          if (!ownsSelection()) return;
          if (acceptedVersionRef.current === expected) rendered = true;
          else if (rendered) return; // Undo or another edit owns the new page now.
          const items = rendered ? targets.flatMap(target => { const item = site.current?.find(target); return item ? [item] : []; }) : [];
          if (items.length) { applySelection(items); reveal(items[0].rect, false); refreshLayers(); return; }
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
      if (selectionsNow().length > 1) {
        const changes = selectionsNow().map(selection => {
          const styles = site.current!.layerStyles(selection.element, direction);
          if (!styles) throw new Error('Some selected items are already at that end of their layout.');
          return { selection, change: { kind: 'layerStyles', styles } as ElementEdit };
        });
        await applyBatch(changes);
      } else {
        const region = pageRef.current.regions.find(item => item.id === selected && !item.completed);
        if (region) await arrangeRequest(region.id, direction);
        else if (elementRef.current) {
          const styles = site.current!.layerStyles(elementRef.current.element, direction);
          if (styles) await applyElement({ kind: 'layerStyles', styles });
        } else throw new Error('Select a layer to arrange.');
      }
      refreshLayers();
    } catch (error) { setElementError(error instanceof Error ? error.message : String(error)); }
    finally { setLayerBusy(false); }
  }
  async function askFrameRequest(selections: ElementSelection[], chosen: PageSelection[] = []) {
    const document = selections[0].element.document;
    if (!document) return;
    const sameDoc = selections.filter(item => sameDocument(item.element.document, document));
    const frames = site.current?.frames() ?? [];
    const frame = frames.find(item => documentKey(item.document) === documentKey(document));
    if (!frame) throw new Error('This framed document is no longer shown. Select it again.');
    const hostRect = unionRect(sameDoc.map(item => item.rect));
    const innerRect = unionRect(sameDoc.map(item => pageRectToInnerDocument(frame.mapping, item.rect)));
    let iframe = frame.iframe;
    if (!iframe.id) iframe = await ensureElementId({ ...iframe }) ?? iframe;
    if (!iframe.id) throw new Error('This frame cannot be targeted yet. Try again.');
    const region = await add(hostRect);
    if (!region) return;
    await setFrameTarget(region.id, { pageId: pageRef.current.id, iframeId: iframe.id, document: frame.document, rect: innerRect });
    if (chosen.length) await changeRequest(region.id, { pages: chosen });
  }
  function askAI() {
    const current = elementRef.current;
    if (!current || editingElement || nudging) return;
    setContext(null);
    const selections = canonicalSelections([current, ...extrasRef.current]);
    // A framed element has no targets on the host page: the request names the document instead.
    if (selections.some(item => item.element.document)) { void askFrameRequest(selections, requestPages).catch(error => setElementError(error instanceof Error ? error.message : String(error))); setRequestPages([]); return; }
    void pickElement(selections, requestPages);
    setRequestPages([]);
  }
  // Select a website element by its structural selector (used by Page check), if the page has rendered.
  function focusElement(selector: string, tag: string, id?: string, expectedPageId?: string, document?: ElementTarget['document']) {
    if (expectedPageId && pageRef.current.id !== expectedPageId) return null;
    const sourceSha256 = site.current?.sourceOf({ selector, tag, label: '', ...(document ? { document } : {}) });
    if (!sourceSha256) return null;
    const found = site.current?.find({ selector, tag, label: '', ...(id ? { id } : { sourceSha256 }), ...(document ? { document } : {}) }) ?? null;
    if (found) { chooseElement(found); reveal(found.rect, false); }
    return found;
  }
  useImperativeHandle(ref, () => ({ liveHistory: action => liveSite.current?.liveHistory(action), focusElement, selectSimilar, addRequest: addRequestInView, focusRequest: focusRegion, askAI, undoElement: stepBackElement, copyLayer, cutLayer, pasteLayer, arrangeLayer, async synchronize(requireBounds = false) {
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
    // Alt-drag on empty space marquees; plain drag still pans and Shift-drag still draws an area.
    if (tool === 'select' && event.altKey && page.content && (!hit || hit.element.tag === 'body')) { beginMarquee(event, event.shiftKey || event.ctrlKey || event.metaKey); return; }
    if (tool === 'request' || event.shiftKey && (!hit || hit.element.tag === 'body')) { begin(event, 'new'); return; }
    if (event.shiftKey || event.ctrlKey || event.metaKey || repicking) { startPan(event, true); return; }
    const current = elementRef.current;
    const inSelection = current && selectionsNow().some(item => item.element.selector !== 'body' && at.x >= item.rect.x && at.x <= item.rect.x + item.rect.width && at.y >= item.rect.y && at.y <= item.rect.y + item.rect.height);
    if (!panKey.current && inSelection) beginElementGesture(event);
    else startPan(event, !panKey.current);
  }
  function beginElementGesture(event: PointerEvent, corner?: ResizeCorner) {
    if (event.button !== 0 || !element || editingElement || nudging || gesture.current || pan.current || elementDrag.current) return;
    if (panKey.current || tool === 'pan') { startPan(event, false); return; }
    if (tool === 'request') { begin(event, 'new'); return; }
    if ((event.shiftKey || event.ctrlKey || event.metaKey || repicking) && !corner) { startPan(event, true); return; }
    event.preventDefault(); event.stopPropagation(); setContext(null);
    const selections = selectionsNow();
    if (!corner && selections.length > 1) {
      try { commonDocument(selections); } catch (error) { setElementError(error instanceof Error ? error.message : String(error)); return; }
    }
    const group = !corner && selections.length > 1 ? selections.map(selection => ({ selection, mover: site.current?.beginMove(selection.element) ?? null })) : null;
    if (group?.some(item => !item.mover)) {
      group.forEach(item => item.mover?.cancel());
      setElementError('Some selected items cannot be moved here. Select them again.');
      return;
    }
    const mover = corner || group ? null : site.current?.beginMove(element.element) ?? null;
    const resizer = corner ? site.current?.beginResize(element.element, corner) ?? null : null;
    if (!mover && !resizer && !group) { setElementError('Choose a container to change this item’s layout.'); return; }
    const client = { x: event.clientX, y: event.clientY };
    elementDrag.current = { pointerId: event.pointerId, pageId: page.id, version: acceptedVersion, start: client, world: point(client), original: element, mover, resizer, group: group?.map(item => ({ selection: item.selection, mover: item.mover! })) ?? null, corner, active: false, fine: event.altKey, x: 0, y: 0, rect: element.rect };
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
      } else if (drag.group) {
        drag.x = gridSettings.snap && !fine ? Math.round((drag.original.rect.x + x) / step.x) * step.x - drag.original.rect.x : x;
        drag.y = gridSettings.snap && !fine ? Math.round((drag.original.rect.y + y) / step.y) * step.y - drag.original.rect.y : y;
        const movedGroup = drag.group.map(item => {
          const result = item.mover.preview(drag.x, drag.y);
          if (!result) throw new Error('This item left its editable page.');
          return { ...item.selection, rect: result.rect };
        });
        moved = movedGroup[0];
        expectSelections(movedGroup);
        storeElement(moved); setExtras(movedGroup.slice(1));
        return;
      } else {
        drag.x = gridSettings.snap && !fine ? Math.round((drag.original.rect.x + x) / step.x) * step.x - drag.original.rect.x : x;
        drag.y = gridSettings.snap && !fine ? Math.round((drag.original.rect.y + y) / step.y) * step.y - drag.original.rect.y : y;
        moved = drag.mover!.preview(drag.x, drag.y);
      }
      if (!moved) throw new Error('This item left its editable page.');
      expectSelections([moved]);
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
    if (settled && revision !== undefined) settledVersion.current = acceptedVersionRef.current;
    refreshLayers();
    setFrameView(page.content ? site.current?.frameOverlays() ?? { pageUrl: null, frames: [] } : { pageUrl: null, frames: [] });
    // Every box the edit aimed at is held until the settled measurement arrives, so a
    // multi-selection does not jump to the promotion's interim reading either.
    const extrasBefore = extrasRef.current;
    const foundExtras = extrasBefore.flatMap(item => { const found = site.current?.find(item.element); return found ? [{ found, before: item }] : []; });
    const extraRects = shownSelectionRects(foundExtras.map(entry => entry.found.rect), foundExtras.map(entry => expectedRect(entry.found.element) ?? entry.before.rect), settled);
    setExtras(foundExtras.map((entry, index) => ({ ...entry.found, rect: extraRects[index] })));
    for (const item of pendingImages.current) site.current?.previewImage(item.selection.element, item.src);
    setPlaceholders(page.content ? (site.current?.placeholders() ?? []).filter(item => !pendingImages.current.some(pending => sameTarget(pending.selection.element, item.element))) : []);
    refreshStaleTargets();
    const current = elementRef.current;
    if (current) {
      const found = site.current?.find(current.element);
      if (found) {
        pendingSelection.current = null;
        // The edit's own target box, or the box already drawn when there is none.
        const [rect] = shownSelectionRects([found.rect], [expectedRect(found.element) ?? current.rect], settled);
        if (settled) expectedSelections.current = [];
        storeElement({ ...found, rect });
      }
      else if (!dialogRef.current && (pendingSelection.current !== current.element || (site.current?.source() && !site.current?.find(current.element)))) deselectElement();
    }
    // Missing targets already have an actionable request banner. The Generate
    // path requires valid bounds and reports an error if any remain unresolved.
    // Request bounds come from the settled frame only; a promotion-time reading is not
    // what the canvas shows and must not be recorded.
    if (settled) void synchronizeBounds().catch(() => {});
  }
  function move(event: PointerEvent) {
    const client = { x: event.clientX, y: event.clientY };
    const band = marquee.current;
    if (band?.pointerId === event.pointerId) {
      if (!band.active && Math.hypot(client.x - band.clientStart.x, client.y - band.clientStart.y) < 3) return;
      band.active = true; plainClicks.current = 0;
      const now = point(client);
      setMarqueeRect({ x: Math.min(band.start.x, now.x), y: Math.min(band.start.y, now.y), width: Math.abs(now.x - band.start.x), height: Math.abs(now.y - band.start.y) });
      return;
    }
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
    if (marquee.current?.pointerId === event.pointerId) {
      const band = marquee.current; cancel(false);
      const end = point({ x: event.clientX, y: event.clientY });
      const rect = { x: Math.min(band.start.x, end.x), y: Math.min(band.start.y, end.y), width: Math.abs(end.x - band.start.x), height: Math.abs(end.y - band.start.y) };
      if (band.active && rect.width >= 2 && rect.height >= 2) {
        try {
          const items = site.current?.elementsWithin(rect) ?? [];
          const next = marqueeSelections(items, rect, band.base);
          applySelection(next);
          if (items.length > MAX_SELECTION) setElementError(`Select up to ${MAX_SELECTION} elements at a time.`);
        } catch (error) { setElementError(error instanceof Error ? error.message : String(error)); }
      }
      return;
    }
    if (elementDrag.current?.pointerId === event.pointerId) {
      updateElementDrag({ x: event.clientX, y: event.clientY }, event.altKey);
      const drag = elementDrag.current; if (!drag) return;
      let css: string | null = null;
      let groupStyles: string[] | null = null;
      try {
        if (drag.active && (drag.resizer ? !sameRect(drag.rect, drag.original.rect) : drag.x || drag.y)) {
          if (drag.group) groupStyles = drag.group.map(item => item.mover.style(drag.x, drag.y));
          else css = drag.resizer ? drag.resizer.style(drag.rect) : drag.mover!.style(drag.x, drag.y);
        }
      } catch (error) { setElementError(String(error)); }
      // Retain the gesture's final pixels and bounds until the committed preview swaps in.
      const placed = elementRef.current;
      const group = drag.group;
      const placedGroup = [elementRef.current, ...extrasRef.current].flatMap(item => item ? [item] : []);
      cancel(drag.active || !!drag.corner, css === null && groupStyles === null);
      if (groupStyles !== null && group) {
        void applyBatch(group.map((item, index) => ({ selection: placedGroup[index] ?? item.selection, change: { kind: 'update', texts: [], attributes: {}, style: groupStyles![index] } as ElementEdit })))
          .catch(() => { group.forEach(item => item.mover.cancel()); refreshElement(); });
      }
      else if (css !== null) void applyElement({ kind: 'update', texts: [], attributes: {}, style: css }, placed ?? drag.original).catch(() => { drag.mover?.cancel(); drag.resizer?.cancel(); refreshElement(); });
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
      if (current.shapeKind === 'freehand') { if (current.active && rect && current.freehand) createRequestInBackground(rect, current.freehand); }
      else if (current.active && rect && rect.width >= 1 && rect.height >= 1) createRequestInBackground(current.shapeKind === 'rect' ? requestPlacement(rect, zoom) : rect, { kind: current.shapeKind });
      else if (!current.active && tool === 'request') { const at = requestAt(current.start, zoom); createRequestInBackground(current.shapeKind === 'ellipse' ? { ...at, height: at.width } : at, { kind: current.shapeKind }); }
      else if (tool === 'select' && page.content) { const at = point({ x: event.clientX, y: event.clientY }); toggleElement(site.current?.pick(at.x, at.y) ?? null); }
      return;
    }
    if (current.region && rect && !sameRect(rect, current.initial)) void changeRequest(current.region.id, { rect }).catch(error => setElementError(error instanceof Error ? error.message : String(error)));
  }
  function finishPlainClick(client: Point, plain: boolean, additive = false) {
    if (repicking && page.content) {
      const at = point(client); const hit = site.current?.pick(at.x, at.y) ?? null;
      if (hit?.element.document) { setElementError('Choose an element on the page itself. Draw a request over the frame to change what is inside it.'); return; }
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
      // A frame itself has no text to edit: a double-click over it places a request.
      if (hit && hit.element.tag !== 'body' && hit.element.tag !== 'iframe') { chooseElement(hit); void editTextAt(at); return; }
      if (hit?.element.tag === 'iframe') { createRequestInBackground(requestAt(at, zoom)); return; }
    }
    const snapped = geometryForGesture({ kind: 'new', start: at, initial: { ...at, width: 0, height: 0 } }, at, page.viewport, gridSettings);
    createRequestInBackground(requestAt({ x: snapped.x, y: snapped.y }, zoom));
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
      createRequestInBackground(requestAt({ x: snapped.x, y: snapped.y }, zoom)); return;
    }
    if (!region && element) {
      if (extrasRef.current.length) {
        if (event.key === 'Delete' || event.key === 'Backspace') { event.preventDefault(); void deleteSelected().catch(error => setElementError(String(error))); return; }
        if (['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(event.key)) {
          event.preventDefault(); const step = event.shiftKey ? 10 : 1;
          nudgeElement(event.key === 'ArrowLeft' ? -step : event.key === 'ArrowRight' ? step : 0, event.key === 'ArrowUp' ? -step : event.key === 'ArrowDown' ? step : 0); return;
        }
        return;
      }
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
    void changeRequest(region.id, { rect: keyboardRectChange(direction, page.viewport, gridSettings, resizing, event.altKey, region.shape && region.shape.kind !== 'rect' ? { x: 1, y: 1 } : minimum) }).then(updated => reveal(updated.rect, resizing)).catch(() => {}).finally(() => setNudging(false));
  }

  useLayoutEffect(() => {
    const node = toolbar.current; if (!node) return;
    const measure = () => { const rect = node.getBoundingClientRect(); if (!rect.width || !rect.height) return; setToolbarSize(current => current.width === rect.width && current.height === rect.height ? current : { width: rect.width, height: rect.height }); };
    measure(); const observer = new ResizeObserver(measure); observer.observe(node); return () => observer.disconnect();
  }, [!!element, !!selected, !!inlineText]);
  const visible = { x: bounds.left + measurements.left / zoom, y: bounds.top + measurements.top / zoom };
  // Frames on screen can offer Interact; none run merely because they are visible.
  const frameViewport = { x: visible.x, y: visible.y, width: measurements.width / zoom, height: measurements.height / zoom };
  const visibleFrames = useMemo(() => page.content && !liveView ? placeOverlays(frameView.frames, frameViewport) : [],
    [frameView, frameViewport.x, frameViewport.y, frameViewport.width, frameViewport.height, liveView, page.content]);
  const overlaySelection = element?.element.tag === 'iframe' && !extras.length ? visibleFrames.find(frame => frame.selector === element.element.selector) ?? null : null;
  const interacting = !!overlaySelection && interactingFrame === overlaySelection.key;
  // Every frame stays frozen while editing. Only an explicit Interact action
  // mounts a running overlay, including external and runtime-only embeds.
  const overlayFrames = useMemo(() => interacting && overlaySelection ? [overlaySelection] : [], [interacting, overlaySelection]);
  const frameSession = useLiveFrameSession(page.id, acceptedVersion, overlayFrames.length > 0);
  const liveOverlays = frameSession ? overlayFrames : [];
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
  const shapedPrompt = selectedRequest?.shape && selectedRequest.shape.kind !== 'rect' ? inViewport(attachedRequestRect(preview ?? selectedRequest.rect, zoom, frameViewport, Math.max(160, measurements.height * .55) / zoom)) : null;
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
  // Mixed values across a multi-selection are shown as mixed instead of as the first item's value.
  const styleVisual = element ? site.current?.visual(element.element) ?? null : null;
  const multiVisuals = extras.length ? [element, ...extras].flatMap(item => item ? [site.current?.visual(item.element) ?? null] : []) : [];
  const mixedStyle = <K extends keyof ElementVisual>(key: K) => multiVisuals.length > 1 && multiVisuals.some(value => (value?.[key] ?? '') !== (multiVisuals[0]?.[key] ?? ''));
  const styleMixed: Partial<Record<keyof ElementVisual, boolean>> = extras.length ? {
    color: mixedStyle('color'), backgroundColor: mixedStyle('backgroundColor'), fontSize: mixedStyle('fontSize'), padding: mixedStyle('padding'), textAlign: mixedStyle('textAlign'),
  } : {};
  // Requests line up with the design only at the page's own width, so they show (and can be added) there.
  const liveRequests = liveShown.requests;
  // Number badges stack upward when request boxes overlap, and move inside a box that sits
  // too close to the top of the workspace for the badge to fit above it.
  const requestBadgeTops = (() => {
    const placed: Rect[] = [];
    const tops = new Map<string, number>();
    for (const region of page.regions) {
      if (region.completed) continue;
      const boxTop = (region.rect.y - bounds.top) * zoom;
      const left = (region.rect.x + region.rect.width - bounds.left) * zoom - 18;
      let inside = boxTop < 22;
      let top = inside ? boxTop + 2 : boxTop - 20;
      let badge = { x: left, y: top, width: 18, height: 16 };
      const overlaps = () => placed.some(other => badge.x < other.x + other.width && badge.x + badge.width > other.x && badge.y < other.y + other.height && badge.y + badge.height > other.y);
      for (let attempt = 0; attempt < 8 && overlaps(); attempt += 1) { top += inside ? 18 : -18; badge = { ...badge, y: top }; if (!inside && top < 2) { inside = true; top = boxTop + 2; badge = { ...badge, y: top }; } }
      placed.push(badge);
      tops.set(region.id, top - boxTop);
    }
    return tops;
  })();
  // Keep callbacks stable so prompt edits cannot re-render the memoized live layer.
  const fixRuntimeErrorsRef = useRef(fixRuntimeErrors); fixRuntimeErrorsRef.current = fixRuntimeErrors;
  const stableFixRuntimeErrors = useCallback((errors: string[]) => fixRuntimeErrorsRef.current?.(errors), []);
  const stopInteracting = useCallback(() => setInteractingFrame(null), []);
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
      { id: 'select-similar', label: 'Select similar', shortcut: 'Ctrl/Cmd+Shift+A', disabled: editBusy, action: selectSimilar },
      ...(fixOnPhones && elementRef.current && elementRef.current.element.selector !== 'body' && !elementRef.current.element.document ? [{ id: 'phones', label: 'Fix on phones', disabled: editBusy, action: () => { const current = elementRef.current; if (current) fixOnPhones(current); } }] : []),
      { id: 'replace', label: 'Replace', disabled: editBusy, action: () => setElementDialog('replace') },
      ...(undoElement && elementRef.current && elementRef.current.element.selector !== 'body' ? [{ id: 'undo-element', label: 'Undo changes to this element', shortcut: 'Alt+Z', disabled: editBusy, action: () => { void stepBackElement(); } }] : []),
    ] : context.kind === 'request' ? [{ id: 'edit', label: 'Edit request', disabled: editBusy, action: () => focusRegion(context.regionId) }] : [{ id: 'add', label: 'Add request here', action: () => { deselectElement(); createRequestInBackground(requestAt(context.at, zoom)); } }]),
    ...(context.kind === 'request' ? [{ id: 'whole-page', label: 'Whole page', checked: page.regions.some(item => item.id === context.regionId && isWholePage(item, page)), action: () => { const region = page.regions.find(item => item.id === context.regionId); if (region) void change(region.id, { wholePage: !isWholePage(region, page) }).catch(() => {}); } }] : []),
    ...(context.kind === 'request' && regionTargets(page.regions.find(item => item.id === context.regionId)).length ? [{ id: 'allow-target-removal', label: 'Allow AI to remove selected items', checked: !!page.regions.find(item => item.id === context.regionId)?.allowTargetRemoval, action: () => setTargetRemoval(context.regionId, !page.regions.find(item => item.id === context.regionId)?.allowTargetRemoval) }] : []),
    ...(context.kind !== 'canvas' ? [
      { id: 'copy', label: 'Copy', disabled: editBusy, shortcut: 'Ctrl/Cmd+C', action: () => { void copyLayer(); } },
      { id: 'cut', label: 'Cut', disabled: editBusy, shortcut: 'Ctrl/Cmd+X', action: () => { void cutLayer(); } },
      { id: 'duplicate', label: 'Duplicate', disabled: editBusy, action: () => { void duplicateSelection(); } },
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
    <p className="sr-only" id="canvas-keyboard-help">Click a website element to select it. Shift, Control or Command click adds or removes a selection, up to 500 elements. Drag the selected element to move it; drag a corner to resize it. Double-click text to edit it in place. Enter saves text; Escape cancels. Ask AI in the right-click menu, or Ctrl or Command K, opens a change request. Double-click empty canvas to add a request. Select, Add request and Pan tools are in the floating canvas toolbar, with V, R and H shortcuts. In Add request, click to place a text box or drag to draw an area. Left-drag on empty space pans; Space or middle drag pans anywhere. Alt-drag on empty space selects everything the rectangle touches; add Shift to keep the current selection. Shift+drag draws an area. Ctrl or Command with the wheel zooms under the pointer. F2 opens local element settings. Arrow keys move an element one pixel; Shift moves ten. Enter selects the next element; Alt+Enter selects its parent. Ctrl or Command Shift A selects every item similar to the selected one. Also on pages in the element toolbar changes the element on the other pages that have it, and its inline list chooses which pages. Shift+Enter adds a request in view. Escape from a request focuses its move grip. Arrow keys on grips move or resize by the grid setting. Hold Alt for one-pixel changes. Delete on a grip removes its request. Home returns to the screen guide. Escape cancels a drag.</p>
    {(() => {
      const key = `page:${announcedJob?.jobId ?? 'start'}:${page.id}`;
      const outcome = jobOutcome(announcedJob, id => page.regions.find(region => region.id === id)?.prompt);
      const gap = outcome.blocked[0];
      const busy = problemBusy || !!generation?.operation || generation?.canStop;
      const failed = (!problems.length && generation?.error) || (announcedJob && ['failed', 'unknown', 'blocked'].includes(announcedJob.status) && !problems.length && !announcedJob.drafts?.some(d => d.applied || d.dismissed) ? generationPageError(announcedJob.message) : '');
      if (runtimeProblems.length && !dismissedProblems.includes(`runtime:${key}`)) return <ProblemBanner message={runtimeError(runtimeProblems[0])} action={{ label: 'Fix with AI', disabled: aiDisabled, onClick: () => fixRuntimeErrors?.(runtimeProblems) }} dismiss={() => dismissProblem(`runtime:${key}`)} />;
      if (!failed || dismissedProblems.includes(key)) return null;
      if (gap) return <ProblemBanner message={gap.reason}
        action={requestEditorOpen(gap.regionId, { selectedRegionId: selected, focusedRegionId: focusRequest }) ? undefined : { label: 'Change request', disabled: busy || !!generation?.operation, onClick: () => { select(gap.regionId); setRequestFocus(previous => ({ id: gap.regionId, serial: previous.serial + 1 })); } }}
        secondaryAction={gap.fix && resolveGenerationFix ? { label: fixLabel(gap.fix), onClick: () => resolveGenerationFix(gap.fix!, gap.regionId) } : undefined}
        dismiss={() => dismissProblem(key)} />;
      return <ProblemBanner message={userError(failed)} action={outcome.retryable.length || outcome.retryAll ? { label: 'Try again', disabled: aiDisabled, onClick: () => { void generation?.prepare(undefined, outcome.retryable.length ? outcome.retryable : undefined).catch(() => {}); } } : undefined} dismiss={() => dismissProblem(key)} />;
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
          {page.content && <div className={`accepted-canvas-preview${liveShown.staticPage ? '' : ' behind-live'}`} data-testid="accepted-canvas-preview" style={positioned({ x: 0, y: 0, width: page.viewport.width, height: previewHeight })} aria-hidden="true">
            <div className="accepted-canvas-content" style={{ width: page.viewport.width, height: previewHeight, transform: `scale(${zoom})` }}>
              <PreviewFrame ref={site} content={page.content} frameDocuments={frameDocuments} referenceHeight={page.viewport.height} onHeight={setSiteHeight} onReady={(settled, revision) => refreshElement(settled, revision)} pageId={page.id} revision={acceptedVersion} title={`${page.name} accepted static preview`} staticView />
            </div>
          </div>}
          {liveView && page.content && <LiveCanvasLayer pageId={page.id} pageName={page.name} revision={`live:${acceptedVersion}`} width={liveView.width} height={liveView.height}
            bounds={bounds} zoom={zoom} passive={tool === 'request' || panReady || panning} aiDisabled={!!aiDisabled} observationsVersion={observationsVersion} frameRef={liveSite}
            onLiveLocation={onLiveLocation} onRuntimeProblems={setRuntimeProblems} fixRuntimeErrors={stableFixRuntimeErrors} />}
          {!liveView && frameSession && <FrameLayer frames={liveOverlays} session={frameSession} pageId={page.id} pageName={page.name} staticPageUrl={frameView.pageUrl}
            bounds={bounds} zoom={zoom} interactingFrame={interactingFrame} interactiveSelector={tool === 'select' && element ? element.element.selector : null} stop={stopInteracting} />}
          <div className={`screen-guide ${page.content ? 'has-preview' : ''}${liveShown.screenGuide ? '' : ' behind-live'}`} data-testid="screen-guide" style={positioned({ x: 0, y: 0, ...page.viewport })} aria-hidden="true">
            <span className="screen-guide-label">Screen · {page.viewport.width} × {page.viewport.height}</span>
          </div>
          {grid && !liveView && <div className="canvas-grid" aria-hidden="true" style={{ ...positioned({ x: 0, y: 0, width: page.viewport.width, height: Math.max(page.viewport.height, page.content ? previewHeight : 0) }), backgroundSize: `${displayStep.x * zoom}px ${displayStep.y * zoom}px`, backgroundPosition: '0 0' }} />}
          {!page.content && !page.regions.some(region => !region.completed) && <div className="canvas-empty-hint" style={positioned({ x: 0, y: 0, ...page.viewport })}>
            <strong>This page is empty</strong><button type="button" className="primary" onClick={addRequestInView}>Add request</button>
          </div>}
          {liveRequests && page.regions.map((region, index) => {
            if (region.completed) return null;
            const rect = gesture.current?.region?.id === region.id && preview ? preview : region.rect;
            return <RequestBox reports={archiveReports[`${projectId}:${region.id}`] ?? []} key={region.id} wholePage={isWholePage(region, page)} region={region} number={index + 1} rect={rect} bounds={bounds} zoom={zoom} viewport={frameViewport} maxHeight={Math.max(160, measurements.height * 0.55)} selected={selected === region.id} focus={focusRequest === region.id} focusToken={requestFocus.id === region.id ? requestFocus.serial : 0} badgeTop={requestBadgeTops.get(region.id)}
              pages={pages} hostPageId={page.id} layer={requestRanks.get(region.id)!} controlsLayer={overlayLayer + 1} stale={stale.has(region.id)} repickNow={() => { deselectElement(); setRepicking(region.id); }} removeNow={() => remove(region.id)} attach={attach ? (files, links) => attach(region.id, files, links) : undefined} detach={detach ? id => detach(region.id, id) : undefined} select={() => { deselectElement(); select(region.id); }} begin={begin} keyboard={keyboard} change={patch => changeRequest(region.id, patch)} />;
          })}
          {element && !liveView && <div className="element-selection direct-selection" data-testid="direct-element-selection" data-selector={element.element.selector} data-element-id={element.element.id} data-source-hash={element.element.sourceSha256} style={{ ...paintStyle(element.rect, bounds, zoom), zIndex: overlayLayer + 1 }} role="group" aria-label="Selected element bounds">
            <span className="element-type-tag">{elementName(element.element.tag)}{foundUrl && <span className="found-tag" title={foundUrl}> · Found online · {foundHost}</span>}</span>
            {!extras.length && element.element.selector !== 'body' && (['nw','ne','sw','se'] as const).map(corner => <button key={corner} type="button" className={`element-resize-corner corner-${corner}`} data-corner={corner} aria-label={`Resize ${corner.includes('n') ? 'top' : 'bottom'}-${corner.includes('w') ? 'left' : 'right'}`} title="Drag to resize; press Enter for size settings" disabled={editingElement || nudging} onPointerDown={event => beginElementGesture(event, corner)} onClick={event => { if (event.detail === 0) setElementDialog('edit'); }} />)}
          </div>}
          {extras.map(item => <div key={item.element.id ?? item.element.selector} data-testid="multi-element-selection" data-element-id={item.element.id} className="element-selection multi-selection" style={{ ...paintStyle(item.rect, bounds, zoom), zIndex: overlayLayer + 1 }} aria-hidden="true" />)}
          {inlineText && <InlineTextEditor key={`${inlineText.selection.element.sourceSha256}:${inlineText.index}`} text={inlineText} origin={origin} zoom={zoom} layer={overlayLayer + 4} disabled={editingElement} finish={finishText} />}
          {placeCallouts(problems, page.regions.map(region => region.rect), origin, zoom, dismissedProblems).map(position => {
            const problem = problems.find(item => item.key === position.key)!;
            const request = page.regions.find(region => region.id === problem.regionId);
            const error = problemErrors[problem.key];
            const busy = problemBusy || !!generation?.operation || generation?.canStop;
            return <div key={problem.key} role="alert" className="area-problem" style={{ left: position.x, top: position.y, width: position.width, height: position.height, zIndex: overlayLayer + 5 }} onPointerDown={event => event.stopPropagation()}>
              <span>{error ? generationError(error, request?.prompt) : problem.message}</span>
              <div>{!problem.retryable ? <>
                {allowWholePage && request && !isWholePage(request, page) && declaredConflict(generation?.job ?? null, problem.regionId) && <button disabled={busy || aiDisabled} onClick={() => { void allowWholePage(problem.regionId).catch(() => {}); }}>Allow whole page and retry</button>}
                {!requestEditorOpen(problem.regionId, { selectedRegionId: selected, focusedRegionId: focusRequest }) && <button disabled={busy} onClick={() => { select(problem.regionId); setRequestFocus(previous => ({ id: problem.regionId, serial: previous.serial + 1 })); }}>Change request</button>}
                {problem.fix && resolveGenerationFix && <button disabled={busy} onClick={() => resolveGenerationFix(problem.fix!, problem.regionId)}>{fixLabel(problem.fix)}</button>}
              </> : <>
                {problem.draftId && <button disabled={busy || !!error} onClick={() => { void applyProblem(problem.regionId, problem.draftId!, problem.key); }}>Apply anyway</button>}
                <button disabled={busy || aiDisabled} onClick={() => { void generation?.prepare(undefined, [problem.regionId]).catch(() => {}); }}>Try again</button>
                {!problem.stale && !error && !requestEditorOpen(problem.regionId, { selectedRegionId: selected, focusedRegionId: focusRequest }) && <button disabled={busy} onClick={() => { select(problem.regionId); setRequestFocus(previous => ({ id: problem.regionId, serial: previous.serial + 1 })); }}>Change request</button>}
              </>}
              <button className="icon-button sm" aria-label="Dismiss problem" title="Dismiss" onClick={() => dismissProblem(problem.key)}><Icon name="close" /></button></div>
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
          {marqueeRect && <div className="marquee-selection" data-testid="marquee-selection" style={{ ...paintStyle(marqueeRect, bounds, zoom), zIndex: overlayLayer + 2 }} aria-hidden="true" />}
        </div>
      </div>
    </div>
    {toolbarRect && !inlineText && !(liveView && element) && <FloatingBar ref={toolbar} className="element-toolbar" role="toolbar" aria-label={element ? 'Selected element' : 'Selected request'} style={toolbarPosition}>
      {confirmMany ? <div className="delete-inline" role="group" aria-label="Delete items">
        <span>Delete {confirmMany.count} items?</span>
        <button type="button" className="danger" onClick={() => { if (confirmMany.skip) setAskBeforeDelete(false); setConfirmMany(null); void (confirmMany.pageId === page.id ? deleteSelected(true, confirmMany.targets) : Promise.resolve()).catch(error => setElementError(error instanceof Error ? error.message : String(error))); }}>Delete</button>
        <button type="button" onClick={() => setConfirmMany(null)}>Cancel</button>
        <details><summary>Options</summary><label className="check-label"><input type="checkbox" checked={confirmMany.skip} onChange={event => setConfirmMany({ ...confirmMany, skip: event.target.checked })} />Don’t ask again</label></details>
      </div> : <>
      {!element && selectedRequest && <button className="ask-ai" aria-pressed={isWholePage(selectedRequest, page)} disabled={editBusy} title="Allow this request to change the page layout, CSS and JavaScript" onClick={() => { void change(selectedRequest.id, { wholePage: !isWholePage(selectedRequest, page) }).catch(() => {}); }}>Whole page</button>}
      {extras.length > 0 ? <>
        <span className="multi-count">{extras.length + 1} selected</span>
        <button className="ask-ai" title={withShortcut('Ask AI', 'askAI')} disabled={editBusy} onClick={askAI}>Ask AI</button>
        {element && <button className="ask-ai" aria-pressed={styleOpen} title="Show or hide quick style controls" onClick={toggleStyle}>Style</button>}
        <button className="wide" disabled={editBusy} title={withShortcut('Select similar', 'selectSimilar')} onClick={selectSimilar}>Select similar</button>
        <button className="wide" disabled={editBusy} title="Copy the selection beside itself" onClick={() => { void duplicateSelection(); }}>Duplicate</button>
        <button className="wide" disabled={editBusy} title="Give every selected item the first item’s width" onClick={() => { void resizeSelection('width'); }}>Same width</button>
        <button className="wide" disabled={editBusy} title="Give every selected item the first item’s height" onClick={() => { void resizeSelection('height'); }}>Same height</button>
        <button className="icon-button" aria-label="Delete selected items" title={withShortcut('Delete selected items', 'remove')} disabled={editBusy} onClick={() => { void deleteSelected().catch(error => setElementError(error instanceof Error ? error.message : String(error))); }}><QuickIcon name="delete" /></button>
        {element && styleOpen && <StyleBar key={`multi:${extras.length}:${element.element.selector}`} visual={styleVisual} mixed={styleMixed} busy={editBusy} apply={change => { void applyStyles(change); }} />}
      </> : <>
      {element?.element.tag === 'img' ? <>
        <button className="ask-ai" title="Choose a picture from your computer, or drop one onto the image" disabled={editBusy} onClick={() => browseImage(elementRef.current)}>Replace image</button>
        <button className="ask-ai" title={withShortcut('Ask AI', 'askAI')} disabled={editBusy} onClick={askAI}>Ask AI</button>
      </> : element && <>
        {element.element.tag === 'iframe' && overlaySelection && <button className="ask-ai" aria-pressed={interacting} title={interacting ? 'Stop playing this frame' : 'Play this frame in place'}
          onClick={() => setInteractingFrame(value => value === overlaySelection.key ? null : overlaySelection.key)}>{interacting ? 'Done' : 'Interact'}</button>}
        {elementDialog !== 'edit' && <button className="ask-ai" title={withShortcut('Change text, links and style', 'editElement')} disabled={editBusy} onClick={() => setElementDialog('edit')}>Edit</button>}
        <button className="wide" title={withShortcut('Select similar', 'selectSimilar')} disabled={editBusy} onClick={selectSimilar}>Select similar</button>
        <button className="ask-ai" title={withShortcut('Ask AI', 'askAI')} disabled={editBusy} onClick={askAI}>Ask AI</button>
        {element.element.selector !== 'body' && <button className="ask-ai" aria-pressed={styleOpen} title="Show or hide quick style controls" onClick={toggleStyle}>Style</button>}
      </>}
      {element && element.element.selector !== 'body' && acrossMatches.length > 0 && <>
        <button className="wide" aria-pressed={across.enabled} title="Apply the next changes to this element on the pages that have it" disabled={editBusy} onClick={() => setAcross(current => setAcrossPages(current, !current.enabled))}>{acrossPagesLabel(acrossMatches.length)}</button>
        {across.enabled && <div className="across-pages" role="group" aria-label="Pages that change">
          {acrossMatches.map(match => { const key = documentKey(match.document); return <label key={key} className="check-label"><input type="checkbox" checked={!across.excluded.includes(key)} onChange={event => setAcross(current => toggleAcrossPage(current, match.document, event.target.checked))} />{match.label || key}</label>; })}
        </div>}
      </>}
      {element && foundUrl && openSource && !element.element.document && <button className="ask-ai" title={`Open where this came from: ${foundUrl}`} onClick={() => openSource(element.element)}>Found online{foundHost ? ` · ${foundHost}` : ''} <Icon name="external" size={12} /></button>}
      {element && undoElement && element.element.selector !== 'body' && <button className="icon-button" aria-label="Undo changes to this element" title={withShortcut('Undo changes to this element', 'undoElement')} disabled={editBusy} onClick={() => { void stepBackElement(); }}><QuickIcon name="undo" /></button>}
      {element && <RequestPages pages={pages} hostPageId={page.id} selection={requestPages} change={setRequestPages} />}
      <button className="icon-button" aria-label="Delete layer" title={withShortcut('Delete layer', 'remove')} disabled={editBusy || element?.element.selector === 'body'} onClick={() => { if (element) deleteElement(); else if (selected) remove(selected); }}><QuickIcon name="delete" /></button>
      {element && styleOpen && !layersOpen && element.element.tag !== 'img' && element.element.selector !== 'body' && <StyleBar key={element.element.selector} visual={styleVisual} busy={editBusy} apply={change => { void applyStyles(change); }} />}
      </>}
      </>}
    </FloatingBar>}
    {context && <ContextMenu key={`${context.kind}:${context.point.x}:${context.point.y}`} label={context.kind === 'element' ? 'Element actions' : context.kind === 'request' ? 'Request actions' : 'Canvas actions'} point={context.point} close={closeContext} items={contextItems} />}
    <LayersPopover open={layersOpen} page={page} siteLayers={siteLayers} selectedElement={element ? targetKey(element.element) : null} selectedRequest={selected} busy={layerBusy || editingElement || nudging} ready={layersReady} anchor={toolbarPoint ? { x: toolbarPoint.x, y: toolbarPoint.y, height: toolbarSize.height } : null} contextElement={openElementContext} contextRequest={openRequestContext}
      chooseElement={item => { chooseElement(item); reveal(item.rect, false); }} chooseRequest={item => { deselectElement(); select(item.id); reveal(item.rect, false); }}
      arrange={direction => { void arrangeLayer(direction); }} copy={() => { void copyLayer(); }} paste={() => { void pasteLayer(); }} close={() => setLayersOpen(false)} />
    <input ref={imagePicker} type="file" hidden accept="image/png,image/jpeg,image/webp,image/gif" onChange={event => { const file = event.target.files?.[0]; const target = browseFor.current; event.target.value = ''; if (file && target) void stageImage(target, file); }} />
    {repicking && <div className="floating-bar repick-hint" role="status">Click the element this request is for. <button type="button" onClick={() => setRepicking(null)}>Cancel</button></div>}

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

function RequestBox({ reports, wholePage, region, number, pages, hostPageId, rect, bounds, zoom, viewport, maxHeight, selected, focus, focusToken, badgeTop, layer, controlsLayer, select, begin, keyboard, change, stale = false, repickNow, removeNow, attach, detach }: {
  reports: import('./types').ArchiveReport[]; wholePage: boolean; pages: Page[]; hostPageId: string;
  stale?: boolean; repickNow?: () => void; removeNow?: () => void; attach?: (files: File[], links: string[]) => void; detach?: (attachmentId: string) => void;
  region: Region; number: number; rect: Rect; bounds: WorkspaceBounds; zoom: number; viewport: Rect; maxHeight: number; selected: boolean; focus: boolean; focusToken: number; badgeTop?: number; layer: number; controlsLayer: number; select: () => void;
  begin: (event: PointerEvent, kind: Gesture['kind'], region?: Region) => void;
  keyboard: (event: KeyboardEvent, region?: Region, resize?: boolean) => void; change: (patch: RequestPatch) => Promise<Region>;
}) {
  const text = useRef<HTMLTextAreaElement>(null);
  const move = useRef<HTMLButtonElement>(null);
  // Typing stays local; a short pause saves once instead of sending every keystroke to the
  // workspace, where each save journals the whole project and republishes the view.
  const [draft, changeDraft, flushDraft] = useDraftText(region.prompt, (_value, latest) => change({ prompt: latest }).then(updated => updated.prompt), 400);
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
  const framed = !!region.frameTarget;
  const attached = target || framed;
  const shaped = !target && !!region.shape && region.shape.kind !== 'rect';
  const promptRect = requestPlacement(attached || shaped ? attachedRequestRect(rect, zoom, viewport) : rect, zoom);
  useLayoutEffect(() => {
    const node = text.current; if (!node) return;
    const old = node.style.height; node.style.height = '0px'; const style = getComputedStyle(node);
    const height = node.scrollHeight + (Number.parseFloat(style.borderTopWidth) || 0) + (Number.parseFloat(style.borderBottomWidth) || 0); node.style.height = old;
    setNaturalHeight(current => current === height ? current : height);
  }, [draft, promptRect.width, zoom]);
  const minimumHeight = promptRect.height * zoom;
  // Text keeps its own height; attachments and actions add normal-flow rows below it.
  const collapseHeight = attached || shaped ? Math.max(minimumHeight, maxHeight) : minimumHeight;
  const fullHeight = Math.max(minimumHeight, naturalHeight);
  const hasMore = fullHeight > collapseHeight;
  useLayoutEffect(() => { if (attached && hasMore && document.activeElement === text.current) setExpanded(true); }, [attached, hasMore, draft]);
  const displayHeight = expanded ? fullHeight : Math.min(collapseHeight, fullHeight);
  const cardHeight = (displayHeight + (hasMore ? 30 : 0)) / zoom;
  // Placement uses the card's displayed height, so a card that had to move up stays fully in view.
  const shown = { ...(attached || shaped ? attachedRequestRect(rect, zoom, viewport, cardHeight) : promptRect), height: cardHeight };
  // Only placement controls are clipped to the workspace. The card grows below
  // its text without changing the area's geometry or clipping wrapped attachments.
  const painted = clipToWorkspace(shown, bounds);
  const at = (x: number, y: number) => x >= bounds.left && x <= bounds.right && y >= bounds.top && y <= bounds.bottom;
  const moveVisible = at(rect.x, rect.y);
  const resizeVisible = at(rect.x + rect.width, rect.y + rect.height);
  return <>
  {shaped && <RegionOutline shape={region.shape!} rect={rect} bounds={bounds} zoom={zoom} layer={layer} selected={selected} regionId={region.id} onPointerDown={event => { if (event.button !== 0) return; event.preventDefault(); event.stopPropagation(); select(); }} />}
  {selected && !target && !shaped && <div className="element-selection request-placement" style={{ ...paintStyle(rect, bounds, zoom), zIndex: controlsLayer }} aria-hidden="true" />}
  {(target || framed) && <div className="element-selection" data-testid="element-selection" style={{ ...paintStyle(rect, bounds, zoom), zIndex: layer }} aria-hidden="true" />}
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
    {!attached && <span className="request-number" aria-hidden="true" style={badgeTop === undefined ? undefined : { top: badgeTop }}>{number}</span>}
    {wholePage && <span className="request-phone-tag" title={region.wholePage ? "Whole page" : "Whole page (automatically detected). Turn off in request controls."}>Whole page</span>}
    {region.mobileOnly && <span className="request-phone-tag">Phone fix</span>}
    <textarea ref={text} aria-label={`Request ${number}`} aria-describedby="canvas-keyboard-help" maxLength={16384} value={draft} placeholder={targets.length === 1 && targets[0].tag === 'img' ? 'Describe the picture…' : target ? 'Describe the change…' : 'Describe what goes here…'}
      style={{ height: displayHeight, borderWidth: 0, outline: 'none' }} onFocus={() => { if (attached && hasMore) setExpanded(true); }} onBlur={flushDraft} onChange={event => { changeDraft(event.target.value); if (attached && hasMore) setExpanded(true); }} onPointerDown={event => event.stopPropagation()}
      onKeyDown={event => { if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); (moveVisible ? move.current : document.getElementById('page-canvas'))?.focus({ preventScroll: true }); } }} />
    {hasMore && <button type="button" className="request-expand" aria-expanded={expanded} onClick={() => setExpanded(value => !value)}>{expanded ? 'Less' : '… More'}</button>}
    {(attachments.length > 0 || reports.length > 0) && <div className="request-attachment-group">
    {attachments.length > 0 && <ul className="request-attachments" aria-label="Attachments" onPointerDown={event => event.stopPropagation()}>{(attachmentsExpanded ? attachments : attachments.slice(0, 3)).map(item => <li key={item.id} title={item.kind === 'link' ? item.url : item.name}>
      {item.kind === 'image' && item.dataUri ? <img src={item.preview ?? item.dataUri} alt="" /> : <span className="attachment-icon" aria-hidden="true"><Icon name={item.kind === 'link' ? 'link' : 'text'} size={12} /></span>}
      <span className="attachment-name">{item.name}</span>{detach && <button type="button" className="icon-button sm" aria-label={`Remove ${item.name}`} title={`Remove ${item.name}`} onClick={() => detach(item.id)}><Icon name="close" /></button>}
    </li>)}
      {attachments.length > 3 && <li><button type="button" aria-expanded={attachmentsExpanded} aria-label={attachmentsExpanded ? 'Show fewer attachments' : `Show ${attachments.length - 3} more attachments`} onClick={() => setAttachmentsExpanded(value => !value)}>{attachmentsExpanded ? 'Less' : `+${attachments.length - 3}`}</button></li>}
    </ul>}
    {reports.map((report, index) => <details className="request-archive-report" key={index} onPointerDown={event => event.stopPropagation()}>
      <summary>From {report.name}: {report.added} added, {report.skipped.length} skipped{report.skipped.length ? ` (${new Set(report.skipped.map(item => item.reason)).size === 1 ? userError(report.skipped[0].reason) : 'multiple reasons'})` : ''}</summary>
      {!!report.skipped.length && <ul>{report.skipped.map((item, i) => <li key={i}>{item.name}: <ProblemBanner message={userError(item.reason)} /></li>)}</ul>}
    </details>)}
    </div>}
    {selected && <div className="request-attach-row">
      <RequestPages pages={pages} hostPageId={hostPageId} selection={region.pages ?? []} change={selection => { void change({ pages: selection }).catch(() => {}); }} />
      {attach && <><button type="button" className="request-attach icon-button sm" title="Attach pictures, text files or ZIPs (or drop them on the box)" aria-label={`Attach files to request ${number}`} onPointerDown={event => event.stopPropagation()} onClick={() => attachInput.current?.click()}><Icon name="paperclip" /></button>
      <input ref={attachInput} type="file" hidden multiple accept="image/png,image/jpeg,image/webp,image/gif,.txt,.md,.csv,.json,.html,.zip,application/zip,text/plain" onChange={event => { const files = Array.from(event.target.files ?? []); event.target.value = ''; if (files.length) attach(files, []); }} /></>}
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

function QuickIcon({ name }: { name: 'delete' | 'undo' }) {
  return <Icon name={name === 'delete' ? 'trash' : 'undo'} />;
}
