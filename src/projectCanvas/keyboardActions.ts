import { isTextField } from '../WorkspaceSurface';
import { arrangeSiblingOrder, type ArrangeOperation } from './arrange';
import { type KeyboardEvent as ReactKeyboardEvent } from 'react';
import {
  selectedPointers, type Boundary, type CanvasCamera, type CanvasTarget, type ElementTarget, type Point,
} from '../projectCanvasGeometry';
import type { Element, ElementKind, ElementMove, ElementPatch, NamedView, Page, Project, Rect, ReferenceItem, RequestArea, RequestShape, RequestTarget } from '../types';
import type { ProjectTool } from '../EditorTools';
import { type SelectionAlign } from './selectionBar';
import { type ScreenRect } from './surface';
import { type LinkCopy } from './linkedChoices';
import { copySummary, linkNeedsChoice, nestedLayerIds, sameStructure } from './links';
import { viewNameForSlot, type ShortcutAction } from '../shortcuts';
import { playIsActive, type PlayState } from './play';
import { unionRects, elementChildrenOf, elementParents, findPageElement } from './elementGeometry';
import { ElementEntry, ClipboardEntry, PasteTarget, LinkDecision, Drag, CanvasActionOptions, isElementTarget } from './interactionTypes';

type Context = {
  drag: { current: Drag | null; };
  cancelDrag: () => void;
  painterArmed: boolean;
  setPainterArmed: import("react").Dispatch<import("react").SetStateAction<boolean>>;
  latest: import("react").RefObject<{ camera: CanvasCamera; boundaries: Boundary[]; project: Project; selection: CanvasTarget[]; focused: string | null; addReference: (item: ReferenceItem, pictureDataUri?: string) => Promise<void>; attachToRequest: (requestId: string, files: File[], links: string[]) => void; selectPointer: (pointerId: string, additive?: boolean) => void; selectReference: (itemId: string, additive?: boolean) => void; selectElement: (pageId: string, elementId: string, sizeId: string | null, additive?: boolean) => void; clearSelection: () => void; removeElements: (elements: { pageId: string; elementId: string; }[]) => void; insertElements: (pageId: string, parentId: string | null, index: number, elements: Element[], pictureDataUri?: string, sizeId?: string | null) => Promise<void>; updateElements: (pageId: string, patches: ElementPatch[]) => Promise<unknown> | void; moveElements: (moves: ElementMove[]) => Promise<unknown> | void; groupElements: (pageId: string, ids: string[]) => Promise<string | null>; ungroupElements: (pageId: string, id: string) => Promise<string[]>; tool: import("../EditorTools").EditorTool; insertKind: ElementKind | null; editing: { pageId: string; sizeId: string; elementId: string; autoWidth: boolean; created: boolean; } | null; play: PlayState; onPlayElement: (pageId: string, elementId: string) => void; }>;
  onPlayExit: () => void;
  requestDraft: { targets: RequestTarget[]; area: RequestArea | null; screen: ScreenRect; } | null;
  polygonDraft: Point[] | null;
  draftArea: RequestShape | null;
  cancelRequestDraft: () => void;
  editing: { pageId: string; sizeId: string; elementId: string; autoWidth: boolean; created: boolean; } | null;
  finishEditingText: () => void;
  selection: CanvasTarget[];
  selectBoundary: (pageId: string | null, sizeId?: string | null, additive?: boolean) => void;
  focused: string | null;
  project: Project;
  selectTargets: (targets: { pageId: string; elementId: string; sizeId?: string | null; }[]) => void;
  pageFor: (pageId: string, sizeId?: string | null) => Page | null;
  elementEntries: (targets: CanvasTarget[]) => ElementEntry[];
  moveElements: (moves: ElementMove[]) => Promise<unknown> | void;
  copiedStyle: import("react").RefObject<import("../types").ElementStyle | null>;
  updateElements: (pageId: string, patches: ElementPatch[]) => Promise<unknown> | void;
  surface: import("react").RefObject<HTMLDivElement | null>;
  boundaryRect: (sizeId: string) => Rect | null;
  removeSelection: (referenceIds: string[], pointerIds: string[], requestIds: string[], elementIds?: { pageId: string; elementId: string; }[]) => void;
  elementScreenRect: (elementId: string, sizeId?: string | null) => ScreenRect | null;
  setLinkChoice: import("react").Dispatch<import("react").SetStateAction<LinkDecision | null>>;
  linkElements: (ids: string[], sourceId: string | null) => Promise<unknown>;
  unlinkElements: (ids: string[]) => Promise<unknown>;
  groupElements: (pageId: string, ids: string[]) => Promise<string | null>;
  ungroupElements: (pageId: string, id: string) => Promise<string[]>;
  rememberRepeatMove: (entries: ElementEntry[], delta: Point) => void;
  clipboard: import("react").RefObject<ClipboardEntry[]>;
  pasteTarget: () => PasteTarget | null;
  deleteBoundary: (pageId: string, sizeId: string | null) => void;
  focusedSize: string | null;
  startRequestOnSelection: () => boolean;
  onTool: (tool: ProjectTool) => void;
  startRequestFromShape: () => boolean;
  copyElements: () => void;
  pasteElements: (alwaysAsk?: boolean) => void;
  duplicateElements: () => void;
  setMinimapPreference: import("react").Dispatch<import("react").SetStateAction<"auto" | "off" | "on">>;
  minimapVisible: boolean;
  saveView: (slot: number | null, camera: CanvasCamera) => void;
  currentCamera: () => CanvasCamera;
  goToView: (view: NamedView) => void;
  closeRequestArea: (input: Rect | Point[], kind: 'rect' | 'ellipse' | 'polygon' | 'freehand') => void;
  openPage: (pageId: string) => void;
};

