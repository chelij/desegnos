import type { ProjectCanvasProps } from './interactionTypes';
import { canvasBoxes } from './canvasView';
import { screenRect } from './surface';
import { type PointerEvent as ReactPointerEvent } from 'react';
import { boundaryAt, boundaryMoves, pageRectFromCanvas, pushApartBoundaries, type Boundary, type BoundaryMove } from './boundaryLayout';
import { clampedReferenceRect, referenceMoves, pushApartReference, type ReferenceCorner, type ReferenceBox, type ReferenceMove } from './referenceGeometry';
import { type CanvasCamera, type Point } from './camera';
import { type CanvasTarget, type ElementTarget } from './canvasTargets';
import type { Element, ElementMove, Page, Project, Rect, ReferenceItem } from '../types';
import { shownReference } from './reference';
import { centre } from './surface';
import { baseSizeId } from './sizes';
import { elementLocalRect } from './elementGeometry';
import { elementTreeIds, cloneElement, elementChildrenOf, elementPath, elementParents, type ElementCorner } from './elementGeometry';
import { findElementIn } from './elementFacts';
import {
  MoveBoundaryDrag, MoveTarget, MoveSelectionDrag, ElementEntry, MoveElementsDrag, Drag, DuplicateRepeat,
  isElementTarget, elementTargetAt, intersects, elementMoveDelta, targetKey, entryTargetKey,
} from './interactionTypes';

export type MovingContext = {
  beginPointerSession: (event: { pointerId: number; }) => void;
  drag: { current: Drag | null; };
  setPanning: import("react").Dispatch<import("react").SetStateAction<boolean>>;
  project: Project;
  surface: import("react").RefObject<HTMLDivElement | null>;
  boundaryRect: (sizeId: string) => Rect | null;
  camera: CanvasCamera;
  pageFor: (pageId: string, sizeId?: string | null) => Page | null;
  duplicateRepeat: import("react").RefObject<DuplicateRepeat | null>;
  insertElements: (pageId: string, parentId: string | null, index: number, elements: Element[], pictureDataUri?: string, sizeId?: string | null) => Promise<void>;
  selectTargets: (targets: { pageId: string; elementId: string; sizeId?: string | null; }[]) => void;
  boundaryUnder: (point: Point) => Boundary | null;
  elementToReference: (pageId: string, elementId: string, rect: Rect, sizeId?: string | null) => Promise<void>;
  moveElements: ProjectCanvasProps['edits']['moveElements'];
  clearSelection: () => void;
  selection: CanvasTarget[];
  worldPoint: (event: { clientX: number; clientY: number; }) => Point;
  movingItems: Record<string, Rect>;
  boundaries: Boundary[];
  referenceBoxes: () => ReferenceBox[];
  busy: import("react").RefObject<boolean>;
  savePlacements: (placements: BoundaryMove[], reference: ReferenceMove[]) => Promise<void>;
  moving: MoveBoundaryDrag | null;
  referenceToElement: (itemId: string, pageId: string, rect: Rect, sizeId?: string | null) => Promise<void>;
  updateReference: (itemId: string, patch: { rect?: Rect; text?: string; url?: string; title?: string; color?: string; }) => void;
};

