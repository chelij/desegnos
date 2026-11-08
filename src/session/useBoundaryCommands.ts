import { sizeLabel } from '../projectCanvas/sizes';
import { type BoundaryToolKind, boundaryDefaults, layoutKind, outputKind, pageKind, projectKind, nextBoundaryName } from '../projectCanvas/boundaryDefaults';
import { useProjectSession } from './useProjectSession';
import { sizeBoundaries, suggestedPagePath } from '../editorShared';
import { useCallback } from 'react';
import { type ProjectCanvasHandle } from '../ProjectCanvas';
import { BOUNDARY_GAP, unionBounds, referenceMoves, pushApartReference, boundaryMoves, pushApartBoundaries } from '../projectCanvasGeometry';
import type { BoundaryKind, Element, PageSize, Rect, SizePreset, WorkspaceView } from '../types';
import { SIZE_PRESETS } from '../projectCanvas/sizes';

export function useBoundaryCommands(current: import('react').RefObject<WorkspaceView | null>, mutate: ReturnType<typeof useProjectSession>['mutate'], canvasRef: import('react').RefObject<ProjectCanvasHandle | null>, selectCanvasBoundary: (pageId: string | null, sizeId?: string | null) => void) {
  const createCanvasBoundary = useCallback(async (bounds: Rect, kind: BoundaryToolKind, sourceId?: string) => {
    let createdId: string | null = null;
    const next = await mutate(project => {
      const source = project.pages.find(page => page.id === sourceId);
      const first = project.pages.find(page => pageKind(page) === kind);
      const template = source ?? (kind === 'page' && (first?.sizes.length ?? 0) > 1 ? first : null);
      const name = nextBoundaryName(project, kind);
      const size: PageSize = { id: crypto.randomUUID(), label: sizeLabel(bounds.width), width: Math.round(bounds.width), height: Math.round(bounds.height), kind: layoutKind(kind, project), x: bounds.x, y: bounds.y };
      let nextX = bounds.x;
      const sizes = template ? template.sizes.map((old, index) => {
        const next = { ...old, id: index === 0 ? size.id : crypto.randomUUID(),
          x: source ? bounds.x + old.x - template.sizes[0].x : nextX,
          y: source ? bounds.y + old.y - template.sizes[0].y : bounds.y,
          ...(index === 0 ? { width: size.width, height: size.height } : {}) };
        nextX = next.x + next.width + BOUNDARY_GAP;
        return next;
      }) : [size];
      const sizeIds = new Map(template?.sizes.map((old, index) => [old.id, sizes[index].id]));
      const copy = (element: Element): Element => ({ ...structuredClone(element), id: crypto.randomUUID(), children: element.children.map(copy), overrides: Object.fromEntries(Object.entries(element.overrides ?? {}).flatMap(([id, value]) => sizeIds.has(id) ? [[sizeIds.get(id)!, value]] : [])) });
      createdId = crypto.randomUUID();
      const page = { id: createdId, name, path: project.pages.length ? suggestedPagePath(name, project.pages.map(page => page.path)) : '/', included: true, output: outputKind(kind), background: source?.background ?? '#ffffff', sizes, elements: source?.elements.map(copy) ?? [] };
      const before = sizeBoundaries(project);
      const additions = sizes.map(item => ({ pageId: page.id, sizeId: item.id, label: item.label, rect: { x: item.x, y: item.y, width: item.width, height: item.height }, kind: item.kind }));
      // Keep the whole linked-size row fixed while making room around it.
      const anchor = { ...additions[0], rect: unionBounds(additions.map(item => item.rect)) };
      const rowWidth = anchor.rect.width + BOUNDARY_GAP;
      const prepared = source ? before.map(item => item.rect.x >= anchor.rect.x
        && item.rect.y < anchor.rect.y + anchor.rect.height && item.rect.y + item.rect.height > anchor.rect.y
        ? { ...item, rect: { ...item.rect, x: item.rect.x + rowWidth } } : item) : before;
      const pushed = pushApartBoundaries([...prepared, anchor], anchor.sizeId, BOUNDARY_GAP).filter(item => item.sizeId !== anchor.sizeId);
      const placed = [...pushed, ...additions];
      const boxes = project.reference.map(item => ({ itemId: item.id, rect: item.rect }));
      const reference = referenceMoves(pushApartReference(boxes, placed), boxes);
      return { type: 'create_boundary' as const, page, after_page_id: sourceId,
        moves: boundaryMoves(placed, [...before, ...additions]).map(({ pageId, sizeId, x, y, kind }) => ({ page_id: pageId, size_id: sizeId, x, y, kind })),
        reference: reference.map(({ itemId, x, y }) => ({ item_id: itemId, x, y })) };
    });
    return next ? createdId : null;
  }, [mutate]);
  async function addProjectBoundary() {
    const project = current.current?.project;
    if (!project) return;
    const kind = projectKind(project);
    const id = await createCanvasBoundary(boundaryDefaults(project, kind), kind);
    selectCanvasBoundary(id);
    if (id) requestAnimationFrame(() => canvasRef.current?.focusBoundary(id));
  }
  const updateCanvasPage = useCallback((pageId: string, patch: { name?: string; path?: string; background?: string; included?: boolean }) => {
    void mutate(project => {
      const page = project.pages.find(item => item.id === pageId);
      if (!page) return null;
      const name = patch.name ?? page.name, path = patch.path ?? page.path, background = patch.background ?? page.background;
      const included = patch.included ?? page.included;
      if (name === page.name && path === page.path && background === page.background && included === page.included) return null;
      return { type: 'update_page', page_id: pageId, name, path, background, included };
    });
  }, [mutate]);
  const updateCanvasPageSize = useCallback((pageId: string, sizeId: string, patch: { label?: string; width?: number; height?: number; kind?: BoundaryKind }) => {
    void mutate(project => {
      const page = project.pages.find(item => item.id === pageId);
      const size = page?.sizes.find(item => item.id === sizeId);
      if (!page || !size) return null;
      const before = sizeBoundaries(project);
      const resized = before.map(item => item.sizeId === sizeId
        ? { ...item, rect: { ...item.rect, width: patch.width ?? item.rect.width, height: patch.height ?? item.rect.height } }
        : item);
      const pushed = pushApartBoundaries(resized, sizeId);
      return {
        type: 'update_page_size' as const, page_id: pageId, size_id: sizeId, ...patch,
        moves: boundaryMoves(pushed, before).map(({ pageId: movePageId, sizeId: movedId, x, y, kind }) => ({ page_id: movePageId, size_id: movedId, x, y, kind })),
        reference: [] };
    });
  }, [mutate]);
  /** Adding a linked size is one undo step, including the boundaries it pushes aside. */
  const addCanvasPageSize = useCallback((pageId: string, preset: SizePreset, width = 0, height = 0) => {
    const sizeId = crypto.randomUUID();
    let focusedSizeId: string = sizeId;
    void mutate(project => {
      const page = project.pages.find(item => item.id === pageId);
      if (!page) return null;
      const last = page.sizes.at(-1)!;
      const dimensions = SIZE_PRESETS.find(item => item.preset === preset)
        ?? { label: sizeLabel(Math.round(width)), width: Math.round(width), height: Math.round(height) };
      // A linked size of the same width already covers this preset: focus it instead of adding a
      // duplicate. The base size is never a match; a small base can still gain a phone size.
      const existing = preset !== 'custom' && page.sizes.slice(1).find(size => size.width === dimensions.width);
      if (existing) { focusedSizeId = existing.id; return null; }
      const size: PageSize = { id: sizeId, ...dimensions, kind: last.kind, x: last.x + last.width + BOUNDARY_GAP, y: last.y };
      const anchor = sizeBoundaries(project);
      const pushed = pushApartBoundaries([...anchor, { pageId, sizeId: size.id, label: size.label, rect: { x: size.x, y: size.y, width: size.width, height: size.height }, kind: size.kind }], size.id);
      const placed = pushed.find(item => item.sizeId === size.id)!;
      return {
        type: 'add_page_size' as const, page_id: pageId,
        size: { ...size, x: placed.rect.x, y: placed.rect.y },
        moves: boundaryMoves(pushed.filter(item => item.sizeId !== size.id), anchor)
          .map(({ pageId: movePageId, sizeId, x, y, kind }) => ({ page_id: movePageId, size_id: sizeId, x, y, kind })),
        reference: [] };
    }).then(next => {
      if (next?.project?.pages.find(page => page.id === pageId)?.sizes.some(size => size.id === focusedSizeId)) selectCanvasBoundary(pageId, focusedSizeId);
    });
  }, [mutate, selectCanvasBoundary]);
  return { createCanvasBoundary, addProjectBoundary, updateCanvasPage, updateCanvasPageSize, addCanvasPageSize };
}
