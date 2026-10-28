import { Fragment, useCallback, useEffect, useMemo, useRef, useState, type FormEvent, type PointerEvent as ReactPointerEvent, type ReactNode } from 'react';
import { Icon, type IconName } from './icons';
import { CompactNumberField, CompactTextField, useFieldDraft } from './CompactField';
import { useDraftText } from './DraftText';
import { boundariesByPosition, canvasTargetKey, pointerBroken, pointerEndpointName, pointerLabel, type Boundary, type CanvasTarget } from './projectCanvasGeometry';
import type { Appearance, BoundaryKind, ElementPatch, NamedView, OutputType, Page, PageSize, PendingRequest, Pointer, PointerStateView, Project, ProjectLocationView, ReferenceItem, RequestAttachment, SizePreset } from './types';
import { ElementLayers } from './projectCanvas/elementLayers';
import { SIZE_PRESETS, resolveForSize, sizeLabel } from './projectCanvas/sizes';
import { requestAreaLabel, requestTargetLabel } from './projectCanvas/requests';

const DOCK_WIDTH_KEY = 'desegnos.dock-width';
const DOCK_COLLAPSED_KEY = 'desegnos.dock-collapsed';
const PANELS_HIDDEN_KEY = 'desegnos.panels-hidden';
const PANEL_WEIGHTS_KEY = 'desegnos.dock-panel-weights';
const PANEL_COLLAPSED_KEY = 'desegnos.dock-panel-collapsed';
export const MIN_DOCK_WIDTH = 224;
export const MAX_DOCK_WIDTH = 480;
const DEFAULT_DOCK_WIDTH = 272;
/** Every expanded panel keeps enough height to read its own controls. */
export const MIN_PANEL_HEIGHT = 160;

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

function DockSection({ name, open, toggle, children, grow, register }: {
  name: string; open: boolean; toggle: () => void; children: ReactNode;
  grow: number; register: (node: HTMLElement | null) => void;
}) {
  return <section ref={register} className={open ? 'dock-panel' : 'dock-panel is-collapsed'} aria-label={name} style={open ? { flexGrow: grow } : undefined}>
    <header className="dock-panel-header">
      <h2>{name}</h2>
      <button type="button" className="icon-button sm" aria-expanded={open} aria-label={`${open ? 'Collapse' : 'Expand'} ${name.toLowerCase()}`} title={open ? `Collapse ${name}` : `Expand ${name}`} onClick={toggle}><Icon name="chevron-right" className={open ? 'expanded' : undefined} /></button>
    </header>
    {open && <div className="dock-panel-body">{children}</div>}
  </section>;
}

function readWeights(): Record<string, number> {
  try {
    const raw: unknown = JSON.parse(readString(PANEL_WEIGHTS_KEY) ?? '{}');
    if (!raw || typeof raw !== 'object') return {};
    return Object.fromEntries(Object.entries(raw).filter(([, value]) => typeof value === 'number' && Number.isFinite(value) && value > 0));
  } catch { return {}; }
}

function readCollapsedPanels(): ReadonlySet<string> {
  try {
    const raw: unknown = JSON.parse(readString(PANEL_COLLAPSED_KEY) ?? '[]');
    return new Set(Array.isArray(raw) ? raw.filter((key): key is string => typeof key === 'string') : []);
  } catch { return new Set(); }
}

const DOCK_ICONS: Record<string, IconName> = { history: 'history', export: 'external', properties: 'properties', layers: 'layers' };
/** Properties carries the most fields, so it starts taller than History and Layers. */
const DEFAULT_PANEL_WEIGHTS: Record<string, number> = { history: 0.8, export: 0.9, properties: 1.6, layers: 1.1 };

