import { useCallback, useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent, type ReactNode } from 'react';
import { Icon, type IconName } from './icons';
import { useDraftText } from './DraftText';
import { boundariesByPosition, canvasTargetKey, pointerBroken, pointerEndpointName, pointerLabel, type Boundary, type CanvasTarget } from './projectCanvasGeometry';
import type { Appearance, BoundaryKind, ElementPatch, OutputType, Page, PageSize, Pointer, PointerStateView, Project, ReferenceItem, SizePreset } from './types';
import { ElementLayers } from './projectCanvas/elementLayers';
import { SIZE_PRESETS, resolveForSize } from './projectCanvas/sizes';

const DOCK_WIDTH_KEY = 'desegnos.dock-width';
const DOCK_COLLAPSED_KEY = 'desegnos.dock-collapsed';
const PANELS_HIDDEN_KEY = 'desegnos.panels-hidden';
export const MIN_DOCK_WIDTH = 224;
export const MAX_DOCK_WIDTH = 480;
const DEFAULT_DOCK_WIDTH = 272;

export type PanelState = { width: number; collapsed: boolean; hidden: boolean };
export type PanelActions = { setWidth: (width: number) => void; setCollapsed: (collapsed: boolean) => void; toggleHidden: () => void };

function readString(key: string): string | null {
  try { return localStorage.getItem(key); } catch { return null; }
}
function store(key: string, value: string) {
  try { localStorage.setItem(key, value); } catch { /* Preferences can be unavailable. */ }
}
export function clampDockWidth(width: number): number {
  return Number.isFinite(width) ? Math.min(MAX_DOCK_WIDTH, Math.max(MIN_DOCK_WIDTH, Math.round(width))) : DEFAULT_DOCK_WIDTH;
}
function readWidth(): number {
  const value = Number(readString(DOCK_WIDTH_KEY));
  return Number.isFinite(value) && value > 0 ? clampDockWidth(value) : DEFAULT_DOCK_WIDTH;
}

// Panel width, collapsed state and visibility survive a restart.
export function usePanelState(): [PanelState, PanelActions] {
  const [width, setWidth] = useState(readWidth);
  const [collapsed, setCollapsed] = useState(() => readString(DOCK_COLLAPSED_KEY) === '1');
  const [hidden, setHidden] = useState(() => readString(PANELS_HIDDEN_KEY) === '1');
  useEffect(() => { store(DOCK_WIDTH_KEY, String(width)); }, [width]);
  useEffect(() => { store(DOCK_COLLAPSED_KEY, collapsed ? '1' : '0'); }, [collapsed]);
  useEffect(() => { store(PANELS_HIDDEN_KEY, hidden ? '1' : '0'); }, [hidden]);
  const toggleHidden = useCallback(() => setHidden(value => !value), []);
  const actions = useMemo<PanelActions>(() => ({ setWidth, setCollapsed, toggleHidden }), [toggleHidden]);
  return [{ width, collapsed, hidden }, actions];
}

function DockSection({ name, collapse, children, className }: { name: string; collapse: () => void; children: ReactNode; className?: string }) {
  return <section className={className ? `dock-panel ${className}` : 'dock-panel'} aria-label={name}>
    <header className="dock-panel-header">
      <h2>{name}</h2>
      <button type="button" className="icon-button sm" aria-label={`Collapse ${name.toLowerCase()}`} title="Collapse panels" onClick={collapse}><Icon name="chevron-right" /></button>
    </header>
    <div className="dock-panel-body">{children}</div>
  </section>;
}

