import type { BoundaryMove, ReferenceMove } from '../projectCanvasGeometry';
import type {
  Command, Element, ElementMove, ElementPatch, LinkedPasteInstance, PendingRequest, Pointer, PointerSource,
  PointerTarget, Rect, ReferenceItem, WorkspaceView,
} from '../types';

type Mutate = (command: Command) => Promise<WorkspaceView>;

export type ReferencePatch = { rect?: Rect; text?: string; url?: string; title?: string; color?: string };
export type PointerPatch = { from?: PointerSource; to?: PointerTarget; comment?: string; kind?: Pointer['kind'] };

/**
 * Every direct edit the canvas and Properties make to the project, as one object. Each edit is
 * one project command; callers never build commands or know their wire names. Edits whose
 * failure the app already reports (layer moves and style patches made while dragging) settle
 * quietly instead of rejecting.
 */
export type ProjectEdits = {
  insertElements: (pageId: string, parentId: string | null, index: number, elements: Element[], pictureDataUri?: string, sizeId?: string | null) => Promise<void>;
  updateElements: (pageId: string, patches: ElementPatch[]) => Promise<void>;
  moveElements: (moves: ElementMove[]) => Promise<void>;
  linkElements: (ids: string[], sourceId: string | null) => Promise<void>;
  unlinkElements: (ids: string[]) => Promise<void>;
  pasteLinked: (items: LinkedPasteInstance[]) => Promise<void>;
  addPointer: (pointer: Pointer) => Promise<void>;
  updatePointer: (pointerId: string, patch: PointerPatch) => Promise<WorkspaceView>;
  addReference: (item: ReferenceItem, pictureDataUri?: string) => Promise<void>;
  updateReference: (itemId: string, patch: ReferencePatch) => void;
  addRequest: (request: PendingRequest) => Promise<void>;
  updateRequest: (request: PendingRequest) => Promise<void>;
  savePlacements: (placements: BoundaryMove[], reference: ReferenceMove[]) => Promise<void>;
};

export function projectEdits(mutate: Mutate): ProjectEdits {
  const run = async (command: Command) => { await mutate(command); };
  // The failed command is already shown by the app; the caller has nothing to undo.
  const quiet = async (command: Command) => { try { await mutate(command); } catch { /* reported by the session */ } };
  return {
    insertElements: (pageId, parentId, index, elements, pictureDataUri, sizeId) => run({
      type: 'insert_elements', page_id: pageId, parent_id: parentId, index, elements,
      ...(pictureDataUri === undefined ? {} : { picture_data_uri: pictureDataUri }),
      ...(sizeId ? { size_id: sizeId } : {}),
    }),
    updateElements: (pageId, patches) => patches.length ? quiet({ type: 'update_elements', page_id: pageId, patches }) : Promise.resolve(),
    moveElements: moves => moves.length ? quiet({ type: 'move_elements', moves }) : Promise.resolve(),
    linkElements: (ids, sourceId) => run({ type: 'link_elements', ids, source_id: sourceId }),
    unlinkElements: ids => run({ type: 'unlink_elements', ids }),
    pasteLinked: items => run({ type: 'paste_linked_instances', items }),
    addPointer: pointer => run({ type: 'add_pointer', pointer }),
    updatePointer: (pointerId, patch) => mutate({ type: 'update_pointer', pointer_id: pointerId, ...patch }),
    addReference: (item, pictureDataUri) => run(pictureDataUri === undefined
      ? { type: 'add_reference_item', item }
      : { type: 'add_reference_item', item, picture_data_uri: pictureDataUri }),
    updateReference: (itemId, patch) => { void quiet({ type: 'update_reference_item', item_id: itemId, ...patch }); },
    addRequest: request => run({ type: 'add_request', request }),
    updateRequest: request => run({ type: 'update_request', request }),
    savePlacements: (placements, reference) => placements.length || reference.length ? run({
      type: 'set_canvas_layout',
      placements: placements.map(({ pageId, sizeId, x, y, kind }) => ({ page_id: pageId, size_id: sizeId, x, y, kind })),
      reference: reference.map(({ itemId, x, y }) => ({ item_id: itemId, x, y })),
    }) : Promise.resolve(),
  };
}