export function EditorDock({ state, actions, properties, layers, history, exportPanel }: {
  state: PanelState; actions: PanelActions; properties: ReactNode; layers: ReactNode; history?: ReactNode; exportPanel?: ReactNode;
}) {
  const panels = [
    ...(history ? [{ key: 'history', name: 'History', content: history }] : []),
    ...(exportPanel ? [{ key: 'export', name: 'Export', content: exportPanel }] : []),
    { key: 'properties', name: 'Properties', content: properties },
    { key: 'layers', name: 'Layers', content: layers },
  ];
  const [weights, setWeights] = useState<Record<string, number>>(readWeights);
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(readCollapsedPanels);
  const nodes = useRef(new Map<string, HTMLElement>());
  useEffect(() => { store(PANEL_WEIGHTS_KEY, JSON.stringify(weights)); }, [weights]);
  useEffect(() => { store(PANEL_COLLAPSED_KEY, JSON.stringify([...collapsed])); }, [collapsed]);

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

  // A splitter trades height between its two neighbouring panels and stops at the readable minimum.
  function startPanelResize(event: ReactPointerEvent<HTMLDivElement>, aboveKey: string, belowKey: string) {
    event.preventDefault();
    const aboveNode = nodes.current.get(aboveKey);
    const belowNode = nodes.current.get(belowKey);
    if (!aboveNode || !belowNode) return;
    const startY = event.clientY;
    const startAbove = aboveNode.getBoundingClientRect().height;
    const startBelow = belowNode.getBoundingClientRect().height;
    const pair = startAbove + startBelow;
    const move = (next: PointerEvent) => {
      const dy = Math.max(MIN_PANEL_HEIGHT - startAbove, Math.min(pair - MIN_PANEL_HEIGHT - startBelow, next.clientY - startY));
      const heights: Record<string, number> = {};
      for (const panel of panels) {
        const node = nodes.current.get(panel.key);
        if (node && !collapsed.has(panel.key)) heights[panel.key] = node.getBoundingClientRect().height;
      }
      heights[aboveKey] = startAbove + dy;
      heights[belowKey] = startBelow - dy;
      setWeights(heights);
    };
    const stop = () => { window.removeEventListener('pointermove', move); window.removeEventListener('pointerup', stop); };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', stop);
  }

  function togglePanel(key: string) {
    const next = new Set(collapsed);
    if (next.has(key)) next.delete(key); else next.add(key);
    // Collapsing every panel is the same thing as the dock's icon strip.
    if (panels.every(panel => next.has(panel.key))) { setCollapsed(new Set()); actions.setCollapsed(true); }
    else setCollapsed(next);
  }

  function expandDock() {
    setCollapsed(new Set());
    actions.setCollapsed(false);
  }

  if (state.hidden) return null;
  if (state.collapsed) return <aside className="editor-dock collapsed" aria-label="Panels">
    {panels.map(panel => <button key={panel.key} type="button" className="dock-icon" aria-label={`Show ${panel.name}`} title={panel.name} onClick={expandDock}><Icon name={DOCK_ICONS[panel.key] ?? 'properties'} /></button>)}
  </aside>;
  return <aside className="editor-dock" style={{ width: state.width }} aria-label="Panels">
    <div className="dock-resizer" role="separator" aria-orientation="vertical" aria-label="Resize panels" onPointerDown={startResize} />
    <div className="dock-panels">
      {panels.map((panel, index) => {
        const open = !collapsed.has(panel.key);
        const previous = panels[index - 1];
        return <Fragment key={panel.key}>
          {previous && open && !collapsed.has(previous.key) && <div className="dock-splitter" role="separator" aria-orientation="horizontal"
            aria-label={`Resize ${previous.name} and ${panel.name}`} onPointerDown={event => startPanelResize(event, previous.key, panel.key)} />}
          <DockSection name={panel.name} open={open} toggle={() => togglePanel(panel.key)} grow={weights[panel.key] ?? DEFAULT_PANEL_WEIGHTS[panel.key] ?? 1}
            register={node => { if (node) nodes.current.set(panel.key, node); else nodes.current.delete(panel.key); }}>{panel.content}</DockSection>
        </Fragment>;
      })}
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
  const state = useFieldDraft(value, commit, valid);
  return <label className="dock-field">{label}
    <input type={type} value={state.draft} disabled={disabled} min={min} max={max} spellCheck={false} data-field={field}
      onChange={event => state.setDraft(event.target.value)}
      onBlur={state.settle}
      onKeyDown={event => {
        if (event.key === 'Enter') { event.preventDefault(); state.settle(); event.currentTarget.blur(); }
        else if (event.key === 'Escape') { event.preventDefault(); state.cancel(); event.currentTarget.blur(); }
      }} />
  </label>;
}

