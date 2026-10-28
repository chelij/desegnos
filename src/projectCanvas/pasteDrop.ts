import { type DragEvent as ReactDragEvent } from 'react';
import { isTextField } from '../WorkspaceSurface';
import { boundaryAt, type Boundary, type CanvasCamera, type CanvasTarget, type Point } from '../projectCanvasGeometry';
import type { Element, ElementKind, ElementMove, ElementPatch, LinkedPasteInstance, Project, Rect, ReferenceItem } from '../types';
import { elementBoxesNow } from './requests';
import { type ScreenRect } from './surface';
import { nestedLayerIds } from './links';
import { pasteDefault, setPasteDefault, type PasteMode } from '../pastePreference';
import { baseSizeId, hasManySizes } from './sizes';
import {
  isClipboardMarker, newClipboardMarker, pastedImageFiles, pastedImageRect, pastedNoteRect, readPastedImage,
} from './clipboardPaste';
import { type PlayState } from './play';
import { createTextElement, forBackground, withPickedColours, elementChildrenOf, elementPath, findPageElement } from './elementGeometry';
import {
  ElementEntry, ClipboardEntry, PasteTarget, PasteChoice, LinkDecision, NIL_UUID, DuplicateRepeat,
  isElementTarget, elementAt, targetKey, entryTargetKey,
} from './interactionTypes';

type Context = {
  elementEntries: (targets: CanvasTarget[]) => ElementEntry[];
  selection: CanvasTarget[];
  clipboard: import("react").RefObject<ClipboardEntry[]>;
  pasteCount: import("react").RefObject<number>;
  setPasteChoice: import("react").Dispatch<import("react").SetStateAction<PasteChoice | null>>;
  clipboardMark: import("react").RefObject<string | null>;
  project: Project;
  boundaryRect: (sizeId: string) => Rect | null;
  lastPointer: import("react").RefObject<Point | null>;
  viewSize: { width: number; height: number; };
  camera: CanvasCamera;
  boundaryUnder: (point: Point) => Boundary | null;
  editingAllSizes: (pageId: string) => boolean;
  pasteLinked: (items: LinkedPasteInstance[]) => Promise<unknown>;
  selectTargets: (targets: { pageId: string; elementId: string; sizeId?: string | null; }[]) => void;
  insertElements: (pageId: string, parentId: string | null, index: number, elements: Element[], pictureDataUri?: string, sizeId?: string | null) => Promise<void>;
  setLinkChoice: import("react").Dispatch<import("react").SetStateAction<LinkDecision | null>>;
  pasteMode: PasteMode | null;
  pasteChoice: PasteChoice | null;
  duplicateRepeat: import("react").RefObject<DuplicateRepeat | null>;
  addReference: (item: ReferenceItem, pictureDataUri?: string) => Promise<void>;
  selectReference: (itemId: string, additive?: boolean) => void;
  elementRectAt: (sizeId: string, parentId: string | null, parentOrigin: Point, boundary: Rect, rect: Rect) => Rect;
  busy: import("react").RefObject<boolean>;
  insertionPoint: (pageId: string, parentId: string | null) => number;
  selectElement: (pageId: string, elementId: string, sizeId: string | null, additive?: boolean) => void;
  placeElement: (kind: ElementKind, pageId: string, sizeId: string, parentId: string | null, parentOrigin: Point, boundary: Rect, rect: Rect, picture?: string, rotation?: number) => Promise<Element>;
  createdElement: import("react").RefObject<string | null>;
  worldPoint: (event: { clientX: number; clientY: number; }) => Point;
  latest: import("react").RefObject<{ camera: CanvasCamera; boundaries: Boundary[]; project: Project; selection: CanvasTarget[]; focused: string | null; addReference: (item: ReferenceItem, pictureDataUri?: string) => Promise<void>; attachToRequest: (requestId: string, files: File[], links: string[]) => void; selectPointer: (pointerId: string, additive?: boolean) => void; selectReference: (itemId: string, additive?: boolean) => void; selectElement: (pageId: string, elementId: string, sizeId: string | null, additive?: boolean) => void; clearSelection: () => void; removeElements: (elements: { pageId: string; elementId: string; }[]) => void; insertElements: (pageId: string, parentId: string | null, index: number, elements: Element[], pictureDataUri?: string, sizeId?: string | null) => Promise<void>; updateElements: (pageId: string, patches: ElementPatch[]) => Promise<unknown> | void; moveElements: (moves: ElementMove[]) => Promise<unknown> | void; groupElements: (pageId: string, ids: string[]) => Promise<string | null>; ungroupElements: (pageId: string, id: string) => Promise<string[]>; tool: import("../EditorTools").EditorTool; insertKind: ElementKind | null; editing: { pageId: string; sizeId: string; elementId: string; autoWidth: boolean; created: boolean; } | null; play: PlayState; onPlayElement: (pageId: string, elementId: string) => void; }>;
  attachToRequest: (requestId: string, files: File[], links: string[]) => void;
};

