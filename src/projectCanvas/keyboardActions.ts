import { sizeTarget } from './canvasTargets';
import { isTextField } from '../textField';
import { pageKind } from './boundaryDefaults';
import type { ProjectCanvasProps } from './interactionTypes';
import { alignSelectionMoves, distributeSelectionMoves, arrangeSiblingOrder, type SelectionAlign, type SelectionDistribution, type ArrangeOperation } from './arrange';
import { type KeyboardEvent as ReactKeyboardEvent } from 'react';
import { BOUNDARY_GAP } from './boundaryLayout';
import { selectedPointers, type CanvasTarget, type ElementTarget } from './canvasTargets';
import { type CanvasCamera, type Point } from './camera';
import type { Element, ElementMove, ElementPatch, NamedView, Page, Project, Rect, RequestArea, RequestShape, RequestTarget } from '../types';
import type { ProjectTool } from '../EditorTools';
import { type ScreenRect } from './surface';
import { type LinkCopy } from './linkedChoices';
import { copySummary, linkNeedsChoice, nestedLayerIds, sameStructure } from './links';
import { actionBinding, viewNameForSlot, type ShortcutAction } from '../shortcuts';
import { playIsActive } from './play';
import { elementChildrenOf, elementParents, findPageElement } from './elementGeometry';
import { ElementEntry, ClipboardEntry, PasteTarget, LinkDecision, Drag, CanvasActionOptions, isElementTarget } from './interactionTypes';

const OPACITIES: Partial<Record<ShortcutAction, number>> = { opacity10: .1, opacity20: .2, opacity30: .3, opacity40: .4, opacity50: .5, opacity60: .6, opacity70: .7, opacity80: .8, opacity90: .9, opacity100: 1 };

export type Painter =
  | { kind: 'empty' }
  | { kind: 'ready'; style: import('../types').ElementStyle }
  | { kind: 'armed'; style: import('../types').ElementStyle };
export type PainterEvent = { kind: 'copy'; style: import('../types').ElementStyle } | { kind: 'arm' | 'disarm' };