export function EditorDock({ state, actions, properties, layers, history, exportPanel }: {
  state: PanelState; actions: PanelActions; properties: ReactNode; layers: ReactNode; history?: ReactNode; exportPanel?: ReactNode;
}) {
  function startResize(event: ReactPointerEvent<HTMLDivElement>) {
    event.preventDefault();
    const edge = event.currentTarget.parentElement?.getBoundingClientRect().right;
    if (edge === undefined) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    const move = (next: PointerEvent) => actions.setWidth(clampDockWidth(edge - next.clientX));
    const stop = () => { window.removeEventListener('pointermove', move); window.removeEventListener('pointerup', stop); };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', stop);
  }
  if (state.hidden) return null;
  if (state.collapsed) return <aside className="editor-dock collapsed" aria-label="Panels">
    <button type="button" className="dock-icon" aria-label="Show Properties" title="Properties" onClick={() => actions.setCollapsed(false)}><Icon name="properties" /></button>
    <button type="button" className="dock-icon" aria-label="Show Layers" title="Layers" onClick={() => actions.setCollapsed(false)}><Icon name="layers" /></button>
    {history && <button type="button" className="dock-icon" aria-label="Show History" title="History" onClick={() => actions.setCollapsed(false)}><Icon name="history" /></button>}
    {exportPanel && <button type="button" className="dock-icon" aria-label="Show Export" title="Export" onClick={() => actions.setCollapsed(false)}><Icon name="external" /></button>}
  </aside>;
  return <aside className="editor-dock" style={{ width: state.width }} aria-label="Panels">
    <div className="dock-resizer" role="separator" aria-orientation="vertical" aria-label="Resize panels" onPointerDown={startResize} />
    <div className="dock-panels">
      {history && <DockSection name="History" className="dock-panel-history" collapse={() => actions.setCollapsed(true)}>{history}</DockSection>}
      {exportPanel && <DockSection name="Export" className="dock-panel-export" collapse={() => actions.setCollapsed(true)}>{exportPanel}</DockSection>}
      <DockSection name="Properties" collapse={() => actions.setCollapsed(true)}>{properties}</DockSection>
      <DockSection name="Layers" collapse={() => actions.setCollapsed(true)}>{layers}</DockSection>
    </div>
  </aside>;
}

function wholeSize(draft: string, min: number, max: number) {
  const value = Number(draft);
  return Number.isInteger(value) && value >= min && value <= max;
}

// A field commits on Enter or blur and Escape puts the old value back.
export function PropertyField({ label, value, commit, valid, type = 'text', min, max, disabled, field }: {
  label: string; value: string | number; commit: (value: string) => void;
  valid?: (draft: string) => boolean;
  type?: 'text' | 'number'; min?: number; max?: number; disabled?: boolean; field?: string;
}) {
  const [draft, setDraft] = useState(String(value));
  const cancelled = useRef(false);
  useEffect(() => { setDraft(String(value)); }, [value]);
  function settle() {
    if (cancelled.current) { cancelled.current = false; setDraft(String(value)); return; }
    if (draft === String(value)) return;
    if (!valid || valid(draft)) commit(draft); else setDraft(String(value));
  }
  return <label className="dock-field">{label}
    <input type={type} value={draft} disabled={disabled} min={min} max={max} spellCheck={false} data-field={field}
      onChange={event => setDraft(event.target.value)}
      onBlur={settle}
      onKeyDown={event => {
        if (event.key === 'Enter') { event.preventDefault(); settle(); event.currentTarget.blur(); }
        else if (event.key === 'Escape') { event.preventDefault(); cancelled.current = true; setDraft(String(value)); event.currentTarget.blur(); }
      }} />
  </label>;
}

export function ChoiceRow<T extends string>({ label, value, choices, choose, disabled }: {
  label: string; value: T; choices: [T, string][]; choose: (value: T) => void; disabled?: boolean;
}) {
  return <div className="dock-choice" role="group" aria-label={label}>
    <span className="dock-choice-label">{label}</span>
    <div className="segmented">{choices.map(([id, text]) => <button key={id} type="button" aria-pressed={value === id} disabled={disabled} onClick={() => choose(id)}>{text}</button>)}</div>
  </div>;
}