export function ChoiceRow<T extends string>({ label, value, choices, choose, disabled, names, hideLabel }: {
  label: string; value: T; choices: [T, string][]; choose: (value: T) => void; disabled?: boolean;
  /** Full names for short labels, used as the accessible name and tooltip. */
  names?: Partial<Record<T, string>>; hideLabel?: boolean;
}) {
  const name = (id: T) => names?.[id];
  return <div className="dock-choice" role="group" aria-label={label}>
    {!hideLabel && <span className="dock-choice-label">{label}</span>}
    <div className="segmented">{choices.map(([id, text]) => <button key={id} type="button" aria-pressed={value === id} aria-label={name(id)} title={name(id)} disabled={disabled} onClick={() => choose(id)}>{text}</button>)}</div>
  </div>;
}

/** Boundary properties: one rhythm — 8px rows inside a section, 16px between sections. */
export function BoundaryProperties({ page, commit, updateSize, addSize, removeSize, remove, busy, setOutput, preview }: {
  page: Page;
  commit: (patch: { name?: string; path?: string; appearance?: Appearance; included?: boolean }) => void;
  setOutput: (output: OutputType) => void;
  preview: () => void;
  updateSize: (sizeId: string, patch: { label?: string; width?: number; height?: number; kind?: BoundaryKind }) => void;
  addSize: (preset: SizePreset, width?: number, height?: number) => void;
  removeSize: (size: PageSize) => void;
  remove: () => void;
  busy: boolean;
}) {
  const [customWidth, setCustomWidth] = useState('1280');
  const [customHeight, setCustomHeight] = useState('800');
  const [customOpen, setCustomOpen] = useState(false);
  const customValid = wholeSize(customWidth, 1, 32768) && wholeSize(customHeight, 1, 100000);
  const hasPath = page.output === 'webpage' || page.output === 'appScreen';
  return <div className="dock-form props boundary-props">
    <div className="dock-layer-title"><strong>Boundary</strong></div>
    <section className="props-section">
      <CompactTextField label="Name" placeholder="Name" value={page.name} disabled={busy} valid={name => name.trim().length > 0} commit={name => commit({ name: name.trim() })} />
      {hasPath && <CompactTextField prefix="URL" label="Path" placeholder="/path" value={page.path} disabled={busy} valid={path => path.trim().startsWith('/')} commit={path => commit({ path: path.trim() })} />}
      <div className="props-choice"><ChoiceRow label="Appearance" hideLabel value={page.appearance} choices={[['light', 'Light'], ['dark', 'Dark']]} names={{ light: 'Light', dark: 'Dark' }} disabled={busy} choose={appearance => commit({ appearance })} /></div>
    </section>
    <div className="props-section boundary-export">
      <h3 className="props-heading">Export as</h3>
      <div className="props-choice"><ChoiceRow label="Output" hideLabel value={page.output} choices={[['webpage', 'Web'], ['appScreen', 'App'], ['slide', 'Slide'], ['image', 'Image']]}
        names={{ webpage: 'Webpage', appScreen: 'App screen', slide: 'Slide', image: 'Image' }} disabled={busy} choose={setOutput} /></div>
      <div className="boundary-export-actions">
        <label className="check-label"><input type="checkbox" checked={page.included} disabled={busy} onChange={event => commit({ included: event.target.checked })} />Include in export</label>
        {hasPath && <button type="button" className="dock-action" disabled={busy} onClick={preview}>Preview export</button>}
      </div>
    </div>
    <div className="props-section boundary-sizes" role="group" aria-label="Sizes">
      <h3 className="props-heading">Sizes</h3>
      {page.sizes.map((size, index) => <div key={size.id} className="dock-size-row" data-size-row={size.id}>
        {index === 0
          ? page.sizes.length > 1 && <span className="dock-size-tag">Base</span>
          : <span className="dock-size-name" title={sizeLabel(size.width)}>{sizeLabel(size.width)}</span>}
        <CompactNumberField prefix="W" label={`Width of ${sizeLabel(size.width)}`} min={1} max={32768} unit="px" value={size.width} disabled={busy} valid={width => wholeSize(width, 1, 32768)}
          commit={width => { const value = Math.round(Number(width)); if (value >= 1 && value <= 32768) updateSize(size.id, { width: value }); }} />
        <CompactNumberField prefix="H" label={`Height of ${sizeLabel(size.width)}`} min={1} max={100000} unit="px" value={size.height} disabled={busy} valid={height => wholeSize(height, 1, 100000)}
          commit={height => { const value = Math.round(Number(height)); if (value >= 1 && value <= 100000) updateSize(size.id, { height: value }); }} />
        <div className="dock-size-tail">
          <select className="dock-kind" value={size.kind} disabled={busy} title="Kind" aria-label={`Kind of ${sizeLabel(size.width)}`} onChange={event => updateSize(size.id, { kind: event.target.value as BoundaryKind })}>
            <option value="page">Page</option><option value="screen">Screen</option>
          </select>
          {index > 0 && <button type="button" className="icon-button sm dock-row-toggle" aria-label={`Remove ${sizeLabel(size.width)} size`} title={`Remove ${sizeLabel(size.width)} size`} disabled={busy} onClick={() => removeSize(size)}><Icon name="trash" size={13} /></button>}
        </div>
      </div>)}
      <div className="dock-size-add">
        <div className="boundary-add-row">
          {SIZE_PRESETS.map(preset => <button key={preset.preset} type="button" className="dock-action" disabled={busy}
            aria-label={`Add ${preset.label}`} title={`Add ${preset.label} · ${preset.width} × ${preset.height}`} onClick={() => addSize(preset.preset)}>{preset.label}</button>)}
          <button type="button" className="icon-button sm" aria-label="Add custom size" title="Add custom size" aria-expanded={customOpen} disabled={busy} onClick={() => setCustomOpen(open => !open)}>
            <Icon name={customOpen ? 'minus' : 'plus'} size={13} />
          </button>
        </div>
        {customOpen && <div className="boundary-custom-row">
          <CompactNumberField prefix="W" label="Custom width" min={1} max={32768} unit="px" value={customWidth} disabled={busy} valid={width => wholeSize(width, 1, 32768)} commit={setCustomWidth} />
          <CompactNumberField prefix="H" label="Custom height" min={1} max={100000} unit="px" value={customHeight} disabled={busy} valid={height => wholeSize(height, 1, 100000)} commit={setCustomHeight} />
          <button type="button" className="dock-action" aria-label="Add custom size" title="Add custom size" disabled={busy || !customValid} onClick={() => addSize('custom', Math.round(Number(customWidth)), Math.round(Number(customHeight)))}>Add</button>
        </div>}
      </div>
    </div>
    <div className="boundary-footer">
      <button type="button" className="dock-action danger" disabled={busy} onClick={remove}>Delete boundary</button>
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

export function ProjectProperties({ project, rename, setGrid, setGridSettings, busy, save }: {
  project: Project; rename: (name: string) => void;
  setGrid: (enabled: boolean) => void; setGridSettings: (patch: { snap?: boolean; rulers?: boolean }) => void;
  busy: boolean; save?: () => void;
}) {
  return <div className="dock-form">
    <PropertyField label="Project" value={project.name} disabled={busy} valid={name => name.trim().length > 0} commit={name => rename(name.trim())} />
    {save && <button type="button" className="dock-action" disabled={busy} onClick={save}>Save project…</button>}
    <div className="dock-checks">
      <label className="check-label"><input type="checkbox" checked={project.showGrid} disabled={busy} onChange={event => setGrid(event.target.checked)} />Dot grid</label>
      <label className="check-label"><input type="checkbox" checked={project.gridSettings.snap} disabled={busy} onChange={event => setGridSettings({ snap: event.target.checked })} />Snap</label>
      <label className="check-label"><input type="checkbox" checked={project.gridSettings.rulers} disabled={busy} onChange={event => setGridSettings({ rulers: event.target.checked })} />Rulers</label>
    </div>
  </div>;
}

/** The first save of an untitled project names it and picks its file, inline in the dock. */
export function UntitledSaveForm({ project, location, busy, chooseLocation, save, cancel }: {
  project: Project; location: ProjectLocationView | null; busy: boolean;
  chooseLocation: (name: string) => Promise<string | null>;
  save: (name: string, path: string) => Promise<void>;
  cancel: () => void;
}) {
  const [name, setName] = useState(project.name === 'Untitled project' ? '' : project.name);
  const [path, setPath] = useState<string | null>(null);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState('');
  const locked = busy || working;
  const fail = (reason: unknown) => setError(reason instanceof Error ? reason.message : String(reason));
  async function choose() {
    if (locked) return;
    setError('');
    try {
      const picked = await chooseLocation(name.trim() || 'Untitled project');
      if (picked) setPath(picked);
    } catch (reason) { fail(reason); }
  }
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (locked || !name.trim()) return;
    setWorking(true); setError('');
    try {
      let target = path;
      if (!target) {
        target = await chooseLocation(name.trim());
        if (!target) return;
        setPath(target);
      }
      await save(name.trim(), target);
    } catch (reason) { fail(reason); }
    finally { setWorking(false); }
  }
  return <form className="dock-form untitled-save" aria-label="Save untitled project" onSubmit={submit}>
    <h3>Save project</h3>
    <label className="dock-field">Project name
      <input autoFocus required maxLength={120} value={name} disabled={locked} autoComplete="off"
        onFocus={event => event.currentTarget.select()} onChange={event => setName(event.target.value)} />
    </label>
    <div className="dock-field">Location
      <p className="field-help" style={{ overflowWrap: 'anywhere' }}>{path ?? location?.path ?? 'Choose a location…'}</p>
      <button type="button" className="dock-action" disabled={locked} onClick={() => { void choose(); }}>Choose location…</button>
    </div>
    {error && <p className="field-help" role="alert">{error}</p>}
    <div className="form-actions">
      <button className="primary" type="submit" disabled={locked || !name.trim()}>{working ? 'Saving…' : 'Save project'}</button>
      <button type="button" disabled={locked} onClick={cancel}>Cancel</button>
    </div>
  </form>;
}