type Context = {
  editProject: ProjectCanvasProps['editProject'];
  createBoundary: ProjectCanvasProps['createBoundary'];
  drag: { current: Drag | null; };
  cancelDrag: () => void;
  painterArmed: boolean;
  transitionPainter: (event: PainterEvent) => void;
  latest: import("react").RefObject<Pick<ProjectCanvasProps, 'play'>>;
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
  elementEntries: (targets: CanvasTarget[], project?: Project) => ElementEntry[];
  moveElements: ProjectCanvasProps['edits']['moveElements'];
  painter: import("react").RefObject<Painter>;
  updateElements: ProjectCanvasProps['edits']['updateElements'];
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
    const { drag, cancelDrag, painterArmed, transitionPainter, latest, onPlayExit, requestDraft, polygonDraft, draftArea, cancelRequestDraft, editing, finishEditingText, selection, selectBoundary, focused } = context();
    if (drag.current) { cancelDrag(); return; }
    if (painterArmed) { transitionPainter({ kind: 'disarm' }); return; }
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
    const { elementEntries, selection, editProject } = context();
    void editProject(project => {
      const entries = elementEntries(selection, project);
      if (!entries.length) return null;
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
        if (order.every((id, index) => id === siblings[index].id)) continue;
        for (const [index, id] of order.entries()) {
          const entry = group.find(entry => entry.id === id);
          if (!entry) continue;
          moves.push({ id, to_page_id: entry.pageId, to_parent_id: entry.parentId, index, rect: entry.rect,
            ...sizeTarget(entry) });
        }
      }
      return moves.length ? { type: 'move_elements', moves } : null;
    });
  }

  function copyStyle() {
    const { elementEntries, selection } = context();
    const entry = elementEntries(selection)[0];
    if (entry) context().transitionPainter({ kind: 'copy', style: structuredClone(entry.element.style) });
  }

  function pasteStyle() {
    const { painter, elementEntries, selection, updateElements } = context();
    const style = painter.current.kind === 'empty' ? null : painter.current.style;
    const entries = elementEntries(selection);
    if (!style || !entries.length) return;
    const byPage = new Map<string, ElementPatch[]>();
    for (const entry of entries) {
      const patch: ElementPatch = { id: entry.id, style: structuredClone(style), ...sizeTarget(entry) };
      byPage.set(entry.pageId, [...(byPage.get(entry.pageId) ?? []), patch]);
    }
    for (const [pageId, patches] of byPage) void updateElements(pageId, patches);
  }

  function paintFormat() {
    const { elementEntries, selection, painter, transitionPainter, surface } = context();
    if (elementEntries(selection).length) copyStyle();
    if (painter.current.kind === 'empty') return;
    transitionPainter({ kind: 'arm' });
    surface.current?.focus({ preventScroll: true });
  }

  function applyPainterStyle(target: ElementTarget) {
    const { painter, updateElements, transitionPainter } = context();
    const style = painter.current.kind === 'empty' ? null : painter.current.style;
    if (!style) return;
    const patch: ElementPatch = { id: target.elementId, style: structuredClone(style), ...sizeTarget(target) };
    void updateElements(target.pageId, [patch]);
    transitionPainter({ kind: 'disarm' });
  }

  function alignSelection(edge: SelectionAlign) {
    const { elementEntries, selection, boundaryRect, editProject } = context();
    void editProject(project => {
      const entries = elementEntries(selection, project);
      const moves = alignSelectionMoves(entries, edge, entry => {
        const parent = entry.parentId ? elementEntries([{ kind: 'element', pageId: entry.pageId,
          elementId: entry.parentId, sizeId: entry.allSizes ? null : entry.sizeId }], project)[0] : null;
        return parent?.world ?? boundaryRect(entry.sizeId) ?? entry.world;
      });
      return moves.length ? { type: 'move_elements', moves } : null;
    });
  }

  function distributeSelection(direction: SelectionDistribution) {
    const { elementEntries, selection, editProject } = context();
    void editProject(project => {
      const moves = distributeSelectionMoves(elementEntries(selection, project), direction);
      return moves.length ? { type: 'move_elements', moves } : null;
    });
  }

  function selectionStyleAction(action: ShortcutAction) {
    const { selection, editing, editProject } = context();
    const targets = editing ? [{ kind: 'element' as const, ...editing, sizeId: editing.sizeId }] : selection.filter(isElementTarget);
    for (const pageId of new Set(targets.map(target => target.pageId))) {
      void editProject(project => {
        const patches: ElementPatch[] = [];
        for (const target of targets.filter(target => target.pageId === pageId)) {
          const element = findPageElement(project, pageId, target.elementId, target.sizeId);
          if (!element) continue;
          const scope = sizeTarget(target);
          if (action === 'toggleHidden') patches.push({ id: element.id, visible: !element.visible, ...scope });
          else if (action === 'toggleLocked') patches.push({ id: element.id, locked: !element.locked, ...scope });
          else {
            const style = structuredClone(element.style);
            if (action === 'flipHorizontal') style.flipX = !style.flipX;
            else if (action === 'flipVertical') style.flipY = !style.flipY;
            else if (OPACITIES[action] !== undefined) style.opacity = OPACITIES[action]!;
            else {
              if (element.kind !== 'text') continue;
              if (action === 'bold') style.textStyle.weight = style.textStyle.weight >= 700 ? 400 : 700;
              else if (action === 'italic') style.textStyle.italic = !style.textStyle.italic;
              else style.textStyle.size = Math.min(512, Math.max(1, style.textStyle.size + (action === 'fontSizeUp' ? 1 : -1)));
              style.references = { ...style.references, text: null };
            }
            patches.push({ id: element.id, style, ...scope });
          }
        }
        return patches.length ? { type: 'update_elements', page_id: pageId, patches } : null;
      });
    }
  }

  function stackSelection() {
    const { selection, editProject, selectTargets } = context();
    const targets = selection.filter(isElementTarget);
    if (!targets.length || targets.some(target => target.pageId !== targets[0].pageId)) return;
    const pageId = targets[0].pageId;
    let before = new Set<string>();
    void editProject(project => {
      before = new Set(elementChildrenOf(project, pageId, elementParents(project, pageId, targets[0].elementId).at(-1) ?? null).map(element => element.id));
      return { type: 'stack_elements', page_id: pageId, ids: targets.map(target => target.elementId) };
    }).then(workspace => {
      if (!workspace?.project) return;
      const parentId = elementParents(workspace.project!, pageId, targets[0].elementId).at(-1);
      const parent = parentId ? findPageElement(workspace.project!, pageId, parentId) : null;
      if (parent && !before.has(parent.id)) selectTargets([{ pageId, elementId: parent.id, sizeId: targets[0].sizeId }]);
    });
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
      void updateElements(pageId, items.map(item => ({ id: item.id, style: { ...item.style, fill: value } })));
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
    void linkElements(ids, sourceId);
  }

  function unlinkSelection() {
    const { selection, unlinkElements } = context();
    const ids = [...new Set(selection.filter(isElementTarget).map(target => target.elementId))];
    if (!ids.length) return;
    void unlinkElements(ids);
  }

  function groupSelection() {
    const { selection, groupElements, selectTargets } = context();
    const targets = selection.filter(isElementTarget);
    if (targets.length < 2) return;
    const pageId = targets[0].pageId;
    if (!targets.every(target => target.pageId === pageId)) return;
    void groupElements(pageId, targets.map(target => target.elementId)).then(id => { if (id) selectTargets([{ pageId, elementId: id }]); });
  }

  function ungroupSelection() {
    const { selection, ungroupElements, selectTargets } = context();
    const targets = selection.filter(isElementTarget);
    if (targets.length !== 1) return;
    const { pageId, elementId } = targets[0];
    void ungroupElements(pageId, elementId).then(ids => { if (ids.length) selectTargets(ids.map(id => ({ pageId, elementId: id }))); });
  }

  function nudgeElements(dx: number, dy: number) {
    const { elementEntries, selection, rememberRepeatMove, editProject } = context();
    const entries = elementEntries(selection);
    if (!entries.length) return;
    rememberRepeatMove(entries, { x: dx, y: dy });
    void editProject({ type: 'nudge_elements', dx, dy, moves: entries.map(entry => ({
      id: entry.id, to_page_id: entry.pageId, to_parent_id: entry.parentId, index: entry.index, rect: entry.rect,
      ...sizeTarget(entry),
    })) });
  }

  function focusElement(direction: 'child' | 'parent') {
    const { selection, project, selectTargets, selectBoundary } = context();
    const target = selection.find(isElementTarget);
    if (!target) return;
    if (direction === 'child') {
      const children = elementChildrenOf(project, target.pageId, target.elementId);
      if (children.length) selectTargets([{ pageId: target.pageId, elementId: children[children.length - 1].id, sizeId: target.sizeId }]);
      return;
    }
    const parents = elementParents(project, target.pageId, target.elementId);
    if (parents.length) selectTargets([{ pageId: target.pageId, elementId: parents.at(-1)!, sizeId: target.sizeId }]);
    else selectBoundary(target.pageId, target.sizeId);
  }

  type CanvasAction = { enabled: () => boolean; run: (options: CanvasActionOptions) => void };
  const elements = () => context().selection.filter(isElementTarget);
  const hasElements = () => elements().length > 0;
  const samePage = () => hasElements() && elements().every(target => target.pageId === elements()[0].pageId);
  const actions: Partial<Record<ShortcutAction, CanvasAction>> = {};
  const add = (ids: ShortcutAction[], enabled: () => boolean, run: (action: ShortcutAction, options: CanvasActionOptions) => void) => {
    for (const id of ids) actions[id] = { enabled, run: options => run(id, options) };
  };
  const alignments: Partial<Record<ShortcutAction, SelectionAlign>> = {
    alignLeft: 'left', alignCentre: 'centre', alignRight: 'right', alignTop: 'top', alignMiddle: 'middle', alignBottom: 'bottom',
  };
  add(Object.keys(alignments) as ShortcutAction[], hasElements, action => alignSelection(alignments[action]!));
  add(['distributeHorizontal', 'distributeVertical'], () => elements().length >= 3,
    action => distributeSelection(action === 'distributeHorizontal' ? 'horizontal' : 'vertical'));
  add(['flipHorizontal', 'flipVertical', 'toggleHidden', 'toggleLocked', ...Object.keys(OPACITIES) as ShortcutAction[]], hasElements, selectionStyleAction);
  add(['bold', 'italic'], () => !!context().editing, selectionStyleAction);
  add(['fontSizeUp', 'fontSizeDown'], () => !context().editing && elements().some(target => findPageElement(context().project, target.pageId, target.elementId)?.kind === 'text'), selectionStyleAction);
  add(['stack'], samePage, stackSelection);
  add(['delete'], () => context().selection.length > 0 || !!context().focused, () => {
    const { selection, focused, focusedSize, deleteBoundary } = context();
    if (selection.length) deleteSelectedLayers(); else if (focused) deleteBoundary(focused, focusedSize);
  });
  add(['requestTool'], () => true, () => { if (!context().startRequestOnSelection()) context().onTool('request'); });
  add(['requestFromShape'], () => elements().length === 1, () => { context().startRequestFromShape(); });
  add(['link'], hasElements, linkSelection);
  add(['unlink'], hasElements, unlinkSelection);
  add(['selectAll'], () => true, selectAllSiblings);
  add(['copy'], hasElements, () => context().copyElements());
  add(['cut'], hasElements, () => { context().copyElements(); deleteSelectedLayers(); });
  add(['paste', 'pasteChoice'], () => context().clipboard.current.length > 0 && context().pasteTarget() !== null,
    action => context().pasteElements(action === 'pasteChoice'));
  add(['duplicate'], () => hasElements() || (!!context().focused && !context().selection.length), () => {
    if (hasElements()) { context().duplicateElements(); return; }
    const { project, focused, createBoundary, selectBoundary } = context();
    const page = project.pages.find(page => page.id === focused);
    if (!page) return;
    const base = page.sizes[0];
    const bounds = { x: Math.max(...page.sizes.map(size => size.x + size.width)) + BOUNDARY_GAP, y: base.y, width: base.width, height: base.height };
    void createBoundary(bounds, pageKind(page), page.id).then(id => { if (id) selectBoundary(id); });
  });
  add(['copyStyle'], hasElements, copyStyle);
  add(['pasteStyle'], () => hasElements() && context().painter.current.kind !== 'empty', pasteStyle);
  add(['paintFormat'], () => hasElements() || context().painter.current.kind !== 'empty', paintFormat);
  add(['group'], () => samePage() && elements().length >= 2, groupSelection);
  add(['ungroup'], () => {
    const targets = elements();
    return targets.length === 1 && findPageElement(context().project, targets[0].pageId, targets[0].elementId)?.kind === 'group';
  }, ungroupSelection);
  add(['forward', 'front', 'backward', 'back'], hasElements, action => arrangeElements(action as ArrangeOperation));
  add(['selectChild'], () => {
    const target = elements()[0];
    return !!target && elementChildrenOf(context().project, target.pageId, target.elementId).length > 0;
  }, () => focusElement('child'));
  add(['selectParent'], hasElements, () => focusElement('parent'));
  add(['nudge', 'nudgeFar'], hasElements, (action, options) => {
    const key = options.key ?? 'ArrowRight', step = action === 'nudgeFar' ? 10 : 1;
    nudgeElements(key === 'ArrowLeft' ? -step : key === 'ArrowRight' ? step : 0, key === 'ArrowUp' ? -step : key === 'ArrowDown' ? step : 0);
  });
  add(['minimap'], () => true, () => context().setMinimapPreference(context().minimapVisible ? 'off' : 'on'));
  add(['saveView'], () => true, (_, options) => context().saveView(options.slot ?? null, context().currentCamera()));
  add(['goToView'], () => (context().project.views ?? []).length > 0, (_, options) => {
    const views = context().project.views ?? [];
    const view = options.slot ? views.find(item => item.name === viewNameForSlot(options.slot!)) : views[0];
    if (view) context().goToView(view);
  });

  function canvasAction(action: ShortcutAction) {
    const handler = actions[action];
    if (!handler) return undefined;
    return {
      ...actionBinding(action),
      enabled: handler.enabled,
      run(options: CanvasActionOptions = {}) {
        if (!handler.enabled() || options.repeat && actionBinding(action).repeat === false) return false;
        handler.run(options);
        return true;
      },
    };
  }

  function keyDown(event: ReactKeyboardEvent<HTMLDivElement>) {
    const { polygonDraft, closeRequestArea, focused, selection, openPage } = context();
    if (event.defaultPrevented || isTextField(event.target)) return;
    if (event.key === 'Enter' && polygonDraft && polygonDraft.length >= 3) { event.preventDefault(); closeRequestArea(polygonDraft, 'polygon'); return; }
    if (event.key === 'Enter' && focused && !selection.length) { event.preventDefault(); openPage(focused); }
  }

  return {
    escapeCanvas, copyStyle, pasteStyle, paintFormat, applyPainterStyle,
    alignSelection, distributeSelection, setSelectionFill, deleteSelectedLayers, linkSelection, linkCopies, unlinkSelection,
    canvasAction,
    keyDown,
  };
}