/** One boundary per linked size, each with its dimensions and kind; removing asks first. */
export function BoundaryProperties({ page, commit, updateSize, addSize, removeSize, busy, setOutput, preview }: {
  page: Page;
  commit: (patch: { name?: string; path?: string; appearance?: Appearance; included?: boolean }) => void;
  setOutput: (output: OutputType) => void;
  preview: () => void;
  updateSize: (sizeId: string, patch: { label?: string; width?: number; height?: number; kind?: BoundaryKind }) => void;
  addSize: (preset: SizePreset, width?: number, height?: number) => void;
  removeSize: (size: PageSize) => void;
  busy: boolean;
}) {
  const [customWidth, setCustomWidth] = useState('1280');
  const [customHeight, setCustomHeight] = useState('800');
  const customValid = wholeSize(customWidth, 1, 32768) && wholeSize(customHeight, 1, 100000);
  return <div className="dock-form">
    <PropertyField label="Name" value={page.name} disabled={busy} valid={name => name.trim().length > 0} commit={name => commit({ name: name.trim() })} />
    <PropertyField label="Path" value={page.path} disabled={busy} valid={path => path.trim().startsWith('/')} commit={path => commit({ path: path.trim() })} />
    <ChoiceRow label="Appearance" value={page.appearance} choices={[['light', 'Light'], ['dark', 'Dark']]} disabled={busy} choose={appearance => commit({ appearance })} />
    <ChoiceRow label="Output" value={page.output} choices={[['webpage', 'Webpage'], ['appScreen', 'App screen'], ['slide', 'Slide'], ['image', 'Image']]} disabled={busy} choose={setOutput} />
    <label className="check-label"><input type="checkbox" checked={page.included} disabled={busy} onChange={event => commit({ included: event.target.checked })} />Include in export</label>
    {page.output === 'webpage' || page.output === 'appScreen' ? <button type="button" className="dock-action" disabled={busy} onClick={preview}>Preview export</button> : <p className="dock-pointer-note">{page.output === 'slide' ? 'Slides export as one PDF deck in canvas order.' : 'Images export as PNG, and SVG when every layer is vector.'}</p>}
    <div className="dock-sizes" role="group" aria-label="Sizes">
      <span className="dock-field-label">Sizes</span>
      {page.sizes.map((size, index) => <div key={size.id} className="dock-size-row" data-size-row={size.id}>
        <span className="dock-size-name">{size.label}{index === 0 ? ' · base' : ''}</span>
        <div className="dock-pair">
          <PropertyField label="Width" type="number" min={1} max={32768} value={size.width} disabled={busy} valid={width => wholeSize(width, 1, 32768)}
            commit={width => { const value = Math.round(Number(width)); if (value >= 1 && value <= 32768) updateSize(size.id, { width: value }); }} />
          <PropertyField label="Height" type="number" min={1} max={100000} value={size.height} disabled={busy} valid={height => wholeSize(height, 1, 100000)}
            commit={height => { const value = Math.round(Number(height)); if (value >= 1 && value <= 100000) updateSize(size.id, { height: value }); }} />
        </div>
        <div className="dock-size-actions">
          <label className="dock-inline-size">Kind
            <select value={size.kind} disabled={busy} aria-label={`Kind of ${size.label}`} onChange={event => updateSize(size.id, { kind: event.target.value as BoundaryKind })}>
              <option value="page">Page</option><option value="screen">Screen</option>
            </select>
          </label>
          {index > 0 && <button type="button" className="dock-action" disabled={busy} onClick={() => removeSize(size)}>Remove</button>}
        </div>
      </div>)}
      <div className="dock-size-add">
        <span className="dock-field-label">Add size</span>
        <div className="form-actions">
          {SIZE_PRESETS.map(preset => <button key={preset.preset} type="button" className="dock-action" disabled={busy}
            title={`${preset.width} × ${preset.height}`} onClick={() => addSize(preset.preset)}>{preset.label}</button>)}
        </div>
        <div className="dock-pair">
          <PropertyField label="Custom width" type="number" min={1} max={32768} value={customWidth} disabled={busy} valid={width => wholeSize(width, 1, 32768)} commit={setCustomWidth} />
          <PropertyField label="Custom height" type="number" min={1} max={100000} value={customHeight} disabled={busy} valid={height => wholeSize(height, 1, 100000)} commit={setCustomHeight} />
        </div>
        <button type="button" className="dock-action" disabled={busy || !customValid} onClick={() => addSize('custom', Math.round(Number(customWidth)), Math.round(Number(customHeight)))}>Add Custom</button>
      </div>
    </div>
  </div>;
}

