import { Fragment, useCallback, useMemo, useRef, useState, type DragEvent, type ReactNode } from 'react';
import { Icon, type IconName } from '../icons';
import type { Element, ElementKind, ElementPatch, Project } from '../types';
import { elementLabel, holdsChildren } from './elementGeometry';
import { nestedLayerIds } from './links';

const KIND_ICONS: Record<ElementKind, IconName> = {
  box: 'rectangle', group: 'group', text: 'text', ellipse: 'ellipse', line: 'line', image: 'image',
  button: 'properties', link: 'link', input: 'properties', textarea: 'note', select: 'chevron-down',
  checkbox: 'check', radio: 'ellipse', video: 'video', audio: 'audio', embed: 'external', code: 'code',
};

type DragState = { pageId: string; sizeId: string; id: string; parentId: string | null; index: number } | null;
export type ElementDropTarget = { pageId: string; sizeId: string; parentId: string | null; index: number };
type DropHint = ElementDropTarget | null;

/** The rows a layer search keeps: matches, their parents, and everything under a match. */
export function matchingLayerIds(elements: Element[], query: string): Set<string> {
  const keep = new Set<string>();
  const needle = query.trim().toLowerCase();
  if (!needle) return keep;
  const keepSubtree = (element: Element) => {
    keep.add(element.id);
    element.children.forEach(keepSubtree);
  };
  const walk = (element: Element, ancestors: string[]): boolean => {
    const matched = `${elementLabel(element)} ${element.name ?? ''}`.toLowerCase().includes(needle);
    if (matched) {
      ancestors.forEach(id => keep.add(id));
      keepSubtree(element);
    }
    let below = false;
    for (const child of element.children) if (walk(child, [...ancestors, element.id])) below = true;
    if (matched || below) keep.add(element.id);
    return matched || below;
  };
  elements.forEach(element => walk(element, []));
  return keep;
}

function sameTarget(first: DropHint, second: DropHint): boolean {
  return first === second || (!!first && !!second && first.pageId === second.pageId && first.sizeId === second.sizeId
    && first.parentId === second.parentId && first.index === second.index);
}

/** The layer a pointer sits over: before/after keep the parent, into makes it a child. */
function dropPosition(element: Element, offset: number): 'before' | 'after' | 'into' {
  if (holdsChildren(element.kind) && offset > 0.3 && offset < 0.7) return 'into';
  return offset < 0.5 ? 'before' : 'after';
}

function dropTarget(element: Element, position: 'before' | 'after' | 'into', pageId: string, sizeId: string, parentId: string | null, index: number): ElementDropTarget {
  return position === 'into'
    ? { pageId, sizeId, parentId: element.id, index: element.children.length }
    : { pageId, sizeId, parentId, index: position === 'before' ? index + 1 : index };
}

