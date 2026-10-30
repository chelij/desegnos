import { Fragment, memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react';
import { Icon, type IconName } from '../icons';
import type { Element, ElementKind, ElementPatch, Project } from '../types';
import { elementLabel, holdsChildren } from './elementGeometry';
import { nestedLayerIds } from './links';

const KIND_ICONS: Record<ElementKind, IconName> = {
  box: 'rectangle', group: 'group', text: 'text', ellipse: 'ellipse', line: 'line', image: 'image',
  button: 'properties', link: 'link', input: 'properties', textarea: 'note', select: 'chevron-down',
  checkbox: 'check', radio: 'ellipse', video: 'video', audio: 'audio', embed: 'external', code: 'code',
};

/** A grabbed layer row and its place in the model. */
export type LayerDrag = { pageId: string; sizeId: string; id: string; parentId: string | null; index: number };
export type ElementDropTarget = { pageId: string; sizeId: string; parentId: string | null; index: number };
type DropHint = ElementDropTarget | null;
/** Pixels the pointer must travel before a press on a layer row becomes a drag. */
const DRAG_THRESHOLD = 4;

/** The rows a layer search keeps: matches, their parents, and everything under a match. */
export function matchingLayerIds(elements: Element[], query: string): Set<string> {
  const keep = new Set<string>();
  const needle = query.trim().toLowerCase();
  if (!needle) return keep;
  const walk = (element: Element, inherited: boolean): boolean => {
    const matched = inherited || `${elementLabel(element)} ${element.name ?? ''}`.toLowerCase().includes(needle);
    let below = false;
    for (const child of element.children) if (walk(child, matched)) below = true;
    if (matched || below) keep.add(element.id);
    return matched || below;
  };
  elements.forEach(element => walk(element, false));
  return keep;
}

function sameTarget(first: DropHint, second: DropHint): boolean {
  return first === second || (!!first && !!second && first.pageId === second.pageId && first.sizeId === second.sizeId
    && first.parentId === second.parentId && first.index === second.index);
}

/** The layer a pointer sits over: before/after keep the parent, into makes it a child. */
export function layerDropPosition(element: Element, offset: number): 'before' | 'after' | 'into' {
  if (holdsChildren(element.kind) && offset > 0.3 && offset < 0.7) return 'into';
  return offset < 0.5 ? 'before' : 'after';
}

/** The drop target a row offers when the pointer sits at `offset` (0–1) of its height. */
export function layerDropTarget(element: Element, offset: number, pageId: string, sizeId: string, parentId: string | null, index: number): ElementDropTarget {
  const position = layerDropPosition(element, offset);
  return position === 'into'
    ? { pageId, sizeId, parentId: element.id, index: element.children.length }
    : { pageId, sizeId, parentId, index: position === 'before' ? index + 1 : index };
}

/** Where a grabbed layer lands, or null when that row is not a valid new home for it. */
export function resolveLayerDrop(source: LayerDrag, target: ElementDropTarget): { parentId: string | null; index: number } | null {
  if (source.pageId !== target.pageId || source.sizeId !== target.sizeId || source.id === target.parentId) return null;
  // Siblings count from the model index, and the grabbed layer still sits in the list.
  const index = source.parentId === target.parentId && source.index < target.index ? target.index - 1 : target.index;
  return { parentId: target.parentId, index };
}

/** One 28px layer row: kind icon, name (double-click renames), then hover lock and eye toggles. */
const LayerRow = memo(function LayerRow({ linkedName, pageId, sizeId, element, parentId, index, selected, open, toggle, select, update, remove, movable, grab, depth, dropLine }: {
  linkedName: string | null; pageId: string; sizeId: string; element: Element; parentId: string | null; index: number;
  selected: boolean; open: boolean; toggle: (id: string) => void;
  select: (pageId: string, id: string, sizeId: string, additive: boolean) => void;
  update: (pageId: string, sizeId: string, patches: ElementPatch[]) => void;
  remove: (pageId: string, sizeId: string, id: string) => void;
  movable: boolean; grab: (event: ReactPointerEvent<HTMLDivElement>, source: LayerDrag) => void;
  depth: number; dropLine: 'before' | 'after' | 'into' | null;
}) {
  const canContain = holdsChildren(element.kind);
  const hasChildren = canContain && element.children.length > 0;
  const label = elementLabel(element);
  // A linked copy keeps its own name; the group name appears in the row title and Properties.
  const renameable = movable;
  const [draft, setDraft] = useState<string | null>(null);
  // Enter commits and sets the draft back to null; the blur that follows must not rename twice.
  const editing = useRef(false);
  function commitName() {
    if (!editing.current) return;
    editing.current = false;
    const name = (draft ?? '').trim();
    setDraft(null);
    if (name !== (element.name ?? '').trim()) update(pageId, sizeId, [{ id: element.id, name: name || null }]);
  }
  return <li style={{ paddingInlineStart: depth * 12, height: LAYER_ROW_HEIGHT }} className={`dock-tree-group element-row virtual-layer-row${element.visible ? '' : ' hidden-in-size'}`} data-drop={dropLine ?? undefined}>
    <div className="dock-tree-row element-row-line" data-draggable={movable && draft === null ? 'true' : undefined}
      data-element-row={element.id}
      data-selected={selected || undefined}
      title={renameable ? `${label}${linkedName ? ` · linked as ${linkedName}` : ''} · double-click to rename`
        : linkedName ? `${label} · linked as ${linkedName}` : label}
      onPointerDown={event => {
        // A press still selects, renames and toggles; only travel past the threshold drags.
        if (!movable || draft !== null || event.button !== 0 || !event.isPrimary) return;
        grab(event, { pageId, sizeId, id: element.id, parentId, index });
      }}>
      {hasChildren
        ? <button type="button" className="dock-tree-disclosure" aria-label={`${open ? 'Collapse' : 'Expand'} ${elementLabel(element)}`} aria-expanded={open} onClick={() => toggle(`${sizeId}:${element.id}`)}>
          <Icon name="chevron-right" size={12} className={open ? 'expanded' : undefined} />
        </button>
        : <span className="dock-tree-spacer" aria-hidden="true" />}
      {draft !== null
        ? <input className="dock-inline-name" aria-label={`Rename ${label}`} maxLength={120} autoFocus value={draft}
          onChange={event => setDraft(event.target.value)} onBlur={commitName}
          onKeyDown={event => {
            if (event.key === 'Enter') { event.preventDefault(); commitName(); }
            else if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); editing.current = false; setDraft(null); }
          }} />
        : <button type="button" className="dock-tree-select element" aria-pressed={selected}
          onClick={event => select(pageId, element.id, sizeId, event.shiftKey || event.ctrlKey || event.metaKey)}
          onDoubleClick={renameable ? () => { editing.current = true; setDraft(element.name ?? ''); } : undefined}>
          <Icon name={linkedName !== null ? 'linked-copies' : KIND_ICONS[element.kind]} size={12} /><span className={element.name?.trim() ? undefined : 'muted'}>{label}</span>
        </button>}
      <button type="button" className="icon-button sm dock-row-toggle" aria-label={`${element.locked ? 'Unlock' : 'Lock'} ${elementLabel(element)}`} title={element.locked ? 'Unlock' : 'Lock'}
        aria-pressed={element.locked} onClick={() => update(pageId, sizeId, [{ id: element.id, locked: !element.locked }])}><Icon name={element.locked ? 'lock' : 'unlock'} size={13} /></button>
      <button type="button" className="icon-button sm dock-row-toggle" aria-label={`${element.visible ? 'Hide' : 'Show'} ${elementLabel(element)}`} title={element.visible ? 'Hide' : 'Show'}
        aria-pressed={!element.visible} onClick={() => update(pageId, sizeId, [{ id: element.id, visible: !element.visible }])}><Icon name={element.visible ? 'eye' : 'eye-off'} size={13} /></button>
      <button type="button" className="icon-button sm dock-row-toggle" aria-label={`Delete ${elementLabel(element)}`} title="Delete layer" onClick={() => remove(pageId, sizeId, element.id)}><Icon name="trash" size={13} /></button>
    </div>
  </li>;
}, (before, after) => {
  // Resolved trees are cloned after edits. Rows only depend on these presentation fields.
  const { element: a, ...aProps } = before, { element: b, ...bProps } = after;
  return a.id === b.id && a.kind === b.kind && a.name === b.name && a.text === b.text && a.language === b.language
    && a.visible === b.visible && a.locked === b.locked && a.children.length === b.children.length
    && Object.keys(aProps).every(key => Object.is(aProps[key as keyof typeof aProps], bProps[key as keyof typeof bProps]));
});