/** A pending request: its text, what it targets, its area and its attachments. */
export function RequestProperties({ request, project, busy, update, remove, attach }: {
  request: PendingRequest; project: Project; busy: boolean;
  update: (request: PendingRequest) => void; remove: () => void; attach: (requestIds: string[], itemIds: string[]) => void;
}) {
  const [draft, changeDraft, flushDraft] = useDraftText(request.text, (_value, latest) => { update({ ...request, text: latest() }); return Promise.resolve(latest()); }, 400);
  const attachable = project.reference.filter(item => !request.attachments.some(attachment => attachment.text && item.kind.kind === 'note' && attachment.text === item.kind.text));
  return <div className="dock-form">
    <div className="dock-layer-title"><strong>Request</strong></div>
    <label className="dock-field">What to make or change
      <textarea value={draft} rows={4} maxLength={16384} placeholder="Describe what should happen here"
        onChange={event => changeDraft(event.target.value)} onBlur={flushDraft}
        onKeyDown={event => { if (event.key === 'Escape') { event.preventDefault(); flushDraft(); event.currentTarget.blur(); } }} />
    </label>
    <div className="dock-field"><span className="dock-field-label">Targets</span>
      {request.targets.length ? <ul className="dock-request-targets">{request.targets.map((target, index) => <li key={`${target.kind}:${index}`}>{target.kind === 'element' ? 'Layer · ' : target.kind === 'boundary' ? 'Boundary · ' : target.kind === 'reference' ? 'Reference · ' : 'Pointer · '}{requestTargetLabel(project, target)}</li>)}</ul>
        : <p className="dock-hint">No targets: this request makes new reference outside every boundary.</p>}
    </div>
    <p className="dock-hint">{requestAreaLabel(request.area)}</p>
    {attachable.length > 0 && <div className="dock-field"><span className="dock-field-label">Attach reference</span>
      <ul className="dock-request-targets">{attachable.map(item => <li key={item.id}><button type="button" className="dock-action" disabled={busy} onClick={() => attach([request.id], [item.id])}>Attach {requestTargetLabel(project, { kind: 'reference', id: item.id })}</button></li>)}</ul>
    </div>}
    {request.attachments.length > 0 && <div className="dock-field"><span className="dock-field-label">Attachments</span>
      <ul className="dock-history-attachments">{request.attachments.map((attachment: RequestAttachment) => <li key={attachment.id}>{attachment.name}</li>)}</ul>
    </div>}
    <button type="button" className="dock-action danger" disabled={busy} onClick={remove}>Delete request</button>
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

// A saved camera: click to glide there, double-click to rename in place, or delete it.
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
      : <input className="dock-inline-name" aria-label={`Rename ${view.name}`} maxLength={80} autoFocus value={draft}
        onChange={event => setDraft(event.target.value)} onBlur={commit}
        onKeyDown={event => {
          if (event.key === 'Enter') { event.preventDefault(); commit(); }
          else if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); editing.current = false; setDraft(null); }
        }} />}
    <button type="button" className="icon-button sm dock-row-toggle" aria-label={`Delete ${view.name}`} title="Delete view" onClick={remove}><Icon name="trash" size={13} /></button>
  </li>;
}

