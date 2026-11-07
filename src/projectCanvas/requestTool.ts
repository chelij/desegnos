import {
  pointerAt, POINTER_HIT_PX, boundaryAt, viewportWorldRect, type Boundary, type CanvasCamera,
  type CanvasTarget, type Point, type RequestBox,
} from '../projectCanvasGeometry';
import type { Command, PendingRequest, Project, Rect, RequestArea, RequestShape, RequestTarget, WorkspaceView } from '../types';
import { coveredTargets, dedupeTargets, requestShapeBounds } from './requests';
import { shownReference } from './reference';
import { pointerEntries } from './pointers';
import { screenRect, type ScreenRect } from './surface';
import { baseSizeId } from './sizes';
import { ElementEntry, REQUEST_EDITOR_WIDTH, REQUEST_EDITOR_HEIGHT, isElementTarget, elementAt } from './interactionTypes';

type Context = {
  requestBoxes: () => RequestBox[];
  project: Project;
  movingItems: Record<string, Rect>;
  pointerWorldNow: () => import("../projectCanvasGeometry").PointerWorld;
  camera: CanvasCamera;
  viewSize: { width: number; height: number; };
  boundaryUnder: (point: Point) => Boundary | null;
  elementScreenRect: (elementId: string, sizeId?: string | null) => ScreenRect | null;
  boundaries: Boundary[];
  setPolygonDraft: (points: Point[] | null) => void;
  setRequestDraft: import("react").Dispatch<import("react").SetStateAction<{ targets: RequestTarget[]; area: RequestArea | null; screen: ScreenRect; } | null>>;
  surface: import("react").RefObject<HTMLDivElement | null>;
  selection: CanvasTarget[];
  focused: string | null;
  focusedSize: string | null;
  elementEntries: (targets: CanvasTarget[]) => ElementEntry[];
  requestDraft: { targets: RequestTarget[]; area: RequestArea | null; screen: ScreenRect; } | null;
  editProject: (command: Command) => Promise<WorkspaceView | null>;
  generate: (elementIds: string[]) => void;
  selectRequest: (requestId: string) => void;
};

export function createRequestTool(context: () => Context) {
  function requestTargetAt(point: Point): RequestTarget | null {
    const { requestBoxes, project, movingItems, pointerWorldNow, camera, viewSize, boundaryUnder } = context();
    const box = elementAt(requestBoxes(), point);
    if (box) {
      const page = project.pages.find(item => item.id === box.pageId);
      return { kind: 'element', page: box.pageId, size: page && page.sizes.length > 1 ? box.sizeId : null, id: box.elementId };
    }
    const item = boundaryAt(project.reference.map(entry => ({ itemId: entry.id, rect: shownReference(entry, movingItems) })), point);
    if (item) return { kind: 'reference', id: item.itemId };
    const hits = pointerAt(pointerEntries(project.pointers ?? [], pointerWorldNow(), new Set(), viewportWorldRect(camera, viewSize)), point, POINTER_HIT_PX / camera.zoom);
    if (hits) return { kind: 'pointer', id: hits.pointer.id };
    const boundary = boundaryUnder(point);
    return boundary ? { kind: 'boundary', page: boundary.pageId, size: boundary.sizeId } : null;
  }

  function requestEditorScreen(targets: RequestTarget[], area: RequestArea | null): ScreenRect {
    const { camera, elementScreenRect, boundaries, project, movingItems, viewSize } = context();
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

  function closeRequestArea(input: Rect | Point[], kind: 'rect' | 'ellipse' | 'polygon' | 'freehand') {
    const { boundaryUnder, project, boundaries, movingItems, setPolygonDraft, setRequestDraft, surface } = context();
    const shape: RequestShape = kind === 'polygon' || kind === 'freehand'
      ? { kind, points: (input as Point[]).map(point => ({ x: point.x, y: point.y })) }
      : { kind, rect: input as Rect };
    const bounds = requestShapeBounds(shape);
    const boundary = boundaryUnder({ x: bounds.x + bounds.width / 2, y: bounds.y + bounds.height / 2 });
    const page = boundary?.pageId ?? null;
    const area: RequestArea = { page, size: boundary?.sizeId ?? null, shape };
    const targets = coveredTargets(project, boundaries, area, movingItems);
    setPolygonDraft(null);
    setRequestDraft({ targets, area, screen: requestEditorScreen(targets, area) });
    surface.current?.focus({ preventScroll: true });
  }

  function targetForSelection(): RequestTarget[] {
    const { selection, focused, boundaries, focusedSize } = context();
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

  function startRequestOnSelection(): boolean {
    const { setRequestDraft, setPolygonDraft, surface } = context();
    const targets = targetForSelection();
    if (!targets.length) return false;
    setRequestDraft({ targets, area: null, screen: requestEditorScreen(targets, null) });
    setPolygonDraft(null);
    surface.current?.focus({ preventScroll: true });
    return true;
  }

  function startRequestFromShape(): boolean {
    const { selection, elementEntries, project, boundaries, movingItems, setRequestDraft, setPolygonDraft, surface } = context();
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
    surface.current?.focus({ preventScroll: true });
    return true;
  }

  function submitRequestDraft(text: string, existing: PendingRequest | null = null) {
    const { requestDraft, setRequestDraft, surface, editProject, selectRequest } = context();
    const draft = requestDraft;
    setRequestDraft(null);
    const trimmed = text.trim();
    if (!draft || !trimmed) { surface.current?.focus({ preventScroll: true }); return Promise.resolve(false); }
    const request: PendingRequest = existing ? { ...existing, text: trimmed } : {
      id: crypto.randomUUID(), text: trimmed, attachments: [], targets: draft.targets, area: draft.area,
      requestedAt: new Date().toISOString(),
    };
    const saved = editProject({ type: existing ? 'update_request' : 'add_request', request });
    return saved.then(workspace => {
      if (!workspace) return false;
      selectRequest(request.id);
      return true;
    })
      .finally(() => surface.current?.focus({ preventScroll: true }));
  }

  /** Saves the draft, then generates for the layers it targets once the request is stored. */
  function askRequestDraft(text: string, existing: PendingRequest | null = null) {
    const { requestDraft, generate } = context();
    const elementIds = (requestDraft?.targets ?? []).flatMap(target => target.kind === 'element' ? [target.id] : []);
    void submitRequestDraft(text, existing).then(saved => { if (saved && elementIds.length) generate(elementIds); });
  }

  function cancelRequestDraft() {
    const { setRequestDraft, setPolygonDraft, surface } = context();
    setRequestDraft(null);
    setPolygonDraft(null);
    surface.current?.focus({ preventScroll: true });
  }

  return {
    requestTargetAt, requestEditorScreen, closeRequestArea, startRequestOnSelection,
    startRequestFromShape, submitRequestDraft, askRequestDraft, cancelRequestDraft,
  };
}