const validHttpUrl = (value: string) => {
  try { const url = new URL(value.trim()); return (url.protocol === 'http:' || url.protocol === 'https:') && !!url.hostname; } catch { return false; }
};
const validColor = (value: string) => /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.test(value.trim());

export function ReferenceProperties({ item, update, remove, busy }: {
  item: ReferenceItem; update: (patch: { text?: string; url?: string; title?: string; color?: string }) => void;
  remove: () => void; busy: boolean;
}) {
  const noteText = item.kind.kind === 'note' ? item.kind.text : '';
  const [draft, changeDraft, flushDraft] = useDraftText(noteText, (_value, latest) => Promise.resolve(update({ text: latest() })).then(() => latest()), 400);
  return <div className="dock-form">
    {item.kind.kind === 'note' && <label className="dock-field">Note
      <textarea value={draft} rows={4} maxLength={16384} placeholder="Reference note"
        onChange={event => changeDraft(event.target.value)} onBlur={flushDraft}
        onKeyDown={event => { if (event.key === 'Escape') { event.preventDefault(); flushDraft(); event.currentTarget.blur(); } }} />
    </label>}
    {item.kind.kind === 'link' && <>
      <PropertyField label="Address" value={item.kind.url} disabled={busy} valid={validHttpUrl} commit={url => update({ url: url.trim() })} />
      <PropertyField label="Title" value={item.kind.title} disabled={busy} commit={title => update({ title })} />
    </>}
    {item.kind.kind === 'swatch' && <PropertyField label="Colour" value={item.kind.color} disabled={busy} valid={validColor} commit={color => update({ color: color.trim().toLowerCase() })} />}
    {item.kind.kind === 'image' && <div className="dock-field">Picture<input value={item.kind.name ?? ''} readOnly /></div>}
    <button type="button" className="dock-action" disabled={busy} onClick={remove}>Remove</button>
  </div>;
}

export function ProjectProperties({ project, rename, setGrid, setGridSettings, busy }: {
  project: Project; rename: (name: string) => void;
  setGrid: (enabled: boolean) => void; setGridSettings: (patch: { snap?: boolean; rulers?: boolean }) => void;
  busy: boolean;
}) {
  return <div className="dock-form">
    <PropertyField label="Project" value={project.name} disabled={busy} valid={name => name.trim().length > 0} commit={name => rename(name.trim())} />
    <div className="dock-checks">
      <label className="check-label"><input type="checkbox" checked={project.showGrid} disabled={busy} onChange={event => setGrid(event.target.checked)} />Dot grid</label>
      <label className="check-label"><input type="checkbox" checked={project.gridSettings.snap} disabled={busy} onChange={event => setGridSettings({ snap: event.target.checked })} />Snap</label>
      <label className="check-label"><input type="checkbox" checked={project.gridSettings.rulers} disabled={busy} onChange={event => setGridSettings({ rulers: event.target.checked })} />Rulers</label>
    </div>
  </div>;
}