/** One 28px layer row: kind icon, name (double-click renames), then hover lock and eye toggles. */
function LayerRow({ project, pageId, sizeId, element, parentId, index, selection, collapsed, searching, toggle, select, update, remove, movable, hint, setHint, onDragStart, onDragEnd, onDrop, children }: {
  project: Project; pageId: string; sizeId: string; element: Element; parentId: string | null; index: number;
  selection: string[]; collapsed: ReadonlySet<string>; searching: boolean; toggle: (id: string) => void;
  select: (pageId: string, id: string, sizeId: string, additive: boolean) => void;
  update: (pageId: string, sizeId: string, patches: ElementPatch[]) => void;
  remove: (pageId: string, sizeId: string, id: string) => void;
  movable: boolean; hint: DropHint; setHint: (hint: DropHint) => void;
  onDragStart: (state: DragState) => void; onDragEnd: () => void;
  onDrop: (event: DragEvent, target: ElementDropTarget) => void;
  children: ReactNode;
}) {
  const canContain = holdsChildren(element.kind);
  const hasChildren = canContain && element.children.length > 0;
  const open = searching || !collapsed.has(`${sizeId}:${element.id}`);
  const definition = element.link ? project.linked?.[element.link] ?? null : null;
  const linkedName = definition?.name ?? null;
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
  function targetAt(offset: number) {
    return dropTarget(element, dropPosition(element, offset), pageId, sizeId, parentId, index);
  }
  function offsetAt(event: DragEvent<HTMLElement>) {
    const bounds = event.currentTarget.getBoundingClientRect();
    return (event.clientY - bounds.top) / Math.max(1, bounds.height);
  }
  return <li className={`dock-tree-group element-row${element.visible ? '' : ' hidden-in-size'}`}>
    <div className="dock-tree-row element-row-line" draggable={movable && draft === null}
      data-element-row={element.id}
      data-selected={selection.includes(element.id) || undefined}
      title={renameable ? `${label}${linkedName ? ` · linked as ${linkedName}` : ''} · double-click to rename`
        : linkedName ? `${label} · linked as ${linkedName}` : label}
      onDragStart={event => {
        if (!movable) { event.preventDefault(); return; }
        event.stopPropagation();
        onDragStart({ pageId, sizeId, id: element.id, parentId, index });
        event.dataTransfer.effectAllowed = 'move';
        event.dataTransfer.setData('text/plain', element.id);
      }}
      onDragEnd={() => onDragEnd()}
      onDragOver={event => {
        if (!movable) return;
        event.preventDefault(); event.stopPropagation();
        event.dataTransfer.dropEffect = 'move';
        setHint(targetAt(offsetAt(event)));
      }}
      onDrop={event => {
        if (!movable) { event.preventDefault(); event.stopPropagation(); return; }
        event.preventDefault(); event.stopPropagation();
        onDrop(event, targetAt(offsetAt(event)));
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
        : <button type="button" className="dock-tree-select element" aria-pressed={selection.includes(element.id)}
          onClick={event => select(pageId, element.id, sizeId, event.shiftKey || event.ctrlKey || event.metaKey)}
          onDoubleClick={renameable ? () => { editing.current = true; setDraft(element.name ?? ''); } : undefined}>
          <Icon name={definition ? 'linked-copies' : KIND_ICONS[element.kind]} size={12} /><span className={element.name?.trim() ? undefined : 'muted'}>{label}</span>
        </button>}
      <button type="button" className="icon-button sm dock-row-toggle" aria-label={`${element.locked ? 'Unlock' : 'Lock'} ${elementLabel(element)}`} title={element.locked ? 'Unlock' : 'Lock'}
        aria-pressed={element.locked} onClick={() => update(pageId, sizeId, [{ id: element.id, locked: !element.locked }])}><Icon name={element.locked ? 'lock' : 'unlock'} size={13} /></button>
      <button type="button" className="icon-button sm dock-row-toggle" aria-label={`${element.visible ? 'Hide' : 'Show'} ${elementLabel(element)}`} title={element.visible ? 'Hide' : 'Show'}
        aria-pressed={!element.visible} onClick={() => update(pageId, sizeId, [{ id: element.id, visible: !element.visible }])}><Icon name={element.visible ? 'eye' : 'eye-off'} size={13} /></button>
      <button type="button" className="icon-button sm dock-row-toggle" aria-label={`Delete ${elementLabel(element)}`} title="Delete layer" onClick={() => remove(pageId, sizeId, element.id)}><Icon name="trash" size={13} /></button>
    </div>
    {children}
  </li>;
}

/** One row tree per size boundary: layers hidden in this size stay listed, dimmed. */
export function ElementLayerRows({ project, pageId, sizeId, elements, parentId, selection, collapsed, keep, searching, toggle, select, update, remove, fixed, hint, setHint, onDragStart, onDragEnd, onDrop }: {
  project: Project; pageId: string; sizeId: string; elements: Element[]; parentId: string | null;
  selection: string[]; collapsed: ReadonlySet<string>; keep: ReadonlySet<string> | null; searching: boolean; toggle: (id: string) => void;
  select: (pageId: string, id: string, sizeId: string, additive: boolean) => void;
  update: (pageId: string, sizeId: string, patches: ElementPatch[]) => void;
  remove: (pageId: string, sizeId: string, id: string) => void;
  fixed: ReadonlySet<string>; hint: DropHint; setHint: (hint: DropHint) => void;
  onDragStart: (state: DragState) => void; onDragEnd: () => void;
  onDrop: (event: DragEvent, target: ElementDropTarget) => void;
}) {
  // A row's insertion line sits at the array index the drop would use: array order runs bottom-up,
  // so the line above row k is insertion index k + 1 and the line below the last row is index 0.
  const line = (at: number) => hint && hint.pageId === pageId && hint.sizeId === sizeId && hint.parentId === parentId && hint.index === at
    ? <li className="dock-drop-line" aria-hidden="true" />
    : null;
  const childHint = hint !== null && hint.pageId === pageId && hint.sizeId === sizeId;
  return <>{[...elements].reverse().map((element, reversedIndex) => {
    if (keep && !keep.has(element.id)) return null;
    const index = elements.length - 1 - reversedIndex;
    const movable = !fixed.has(element.id);
    const hasChildren = holdsChildren(element.kind) && element.children.length > 0;
    const open = searching || !collapsed.has(`${sizeId}:${element.id}`);
    const dropInto = childHint && hint.parentId === element.id;
    return <Fragment key={element.id}>
      {line(index + 1)}
      <LayerRow project={project} pageId={pageId} sizeId={sizeId} element={element} parentId={parentId} index={index}
        selection={selection} collapsed={collapsed} searching={searching} toggle={toggle} select={select} update={update} remove={remove}
        movable={movable} hint={hint} setHint={setHint} onDragStart={onDragStart} onDragEnd={onDragEnd} onDrop={onDrop}>
        {holdsChildren(element.kind) && open && (hasChildren || dropInto) && <ul className="dock-tree-element-children">
          <ElementLayerRows project={project} pageId={pageId} sizeId={sizeId} elements={element.children} parentId={element.id}
            selection={selection} collapsed={collapsed} keep={keep} searching={searching} toggle={toggle} select={select} update={update} remove={remove} fixed={fixed}
            hint={hint} setHint={setHint} onDragStart={onDragStart} onDragEnd={onDragEnd} onDrop={onDrop} />
        </ul>}
      </LayerRow>
    </Fragment>;
  })}{keep ? null : line(0)}</>;
}

export function ElementLayers({ project, pageId, sizeId, elements, selection, select, update, remove, move, keep = null }: {
  project: Project; pageId: string; sizeId: string; elements: Element[]; selection: string[];
  select: (pageId: string, id: string, sizeId: string, additive: boolean) => void;
  update: (pageId: string, sizeId: string, patches: ElementPatch[]) => void;
  remove: (pageId: string, sizeId: string, id: string) => void;
  move: (pageId: string, sizeId: string, id: string, parentId: string | null, index: number) => void;
  /** Rows a Layers search keeps; null lists every layer. */
  keep?: ReadonlySet<string> | null;
}) {
  const fixed = useMemo(() => nestedLayerIds(project), [project]);
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(() => new Set());
  const [dragging, setDragging] = useState<DragState>(null);
  const [hint, setHint] = useState<DropHint>(null);
  const updateHint = useCallback((next: DropHint) => setHint(current => sameTarget(current, next) ? current : next), []);
  const endDrag = useCallback(() => { setDragging(null); setHint(null); }, []);
  const toggle = (id: string) => setCollapsed(current => {
    const next = new Set(current);
    if (next.has(id)) next.delete(id); else next.add(id);
    return next;
  });
  if (!elements.length) return null;
  if (keep && !keep.size) return null;
  const searching = keep !== null;
  function drop(event: DragEvent, target: ElementDropTarget) {
    event.preventDefault();
    setHint(null);
    if (!dragging || dragging.pageId !== target.pageId || dragging.sizeId !== target.sizeId || dragging.id === target.parentId) { setDragging(null); return; }
    let index = target.index;
    if (dragging.parentId === target.parentId && dragging.index < index) index -= 1;
    setDragging(null);
    move(target.pageId, target.sizeId, dragging.id, target.parentId, index);
  }
  return <ul className="dock-tree-elements" onDragOver={event => { if (dragging) event.preventDefault(); }}
    onDrop={event => { event.preventDefault(); if (dragging) move(pageId, sizeId, dragging.id, null, elements.length); setDragging(null); setHint(null); }}>
    <ElementLayerRows project={project} pageId={pageId} sizeId={sizeId} elements={elements} parentId={null}
      selection={selection} collapsed={collapsed} keep={keep} searching={searching} toggle={toggle} select={select} update={update} remove={remove} fixed={fixed}
      hint={hint} setHint={updateHint} onDragStart={setDragging} onDragEnd={endDrag} onDrop={drop} />
  </ul>;
}
