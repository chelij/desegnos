import type { ProjectCanvasProps } from './interactionTypes';
import { type PointerEvent as ReactPointerEvent } from 'react';
import { boundaryAt, type Boundary } from './boundaryLayout';
import { pointerRoute } from './pointerGeometry';
import { type Point } from './camera';
import { type RequestBox } from './canvasTargets';
import type { PointerSource, PointerTarget, Project, Rect } from '../types';
import { shownReference } from './reference';
import { pointerWorld } from './pointerLayout';
import { Drag, elementAt } from './interactionTypes';

type Context = {
  latest: import("react").RefObject<Pick<ProjectCanvasProps, 'project' | 'selectPointer'> & { boundaries: Boundary[] }>;
  setArmedPointer: import("react").Dispatch<import("react").SetStateAction<{ pointerId: string; end: 'from' | 'to'; } | null>>;
  surface: import("react").RefObject<HTMLDivElement | null>;
  requestBoxes: () => RequestBox[];
  project: Project;
  movingItems: Record<string, Rect>;
  boundaryUnder: (point: Point) => Boundary | null;
  pointerWorldNow: () => import("./pointerGeometry").PointerWorld;
  beginPointerSession: (event: { pointerId: number; }) => void;
  worldPoint: (event: { clientX: number; clientY: number; }) => Point;
  drag: { current: Drag | null; };
};

export function createPointerTool(context: () => Context) {
  function armPointer(pointerId: string) {
    const { latest, setArmedPointer, surface } = context();
    const pointer = (latest.current.project.pointers ?? []).find(item => item.id === pointerId);
    if (!pointer) return;
    const world = pointerWorld(latest.current.project, latest.current.boundaries, null, {}, {});
    const route = pointerRoute(pointer, world);
    setArmedPointer({ pointerId, end: route?.brokenEnd ?? 'to' });
    latest.current.selectPointer(pointerId);
    surface.current?.focus({ preventScroll: true });
  }

  function endpointAt(point: Point): { endpoint: PointerSource | PointerTarget; rect: Rect } | null {
    const { requestBoxes, project, movingItems, boundaryUnder } = context();
    const request = elementAt(requestBoxes(), point);
    if (request) return { endpoint: { kind: 'element', pageId: request.pageId, elementId: request.elementId, sizeId: request.sizeId }, rect: request.rect };
    const item = boundaryAt(project.reference.map(entry => ({ itemId: entry.id, rect: shownReference(entry, movingItems), kind: entry.kind })), point);
    if (item) return { endpoint: item.kind.kind === 'link' ? { kind: 'url', url: item.kind.url } : { kind: 'reference', itemId: item.itemId }, rect: item.rect };
    const boundary = boundaryUnder(point);
    if (boundary) return { endpoint: { kind: 'boundary', pageId: boundary.pageId, sizeId: boundary.sizeId }, rect: boundary.rect };
    return null;
  }

  function pointerSourceAt(point: Point): { endpoint: PointerSource; rect: Rect } | null {
    const { requestBoxes, boundaryUnder } = context();
    const request = elementAt(requestBoxes(), point);
    if (request) return { endpoint: { kind: 'element', pageId: request.pageId, elementId: request.elementId, sizeId: request.sizeId }, rect: request.rect };
    const boundary = boundaryUnder(point);
    return boundary ? { endpoint: { kind: 'boundary', pageId: boundary.pageId, sizeId: boundary.sizeId }, rect: boundary.rect } : null;
  }

  function startReattach(event: ReactPointerEvent<HTMLButtonElement>, pointerId: string, end: 'from' | 'to') {
    const { surface, project, pointerWorldNow, beginPointerSession, worldPoint, drag } = context();
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

  return { armPointer, endpointAt, pointerSourceAt, startReattach };
}
