import { boundaryKindLabel, boundaryIcon, pageKind, projectKind } from '../projectCanvas/boundaryDefaults';
import { useMemo, useRef, useState, type ReactNode } from 'react';
import { Icon, type IconName } from '../icons';
import { CompactNumberField, CompactTextField } from '../CompactField';
import { boundariesByPosition, type Boundary } from '../projectCanvas/boundaryLayout';
import { canvasTargetKey, type CanvasTarget } from '../projectCanvas/canvasTargets';
import { pointerBroken } from '../projectCanvas/pointerGeometry';
import { pointerLabel } from '../projectCanvas/pointerNames';
import type { ElementPatch, NamedView, Project, ReferenceItem } from '../types';
import { ColourField } from '../projectCanvas/ColourField';
import { nestedLayerIds } from '../projectCanvas/links';
import { ElementLayers, matchingLayerIds } from './elementLayers';
import { resolveForSize } from '../projectCanvas/sizes';
import { SectionHeader, useSectionOpen, PropsSection, useCollapsedSections, setSectionCollapsed } from './sections';
import { MAX_NAMED_VIEW_NAME_CHARS } from '../generated/limits';
function ViewRow({ view, jump, rename, remove }: { view: NamedView; jump: () => void; rename: (name: string) => void; remove: () => void }) {
  const [draft, setDraft] = useState<string | null>(null);
  // Enter commits and removes the input; the blur that follows must not rename twice.
  const editing = useRef(false);
  function commit() {
    if (!editing.current) return;
    editing.current = false;
    const name = (draft ?? '').trim();
    if (name && name !== view.name) rename(name);
    setDraft(null);
  }
  return <li className="dock-linked-row">
    {draft === null
      ? <button type="button" className="dock-tree-select dock-tree-item" title={`Go to ${view.name} · double-click to rename`} onClick={jump} onDoubleClick={() => { editing.current = true; setDraft(view.name); }}>
        <Icon name="canvas" size={12} /><span>{view.name}</span>
      </button>
      : <input className="dock-inline-name" aria-label={`Rename ${view.name}`} maxLength={MAX_NAMED_VIEW_NAME_CHARS} autoFocus value={draft}
        onChange={event => setDraft(event.target.value)} onBlur={commit}
        onKeyDown={event => {
          if (event.key === 'Enter') { event.preventDefault(); commit(); }
          else if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); editing.current = false; setDraft(null); }
        }} />}
    <button type="button" className="icon-button sm dock-row-toggle" aria-label={`Delete ${view.name}`} title="Delete view" onClick={remove}><Icon name="trash" size={13} /></button>
  </li>;
}

function LayerGroup({ section, title, add, addLabel, children }: {
  section: string; title: string; add?: () => void; addLabel?: string; children?: ReactNode;
}) {
  const [open, toggle] = useSectionOpen(`layers:${section}`);
  return <li className="dock-tree-group">
    <SectionHeader title={title} open={open} toggle={toggle} add={add} addLabel={addLabel} />
    {open && children && <ul className="dock-tree-items">{children}</ul>}
  </li>;
}

function StylesSection({ project, command, busy = false }: { project: Project; command?: (command: import('../types').Command) => Promise<unknown>; busy?: boolean }) {
  const disabled = busy || !command;
  const send = (edit: import('../types').Command) => { void command?.(edit); };
  return <PropsSection id="project-styles" title="Styles">
    {project.styles?.colours.map(style => <div key={style.id}>
      <CompactTextField label="Colour style name" value={style.name} disabled={disabled} valid={value => !!value.trim()} commit={name => send({ type: 'update_colour_style', style: { ...style, name: name.trim() } })} />
      <div className="dock-inline">
        <ColourField label={`${style.name} colour`} value={style.value} optional={false} disabled={disabled} commit={value => send({ type: 'update_colour_style', style: { ...style, value } })} />
        <button type="button" className="icon-button sm" aria-label={`Delete ${style.name} style`} disabled={disabled} onClick={() => send({ type: 'remove_colour_style', id: style.id })}><Icon name="close" size={12} /></button>
      </div>
    </div>)}
    {project.styles?.text.map(style => <details key={style.id}>
      <summary>{style.name}</summary>
      <div className="dock-inline"><CompactTextField label="Text style name" value={style.name} disabled={disabled} valid={value => !!value.trim()} commit={name => send({ type: 'update_text_style', style: { ...style, name: name.trim() } })} />
        <button type="button" className="icon-button sm" aria-label={`Delete ${style.name} style`} disabled={disabled} onClick={() => send({ type: 'remove_text_style', id: style.id })}><Icon name="close" size={12} /></button></div>
      <CompactTextField label="Font family" value={style.fontFamily} disabled={disabled} commit={fontFamily => send({ type: 'update_text_style', style: { ...style, fontFamily } })} />
      <div className="props-grid">{(['size', 'weight', 'lineHeight', 'letterSpacing'] as const).map(key => <CompactNumberField key={key} prefix={key === 'size' ? 'S' : key === 'weight' ? 'W' : key === 'lineHeight' ? 'LH' : 'LS'} label={`${style.name} ${key}`} value={style[key]} disabled={disabled}
        valid={value => value.trim() !== '' && Number.isFinite(Number(value))} commit={value => send({ type: 'update_text_style', style: { ...style, [key]: Number(value) } })} />)}</div>
      <label className="check-label"><input type="checkbox" checked={style.italic} disabled={disabled} onChange={event => send({ type: 'update_text_style', style: { ...style, italic: event.target.checked } })} />Italic</label>
    </details>)}
  </PropsSection>;
}