export function createMovingActions(context: () => MovingContext) {
  function startPan(event: ReactPointerEvent<HTMLDivElement>, pageId: string | null) {
    const { camera } = context();
    const { beginPointerSession, drag, setPanning } = context();
    event.preventDefault();
    beginPointerSession(event);
    drag.current = { kind: 'pan', pointerId: event.pointerId, start: { x: event.clientX, y: event.clientY }, camera, moved: false, pageId };
    setPanning(true);
  }

  function beginMove(event: ReactPointerEvent, point: Point, targets: CanvasTarget[]): MoveSelectionDrag | null {
    const { project } = context();
    const movable: MoveTarget[] = targets.flatMap((target): MoveTarget[] => {
      if (target.kind === 'pointer' || target.kind === 'element' || target.kind === 'request') return [];
      const item = project.reference.find(entry => entry.id === target.itemId);
      return item ? [{ kind: 'reference' as const, itemId: item.id, origin: item.rect }] : [];
    });
    if (!movable.length) return null;
    return { kind: 'move-selection', pointerId: event.pointerId, start: point, targets: movable, rects: {}, moved: false, axisLocked: false };
  }

  function boxAtPoint(pageId: string, sizeId: string, point: Point, skip: Set<string>): { id: string; origin: Point } | null {
    const { surface, boundaryRect, camera, project } = context();
    const surfaceNode = surface.current;
    const boundary = boundaryRect(sizeId);
    if (!surfaceNode || !boundary) return null;
    const host = surfaceNode.getBoundingClientRect();
    const client = { x: host.left + point.x * camera.zoom + camera.x, y: host.top + point.y * camera.zoom + camera.y };
    let found: { id: string; origin: Point; area: number } | null = null;
    for (const node of surfaceNode.querySelectorAll<HTMLElement>('[data-element-kind="box"]')) {
      if (node.closest<HTMLElement>('[data-size]')?.dataset.size !== sizeId) continue;
      const id = node.dataset.element!;
      if (skip.has(id)) continue;
      const box = node.getBoundingClientRect();
      if (client.x < box.left || client.x > box.right || client.y < box.top || client.y > box.bottom) continue;
      const path = elementPath(project, pageId, id, sizeId);
      const origin = path?.[path.length - 1]?.origin;
      if (!origin) continue;
      const area = box.width * box.height;
      if (!found || area < found.area) found = { id, origin, area };
    }
    if (!found) for (const box of canvasBoxes(project, context().boundaries)) {
      if (box.pageId !== pageId || box.sizeId !== sizeId || skip.has(box.elementId)) continue;
      const path = elementPath(project, pageId, box.elementId, sizeId);
      const last = path?.at(-1);
      if (!last || last.element.kind !== 'box' || path!.some(entry => !entry.element.visible || entry.element.locked)) continue;
      if (point.x < box.rect.x || point.x > box.rect.x + box.rect.width || point.y < box.rect.y || point.y > box.rect.y + box.rect.height) continue;
      const area = box.rect.width * box.rect.height * camera.zoom ** 2;
      if (!found || area < found.area) found = { id: last.element.id, origin: last.origin, area };
    }
    return found ? { id: found.id, origin: found.origin } : null;
  }

  function marqueeTargets(rect: Rect): ElementTarget[] {
    const { surface, project, pageFor } = context();
    const surfaceNode = surface.current;
    if (!surfaceNode) return [];
    const host = surfaceNode.getBoundingClientRect();
    const candidates: (ElementTarget & { rect: Rect })[] = [];
    const rendered = new Set<string>();
    for (const node of surfaceNode.querySelectorAll<HTMLElement>('[data-element]')) {
      const size = node.closest<HTMLElement>('[data-size]')?.dataset.size;
      rendered.add(`${size}:${node.dataset.element}`);
      const box = node.getBoundingClientRect();
      const screen = { x: box.left - host.left, y: box.top - host.top, width: box.width, height: box.height };
      if (!intersects(screen, rect)) continue;
      const hit = elementTargetAt(project, [], node);
      if (!hit) continue;
      candidates.push({ ...hit, rect: screen });
    }
    // DOM measurements preserve flow/text geometry where mounted; model boxes cover every
    // culled boundary too, so a marquee spanning the viewport never loses distant layers.
    const { boundaries, camera } = context();
    const boxes = canvasBoxes(project, boundaries);
    const byId = new Map(boxes.map(box => [`${box.sizeId}:${box.elementId}`, box]));
    for (const box of boxes) {
      if (rendered.has(`${box.sizeId}:${box.elementId}`)) continue;
      const path = elementPath(project, box.pageId, box.elementId, box.sizeId);
      if (!path || path.some(entry => entry.element.locked || !entry.element.visible)) continue;
      const entry = path.find(entry => entry.element.kind === 'group') ?? path.at(-1)!;
      const world = byId.get(`${box.sizeId}:${entry.element.id}`)!;
      const screen = screenRect(world.rect, camera);
      const bounds = { x: screen.left, y: screen.top, width: screen.width, height: screen.height };
      if (intersects(bounds, rect)) candidates.push({ kind: 'element', pageId: box.pageId, sizeId: box.sizeId ?? null, elementId: entry.element.id, rect: bounds });
    }
    const unique = new Map<string, ElementTarget>();
    for (const candidate of candidates) {
      const element = findElementIn(pageFor(candidate.pageId, candidate.sizeId)?.elements ?? [], candidate.elementId);
      if (!element) continue;
      const ancestor = candidates.some(other => other.pageId === candidate.pageId && other.elementId !== candidate.elementId
        && elementParents(project, candidate.pageId, other.elementId).includes(candidate.elementId) && element.kind !== 'group');
      const { rect: _rect, ...target } = candidate;
      if (!ancestor) unique.set(`${candidate.pageId}:${candidate.elementId}:${candidate.sizeId ?? 'all'}`, target);
    }
    return [...unique.values()];
  }

  function rememberRepeatMove(entries: ElementEntry[], delta: Point) {
    const { duplicateRepeat } = context();
    const repeat = duplicateRepeat.current;
    if (repeat && repeat.key === entryTargetKey(entries)) {
      duplicateRepeat.current = { key: repeat.key, offset: { x: repeat.offset.x + delta.x, y: repeat.offset.y + delta.y } };
    }
  }

  async function duplicateElementMove(active: MoveElementsDrag, point: Point, snap: boolean) {
    const { project, insertElements, duplicateRepeat, selectTargets } = context();
    const delta = elementMoveDelta(active.entries, active.start, point, snap, active.axisLocked, project.gridSettings.spacing);
    const groups = new Map<string, { pageId: string; parentId: string | null; index: number; sizeId: string | null; allSizes: boolean; elements: Element[]; targets: { pageId: string; elementId: string; sizeId: string | null }[] }>();
    for (const entry of active.entries) {
      const key = `${entry.pageId}\u0000${entry.parentId ?? ''}\u0000${entry.sizeId}\u0000${entry.allSizes}`;
      const group = groups.get(key) ?? { pageId: entry.pageId, parentId: entry.parentId, index: entry.index + 1, sizeId: entry.allSizes ? null : entry.sizeId, allSizes: entry.allSizes, elements: [], targets: [] };
      const element = cloneElement(entry.element);
      element.rect = { ...entry.rect, x: entry.rect.x + delta.x, y: entry.rect.y + delta.y };
      group.elements.push(element);
      group.targets.push({ pageId: entry.pageId, elementId: element.id, sizeId: entry.allSizes ? null : entry.sizeId });
      groups.set(key, group);
    }
    const targets: { pageId: string; elementId: string; sizeId: string | null }[] = [];
    for (const group of groups.values()) {
      await insertElements(group.pageId, group.parentId, group.index, group.elements, undefined, group.sizeId);
      targets.push(...group.targets);
    }
    if (targets.length) {
      // An Alt-drag duplicate arms Ctrl/Cmd+D to repeat the same offset.
      duplicateRepeat.current = { key: targetKey(targets), offset: delta };
      selectTargets(targets);
    }
  }

  async function commitElementMove(active: MoveElementsDrag, point: Point, snap: boolean): Promise<boolean> {
    const { boundaryUnder, elementToReference, boundaryRect, project, moveElements, clearSelection, selectTargets } = context();
    const targetBoundary = boundaryUnder(point);
    const skip = new Set(active.entries.map(entry => entry.id));
    for (const entry of active.entries) for (const id of elementTreeIds(entry.element.children)) skip.add(id);
    const moves: ElementMove[] = [];
    const delta = elementMoveDelta(active.entries, active.start, point, snap, active.axisLocked, project.gridSettings.spacing);
    rememberRepeatMove(active.entries, delta);
    let converted = false;
    // Every entry is dropped at the same pointer position with the same excluded subtree.
    const dropBoxes = new Map<string, ReturnType<typeof boxAtPoint>>();
    for (const entry of active.entries) {
      const world = { ...entry.world, x: entry.world.x + delta.x, y: entry.world.y + delta.y };
      if (!targetBoundary && entry.parentId === null) {
        await elementToReference(entry.pageId, entry.id, world, entry.sizeId);
        converted = true;
        continue;
      }
      const pageId = targetBoundary?.pageId ?? entry.pageId;
      const pageChanged = pageId !== entry.pageId;
      // A drag inside one size moves that size's position; a drag across sizes belongs to the
      // size it landed in. Moving to another page changes the shared base position.
      const sizeId = pageChanged ? null : targetBoundary?.sizeId ?? entry.sizeId;
      const allSizes = !pageChanged && entry.allSizes && (!targetBoundary || targetBoundary.sizeId === entry.sizeId);
      // A layer dropped on another boundary takes the shared base position there, so measure
      // from that page's base boundary even when the pointer landed on a linked size.
      const baseBoundary = pageChanged
        ? boundaryRect(project.pages.find(page => page.id === pageId)?.sizes[0]?.id ?? '')
        : null;
      const boundary = pageChanged ? baseBoundary ?? targetBoundary?.rect ?? null : boundaryRect(sizeId ?? entry.sizeId);
      if (!boundary) continue;
      if (targetBoundary && sizeId && !dropBoxes.has(sizeId)) dropBoxes.set(sizeId, boxAtPoint(pageId, sizeId, point, skip));
      const box = targetBoundary && sizeId ? dropBoxes.get(sizeId) : null;
      const parentId = box?.id ?? (pageChanged ? null : entry.parentId);
      const parentOrigin = box && parentId !== entry.parentId ? box.origin : parentId === entry.parentId && !pageChanged ? entry.parentOrigin : { x: 0, y: 0 };
      const sameParent = pageId === entry.pageId && parentId === entry.parentId;
      const index = sameParent ? entry.index : elementChildrenOf(project, pageId, parentId).length;
      moves.push({
        id: entry.id, to_page_id: pageId, to_parent_id: parentId, index,
        rect: elementLocalRect(world, boundary, parentOrigin),
        size_id: allSizes || pageChanged ? null : sizeId, all_sizes: allSizes,
      });
    }
    if (moves.length) await moveElements(moves);
    if (converted) clearSelection();
    else selectTargets(moves.map(move => ({ pageId: move.to_page_id, elementId: move.id, sizeId: move.all_sizes ? null : move.size_id ?? null })));
    return true;
  }

  function startElementResize(event: ReactPointerEvent<HTMLButtonElement>, elementId: string, corner: ElementCorner, sizeId: string | null) {
    const { surface, selection, project, boundaryRect, beginPointerSession, drag, worldPoint } = context();
    if (!surface.current) return;
    const target = selection.find(isElementTarget);
    if (!target || target.elementId !== elementId) return;
    const page = project.pages.find(item => item.id === target.pageId);
    if (!page) return;
    const editSizeId = sizeId ?? target.sizeId ?? baseSizeId(page);
    const path = elementPath(project, target.pageId, elementId, editSizeId);
    const last = path?.[path.length - 1];
    const boundary = boundaryRect(editSizeId);
    if (!last || !boundary) return;
    const over = last.element.overrides?.[editSizeId];
    const rect = over?.rect ?? last.element.rect;
    const origin = { x: last.parentOrigin.x + rect.x, y: last.parentOrigin.y + rect.y };
    event.preventDefault();
    event.stopPropagation();
    beginPointerSession(event);
    drag.current = { kind: 'resize-element', pointerId: event.pointerId, start: worldPoint(event), corner, rect,
      entry: { id: elementId, element: last.element, pageId: target.pageId, sizeId: editSizeId, allSizes: target.sizeId === null, parentId: last.parentId, index: last.index, rect, parentOrigin: last.parentOrigin,
        boundaryOrigin: { x: boundary.x, y: boundary.y }, world: { x: boundary.x + origin.x, y: boundary.y + origin.y, width: rect.width, height: rect.height } }, moved: false };
    surface.current.focus({ preventScroll: true });
  }

  function startReferenceResize(event: ReactPointerEvent<HTMLButtonElement>, corner: ReferenceCorner, item: ReferenceItem) {
    const { surface, beginPointerSession, drag, worldPoint, movingItems } = context();
    if (!surface.current) return;
    event.preventDefault();
    event.stopPropagation();
    beginPointerSession(event);
    drag.current = { kind: 'resize-reference', pointerId: event.pointerId, start: worldPoint(event), corner, itemId: item.id, origin: shownReference(item, movingItems), rect: shownReference(item, movingItems), moved: false };
    surface.current.focus({ preventScroll: true });
  }

  async function persistMove(sizeId: string, rect: Rect) {
    const { boundaries, referenceBoxes } = context();
    const anchor = boundaries.find(item => item.sizeId === sizeId);
    if (!anchor) return;
    const before = boundaries.map(item => item.sizeId === sizeId ? { ...item, rect } : item);
    const next = pushApartBoundaries(before, sizeId);
    const boxes = referenceBoxes();
    const pushedItems = pushApartReference(boxes, next);
    await saveMoves(boundaryMoves(next, boundaries), referenceMoves(pushedItems, boxes));
  }

  async function saveMoves(moves: BoundaryMove[], reference: ReferenceMove[]) {
    const { busy, savePlacements } = context();
    if (!moves.length && !reference.length) return;
    busy.current = true;
    try { await savePlacements(moves, reference); } finally { busy.current = false; }
  }

  async function commitMove(targets: MoveTarget[], rects: Record<string, Rect>) {
    const { busy, boundaries, moving, project, referenceToElement, updateReference } = context();
    busy.current = true;
    try {
      const boundaryList = boundaries.map(item => moving?.sizeId === item.sizeId ? { ...item, rect: moving.rect } : item);
      for (const target of targets) {
        const rect = rects[target.itemId];
        if (!rect) continue;
        const boundary = boundaryAt(boundaryList, centre(rect));
        const item = project.reference.find(entry => entry.id === target.itemId);
        if (boundary && item) {
          await referenceToElement(target.itemId, boundary.pageId, pageRectFromCanvas(item.kind.kind === 'element' ? rect : clampedReferenceRect(rect, boundary.rect, project.gridSettings.snap ? project.gridSettings.spacing : 0), boundary.rect), boundary.sizeId);
        } else updateReference(target.itemId, { rect });
      }
    } finally { busy.current = false; }
  }

  return {
    startPan, beginMove, marqueeTargets, rememberRepeatMove, duplicateElementMove,
    commitElementMove, startElementResize, startReferenceResize, persistMove, commitMove,
  };
}
