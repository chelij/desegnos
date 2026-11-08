import { useCallback, type Dispatch, type RefObject, type SetStateAction } from 'react';
import type { ProjectCanvasHandle } from '../ProjectCanvas';
import type { CanvasTarget } from '../projectCanvas/canvasTargets';
import { elementPath, elementTree, elementTreeIds } from '../projectCanvas/elementGeometry';
import { findElementIn } from '../projectCanvas/elementFacts';
import { resolveForSize } from '../projectCanvas/sizes';
import type { Command, GridSettings, Pointer, Project, Rect, WorkspaceView } from '../types';
import type { ProjectEdits } from './projectEdits';

type Mutate = (command: Command | ((project: Project) => Command | null)) => Promise<WorkspaceView | null>;

/** Canvas edits that also move the selection or announce an undoable result. */
export function useCanvasEdits({ current, mutate, edits, run, setNotice, travel, canvasRef, setCanvasSelection, setCanvasFocus, setCanvasFocusSize }: {
  current: RefObject<WorkspaceView | null>;
  mutate: Mutate;
  edits: ProjectEdits;
  run: (operation: () => Promise<unknown>) => void;
  setNotice: (message: string, undo?: () => void) => void;
  travel: (command: 'undo_project' | 'redo_project') => Promise<void>;
  canvasRef: RefObject<ProjectCanvasHandle | null>;
  setCanvasSelection: Dispatch<SetStateAction<CanvasTarget[]>>;
  setCanvasFocus: Dispatch<SetStateAction<string | null>>;
  setCanvasFocusSize: Dispatch<SetStateAction<string | null>>;
}) {
  const pageElements = (pageId: string) => current.current?.project?.pages.find(page => page.id === pageId)?.elements ?? [];

  const removedWithUndo = useCallback((message: string) => {
    requestAnimationFrame(() => { if (!document.querySelector('.workspace-surface:focus-within')) canvasRef.current?.focusCanvas(); });
    setNotice(message, () => { run(() => travel('undo_project')); });
  }, [setNotice, travel]);

  const groupCanvasElements = useCallback(async (pageId: string, ids: string[]) => {
    const before = new Set(elementTreeIds(pageElements(pageId)));
    const next = await mutate({ type: 'group_elements', page_id: pageId, ids });
    const page = next?.project?.pages.find(page => page.id === pageId);
    return page ? elementTree(page.elements).find(element => element.kind === 'group' && !before.has(element.id))?.id ?? null : null;
  }, [mutate]);
  const ungroupCanvasElements = useCallback(async (pageId: string, id: string) => {
    const group = findElementIn(pageElements(pageId), id);
    const children = group?.kind === 'group' ? group.children.map(child => child.id) : [];
    const next = await mutate({ type: 'ungroup_elements', page_id: pageId, id });
    return next ? children : [];
  }, [mutate]);
  const removeLinkedDefinition = useCallback((id: string) => {
    const name = current.current?.project?.linked[id]?.name ?? 'this linked element';
    void mutate({ type: 'delete_items', linked_ids: [id] }).then(next => { if (next) removedWithUndo(`Removed ${name}.`); });
  }, [mutate, removedWithUndo]);
  const renameCanvasElement = useCallback((pageId: string, elementId: string, name: string | null) =>
    mutate({ type: 'update_elements', page_id: pageId, patches: [{ id: elementId, name }] }).then(() => undefined), [mutate]);

  const keepPointerComment = useCallback((pointerId: string) => {
    void mutate({ type: 'set_pointer_wiring', pointer_id: pointerId, wiring: null });
  }, [mutate]);
  const keepPointerThis = useCallback((pointerId: string, otherId: string) => {
    void mutate({ type: 'resolve_pointer_clash', keep: pointerId, drop: [otherId] });
  }, [mutate]);
  const mergePointerClash = useCallback((pointer: Pointer, other: Pointer) => {
    const comment = [pointer.comment.trim(), other.comment.trim()].filter(Boolean).join('\n\n');
    void mutate({ type: 'resolve_pointer_clash', keep: pointer.id, drop: [other.id], merged_comment: comment });
  }, [mutate]);

  const elementToReference = useCallback(async (pageId: string, elementId: string, rect: Rect, sizeId?: string | null) => {
    if (await mutate({ type: 'element_to_reference', page_id: pageId, element_id: elementId, rect, size_id: sizeId })) setCanvasSelection([]);
  }, [mutate]);
  const referenceToElement = useCallback(async (itemId: string, pageId: string, rect: Rect, sizeId?: string | null) => {
    const next = await mutate({ type: 'reference_to_element', item_id: itemId, page_id: pageId, rect, size_id: sizeId });
    if (!next) return;
    const added = next.project?.pages.find(page => page.id === pageId)?.elements.at(-1);
    setCanvasFocus(pageId);
    setCanvasSelection(added ? [{ kind: 'element', pageId, elementId: added.id, sizeId: sizeId ?? null }] : []);
  }, [mutate]);
  const attachReferenceContext = useCallback(async (requestIds: string[], itemIds: string[]) => {
    if (!requestIds.length || !itemIds.length) return;
    await mutate({ type: 'attach_reference_context', request_ids: requestIds, item_ids: itemIds });
  }, [mutate]);

  const renameProject = useCallback((name: string) => { void mutate({ type: 'rename_project', name }); }, [mutate]);
  const showDotGrid = useCallback((enabled: boolean) => { void mutate({ type: 'set_grid', enabled }); }, [mutate]);
  const changeGridFlags = useCallback((patch: Partial<Pick<GridSettings, 'snap'>>) => {
    void mutate(project => ({ type: 'set_grid_settings', settings: { ...project.gridSettings, ...patch } }));
  }, [mutate]);

  function removeCanvasSelection(referenceIds: string[], pointerIds: string[] = [], requestIds: string[] = [], elementIds: { pageId: string; elementId: string }[] = []) {
    const count = referenceIds.length + pointerIds.length + requestIds.length + elementIds.length;
    if (!count) return;
    run(async () => {
      if (!await mutate({
        type: 'delete_items',
        elements: elementIds.map(item => [item.pageId, item.elementId]) as [string, string][],
        reference_ids: referenceIds,
        pointer_ids: pointerIds,
        request_ids: requestIds })) return;
      setCanvasSelection([]);
      removedWithUndo(`${count} ${count === 1 ? 'item' : 'items'} deleted.`);
    });
  }
  // The base size carries the values the other sizes inherit, so deleting it deletes the page.
  function deleteBoundary(pageId: string, sizeId: string | null) {
    const page = current.current?.project?.pages.find(item => item.id === pageId);
    if (!page) return;
    const size = sizeId ? page.sizes.find(item => item.id === sizeId) : null;
    const removeSize = !!size && size.id !== page.sizes[0].id;
    const name = removeSize ? `${page.name} · ${size.label}` : page.name;
    run(async () => {
      if (!await mutate(removeSize ? { type: 'delete_items', size_ids: [[pageId, size!.id]] } : { type: 'delete_items', page_ids: [pageId] })) return;
      setCanvasFocus(null); setCanvasFocusSize(null);
      setCanvasSelection([]);
      removedWithUndo(`Removed ${name}.`);
    });
  }
  function moveCanvasElementTo(pageId: string, sizeId: string, elementId: string, parentId: string | null, index: number) {
    const projectNow = current.current?.project;
    const page = projectNow?.pages.find(page => page.id === pageId);
    if (!projectNow || !page) return;
    const element = findElementIn(resolveForSize(projectNow, page, sizeId), elementId);
    const path = elementPath(projectNow, pageId, elementId, sizeId);
    const origin = path?.[path.length - 1]?.origin;
    const parentOrigin = parentId ? elementPath(projectNow, pageId, parentId, sizeId)?.at(-1)?.origin ?? { x: 0, y: 0 } : { x: 0, y: 0 };
    if (!element || !path || !origin) return;
    const over = element.overrides?.[sizeId];
    const shown = over?.rect ?? element.rect;
    const rect = { x: origin.x - parentOrigin.x, y: origin.y - parentOrigin.y, width: shown.width, height: shown.height };
    void edits.moveElements([{ id: elementId, to_page_id: pageId, to_parent_id: parentId, index, rect, size_id: sizeId }]);
  }

  return {
    removedWithUndo, groupCanvasElements, ungroupCanvasElements, removeLinkedDefinition, renameCanvasElement,
    keepPointerComment, keepPointerThis, mergePointerClash, elementToReference, referenceToElement, attachReferenceContext,
    renameProject, showDotGrid, changeGridFlags, removeCanvasSelection, deleteBoundary, moveCanvasElementTo };
}