export function CanvasLayers({ project, boundaries, selection, focused, selectBoundary, selectReference, selectPointer, selectRequest, selectElement, updateElements, moveElement, removeLinked, removeElements, deleteBoundary, removeRequest, removeReference, removePointer, addBoundary, views, addView, jumpToView, renameView, removeView, styleCommand }: {
  project: Project; boundaries: Boundary[]; selection: CanvasTarget[]; focused: string | null;
  selectBoundary: (pageId: string, sizeId?: string) => void;
  selectReference: (itemId: string) => void; selectPointer: (pointerId: string) => void; selectRequest: (requestId: string) => void;
  selectElement: (pageId: string, elementId: string, sizeId: string | null, additive: boolean) => void;
  updateElements: (pageId: string, sizeId: string, patches: ElementPatch[]) => void;
  moveElement: (pageId: string, sizeId: string, elementId: string, parentId: string | null, index: number) => void;
  removeLinked: (id: string) => void;
  removeElements: (ids: string[]) => void;
  deleteBoundary: (pageId: string, sizeId: string | null) => void;
  removeRequest: (id: string) => void;
  removeReference: (id: string) => void;
  removePointer: (id: string) => void;
  styleCommand?: (command: import('../types').Command) => Promise<unknown>;
  addBoundary: () => void;
  views: NamedView[]; addView: () => void; jumpToView: (view: NamedView) => void;
  renameView: (view: NamedView, name: string) => void; removeView: (view: NamedView) => void;
}) {
  const collapsed = useCollapsedSections();
  const toggleSection = (key: string) => setSectionCollapsed(key, !collapsed.has(key));
  const [query, setQuery] = useState('');
  const needle = query.trim().toLowerCase();
  const matches = (text: string) => !needle || text.toLowerCase().includes(needle);
  const ordered = useMemo(() => boundariesByPosition(boundaries), [boundaries]);
  const fixedLayerIds = useMemo(() => nestedLayerIds(project), [project]);
  const layerBoundaries = useMemo(() => {
    const pages = new Map(project.pages.map(page => [page.id, page]));
    return ordered.flatMap(boundary => {
      const page = pages.get(boundary.pageId);
      const size = page?.sizes.find(size => size.id === boundary.sizeId);
      if (!page || !size) return [];
      const elements = resolveForSize(project, page, size.id);
      const name = page.sizes.length > 1 ? `${page.name} · ${boundary.label}` : page.name;
      const keep = needle && !name.toLowerCase().includes(needle) ? matchingLayerIds(elements, needle) : null;
      return needle && keep && !keep.size ? [] : [{ ...boundary, page, elements, name, keep }];
    });
  }, [project, ordered, needle]);
  const selected = new Set(selection.map(canvasTargetKey));
  const selectedElements = selection.flatMap(target => target.kind === 'element' ? [target.elementId] : []);
  const referenceLabel = (item: ReferenceItem) => {
    switch (item.kind.kind) {
      case 'element': return item.kind.element.name || item.kind.element.kind;
      case 'note': return item.kind.text.trim().split('\n')[0] || 'Note';
      case 'link': return item.kind.title.trim() || item.kind.url;
      case 'image': return item.kind.name || 'Picture';
      case 'swatch': return item.kind.color;
    }
  };
  const referenceIcon = (item: ReferenceItem): IconName => item.kind.kind === 'link' ? 'link' : item.kind.kind === 'swatch' ? 'fill-color' : item.kind.kind === 'image' ? 'image' : 'note';
  const pointers = project.pointers ?? [];
  const linked = Object.values(project.linked ?? {}).sort((first, second) => first.name.localeCompare(second.name));
  const requests = project.requests ?? [];
  const shownRequests = requests.filter(request => matches(request.text.trim().split('\n')[0] || 'Empty request'));
  const shownPointers = pointers.filter(pointer => matches(pointerLabel(pointer, project)));
  const shownLinked = linked.filter(definition => matches(definition.name));
  const shownReference = [...project.reference].sort((first, second) => first.rect.y - second.rect.y || first.rect.x - second.rect.x).filter(item => matches(referenceLabel(item)));
  return <ul className="dock-tree">
    <li className="dock-search">
      <input className="dock-search-input" type="search" aria-label="Search layers" placeholder="Search layers" value={query}
        onChange={event => setQuery(event.target.value)}
        onKeyDown={event => { if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); setQuery(''); } }} />
    </li>
    <li><button type="button" className="dock-action" onClick={addBoundary}><Icon name="plus" />Add {boundaryKindLabel(projectKind(project)).toLowerCase()}</button></li>
    <li><StylesSection project={project} command={styleCommand} /></li>
    <LayerGroup section="views" title="Views" add={addView} addLabel="Save the current view">
      {views.length > 0 && views.map(view => <ViewRow key={view.id} view={view} jump={() => jumpToView(view)} rename={name => renameView(view, name)} remove={() => removeView(view)} />)}
    </LayerGroup>
    {layerBoundaries.map(({ pageId, sizeId, page, elements, name, keep }) => {
      const sectionKey = `boundary:${sizeId}`;
      const open = needle ? true : !collapsed.has(sectionKey);
      const boundarySelected = focused === pageId && !selection.some(target => target.kind === 'element' && target.pageId === pageId);
      return <li key={sizeId} className="dock-tree-group">
        <div className="dock-tree-row layer-head-row" data-selected={boundarySelected || undefined}>
          {elements.length > 0
            ? <button type="button" className="dock-tree-disclosure" aria-label={`${open ? 'Collapse' : 'Expand'} ${name}`} aria-expanded={open}
              onClick={() => toggleSection(sectionKey)}>
              <Icon name="chevron-right" size={12} className={open ? 'expanded' : undefined} />
            </button>
            : <span className="dock-tree-spacer" aria-hidden="true" />}
          <button type="button" className="dock-tree-select" data-boundary-row={sizeId} aria-pressed={boundarySelected} title={page.path} onClick={() => selectBoundary(pageId, sizeId)}>
            <Icon name={boundaryIcon(pageKind(page))} size={13} /><span>{name}</span>
          </button>
          <button type="button" className="icon-button sm dock-row-toggle" aria-label={`Delete ${name}`} title="Delete boundary" onClick={() => deleteBoundary(pageId, sizeId)}><Icon name="trash" size={13} /></button>
        </div>
        {open && elements.length > 0 && <ElementLayers project={project} pageId={pageId} sizeId={sizeId} elements={elements} selection={selectedElements} fixed={fixedLayerIds} keep={needle ? keep : null}
          select={selectElement} update={updateElements} remove={(_page, _size, id) => removeElements([id])} move={moveElement} />}
      </li>;
    })}
    {shownRequests.length > 0 && <LayerGroup section="requests" title="Requests">
      {shownRequests.map(request => <li key={request.id} className="dock-linked-row">
        <button type="button" className="dock-tree-select" data-request-row={request.id} aria-pressed={selected.has(canvasTargetKey({ kind: 'request', requestId: request.id }))} onClick={() => selectRequest(request.id)}>
          <Icon name="text" size={12} /><span>{request.text.trim().split('\n')[0] || 'Empty request'}</span>
        </button>
        <button type="button" className="icon-button sm dock-row-toggle" aria-label="Delete request" title="Delete request" onClick={() => removeRequest(request.id)}><Icon name="trash" size={13} /></button>
      </li>)}
    </LayerGroup>}
    {shownReference.length > 0 && <LayerGroup section="reference" title="Reference">
      {shownReference.map(item => <li key={item.id} className="dock-linked-row">
        <button type="button" className="dock-tree-select" aria-pressed={selected.has(canvasTargetKey({ kind: 'reference', itemId: item.id }))} onClick={() => selectReference(item.id)}>
          <Icon name={referenceIcon(item)} size={12} /><span>{referenceLabel(item)}</span>
        </button>
        <button type="button" className="icon-button sm dock-row-toggle" aria-label={`Delete ${referenceLabel(item)}`} title="Delete reference item" onClick={() => removeReference(item.id)}><Icon name="trash" size={13} /></button>
      </li>)}
    </LayerGroup>}
    {shownLinked.length > 0 && <LayerGroup section="linked" title="Linked">
      {shownLinked.map(definition => <li key={definition.id} className="dock-linked-row">
        <span className="dock-tree-select dock-tree-item" title={`Linked element ${definition.name}`}><Icon name="linked-copies" size={12} /><span>{definition.name}</span></span>
        <button type="button" className="icon-button sm dock-row-toggle" aria-label={`Delete linked element ${definition.name}`} title="Delete linked element"
          onClick={() => removeLinked(definition.id)}><Icon name="trash" size={13} /></button>
      </li>)}
    </LayerGroup>}
    {shownPointers.length > 0 && <LayerGroup section="pointers" title="Pointers">
      {shownPointers.map(pointer => <li key={pointer.id}>
        <button type="button" className={`dock-tree-select dock-tree-item${pointerBroken(pointer, project) ? ' broken' : ''}`} aria-pressed={selected.has(canvasTargetKey({ kind: 'pointer', pointerId: pointer.id }))}
          title={pointerLabel(pointer, project)} onClick={() => selectPointer(pointer.id)}>
          <Icon name="arrow-right" size={12} /><span>{pointerLabel(pointer, project)}</span>
        </button>
        <button type="button" className="icon-button sm dock-row-toggle" aria-label={`Delete ${pointerLabel(pointer, project)}`} title="Delete pointer" onClick={() => removePointer(pointer.id)}><Icon name="trash" size={13} /></button>
      </li>)}
    </LayerGroup>}
  </ul>;
}