export function PointerProperties({ pointer, project, state, change, reattach, remove, keepComment, keepCode, keepThis, merge, connectAi, aiConnected, rewriting, busy }: {
  pointer: Pointer; project: Project; state: PointerStateView | null; change: (comment: string) => Promise<unknown>;
  reattach: () => void; remove: () => void; keepComment: () => void; keepCode: () => void; keepThis: () => void; merge: () => void;
  connectAi: () => void; aiConnected: boolean; rewriting: boolean; busy: boolean;
}) {
  const broken = pointerBroken(pointer, project);
  const [draft, changeDraft, flushDraft] = useDraftText(pointer.comment, (_value, latest) => Promise.resolve(change(latest())).then(() => latest()), 400);
  return <div className="dock-form">
    <dl className="dock-pointer-endpoints">
      <div><dt>From</dt><dd>{pointerEndpointName(pointer.from, project, 'source')}</dd></div>
      <div><dt>To</dt><dd>{pointerEndpointName(pointer.to, project, 'target')}</dd></div>
    </dl>
    <label className="dock-field">Comment
      <textarea value={draft} rows={4} maxLength={16384} placeholder="What should this pointer do?"
        onChange={event => changeDraft(event.target.value)} onBlur={flushDraft}
        onKeyDown={event => { if (event.key === 'Escape') { event.preventDefault(); flushDraft(); event.currentTarget.blur(); } }} />
    </label>
    {!broken && state?.state === 'needsGenerating' && <p className="dock-pointer-note">Needs generating.</p>}
    {!broken && state?.state === 'needsExporting' && <p className="dock-pointer-note">Needs exporting. The next export rebuilds its code.</p>}
    {!broken && state?.state === 'wired' && <p className="dock-pointer-note">Wired to its generated code.</p>}
    {!broken && state?.state === 'codeChanged' && <div className="dock-pointer-clash" role="group" aria-label="Exported code changed">
      <p>Its exported code changed outside the pointer.</p>
      <div className="form-actions">
        <button type="button" className="dock-action" disabled={busy} onClick={keepComment}>Keep comment</button>
        <button type="button" className="dock-action" disabled={busy || !aiConnected} title={aiConnected ? 'Ask the AI to rewrite the comment' : 'Connect AI first'} onClick={keepCode}>Keep code</button>
      </div>
      {!aiConnected && <p className="muted">Connect AI to rewrite the comment from the code.</p>}
    </div>}
    {!broken && state?.state === 'contradicts' && <div className="dock-pointer-clash" role="group" aria-label="Contradicting pointer">
      <p>Another pointer from the same source says something different.</p>
      <div className="form-actions">
        <button type="button" className="dock-action" disabled={busy} onClick={keepThis}>Keep this one</button>
        <button type="button" className="dock-action" disabled={busy} onClick={merge}>Merge</button>
      </div>
    </div>}
    {broken && <div className="dock-pointer-broken" role="group" aria-label="Broken pointer">
      <p>Its {pointerEndpointName(pointer.from, project, 'source').startsWith('Missing') ? 'start' : 'target'} no longer exists.</p>
      <div className="form-actions">
        <button type="button" className="dock-action" disabled={busy} onClick={reattach}>Reattach</button>
        <button type="button" className="dock-action danger" disabled={busy} onClick={remove}>Remove</button>
      </div>
    </div>}
  </div>;
}

