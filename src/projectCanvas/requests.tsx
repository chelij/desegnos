import { canvasBoxes } from './canvasView';
import { useEffect, useRef, useState } from 'react';
import type { Boundary, Point, RequestBox } from '../projectCanvasGeometry';
import { pointerRoute } from '../projectCanvasGeometry';
import { pointerWorld } from './pointers';
import type { PendingRequest, Project, Rect, ReferenceItem, RequestArea, RequestShape, RequestTarget } from '../types';

import { elementParents, findElementIn } from './elementGeometry';
import { requestAreaContains, requestShapeBounds, requestShapePath } from './requestGeometry';
import { MAX_PROMPT_CHARS } from '../generated/limits';

export { requestAreaContains, requestShapeBounds, requestShapePath } from './requestGeometry';

export type RequestDraft = { targets: RequestTarget[]; area: RequestArea | null } | null;
export type BoundaryShift = { pageId: string; sizeId: string; rect: Rect } | null;

/** A moving boundary offsets its cached geometry; all other boxes retain their identity. */
export function elementBoxes(project: Project, boundaries: Boundary[], moving: BoundaryShift): RequestBox[] {
  const boxes = canvasBoxes(project, boundaries);
  if (!moving) return boxes;
  const boundary = boundaries.find(boundary => boundary.sizeId === moving.sizeId);
  if (!boundary) return boxes;
  return boxes.map(box => box.sizeId === moving.sizeId ? { ...box, rect: { ...box.rect,
    x: box.rect.x + moving.rect.x - boundary.rect.x, y: box.rect.y + moving.rect.y - boundary.rect.y } } : box);
}

export const elementBoxesNow = (project: Project, boundaries: Boundary[]): RequestBox[] => elementBoxes(project, boundaries, null);

/** Pending requests in project order, with the size their marker sits in. */
export function pendingRequests(project: Project): { request: PendingRequest; pageId: string | null; sizeId: string | null }[] {
  return (project.requests ?? []).map(request => {
    const element = request.targets.find((target): target is Extract<RequestTarget, { kind: 'element' }> => target.kind === 'element');
    const pageId = request.area?.page ?? element?.page ?? request.targets.find(target => target.kind === 'boundary')?.page ?? null;
    const page = project.pages.find(item => item.id === pageId);
    return { request, pageId, sizeId: request.area?.size ?? element?.size ?? page?.sizes[0]?.id ?? null };
  });
}

/** The world rectangle a request marker sits at: its area, its target, or the canvas centre. */
export function requestMarkerRect(project: Project, boundaries: Boundary[], request: PendingRequest): Rect {
  if (request.area) return requestShapeBounds(request.area.shape);
  for (const target of request.targets) {
    if (target.kind === 'element') {
      const box = elementBoxes(project, boundaries, null).find(row => row.pageId === target.page && row.elementId === target.id && (!target.size || row.sizeId === target.size));
      if (box) return box.rect;
    }
    if (target.kind === 'boundary') {
      const boundary = boundaries.find(row => row.pageId === target.page && (!target.size || row.sizeId === target.size));
      if (boundary) return boundary.rect;
    }
    if (target.kind === 'reference') {
      const item = project.reference.find(row => row.id === target.id);
      if (item) return item.rect;
    }
  }
  return { x: 0, y: 0, width: 1, height: 1 };
}

/** World rectangles a request highlights for its targets. */
export function requestTargetRects(project: Project, boundaries: Boundary[], request: PendingRequest): Rect[] {
  const rects: Rect[] = [];
  const elements = elementBoxes(project, boundaries, null);
  for (const target of request.targets) {
    if (target.kind === 'element') {
      const box = elements.find(row => row.pageId === target.page && row.elementId === target.id && (!target.size || row.sizeId === target.size));
      if (box) rects.push(box.rect);
    } else if (target.kind === 'boundary') {
      const boundary = boundaries.find(row => row.pageId === target.page && (!target.size || row.sizeId === target.size));
      if (boundary) rects.push(boundary.rect);
    } else if (target.kind === 'reference') {
      const item = project.reference.find(row => row.id === target.id);
      if (item) rects.push(item.rect);
    }
  }
  return rects;
}

/** The real pointer line of one pointer, or null when it is broken. */
function pointerSegment(project: Project, boundaries: Boundary[], pointerId: string): { from: Point; to: Point } | null {
  const pointer = (project.pointers ?? []).find(item => item.id === pointerId);
  if (!pointer) return null;
  const route = pointerRoute(pointer, pointerWorld(project, boundaries, null, {}, {}));
  return route ? { from: route.from, to: route.to } : null;
}

function segmentCrosses(shape: RequestShape, from: Point, to: Point): boolean {
  const length = Math.hypot(to.x - from.x, to.y - from.y);
  const steps = Math.max(2, Math.min(400, Math.ceil(length / 8)));
  for (let index = 0; index <= steps; index++) {
    const t = index / steps;
    if (requestAreaContains(shape, { x: from.x + (to.x - from.x) * t, y: from.y + (to.y - from.y) * t })) return true;
  }
  return false;
}