export const LAYER_ROW_HEIGHT = 28;
const OVERSCAN = 4;
export type FlatLayer = { element: Element; parentId: string | null; index: number; depth: number; open: boolean };

/** Display order is reversed within each sibling list, while drop indices retain model order. */
export function flattenLayers(elements: Element[], sizeId: string, collapsed: ReadonlySet<string>, keep: ReadonlySet<string> | null): FlatLayer[] {
  const rows: FlatLayer[] = [];
  const visit = (children: Element[], parentId: string | null, depth: number) => {
    for (let index = children.length - 1; index >= 0; index--) {
      const element = children[index];
      if (keep && !keep.has(element.id)) continue;
      const open = keep !== null || !collapsed.has(`${sizeId}:${element.id}`);
      rows.push({ element, parentId, index, depth, open });
      if (open && holdsChildren(element.kind)) visit(element.children, element.id, depth + 1);
    }
  };
  visit(elements, null, 0);
  return rows;
}

export function ElementLayers({ project, pageId, sizeId, elements, selection, select, update, remove, move, keep = null, fixed: fixedIds }: {
  project: Project; pageId: string; sizeId: string; elements: Element[]; selection: string[];
  select: (pageId: string, id: string, sizeId: string, additive: boolean) => void;
  update: (pageId: string, sizeId: string, patches: ElementPatch[]) => void;
  remove: (pageId: string, sizeId: string, id: string) => void;
  move: (pageId: string, sizeId: string, id: string, parentId: string | null, index: number) => void;
  fixed?: ReadonlySet<string>;
  /** Rows a Layers search keeps; null lists every layer. */
  keep?: ReadonlySet<string> | null;
}) {
  const fixed = useMemo(() => fixedIds ?? nestedLayerIds(project), [fixedIds, project]);
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(() => new Set());
  const [draggedId, setDraggedId] = useState<string | null>(null);
  const [focusedId, setFocusedId] = useState<string | null>(null);
  const [hint, setHint] = useState<DropHint>(null);
  const updateHint = useCallback((next: DropHint) => setHint(current => sameTarget(current, next) ? current : next), []);
  // A gesture outlives renders, so its window listeners are torn down through this handle.
  const endGesture = useRef<(() => void) | null>(null);
  useEffect(() => () => endGesture.current?.(), []);
  const toggle = useCallback((id: string) => setCollapsed(current => {
    const next = new Set(current);
    if (next.has(id)) next.delete(id); else next.add(id);
    return next;
  }), []);
  // Stable event delegates keep callback churn in the editor out of memoised rows.
  const actions = useRef({ select, update, remove, grab: startRowDrag });
  useLayoutEffect(() => { actions.current = { select, update, remove, grab: startRowDrag }; });
  const selectRow = useCallback((...args: Parameters<typeof select>) => actions.current.select(...args), []);
  const updateRow = useCallback((...args: Parameters<typeof update>) => actions.current.update(...args), []);
  const removeRow = useCallback((...args: Parameters<typeof remove>) => actions.current.remove(...args), []);
  const grabRow = useCallback((...args: Parameters<typeof startRowDrag>) => actions.current.grab(...args), []);
  const rows = useMemo(() => flattenLayers(elements, sizeId, collapsed, keep), [elements, sizeId, collapsed, keep]);
  const selected = useMemo(() => new Set(selection), [selection]);
  const list = useRef<HTMLUListElement>(null);
  /** The layer row under the pointer, so a drop follows the cursor rather than the source row. */
  function targetUnder(clientX: number, clientY: number): ElementDropTarget | null {
    const listNode = list.current;
    if (!listNode) return null;
    const bounds = listNode.getBoundingClientRect();
    if (clientX < bounds.left || clientX > bounds.right || clientY < bounds.top || clientY > bounds.bottom) return null;
    const targetFor = (node: HTMLElement, offset: number): ElementDropTarget | null => {
      const row = rows.find(candidate => candidate.element.id === node.dataset.elementRow);
      return row ? layerDropTarget(row.element, offset, pageId, sizeId, row.parentId, row.index) : null;
    };
    const hit = document.elementFromPoint(clientX, clientY)?.closest<HTMLElement>('[data-element-row]');
    if (hit) {
      const box = hit.getBoundingClientRect();
      const target = targetFor(hit, (clientY - box.top) / Math.max(1, box.height));
      if (target) return target;
    }
    // Spacer rows carry no layer; the nearest mounted row decides before or after.
    const mounted = Array.from(listNode.querySelectorAll<HTMLElement>('[data-element-row]'))
      .map(node => ({ node, box: node.getBoundingClientRect() }))
      .sort((first, second) => first.box.top - second.box.top);
    const above = mounted.filter(entry => entry.box.bottom <= clientY).at(-1);
    if (above) {
      // Below the last layer the panel's empty tail still drops at the end of the top-level list.
      if (above.node.dataset.elementRow === rows.at(-1)?.element.id) return { pageId, sizeId, parentId: null, index: elements.length };
      return targetFor(above.node, 1);
    }
    const below = mounted.find(entry => entry.box.top >= clientY);
    return below ? targetFor(below.node, 0) : null;
  }
  /** Commit a finished gesture through the drop rules the panel has always used. */
  function commitGesture(source: LayerDrag, target: ElementDropTarget) {
    const drop = resolveLayerDrop(source, target);
    if (drop) move(target.pageId, target.sizeId, source.id, drop.parentId, drop.index);
  }
  /** Track a press on a row: capture the pointer past the threshold, then drop at pointerup. */
  function startRowDrag(event: ReactPointerEvent<HTMLDivElement>, source: LayerDrag) {
    // A press always wins: a gesture whose pointerup was lost outside the window cannot wedge the panel.
    endGesture.current?.();
    const node = event.currentTarget;
    const pointerId = event.pointerId;
    const originX = event.clientX;
    const originY = event.clientY;
    let dragging = false;
    const capture = (active: boolean) => {
      try {
        if (active) node.setPointerCapture(pointerId);
        else if (node.hasPointerCapture(pointerId)) node.releasePointerCapture(pointerId);
      } catch { /* WebKitGTK refuses capture for a pointer it already dropped. */ }
    };
    const finish = () => {
      window.removeEventListener('pointermove', handleMove);
      window.removeEventListener('pointerup', handleUp);
      window.removeEventListener('pointercancel', handleCancel);
      window.removeEventListener('keydown', handleKey, true);
      capture(false);
      endGesture.current = null;
      setDraggedId(null);
      setHint(null);
    };
    const handleMove = (moveEvent: PointerEvent) => {
      if (moveEvent.pointerId !== pointerId) return;
      if (!dragging) {
        if (Math.hypot(moveEvent.clientX - originX, moveEvent.clientY - originY) < DRAG_THRESHOLD) return;
        dragging = true;
        capture(true);
        setDraggedId(source.id);
        window.getSelection()?.removeAllRanges();
      }
      moveEvent.preventDefault();
      updateHint(targetUnder(moveEvent.clientX, moveEvent.clientY));
    };
    const handleUp = (upEvent: PointerEvent) => {
      if (upEvent.pointerId !== pointerId) return;
      const target = dragging ? targetUnder(upEvent.clientX, upEvent.clientY) : null;
      finish();
      if (target) commitGesture(source, target);
    };
    const handleCancel = (cancelEvent: PointerEvent) => { if (cancelEvent.pointerId === pointerId) finish(); };
    const handleKey = (keyEvent: KeyboardEvent) => {
      if (keyEvent.key !== 'Escape' || !dragging) return;
      keyEvent.preventDefault(); keyEvent.stopPropagation();
      finish();
    };
    endGesture.current = finish;
    window.addEventListener('pointermove', handleMove);
    window.addEventListener('pointerup', handleUp);
    window.addEventListener('pointercancel', handleCancel);
    window.addEventListener('keydown', handleKey, true);
  }
  const [viewport, setViewport] = useState({ top: 0, height: 280 });
  useLayoutEffect(() => {
    const node = list.current, scroll = node?.closest<HTMLElement>('.dock-panel-body');
    if (!node || !scroll) return;
    const measure = () => {
      const top = scroll.getBoundingClientRect().top - node.getBoundingClientRect().top;
      const height = scroll.clientHeight;
      setViewport(current => current.top === top && current.height === height ? current : { top, height });
    };
    measure();
    scroll.addEventListener('scroll', measure, { passive: true });
    const observer = new ResizeObserver(measure);
    observer.observe(scroll); observer.observe(scroll.firstElementChild ?? scroll);
    return () => { scroll.removeEventListener('scroll', measure); observer.disconnect(); };
  }, [rows.length]);
  const reveal = useCallback((index: number) => {
    const node = list.current, scroll = node?.closest<HTMLElement>('.dock-panel-body');
    if (!node || !scroll) return;
    const top = node.getBoundingClientRect().top - scroll.getBoundingClientRect().top + index * LAYER_ROW_HEIGHT;
    if (top < 0) scroll.scrollTop += top;
    else if (top + LAYER_ROW_HEIGHT > scroll.clientHeight) scroll.scrollTop += top + LAYER_ROW_HEIGHT - scroll.clientHeight;
    setViewport({ top: scroll.getBoundingClientRect().top - node.getBoundingClientRect().top, height: scroll.clientHeight });
  }, []);
  const selectedId = selection.at(-1);
  useLayoutEffect(() => {
    const index = rows.findIndex(row => row.element.id === selectedId);
    if (index >= 0) reveal(index);
  }, [selectedId, reveal]);
  const start = Math.min(rows.length, Math.max(0, Math.floor(viewport.top / LAYER_ROW_HEIGHT) - OVERSCAN));
  const end = Math.max(start, Math.min(rows.length, Math.ceil((viewport.top + viewport.height) / LAYER_ROW_HEIGHT) + OVERSCAN));
  // Keep the focused editor and the grabbed row mounted when scrolling past them.
  const visible = new Set(Array.from({ length: end - start }, (_, index) => start + index));
  rows.forEach((row, index) => { if (row.element.id === focusedId || row.element.id === draggedId) visible.add(index); });
  const indices = [...visible].sort((a, b) => a - b);
  let previous = -1;
  return <ul ref={list} className="dock-tree-elements" onFocusCapture={event => {
    setFocusedId((event.target as HTMLElement).closest<HTMLElement>('[data-element-row]')?.dataset.elementRow ?? null);
  }} onBlurCapture={event => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setFocusedId(null); }} onKeyDown={event => {
    if (event.target instanceof HTMLInputElement || event.altKey || event.ctrlKey || event.metaKey) return;
    const rowNode = (event.target as HTMLElement).closest<HTMLElement>('[data-element-row]');
    const index = rows.findIndex(row => row.element.id === rowNode?.dataset.elementRow);
    if (index < 0) return;
    const row = rows[index];
    if (event.key === 'Tab') {
      const buttons = rowNode?.querySelectorAll<HTMLElement>('button, input');
      const atEdge = buttons && event.target === buttons[event.shiftKey ? 0 : buttons.length - 1];
      const next = index + (event.shiftKey ? -1 : 1);
      if (!atEdge || next < 0 || next >= rows.length || (next >= start && next < end)) return;
      event.preventDefault(); event.stopPropagation();
      reveal(next);
      const id = rows[next].element.id;
      requestAnimationFrame(() => {
        const node = Array.from(list.current?.querySelectorAll<HTMLElement>('[data-element-row]') ?? []).find(node => node.dataset.elementRow === id);
        const targets = node?.querySelectorAll<HTMLElement>('button, input');
        targets?.[event.shiftKey ? targets.length - 1 : 0]?.focus({ preventScroll: true });
      });
      return;
    }
    let next = index;

    if (event.key === 'ArrowDown') next++;
    else if (event.key === 'ArrowUp') next--;
    else if (event.key === 'Home') next = 0;
    else if (event.key === 'End') next = rows.length - 1;
    else if (event.key === 'ArrowRight' && row.element.children.length && !row.open) toggle(`${sizeId}:${row.element.id}`);
    else if (event.key === 'ArrowLeft' && row.element.children.length && row.open) toggle(`${sizeId}:${row.element.id}`);
    else return;
    event.preventDefault(); event.stopPropagation();
    next = Math.max(0, Math.min(rows.length - 1, next));
    reveal(next);
    const id = rows[next].element.id;
    selectRow(pageId, id, sizeId, event.shiftKey);
    requestAnimationFrame(() => {
      const nodes = list.current?.querySelectorAll<HTMLElement>('[data-element-row]');
      const node = Array.from(nodes ?? []).find(node => node.dataset.elementRow === id);
      node?.querySelector<HTMLElement>('.dock-tree-select')?.focus({ preventScroll: true });
    });
  }} data-dragging={draggedId === null ? undefined : 'true'}>
    {indices.map(rowIndex => {
      const { element, parentId, index, depth, open } = rows[rowIndex];
      const gap = rowIndex - previous - 1;
      previous = rowIndex;
      const target = hint?.pageId === pageId && hint.sizeId === sizeId ? hint : null;
      const dropLine = target?.parentId === element.id ? 'into'
        : target && target.parentId === parentId && target.index === index + 1 ? 'before'
        : target && target.parentId === parentId && target.index === 0 && index === 0 ? 'after' : null;
      return <Fragment key={element.id}>
        {gap > 0 && <li aria-hidden="true" style={{ height: gap * LAYER_ROW_HEIGHT }} />}
        <LayerRow linkedName={element.link ? project.linked?.[element.link]?.name ?? null : null}
        pageId={pageId} sizeId={sizeId} element={element} parentId={parentId} index={index} depth={depth} open={open}
        selected={selected.has(element.id)} toggle={toggle} select={selectRow} update={updateRow} remove={removeRow}
        movable={!fixed.has(element.id)} dropLine={dropLine} grab={grabRow} />
      </Fragment>;
    })}
    <li aria-hidden="true" style={{ height: (rows.length - previous - 1) * LAYER_ROW_HEIGHT }} />
  </ul>;
}
