import type { BoundaryMove, ReferenceMove } from '../projectCanvasGeometry';
import type { Command, Element, ElementMove, ElementPatch, LinkedPasteInstance, PendingRequest, Pointer, PointerSource, PointerTarget, Rect, ReferenceItem, WorkspaceView } from '../types';

type Mutate = (command: Command) => Promise<WorkspaceView | null>;

export type ReferencePatch = { rect?: Rect; text?: string; url?: string; title?: string; color?: string };
export type PointerPatch = { from?: PointerSource; to?: PointerTarget; comment?: string; kind?: Pointer['kind'] };

export type ProjectEdits = {
  setDecision: (decision: Omit<Extract<Command, { type: 'set_decision' }>, 'type'>) => Promise<WorkspaceView | null>;
  removeDecision: (id: string) => Promise<WorkspaceView | null>;
  addException: (exception: Omit<Extract<Command, { type: 'add_exception' }>, 'type'>) => Promise<WorkspaceView | null>;
  removeException: (id: string) => Promise<WorkspaceView | null>;
  insertElements: (pageId: string, parentId: string | null, index: number, elements: Element[], pictureDataUri?: string, sizeId?: string | null) => Promise<void>;
  updateElements: (pageId: string, patches: ElementPatch[]) => Promise<void>;
  moveElements: (moves: ElementMove[]) => Promise<void>;
  linkElements: (ids: string[], sourceId: string | null) => Promise<void>;
  unlinkElements: (ids: string[]) => Promise<void>;
  pasteLinked: (items: LinkedPasteInstance[]) => Promise<void>;
  addPointer: (pointer: Pointer) => Promise<void>;
  updatePointer: (pointerId: string, patch: PointerPatch) => Promise<WorkspaceView | null>;
  addReference: (item: ReferenceItem, pictureDataUri?: string) => Promise<void>;
  updateReference: (itemId: string, patch: ReferencePatch) => void;
  addRequest: (request: PendingRequest) => Promise<void>;
  updateRequest: (request: PendingRequest) => Promise<void>;
  savePlacements: (placements: BoundaryMove[], reference: ReferenceMove[]) => Promise<void>;
};

export function projectEdits(mutate: Mutate): ProjectEdits {
  const run = async (command: Command) => { await mutate(command); };
  return {
    setDecision: decision => mutate({ type: 'set_decision', ...decision }),
    removeDecision: id => mutate({ type: 'remove_decision', decision_id: id }),
    addException: exception => mutate({ type: 'add_exception', ...exception }),
    removeException: id => mutate({ type: 'remove_exception', exception_id: id }),
    insertElements: (pageId, parentId, index, elements, pictureDataUri, sizeId) => run({
      type: 'insert_elements', page_id: pageId, parent_id: parentId, index, elements,
      ...(pictureDataUri === undefined ? {} : { picture_data_uri: pictureDataUri }),
      ...(sizeId ? { size_id: sizeId } : {}) }),
    updateElements: (pageId, patches) => patches.length ? run({ type: 'update_elements', page_id: pageId, patches }) : Promise.resolve(),
    moveElements: moves => moves.length ? run({ type: 'move_elements', moves }) : Promise.resolve(),
    linkElements: (ids, sourceId) => run({ type: 'link_elements', ids, source_id: sourceId }),
    unlinkElements: ids => run({ type: 'unlink_elements', ids }),
    pasteLinked: items => run({ type: 'paste_linked_instances', items }),
    addPointer: pointer => run({ type: 'add_pointer', pointer }),
    updatePointer: (pointerId, patch) => mutate({ type: 'update_pointer', pointer_id: pointerId, ...patch }),
    addReference: (item, pictureDataUri) => run(pictureDataUri === undefined
      ? { type: 'add_reference_item', item }
      : { type: 'add_reference_item', item, picture_data_uri: pictureDataUri }),
    updateReference: (itemId, patch) => { void run({ type: 'update_reference_item', item_id: itemId, ...patch }); },
    addRequest: request => run({ type: 'add_request', request }),
    updateRequest: request => run({ type: 'update_request', request }),
    savePlacements: (placements, reference) => placements.length || reference.length ? run({
      type: 'set_canvas_layout',
      placements: placements.map(({ pageId, sizeId, x, y, kind }) => ({ page_id: pageId, size_id: sizeId, x, y, kind })),
      reference: reference.map(({ itemId, x, y }) => ({ item_id: itemId, x, y })) }) : Promise.resolve() };
}