/** A rectangle is covered when most sample points across it fall inside the outline. */
function mostlyInside(shape: RequestShape, rect: Rect): boolean {
  let inside = 0;
  const samples: Point[] = [];
  for (const dx of [0.1, 0.5, 0.9]) for (const dy of [0.1, 0.5, 0.9]) samples.push({ x: rect.x + rect.width * dx, y: rect.y + rect.height * dy });
  samples.push({ x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 });
  for (const point of samples) if (requestAreaContains(shape, point)) inside += 1;
  return inside >= Math.ceil(samples.length / 2);
}

/**
 * What an area's real outline covers: layers mostly inside, covered boundaries, reference items
 * and pointers whose line crosses it. Empty space inside a boundary targets that boundary so
 * new content can fill the area.
 */
export function coveredTargets(project: Project, boundaries: Boundary[], area: RequestArea, movingItems: Record<string, Rect> = {}): RequestTarget[] {
  const shape = area.shape;
  const boxes = elementBoxes(project, boundaries, null);
  const targets: RequestTarget[] = [];
  const coveredElementIds = new Set<string>();
  for (const box of boxes) if (mostlyInside(shape, box.rect)) coveredElementIds.add(`${box.pageId}:${box.elementId}:${box.sizeId}`);
  for (const box of boxes) {
    if (!coveredElementIds.has(`${box.pageId}:${box.elementId}:${box.sizeId}`)) continue;
    // A covered ancestor already carries its children into the request.
    if (elementParents(project, box.pageId, box.elementId).some(id => coveredElementIds.has(`${box.pageId}:${id}:${box.sizeId}`))) continue;
    const page = project.pages.find(item => item.id === box.pageId);
    targets.push({ kind: 'element', page: box.pageId, size: page && page.sizes.length > 1 ? box.sizeId : null, id: box.elementId });
  }
  const pagesWithElements = new Set(targets.flatMap(target => target.kind === 'element' ? [target.page] : []));
  for (const boundary of boundaries) {
    if (!mostlyInside(shape, boundary.rect)) continue;
    // A whole boundary is a target when the area asks for the boundary itself.
    if (pagesWithElements.has(boundary.pageId)) continue;
    targets.push({ kind: 'boundary', page: boundary.pageId, size: boundary.sizeId });
  }
  for (const item of project.reference as ReferenceItem[]) {
    if (mostlyInside(shape, movingItems[item.id] ?? item.rect)) targets.push({ kind: 'reference', id: item.id });
  }
  for (const pointer of project.pointers ?? []) {
    const segment = pointerSegment(project, boundaries, pointer.id);
    if (segment && segmentCrosses(shape, segment.from, segment.to)) targets.push({ kind: 'pointer', id: pointer.id });
  }
  // An area over empty space inside a boundary still needs that boundary as its target.
  if (!targets.length && area.page) {
    const page = project.pages.find(item => item.id === area.page);
    targets.push({ kind: 'boundary', page: area.page, size: page && page.sizes.length > 1 ? area.size ?? null : null });
  }
  return dedupeTargets(targets);
}

