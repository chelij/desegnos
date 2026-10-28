import { type PointerEvent as ReactPointerEvent } from 'react';
import {
  referenceItemAt, pointerRoute, type Boundary, type CanvasCamera, type CanvasTarget, type Point,
  type RequestBox,
} from '../projectCanvasGeometry';
import type { Element, ElementKind, ElementMove, ElementPatch, PointerSource, PointerTarget, Project, Rect, ReferenceItem } from '../types';
import { shownReference } from './reference';
import { pointerWorld } from './pointers';
import { type PlayState } from './play';
import { Drag, elementAt } from './interactionTypes';

type Context = {
  latest: import("react").RefObject<{ camera: CanvasCamera; boundaries: Boundary[]; project: Project; selection: CanvasTarget[]; focused: string | null; addReference: (item: ReferenceItem, pictureDataUri?: string) => Promise<void>; attachToRequest: (requestId: string, files: File[], links: string[]) => void; selectPointer: (pointerId: string, additive?: boolean) => void; selectReference: (itemId: string, additive?: boolean) => void; selectElement: (pageId: string, elementId: string, sizeId: string | null, additive?: boolean) => void; clearSelection: () => void; removeElements: (elements: { pageId: string; elementId: string; }[]) => void; insertElements: (pageId: string, parentId: string | null, index: number, elements: Element[], pictureDataUri?: string, sizeId?: string | null) => Promise<void>; updateElements: (pageId: string, patches: ElementPatch[]) => Promise<unknown> | void; moveElements: (moves: ElementMove[]) => Promise<unknown> | void; groupElements: (pageId: string, ids: string[]) => Promise<string | null>; ungroupElements: (pageId: string, id: string) => Promise<string[]>; tool: import("../EditorTools").EditorTool; insertKind: ElementKind | null; editing: { pageId: string; sizeId: string; elementId: string; autoWidth: boolean; created: boolean; } | null; play: PlayState; onPlayElement: (pageId: string, elementId: string) => void; }>;
  setArmedPointer: import("react").Dispatch<import("react").SetStateAction<{ pointerId: string; end: 'from' | 'to'; } | null>>;
  surface: import("react").RefObject<HTMLDivElement | null>;
  requestBoxes: () => RequestBox[];
  project: Project;
  movingItems: Record<string, Rect>;
  boundaryUnder: (point: Point) => Boundary | null;
  pointerWorldNow: () => import("../projectCanvasGeometry").PointerWorld;
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
    const item = referenceItemAt(project.reference.map(entry => ({ itemId: entry.id, rect: shownReference(entry, movingItems), kind: entry.kind })), point);
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