export function createKeyboardActions(context: () => Context) {
  function escapeCanvas() {
    const { drag, cancelDrag, painterArmed, setPainterArmed, latest, onPlayExit, requestDraft, polygonDraft, draftArea, cancelRequestDraft, editing, finishEditingText, selection, selectBoundary, focused } = context();
    if (drag.current) { cancelDrag(); return; }
    if (painterArmed) { setPainterArmed(false); return; }
    if (playIsActive(latest.current.play)) { onPlayExit(); return; }
    if (requestDraft || polygonDraft || draftArea) { cancelRequestDraft(); return; }
    if (editing) { finishEditingText(); return; }
    if (selection.length) {
      const target = selection.find(target => target.kind === 'element');
      selectBoundary(target?.pageId ?? null);
      return;
    }
    if (focused) selectBoundary(null);
  }

  function selectAllSiblings() {
    const { selection, project, selectTargets, focused, pageFor } = context();
    const target = selection.find(isElementTarget);
    if (target) {
      const children = elementChildrenOf(project, target.pageId, elementParents(project, target.pageId, target.elementId).at(-1) ?? null);
      selectTargets(children.map(element => ({ pageId: target.pageId, elementId: element.id })));
      return;
    }
    const pageId = focused ?? project.homePageId;
    const page = pageFor(pageId);
    if (page) selectTargets(page.elements.map(element => ({ pageId, elementId: element.id })));
  }

  function arrangeElements(operation: ArrangeOperation) {
    const { elementEntries, selection, project, moveElements } = context();
    const entries = elementEntries(selection);
    if (!entries.length) return;
    const moves: ElementMove[] = [];
    const groups = new Map<string, ElementEntry[]>();
    for (const entry of entries) {
      const key = `${entry.pageId}:${entry.parentId}`;
      groups.set(key, [...(groups.get(key) ?? []), entry]);
    }
    for (const group of groups.values()) {
      const first = group[0];
      const siblings = elementChildrenOf(project, first.pageId, first.parentId);
      const order = arrangeSiblingOrder(siblings.map(element => element.id), group.map(entry => entry.id), operation);
      // Native removes all moved layers before inserting them. Insert the selected
      // layers at their final indices in ascending order; other siblings stay untouched.
      if (order.every((id, index) => id === siblings[index].id)) continue;
      for (const [index, id] of order.entries()) {
        const entry = group.find(entry => entry.id === id);
        if (!entry) continue;
        moves.push({ id, to_page_id: entry.pageId, to_parent_id: entry.parentId, index, rect: entry.rect,
          size_id: entry.allSizes ? null : entry.sizeId, all_sizes: entry.allSizes });
      }
    }
    if (moves.length) void Promise.resolve(moveElements(moves)).catch(() => {});
  }

  function copyStyle() {
    const { elementEntries, selection, copiedStyle } = context();
    const entry = elementEntries(selection)[0];
    if (entry) copiedStyle.current = structuredClone(entry.element.style);
  }

  function pasteStyle() {
    const { copiedStyle, elementEntries, selection, updateElements } = context();
    const style = copiedStyle.current;
    const entries = elementEntries(selection);
    if (!style || !entries.length) return;
    const byPage = new Map<string, ElementPatch[]>();
    for (const entry of entries) {
      const patch: ElementPatch = { id: entry.id, style: structuredClone(style), ...(entry.allSizes ? { all_sizes: true } : { size_id: entry.sizeId }) };
      byPage.set(entry.pageId, [...(byPage.get(entry.pageId) ?? []), patch]);
    }
    for (const [pageId, patches] of byPage) void Promise.resolve(updateElements(pageId, patches)).catch(() => {});
  }

  function paintFormat() {
    const { elementEntries, selection, copiedStyle, setPainterArmed, surface } = context();
    if (elementEntries(selection).length) copyStyle();
    if (!copiedStyle.current) return;
    setPainterArmed(true);
    surface.current?.focus({ preventScroll: true });
  }

  function applyPainterStyle(target: ElementTarget) {
    const { copiedStyle, updateElements, setPainterArmed } = context();
    const style = copiedStyle.current;
    if (!style) return;
    const patch: ElementPatch = { id: target.elementId, style: structuredClone(style), ...(target.sizeId === null ? { all_sizes: true } : { size_id: target.sizeId }) };
    void Promise.resolve(updateElements(target.pageId, [patch])).catch(() => {});
    setPainterArmed(false);
  }

  function alignSelection(edge: SelectionAlign) {
    const { elementEntries, selection, boundaryRect, moveElements } = context();
    const entries = elementEntries(selection);
    if (!entries.length) return;
    const groups = new Map<string, ElementEntry[]>();
    for (const entry of entries) {
      const key = `${entry.pageId}\u0000${entry.allSizes ? '*' : entry.sizeId}`;
      const group = groups.get(key);
      if (group) group.push(entry); else groups.set(key, [entry]);
    }
    const moves: ElementMove[] = [];
    for (const group of groups.values()) {
      let box = unionRects(group.map(entry => entry.world));
      if (group.length === 1) {
        const only = group[0];
        const parent = only.parentId
          ? elementEntries([{ kind: 'element', pageId: only.pageId, elementId: only.parentId, sizeId: only.allSizes ? null : only.sizeId }])[0]
          : undefined;
        const boundary = boundaryRect(only.sizeId);
        box = parent?.world ?? boundary ?? only.world;
      }
      for (const entry of group) {
        let dx = 0; let dy = 0;
        if (edge === 'left') dx = box.x - entry.world.x;
        else if (edge === 'centre') dx = box.x + (box.width - entry.world.width) / 2 - entry.world.x;
        else if (edge === 'right') dx = box.x + box.width - entry.world.width - entry.world.x;
        else if (edge === 'top') dy = box.y - entry.world.y;
        else if (edge === 'middle') dy = box.y + (box.height - entry.world.height) / 2 - entry.world.y;
        else if (edge === 'bottom') dy = box.y + box.height - entry.world.height - entry.world.y;
        if (!dx && !dy) continue;
        moves.push({
          id: entry.id, to_page_id: entry.pageId, to_parent_id: entry.parentId, index: entry.index,
          rect: { ...entry.rect, x: Math.round(entry.rect.x + dx), y: Math.round(entry.rect.y + dy) },
          size_id: entry.allSizes ? null : entry.sizeId, all_sizes: entry.allSizes,
        });
      }
    }
    if (moves.length) void Promise.resolve(moveElements(moves)).catch(() => {});
  }

  function setSelectionFill(value: string) {
    const { selection, project, updateElements } = context();
    const groups = new Map<string, { id: string; style: Element['style'] }[]>();
    for (const target of selection.filter(isElementTarget)) {
      const element = findPageElement(project, target.pageId, target.elementId);
      if (!element) continue;
      const list = groups.get(target.pageId) ?? [];
      if (!list.some(item => item.id === element.id)) list.push({ id: element.id, style: structuredClone(element.style) });
      groups.set(target.pageId, list);
    }
    for (const [pageId, items] of groups) {
      void Promise.resolve(updateElements(pageId, items.map(item => ({ id: item.id, style: { ...item.style, fill: value } })))).catch(() => {});
    }
  }

  function deleteSelectedLayers() {
    const { selection, removeSelection } = context();
    const elements = selection.flatMap(target => target.kind === 'element' ? [{ pageId: target.pageId, elementId: target.elementId }] : []);
    const requests = selection.flatMap(target => target.kind === 'request' ? [target.requestId] : []);
    const items = selection.flatMap(target => target.kind === 'reference' ? [target.itemId] : []);
    removeSelection(items, selectedPointers(selection), requests, elements);
  }

  function linkSelection() {
    const { selection, project, elementScreenRect, setLinkChoice } = context();
    const targets = selection.filter(isElementTarget);
    const copies: { id: string; element: Element; pageId: string }[] = [];
    for (const target of targets) {
      const element = findPageElement(project, target.pageId, target.elementId);
      if (element && !copies.some(copy => copy.id === element.id)) copies.push({ id: element.id, element, pageId: target.pageId });
    }
    if (!copies.length) return;
    const screen = elementScreenRect(copies[0].id) ?? { left: 24, top: 24, width: 0, height: 0 };
    const nested = nestedLayerIds(project);
    if (copies.some(copy => nested.has(copy.id))) {
      setLinkChoice({ copies: [], error: "These layers are already linked", screen });
      return;
    }
    // One layer becomes linked on its own; the core names it from its label or the next free name.
    if (copies.length === 1) { linkCopies([copies[0].id], null); return; }
    const boundaryName = (pageId: string) => project.pages.find(page => page.id === pageId)?.name ?? 'Boundary';
    const choices: LinkCopy[] = copies.map(copy => ({ id: copy.id, label: copySummary(copy.element), boundary: boundaryName(copy.pageId) }));
    if (!copies.every(copy => sameStructure(copies[0].element, copy.element))) {
      setLinkChoice({ copies: [], error: "Different structure, can't link", screen });
      return;
    }
    if (!linkNeedsChoice(copies.map(copy => copy.element))) { linkCopies(copies.map(copy => copy.id), copies[0].id); return; }
    setLinkChoice({ copies: choices, error: null, screen });
  }

  function linkCopies(ids: string[], sourceId: string | null) {
    const { setLinkChoice, linkElements } = context();
    setLinkChoice(null);
    void Promise.resolve(linkElements(ids, sourceId)).catch(() => {});
  }

  function unlinkSelection() {
    const { selection, unlinkElements } = context();
    const ids = [...new Set(selection.filter(isElementTarget).map(target => target.elementId))];
    if (!ids.length) return;
    void Promise.resolve(unlinkElements(ids)).catch(() => {});
  }

  function groupSelection() {
    const { selection, groupElements, selectTargets } = context();
    const targets = selection.filter(isElementTarget);
    if (targets.length < 2) return;
    const pageId = targets[0].pageId;
    if (!targets.every(target => target.pageId === pageId)) return;
    void groupElements(pageId, targets.map(target => target.elementId)).then(id => { if (id) selectTargets([{ pageId, elementId: id }]); }).catch(() => {});
  }

  function ungroupSelection() {
    const { selection, ungroupElements, selectTargets } = context();
    const targets = selection.filter(isElementTarget);
    if (targets.length !== 1) return;
    const { pageId, elementId } = targets[0];
    void ungroupElements(pageId, elementId).then(ids => { if (ids.length) selectTargets(ids.map(id => ({ pageId, elementId: id }))); }).catch(() => {});
  }

  function nudgeElements(dx: number, dy: number) {
    const { elementEntries, selection, rememberRepeatMove, moveElements } = context();
    const entries = elementEntries(selection);
    if (!entries.length) return;
    rememberRepeatMove(entries, { x: dx, y: dy });
    void Promise.resolve(moveElements(entries.map(entry => ({
      id: entry.id, to_page_id: entry.pageId, to_parent_id: entry.parentId, index: entry.index,
      rect: { ...entry.rect, x: entry.rect.x + dx, y: entry.rect.y + dy },
      size_id: entry.allSizes ? null : entry.sizeId, all_sizes: entry.allSizes,
    })))).catch(() => {});
  }

  function focusElement(direction: 'child' | 'parent') {
    const { selection, project, selectTargets, selectBoundary } = context();
    const target = selection.find(isElementTarget);
    if (!target) return;
    if (direction === 'child') {
      const children = elementChildrenOf(project, target.pageId, target.elementId);
      if (children.length) selectTargets([{ pageId: target.pageId, elementId: children[0].id }]);
      return;
    }
    const parents = elementParents(project, target.pageId, target.elementId);
    if (parents.length) selectTargets([{ pageId: target.pageId, elementId: parents.at(-1)! }]);
    else selectBoundary(target.pageId, target.sizeId);
  }

  function canRunCanvasAction(action: ShortcutAction): boolean | null {
    const { selection, project, focused, clipboard, pasteTarget, copiedStyle } = context();
    const elements = selection.filter(isElementTarget);
    const views = project.views ?? [];
    switch (action) {
      case 'requestTool': case 'selectAll': case 'minimap': case 'saveView': return true;
      case 'goToView': return views.length > 0;
      case 'delete': return selection.length > 0 || !!focused;
      case 'requestFromShape': return elements.length === 1;
      case 'link': return elements.length >= 2;
      case 'group': return elements.length >= 2;
      case 'ungroup': {
        const only = elements.length === 1 ? findPageElement(project, elements[0].pageId, elements[0].elementId) : null;
        return only?.kind === 'group';
      }
      case 'selectChild': {
        const first = elements[0];
        return !!first && elementChildrenOf(project, first.pageId, first.elementId).length > 0;
      }
      case 'paste': case 'pasteChoice': return clipboard.current.length > 0 && pasteTarget() !== null;
      case 'pasteStyle': return elements.length > 0 && copiedStyle.current !== null;
      case 'paintFormat': return elements.length > 0 || copiedStyle.current !== null;
      case 'copy': case 'cut': case 'duplicate': case 'copyStyle': case 'unlink':
      case 'forward': case 'backward': case 'front': case 'back':
      case 'selectParent': case 'nudge': case 'nudgeFar':
        return elements.length > 0;
      default: return null;
    }
  }

  function runCanvasAction(action: ShortcutAction, options: CanvasActionOptions = {}): boolean {
    const { selection, focused, deleteBoundary, focusedSize, removeSelection, startRequestOnSelection, onTool, startRequestFromShape, copyElements, clipboard, pasteElements, duplicateElements, copiedStyle, setMinimapPreference, minimapVisible, saveView, currentCamera, project, goToView } = context();
    const elements = selection.filter(isElementTarget);
    switch (action) {
      case 'delete': {
        if (!selection.length) { if (focused) deleteBoundary(focused, focusedSize); return true; }
        const elementIds = elements.map(target => ({ pageId: target.pageId, elementId: target.elementId }));
        const requests = selection.flatMap(target => target.kind === 'request' ? [target.requestId] : []);
        const items = selection.flatMap(target => target.kind === 'reference' ? [target.itemId] : []);
        removeSelection(items, selectedPointers(selection), requests, elementIds);
        return true;
      }
      case 'requestTool':
        // With something selected F starts a request on it; otherwise it picks the Request tool.
        if (!startRequestOnSelection()) onTool('request');
        return true;
      case 'requestFromShape':
        if (elements.length !== 1) return false;
        startRequestFromShape();
        return true;
      case 'link':
        if (!elements.length) return false;
        linkSelection();
        return true;
      case 'unlink':
        if (!elements.length) return false;
        unlinkSelection();
        return true;
      case 'selectAll': selectAllSiblings(); return true;
      case 'copy':
        if (!elements.length) return false;
        copyElements();
        return true;
      case 'cut':
        if (!elements.length) return false;
        copyElements();
        removeSelection([], [], [], elements.map(target => ({ pageId: target.pageId, elementId: target.elementId })));
        return true;
      // Plain paste belongs to the paste event, which can also carry content from other apps.
      case 'paste':
        if (!clipboard.current.length) return false;
        pasteElements(false);
        return true;
      case 'pasteChoice':
        if (!clipboard.current.length) return false;
        pasteElements(true);
        return true;
      case 'duplicate':
        if (!elements.length) return false;
        duplicateElements();
        return true;
      case 'copyStyle':
        if (!elements.length) return false;
        copyStyle();
        return true;
      case 'pasteStyle':
        if (!elements.length || !copiedStyle.current) return false;
        pasteStyle();
        return true;
      case 'paintFormat':
        if (!elements.length && !copiedStyle.current) return false;
        paintFormat();
        return true;
      case 'group':
        if (!elements.length) return false;
        groupSelection();
        return true;
      case 'ungroup':
        if (!elements.length) return false;
        ungroupSelection();
        return true;
      case 'forward': case 'front':
        if (!elements.length) return false;
        arrangeElements(action);
        return true;
      case 'backward': case 'back':
        if (!elements.length) return false;
        arrangeElements(action);
        return true;
      case 'selectChild':
        if (!elements.length) return false;
        focusElement('child');
        return true;
      case 'selectParent':
        if (!elements.length) return false;
        focusElement('parent');
        return true;
      case 'nudge': case 'nudgeFar': {
        if (!elements.length) return false;
        const key = options.key ?? 'ArrowRight';
        const step = action === 'nudgeFar' ? 10 : 1;
        nudgeElements(key === 'ArrowLeft' ? -step : key === 'ArrowRight' ? step : 0, key === 'ArrowUp' ? -step : key === 'ArrowDown' ? step : 0);
        return true;
      }
      case 'minimap': setMinimapPreference(minimapVisible ? 'off' : 'on'); return true;
      // A held key must not keep overwriting the same view.
      case 'saveView':
        if (options.repeat) return false;
        saveView(options.slot ?? null, currentCamera());
        return true;
      case 'goToView': {
        const views = project.views ?? [];
        const view = options.slot ? views.find(item => item.name === viewNameForSlot(options.slot!)) ?? null : views[0] ?? null;
        if (!view) return false;
        goToView(view);
        return true;
      }
      default: return false;
    }
  }

  function keyDown(event: ReactKeyboardEvent<HTMLDivElement>) {
    const { polygonDraft, closeRequestArea, focused, selection, openPage } = context();
    if (event.defaultPrevented || isTextField(event.target)) return;
    if (event.key === 'Enter' && polygonDraft && polygonDraft.length >= 3) { event.preventDefault(); closeRequestArea(polygonDraft, 'polygon'); return; }
    if (event.key === 'Enter' && focused && !selection.length) { event.preventDefault(); openPage(focused); }
    else if (event.key === 'Escape') { event.preventDefault(); escapeCanvas(); }
  }

  return {
    escapeCanvas, selectAllSiblings, arrangeElements, copyStyle, pasteStyle, paintFormat, applyPainterStyle,
    alignSelection, setSelectionFill, deleteSelectedLayers, linkSelection, linkCopies, unlinkSelection,
    groupSelection, ungroupSelection, nudgeElements, focusElement, canRunCanvasAction, runCanvasAction,
    keyDown,
  };
}