export function dedupeTargets(targets: RequestTarget[]): RequestTarget[] {
  const seen = new Set<string>();
  return targets.filter(target => {
    const key = requestTargetKey(target);
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

export function requestTargetKey(target: RequestTarget): string {
  switch (target.kind) {
    case 'element': return `element:${target.page}:${target.size ?? ''}:${target.id}`;
    case 'boundary': return `boundary:${target.page}:${target.size ?? ''}`;
    case 'reference': return `reference:${target.id}`;
    case 'pointer': return `pointer:${target.id}`;
  }
}

/** The one pending request that targets this layer, when a draft targets that layer alone. */
export function singleTargetRequest(project: Project, targets: RequestTarget[]): PendingRequest | null {
  const target = targets.length === 1 && targets[0].kind === 'element' ? targets[0] : null;
  if (!target) return null;
  return (project.requests ?? []).find(request => request.targets.length === 1 && request.targets[0].kind === 'element' && request.targets[0].id === target.id) ?? null;
}

export function requestTargetLabel(project: Project, target: RequestTarget): string {
  if (target.kind === 'element') {
    const page = project.pages.find(item => item.id === target.page);
    const element = page ? findElementIn(page.elements, target.id) : null;
    return element?.name?.trim() || element?.text.trim().split('\n')[0] || (element ? element.kind[0].toUpperCase() + element.kind.slice(1) : 'Layer');
  }
  if (target.kind === 'boundary') return project.pages.find(item => item.id === target.page)?.name ?? 'Boundary';
  if (target.kind === 'reference') {
    const item = project.reference.find(row => row.id === target.id);
    if (!item) return 'Reference';
    return item.kind.kind === 'element' ? item.kind.element.name || item.kind.element.kind : item.kind.kind === 'note' ? item.kind.text.trim().split('\n')[0] || 'Note' : item.kind.kind === 'link' ? item.kind.title || item.kind.url : item.kind.kind === 'image' ? item.kind.name || 'Picture' : item.kind.color;
  }
  return 'Pointer';
}

export function requestAreaLabel(area: RequestArea | null): string {
  if (!area) return 'No area';
  switch (area.shape.kind) {
    case 'rect': return 'Rectangle area';
    case 'ellipse': return 'Ellipse area';
    case 'polygon': return 'Polygon area';
    case 'freehand': return 'Freehand area';
  }
}

/** Pending requests draw a small marker and their area outline; a selected or drafted zone
 * highlights its targets. Everything here lives in the transformed canvas layer. */
export function RequestLayer({ project, boundaries, selection, draft, draftArea, inverse, selectRequest }: {
  project: Project; boundaries: Boundary[]; selection: { kind: string; requestId?: string }[];
  draft: RequestDraft; draftArea: RequestShape | null; inverse: number; selectRequest: (id: string) => void;
}) {
  const selected = new Set(selection.filter(target => target.kind === 'request').map(target => target.requestId));
  const rows = pendingRequests(project);
  const targetRects = (request: PendingRequest) => requestTargetRects(project, boundaries, request);
  return <div className="request-layer">
    {rows.map(({ request }) => {
      const bounds = request.area ? requestShapeBounds(request.area.shape) : requestMarkerRect(project, boundaries, request);
      const highlight = selected.has(request.id);
      return <div key={request.id} className={`request-item${highlight ? ' selected' : ''}`} data-request={request.id}>
        {request.area && <RequestOutline shape={request.area.shape} inverse={inverse} draft={false} />}
        {highlight && targetRects(request).map((rect, index) => <div key={index} className="request-target-highlight" style={{ left: rect.x, top: rect.y, width: rect.width, height: rect.height, borderWidth: inverse }} />)}
        <button type="button" className="request-marker" style={{ left: bounds.x, top: Math.max(0, bounds.y - inverse * 20), fontSize: 11 * inverse, maxWidth: Math.max(80, bounds.width) }}
          title={request.text || 'Empty request'} onClick={event => { event.stopPropagation(); selectRequest(request.id); }}>
          <span>{request.text.trim().split('\n')[0] || 'Request'}</span>
        </button>
      </div>;
    })}
    {draft && draft.targets.map(target => {
      const rect = targetRectsFor(project, boundaries, [target])[0];
      return rect ? <div key={requestTargetKey(target)} className="request-target-highlight draft" style={{ left: rect.x, top: rect.y, width: rect.width, height: rect.height, borderWidth: inverse }} /> : null;
    })}
    {draft?.area && <RequestOutline shape={draft.area.shape} inverse={inverse} draft />}
    {draftArea && <RequestOutline shape={draftArea} inverse={inverse} draft />}
  </div>;
}

export function targetRectsFor(project: Project, boundaries: Boundary[], targets: RequestTarget[]): Rect[] {
  return requestTargetRects(project, boundaries, { id: 'draft', text: '', attachments: [], targets, area: null, requestedAt: '' });
}

function RequestOutline({ shape, inverse, draft }: { shape: RequestShape; inverse: number; draft: boolean }) {
  const bounds = requestShapeBounds(shape);
  return <svg className={`request-area${draft ? ' draft' : ''}`} style={{ left: bounds.x, top: bounds.y, width: bounds.width, height: bounds.height }}
    viewBox={`0 0 ${bounds.width} ${bounds.height}`} preserveAspectRatio="none" aria-hidden="true">
    <path d={requestShapePath(shape)} vectorEffect="non-scaling-stroke" strokeWidth={Math.max(1, inverse)} />
  </svg>;
}

/** The request box: native screen pixels beside the targets, never inside the transformed layer. */
export function RequestBoxEditor({ screen, text, ask, submit, close }: {
  screen: { left: number; top: number; width: number; height: number }; text: string;
  ask?: (text: string) => void;
  submit: (text: string) => void; close: () => void;
}) {
  const [draft, setDraft] = useState(text);
  const area = useRef<HTMLTextAreaElement>(null);
  useEffect(() => { area.current?.focus({ preventScroll: true }); area.current?.select(); }, []);
  const send = ask ?? submit;
  return <div className="request-box-editor" style={{ left: screen.left, top: screen.top, width: screen.width, minHeight: screen.height }} onPointerDown={event => event.stopPropagation()}>
    <textarea ref={area} aria-label="Describe the request" maxLength={MAX_PROMPT_CHARS} value={draft} placeholder="What should happen here?" spellCheck
      onChange={event => setDraft(event.target.value)}
      onKeyDown={event => {
        if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); close(); return; }
        if (event.key === 'Enter' && !event.shiftKey) { event.preventDefault(); event.stopPropagation(); send(draft); }
      }} />
    <div className="request-box-actions">
      <button type="button" className="dock-action primary" disabled={!draft.trim()} onClick={() => send(draft)}>Request</button>
    </div>
  </div>;
}
