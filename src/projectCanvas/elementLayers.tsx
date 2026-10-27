import { useMemo, useState, type DragEvent } from 'react';
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

/** One row tree per size boundary: layers hidden in this size stay listed, dimmed. */
export function ElementLayerRows({ project, pageId, sizeId, elements, parentId, depth, selection, collapsed, toggle, select, update, fixed, onDragStart, onDrop }: {
  project: Project; pageId: string; sizeId: string; elements: Element[]; parentId: string | null; depth: number;
  selection: string[]; collapsed: ReadonlySet<string>; toggle: (id: string) => void;
  select: (pageId: string, id: string, sizeId: string, additive: boolean) => void;
  update: (pageId: string, sizeId: string, patches: ElementPatch[]) => void;
  fixed: ReadonlySet<string>;
  onDragStart: (state: DragState) => void;
  onDrop: (event: DragEvent, target: ElementDropTarget) => void;
}) {
  return <>{[...elements].reverse().map((element, reversedIndex) => {
    const index = elements.length - 1 - reversedIndex;
    const canContain = holdsChildren(element.kind);
    const hasChildren = canContain && element.children.length > 0;
    const open = !collapsed.has(`${sizeId}:${element.id}`);
    const definition = element.link ? project.linked?.[element.link] ?? null : null;
    const label = definition?.name ?? elementLabel(element);
    // A layer inside a linked element shares its order and parent with every copy.
    const movable = !fixed.has(element.id);
    const drop = (event: DragEvent, position: 'before' | 'after' | 'into') => {
      event.preventDefault();
      event.stopPropagation();
      const target: ElementDropTarget = position === 'into'
        ? { pageId, sizeId, parentId: element.id, index: element.children.length }
        : { pageId, sizeId, parentId, index: position === 'before' ? index + 1 : index };
      onDrop(event, target);
    };
    return <li key={element.id} className={`dock-tree-group element-row${element.visible ? '' : ' hidden-in-size'}`}>
      <div className="dock-tree-row element-row-line" draggable={movable}
        data-element-row={element.id}
        title={definition ? `${definition.name} · linked` : elementLabel(element)}
        onDragStart={event => { event.stopPropagation(); onDragStart({ pageId, sizeId, id: element.id, parentId, index }); event.dataTransfer.effectAllowed = 'move'; event.dataTransfer.setData('text/plain', element.id); }}
        onDragOver={event => { if (!movable) return; event.preventDefault(); event.stopPropagation(); event.dataTransfer.dropEffect = 'move'; }}
        onDrop={event => {
          if (!movable) { event.preventDefault(); event.stopPropagation(); return; }
          const bounds = event.currentTarget.getBoundingClientRect();
          const offset = (event.clientY - bounds.top) / Math.max(1, bounds.height);
          drop(event, canContain && offset > 0.3 && offset < 0.7 ? 'into' : offset < 0.5 ? 'before' : 'after');
        }}
        style={{ paddingInlineStart: depth * 12 }}>
        {hasChildren
          ? <button type="button" className="dock-tree-disclosure" aria-label={`${open ? 'Collapse' : 'Expand'} ${elementLabel(element)}`} aria-expanded={open} onClick={() => toggle(`${sizeId}:${element.id}`)}>
            <Icon name="chevron-right" size={12} className={open ? 'expanded' : undefined} />
          </button>
          : <span className="dock-tree-spacer" aria-hidden="true" />}
        <button type="button" className="dock-tree-select element" aria-pressed={selection.includes(element.id)}
          onClick={event => select(pageId, element.id, sizeId, event.shiftKey || event.ctrlKey || event.metaKey)}>
          <Icon name={definition ? 'link' : KIND_ICONS[element.kind]} size={12} /><span className={definition || element.name?.trim() ? undefined : 'muted'}>{label}</span>
        </button>
        <button type="button" className="icon-button sm dock-row-toggle" aria-label={`${element.visible ? 'Hide' : 'Show'} ${elementLabel(element)}`} title={element.visible ? 'Hide' : 'Show'}
          aria-pressed={!element.visible} onClick={() => update(pageId, sizeId, [{ id: element.id, visible: !element.visible }])}><Icon name={element.visible ? 'eye' : 'eye-off'} size={13} /></button>
        <button type="button" className="icon-button sm dock-row-toggle" aria-label={`${element.locked ? 'Unlock' : 'Lock'} ${elementLabel(element)}`} title={element.locked ? 'Unlock' : 'Lock'}
          aria-pressed={element.locked} onClick={() => update(pageId, sizeId, [{ id: element.id, locked: !element.locked }])}><Icon name={element.locked ? 'lock' : 'unlock'} size={13} /></button>
      </div>
      {hasChildren && open && <ul className="dock-tree-element-children">
        <ElementLayerRows project={project} pageId={pageId} sizeId={sizeId} elements={element.children} parentId={element.id} depth={depth + 1}
          selection={selection} collapsed={collapsed} toggle={toggle} select={select} update={update} fixed={fixed} onDragStart={onDragStart} onDrop={onDrop} />
      </ul>}
    </li>;
  })}</>;
}

export function ElementLayers({ project, pageId, sizeId, elements, selection, select, update, move }: {
  project: Project; pageId: string; sizeId: string; elements: Element[]; selection: string[];
  select: (pageId: string, id: string, sizeId: string, additive: boolean) => void;
  update: (pageId: string, sizeId: string, patches: ElementPatch[]) => void;
  move: (pageId: string, sizeId: string, id: string, parentId: string | null, index: number) => void;
}) {
  const fixed = useMemo(() => nestedLayerIds(project), [project]);
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(() => new Set());
  const [dragging, setDragging] = useState<DragState>(null);
  if (!elements.length) return null;
  const toggle = (id: string) => setCollapsed(current => {
    const next = new Set(current);
    if (next.has(id)) next.delete(id); else next.add(id);
    return next;
  });
  function drop(event: DragEvent, target: ElementDropTarget) {
    if (!dragging || dragging.pageId !== target.pageId || dragging.sizeId !== target.sizeId || dragging.id === target.parentId) { setDragging(null); return; }
    let index = target.index;
    if (dragging.parentId === target.parentId && dragging.index < index) index -= 1;
    setDragging(null);
    move(target.pageId, target.sizeId, dragging.id, target.parentId, index);
  }
  return <ul className="dock-tree-elements" onDragOver={event => { if (dragging) event.preventDefault(); }}
    onDrop={event => { event.preventDefault(); if (dragging) move(pageId, sizeId, dragging.id, null, elements.length); setDragging(null); }}>
    <ElementLayerRows project={project} pageId={pageId} sizeId={sizeId} elements={elements} parentId={null} depth={0}
      selection={selection} collapsed={collapsed} toggle={toggle} select={select} update={update} fixed={fixed} onDragStart={setDragging} onDrop={drop} />
  </ul>;
}