// Boundaries read top-left first, each opening onto its layers; reference items and pointers follow.
export function CanvasLayers({ project, boundaries, selection, focused, selectBoundary, selectReference, selectPointer, selectElement, updateElements, moveElement, removeLinked }: {
  project: Project; boundaries: Boundary[]; selection: CanvasTarget[]; focused: string | null;
  selectBoundary: (pageId: string, sizeId?: string) => void;
  selectReference: (itemId: string) => void; selectPointer: (pointerId: string) => void;
  selectElement: (pageId: string, elementId: string, sizeId: string | null, additive: boolean) => void;
  updateElements: (pageId: string, sizeId: string, patches: ElementPatch[]) => void;
  moveElement: (pageId: string, sizeId: string, elementId: string, parentId: string | null, index: number) => void;
  removeLinked: (id: string) => void;
}) {
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(() => new Set());
  const ordered = boundariesByPosition(boundaries);
  const selected = new Set(selection.map(canvasTargetKey));
  const selectedElements = selection.flatMap(target => target.kind === 'element' ? [target.elementId] : []);
  const referenceLabel = (item: ReferenceItem) => {
    switch (item.kind.kind) {
      case 'note': return item.kind.text.trim().split('\n')[0] || 'Note';
      case 'link': return item.kind.title.trim() || item.kind.url;
      case 'image': return item.kind.name || 'Picture';
      case 'swatch': return item.kind.color;
    }
  };
  const referenceIcon = (item: ReferenceItem): IconName => item.kind.kind === 'link' ? 'link' : item.kind.kind === 'swatch' ? 'fill-color' : item.kind.kind === 'image' ? 'canvas' : 'text';
  const pointers = project.pointers ?? [];
  const linked = Object.values(project.linked ?? {}).sort((first, second) => first.name.localeCompare(second.name));
  if (!ordered.length && !project.reference.length && !pointers.length && !linked.length) return null;
  return <ul className="dock-tree">
    {ordered.map(({ pageId, sizeId, label, kind }) => {
      const page = project.pages.find(item => item.id === pageId);
      const size = page?.sizes.find(item => item.id === sizeId);
      if (!page || !size) return null;
      const open = !collapsed.has(sizeId);
      const elements = resolveForSize(project, page, sizeId);
      const name = page.sizes.length > 1 ? `${page.name} · ${label}` : page.name;
      return <li key={sizeId} className="dock-tree-group">
        <div className="dock-tree-row">
          {elements.length > 0
            ? <button type="button" className="dock-tree-disclosure" aria-label={`${open ? 'Collapse' : 'Expand'} ${name}`} aria-expanded={open}
              onClick={() => setCollapsed(current => { const next = new Set(current); if (next.has(sizeId)) next.delete(sizeId); else next.add(sizeId); return next; })}>
              <Icon name="chevron-right" size={12} className={open ? 'expanded' : undefined} />
            </button>
            : <span className="dock-tree-spacer" aria-hidden="true" />}
          <button type="button" className="dock-tree-select" aria-pressed={focused === pageId && !selection.some(target => target.kind === 'element' && target.pageId === pageId)} title={page.path} onClick={() => selectBoundary(pageId, sizeId)}>
            <Icon name={kind === 'screen' ? 'monitor' : 'canvas'} size={14} /><span>{name}</span>
          </button>
        </div>
        {open && elements.length > 0 && <ElementLayers project={project} pageId={pageId} sizeId={sizeId} elements={elements} selection={selectedElements}
          select={selectElement} update={updateElements} move={moveElement} />}
      </li>;
    })}
    {project.reference.length > 0 && <li className="dock-tree-group reference-group">
      <div className="dock-tree-row"><span className="dock-tree-spacer" aria-hidden="true" /><span className="dock-tree-heading">Reference</span></div>
      <ul className="dock-tree-frames">
        {[...project.reference].sort((first, second) => first.rect.y - second.rect.y || first.rect.x - second.rect.x).map(item => <li key={item.id}>
          <button type="button" className="dock-tree-select frame" aria-pressed={selected.has(canvasTargetKey({ kind: 'reference', itemId: item.id }))} onClick={() => selectReference(item.id)}>
            <Icon name={referenceIcon(item)} size={12} /><span>{referenceLabel(item)}</span>
          </button>
        </li>)}
      </ul>
    </li>}
    {linked.length > 0 && <li className="dock-tree-group linked-group">
      <div className="dock-tree-row"><span className="dock-tree-spacer" aria-hidden="true" /><span className="dock-tree-heading">Linked</span></div>
      <ul className="dock-tree-frames">
        {linked.map(definition => <li key={definition.id} className="dock-linked-row">
          <span className="dock-tree-select frame" title={`Linked element ${definition.name}`}><Icon name="link" size={12} /><span>{definition.name}</span></span>
          <button type="button" className="icon-button sm dock-row-toggle" aria-label={`Delete linked element ${definition.name}`} title="Delete linked element"
            onClick={() => removeLinked(definition.id)}><Icon name="trash" size={13} /></button>
        </li>)}
      </ul>
    </li>}
    {pointers.length > 0 && <li className="dock-tree-group pointers-group">
      <div className="dock-tree-row"><span className="dock-tree-spacer" aria-hidden="true" /><span className="dock-tree-heading">Pointers</span></div>
      <ul className="dock-tree-frames">
        {pointers.map(pointer => <li key={pointer.id}>
          <button type="button" className={`dock-tree-select frame${pointerBroken(pointer, project) ? ' broken' : ''}`} aria-pressed={selected.has(canvasTargetKey({ kind: 'pointer', pointerId: pointer.id }))}
            title={pointerLabel(pointer, project)} onClick={() => selectPointer(pointer.id)}>
            <Icon name="link" size={12} /><span>{pointerLabel(pointer, project)}</span>
          </button>
        </li>)}
      </ul>
    </li>}
  </ul>;
}