export function createPasteDrop(context: () => Context) {
  function copyElements() {
    const { elementEntries, selection, clipboard, pasteCount, setPasteChoice, clipboardMark } = context();
    const entries = elementEntries(selection);
    if (!entries.length) return;
    clipboard.current = entries.map(entry => ({ id: entry.id, pageId: entry.pageId, parentId: entry.parentId, index: entry.index,
      element: structuredClone(entry.element), world: entry.world, rect: entry.rect }));
    pasteCount.current = 0;
    setPasteChoice(null);
    // The marker says a later paste reads these layers, not whatever was on the clipboard before.
    // When it cannot be written, paste falls back to the layers copied here.
    const mark = newClipboardMarker();
    clipboardMark.current = mark;
    const unwritten = () => { if (clipboardMark.current === mark) clipboardMark.current = null; };
    try { void navigator.clipboard.writeText(mark).catch(unwritten); } catch { unwritten(); }
  }

  function pasteTarget(): PasteTarget | null {
    const { selection, project, boundaryRect, lastPointer, viewSize, camera, boundaryUnder, editingAllSizes } = context();
    const selected = selection.find(isElementTarget);
    const shared = nestedLayerIds(project);
    if (selected && !shared.has(selected.elementId)) {
      const page = project.pages.find(item => item.id === selected.pageId);
      const sizeId = selected.sizeId ?? (page ? baseSizeId(page) : null);
      const element = findPageElement(project, selected.pageId, selected.elementId, sizeId);
      const path = sizeId ? elementPath(project, selected.pageId, selected.elementId, sizeId) : null;
      const boundary = sizeId ? boundaryRect(sizeId) : null;
      const last = path?.[path.length - 1];
      if (element?.kind === 'box' && last && boundary && sizeId) return { pageId: selected.pageId, sizeId, allSizes: selected.sizeId === null, parentId: element.id, parentOrigin: last.origin, boundary };
    }
    const point = lastPointer.current ?? { x: (viewSize.width / 2 - camera.x) / camera.zoom, y: (viewSize.height / 2 - camera.y) / camera.zoom };
    const boundary = boundaryUnder(point);
    if (!boundary) return null;
    return { pageId: boundary.pageId, sizeId: boundary.sizeId, allSizes: editingAllSizes(boundary.pageId), parentId: null, parentOrigin: { x: 0, y: 0 }, boundary: boundary.rect };
  }

  function pasteScreen(): ScreenRect {
    const { lastPointer, camera } = context();
    const point = lastPointer.current;
    if (!point) return { left: 24, top: 24, width: 0, height: 0 };
    return { left: point.x * camera.zoom + camera.x, top: point.y * camera.zoom + camera.y, width: 0, height: 0 };
  }

  function pasteEntries(entries: ClipboardEntry[], mode: PasteMode, target: PasteTarget, offset: Point,
    onInserted?: (targets: { pageId: string; elementId: string; sizeId: string | null }[]) => void, selectAfter = true) {
    const { project, pasteLinked, selectTargets, insertElements } = context();
    if (!entries.length) return;
    const index = elementChildrenOf(project, target.pageId, target.parentId).length;
    if (mode === 'linked') {
      const items: LinkedPasteInstance[] = entries.map((entry, position) => {
        const world = { x: entry.world.x + offset.x, y: entry.world.y + offset.y };
        return {
          id: crypto.randomUUID(), sourceId: entry.id, pageId: target.pageId, parentId: target.parentId, index: index + position,
          rect: { x: world.x - target.boundary.x - target.parentOrigin.x, y: world.y - target.boundary.y - target.parentOrigin.y, width: entry.rect.width, height: entry.rect.height },
        };
      });
      const targets = items.map(item => ({ pageId: item.pageId, elementId: item.id, sizeId: target.allSizes ? null : target.sizeId }));
      void pasteLinked(items).then(() => { onInserted?.(targets); if (selectAfter) selectTargets(targets); }).catch(() => {});
      return;
    }
    const inserted: { pageId: string; elementId: string; sizeId: string | null }[] = [];
    const elements = entries.map(entry => {
      const world = { x: entry.world.x + offset.x, y: entry.world.y + offset.y };
      const element: Element = { ...structuredClone(entry.element), id: crypto.randomUUID(), link: null, overrides: {},
        rect: { x: world.x - target.boundary.x - target.parentOrigin.x, y: world.y - target.boundary.y - target.parentOrigin.y, width: entry.rect.width, height: entry.rect.height },
        children: cloneChildren(entry.element.children) };
      inserted.push({ pageId: target.pageId, elementId: element.id, sizeId: target.allSizes ? null : target.sizeId });
      return element;
    });
    // A layer pasted in one size is hidden in the others, exactly like a newly drawn layer.
    void insertElements(target.pageId, target.parentId, index, elements, undefined, target.allSizes ? null : target.sizeId)
      .then(() => { onInserted?.(inserted); if (selectAfter) selectTargets(inserted); }).catch(() => {});
  }

  function pasteElements(alwaysAsk = false) {
    const { clipboard, setLinkChoice, pasteMode, pasteCount, setPasteChoice } = context();
    if (!clipboard.current.length) return;
    setLinkChoice(null);
    const target = pasteTarget();
    if (!target) return;
    const preferred = pasteMode ?? pasteDefault() ?? 'unlinked';
    if (!alwaysAsk) {
      pasteCount.current += 1;
      pasteEntries(clipboard.current, preferred, target, { x: 16 * pasteCount.current, y: 16 * pasteCount.current });
      return;
    }
    setPasteChoice({ target, offset: 16 * (pasteCount.current + 1), value: preferred, useDefault: false, screen: pasteScreen() });
  }

  function confirmPaste() {
    const { pasteChoice, setPasteChoice, pasteCount, clipboard } = context();
    const choice = pasteChoice;
    if (!choice) return;
    setPasteChoice(null);
    pasteCount.current += 1;
    if (choice.useDefault) setPasteDefault(choice.value);
    pasteEntries(clipboard.current, choice.value, choice.target, { x: choice.offset, y: choice.offset });
  }

  function cloneChildren(children: Element[]): Element[] {
    return children.map(child => ({ ...structuredClone(child), id: crypto.randomUUID(), children: cloneChildren(child.children) }));
  }

  function duplicateElements() {
    const { elementEntries, selection, duplicateRepeat, pasteMode, project, boundaryRect, selectTargets } = context();
    const entries = elementEntries(selection);
    if (!entries.length) return;
    const repeat = duplicateRepeat.current;
    const key = entryTargetKey(entries);
    const offset = repeat?.key === key ? repeat.offset : { x: 16, y: 16 };
    const mode = pasteMode ?? pasteDefault() ?? 'unlinked';
    const shared = nestedLayerIds(project);
    const groups = new Map<string, { target: PasteTarget; entries: ClipboardEntry[] }>();
    for (const entry of entries) {
      const boundary = boundaryRect(entry.sizeId);
      if (!boundary) continue;
      // A layer inside a linked element is copied beside its copy, not into the shared box.
      const parentId = entry.parentId && !shared.has(entry.parentId) ? entry.parentId : null;
      const parentOrigin = parentId ? entry.parentOrigin : { x: 0, y: 0 };
      const groupKey = `${entry.pageId}\u0000${entry.sizeId}\u0000${parentId ?? ''}`;
      const group = groups.get(groupKey) ?? { target: { pageId: entry.pageId, sizeId: entry.sizeId, allSizes: entry.allSizes, parentId, parentOrigin, boundary }, entries: [] };
      group.entries.push({ id: entry.id, pageId: entry.pageId, parentId: entry.parentId, index: entry.index, element: entry.element, world: entry.world, rect: entry.rect });
      groups.set(groupKey, group);
    }
    // Copies can land in several boxes; select them all and arm the repeat once they exist.
    const inserted: { pageId: string; elementId: string; sizeId: string | null }[] = [];
    let pending = groups.size;
    for (const group of groups.values()) {
      pasteEntries(group.entries, mode, group.target, offset, targets => {
        inserted.push(...targets);
        pending -= 1;
        if (pending > 0) return;
        duplicateRepeat.current = { key: targetKey(inserted), offset };
        selectTargets(inserted);
      }, false);
    }
  }

  function createReference(text: string, point: Point): boolean {
    const { addReference, selectReference } = context();
    if (/^#([0-9a-f]{3}|[0-9a-f]{6})$/i.test(text)) {
      const id = crypto.randomUUID();
      void addReference({ id, rect: { x: point.x, y: point.y, width: 120, height: 120 }, kind: { kind: 'swatch', color: text.toLowerCase() } })
        .then(() => selectReference(id)).catch(() => {});
      return true;
    }
    let url: URL;
    try { url = new URL(text); } catch { return false; }
    if (url.protocol !== 'http:' && url.protocol !== 'https:') return false;
    const id = crypto.randomUUID();
    void addReference({ id, rect: { x: point.x, y: point.y, width: 240, height: 100 }, kind: { kind: 'link', url: text, title: url.hostname } })
      .then(() => selectReference(id)).catch(() => {});
    return true;
  }

  function referencePoint(): Point | null {
    const { lastPointer, viewSize, camera } = context();
    if (lastPointer.current) return lastPointer.current;
    if (viewSize.width < 1 || viewSize.height < 1) return null;
    return { x: (viewSize.width / 2 - camera.x) / camera.zoom, y: (viewSize.height / 2 - camera.y) / camera.zoom };
  }

  async function pasteText(text: string, point: Point) {
    const { boundaryUnder, addReference, selectReference, project, elementRectAt, editingAllSizes, busy, insertElements, insertionPoint, selectElement } = context();
    const boundary = boundaryUnder(point);
    if (!boundary) {
      if (createReference(text, point)) return;
      const id = crypto.randomUUID();
      await addReference({ id, rect: pastedNoteRect(point), kind: { kind: 'note', text: text.slice(0, 16384) } })
        .then(() => selectReference(id)).catch(() => {});
      return;
    }
    const page = project.pages.find(item => item.id === boundary.pageId);
    const local = elementRectAt(boundary.sizeId, null, { x: 0, y: 0 }, boundary.rect, { ...point, width: 0, height: 0 });
    const element = withPickedColours(forBackground(createTextElement({ ...local, width: 0, height: 0 }, text), page?.background ?? '#ffffff'));
    const singleSize = !!page && hasManySizes(page) && !editingAllSizes(boundary.pageId);
    busy.current = true;
    try {
      await insertElements(boundary.pageId, null, insertionPoint(boundary.pageId, null), [element], undefined, singleSize ? boundary.sizeId : null);
      selectElement(boundary.pageId, element.id, singleSize ? boundary.sizeId : null);
    } catch { /* App surfaces the failed command. */ }
    finally { busy.current = false; }
  }

  async function pasteImage(files: File[]) {
    const { boundaryUnder, busy, placeElement, createdElement, addReference, selectReference } = context();
    // One picture per paste keeps the whole paste a single undo step.
    const point = referencePoint();
    const image = await readPastedImage(files[0]);
    if (!image || !point) return;
    const rect = pastedImageRect(point, image.width, image.height);
    const boundary = boundaryUnder(point);
    if (boundary) {
      busy.current = true;
      try { await placeElement('image', boundary.pageId, boundary.sizeId, null, { x: 0, y: 0 }, boundary.rect, rect, image.uri); }
      catch { /* App surfaces the failed command. */ }
      finally { busy.current = false; createdElement.current = null; }
      return;
    }
    const id = crypto.randomUUID();
    await addReference({ id, rect, kind: { kind: 'image', picture: NIL_UUID, name: image.name.slice(0, 120) } }, image.uri)
      .then(() => selectReference(id)).catch(() => {});
  }

  function pasteClipboard(data: DataTransfer | null, target: EventTarget | null): boolean {
    const { clipboard, clipboardMark } = context();
    if (!data || isTextField(target)) return false;
    const images = pastedImageFiles(data);
    if (images.length) { void pasteImage(images); return true; }
    const text = data.getData('text').trim();
    const internal = clipboard.current.length > 0;
    // Our marker on the clipboard means the layers copied here; unreadable text falls back to them.
    if (internal && (!clipboardMark.current || !text || text === clipboardMark.current)) { pasteElements(false); return true; }
    if (!text) return false;
    if (isClipboardMarker(text)) return true;
    const point = referencePoint();
    if (!point) return false;
    void pasteText(text, point);
    return true;
  }

  function paste(event: React.ClipboardEvent<HTMLDivElement>) {
    if (pasteClipboard(event.clipboardData, event.target)) event.preventDefault();
  }

  function dropText(event: ReactDragEvent<HTMLDivElement>) {
    const { worldPoint, latest, attachToRequest } = context();
    if (isTextField(event.target) || event.dataTransfer.files.length) return;
    const text = (event.dataTransfer.getData('text/uri-list') || event.dataTransfer.getData('text/plain') || '').trim();
    if (!text) return;
    event.preventDefault();
    const point = worldPoint(event);
    if (boundaryAt(latest.current.boundaries, point)) {
      const element = elementAt(elementBoxesNow(latest.current.project, latest.current.boundaries), point);
      if (element) attachToRequest(element.elementId, [], [text]);
      return;
    }
    createReference(text, point);
  }

  return {
    copyElements, pasteTarget, pasteScreen, pasteEntries, pasteElements, confirmPaste, cloneChildren,
    duplicateElements, createReference, referencePoint, pasteText, pasteImage, pasteClipboard, paste, dropText,
  };
}