// Boundaries read top-left first, each opening onto its layers; reference items and pointers follow.
export function CanvasLayers({ project, boundaries, selection, focused, selectBoundary, selectReference, selectPointer, selectRequest, selectElement, updateElements, moveElement, removeLinked, removeElements, deleteBoundary, removeRequest, removeReference, removePointer, views, addView, jumpToView, renameView, removeView }: {
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
  views: NamedView[]; addView: () => void; jumpToView: (view: NamedView) => void;
  renameView: (view: NamedView, name: string) => void; removeView: (view: NamedView) => void;
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
  const requests = project.requests ?? [];
  if (!ordered.length && !project.reference.length && !pointers.length && !linked.length && !requests.length && !views.length) return null;
  return <ul className="dock-tree">
    <li className="dock-tree-group views-group">
      <div className="dock-tree-row">
        <span className="dock-tree-spacer" aria-hidden="true" /><span className="dock-tree-heading">Views</span>
        <button type="button" className="icon-button sm dock-row-toggle" aria-label="Save the current view" title="Save the current view" onClick={addView}><Icon name="plus" size={13} /></button>
      </div>
      {views.length > 0 && <ul className="dock-tree-items">
        {views.map(view => <ViewRow key={view.id} view={view} jump={() => jumpToView(view)} rename={name => renameView(view, name)} remove={() => removeView(view)} />)}
      </ul>}
    </li>
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
          <button type="button" className="dock-tree-select" data-boundary-row={sizeId} aria-pressed={focused === pageId && !selection.some(target => target.kind === 'element' && target.pageId === pageId)} title={page.path} onClick={() => selectBoundary(pageId, sizeId)}>
            <Icon name={kind === 'screen' ? 'monitor' : 'canvas'} size={14} /><span>{name}</span>
          </button>
        </div>
        <button type="button" className="icon-button sm dock-row-toggle" aria-label={`Delete ${name}`} title="Delete boundary" onClick={() => deleteBoundary(pageId, sizeId)}><Icon name="trash" size={13} /></button>
        {open && elements.length > 0 && <ElementLayers project={project} pageId={pageId} sizeId={sizeId} elements={elements} selection={selectedElements}
          select={selectElement} update={updateElements} remove={(page, size, id) => removeElements([id])} move={moveElement} />}
      </li>;
    })}
    {requests.length > 0 && <li className="dock-tree-group request-group">
      <div className="dock-tree-row"><span className="dock-tree-spacer" aria-hidden="true" /><span className="dock-tree-heading">Requests</span></div>
      <ul className="dock-tree-items">
        {requests.map(request => <li key={request.id} className="dock-linked-row">
          <button type="button" className="dock-tree-select" data-request-row={request.id} aria-pressed={selected.has(canvasTargetKey({ kind: 'request', requestId: request.id }))} onClick={() => selectRequest(request.id)}>
            <Icon name="text" size={12} /><span>{request.text.trim().split('\n')[0] || 'Empty request'}</span>
          </button>
          <button type="button" className="icon-button sm dock-row-toggle" aria-label="Delete request" title="Delete request" onClick={() => removeRequest(request.id)}><Icon name="trash" size={13} /></button>
        </li>)}
      </ul>
    </li>}
    {project.reference.length > 0 && <li className="dock-tree-group reference-group">
      <div className="dock-tree-row"><span className="dock-tree-spacer" aria-hidden="true" /><span className="dock-tree-heading">Reference</span></div>
      <ul className="dock-tree-items">
        {[...project.reference].sort((first, second) => first.rect.y - second.rect.y || first.rect.x - second.rect.x).map(item => <li key={item.id}>
          <button type="button" className="dock-tree-select dock-tree-item" aria-pressed={selected.has(canvasTargetKey({ kind: 'reference', itemId: item.id }))} onClick={() => selectReference(item.id)}>
            <Icon name={referenceIcon(item)} size={12} /><span>{referenceLabel(item)}</span>
          </button>
          <button type="button" className="icon-button sm dock-row-toggle" aria-label={`Delete ${referenceLabel(item)}`} title="Delete reference item" onClick={() => removeReference(item.id)}><Icon name="trash" size={13} /></button>
        </li>)}
      </ul>
    </li>}
    {linked.length > 0 && <li className="dock-tree-group linked-group">
      <div className="dock-tree-row"><span className="dock-tree-spacer" aria-hidden="true" /><span className="dock-tree-heading">Linked</span></div>
      <ul className="dock-tree-items">
        {linked.map(definition => <li key={definition.id} className="dock-linked-row">
          <span className="dock-tree-select dock-tree-item" title={`Linked element ${definition.name}`}><Icon name="link" size={12} /><span>{definition.name}</span></span>
          <button type="button" className="icon-button sm dock-row-toggle" aria-label={`Delete linked element ${definition.name}`} title="Delete linked element"
            onClick={() => removeLinked(definition.id)}><Icon name="trash" size={13} /></button>
        </li>)}
      </ul>
    </li>}
    {pointers.length > 0 && <li className="dock-tree-group pointers-group">
      <div className="dock-tree-row"><span className="dock-tree-spacer" aria-hidden="true" /><span className="dock-tree-heading">Pointers</span></div>
      <ul className="dock-tree-items">
        {pointers.map(pointer => <li key={pointer.id}>
          <button type="button" className={`dock-tree-select dock-tree-item${pointerBroken(pointer, project) ? ' broken' : ''}`} aria-pressed={selected.has(canvasTargetKey({ kind: 'pointer', pointerId: pointer.id }))}
            title={pointerLabel(pointer, project)} onClick={() => selectPointer(pointer.id)}>
            <Icon name="link" size={12} /><span>{pointerLabel(pointer, project)}</span>
          </button>
          <button type="button" className="icon-button sm dock-row-toggle" aria-label={`Delete ${pointerLabel(pointer, project)}`} title="Delete pointer" onClick={() => removePointer(pointer.id)}><Icon name="trash" size={13} /></button>
        </li>)}
      </ul>
    </li>}
  </ul>;
}
