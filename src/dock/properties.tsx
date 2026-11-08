import { TEMPLATE_DEFAULTS } from '../generated/limits';
import { useEffect, useRef, useState, type FormEvent } from 'react';
import { Icon } from '../icons';
import { CompactNumberField, CompactTextField } from '../CompactField';
import { useDraftText } from '../DraftText';
import { pointerBroken, pointerEndpointName } from '../projectCanvasGeometry';
import type { BoundaryKind, Page, PageSize, PendingRequest, Pointer, PointerStateView, Project, ProjectLocationView, ReferenceItem, RequestAttachment, SizePreset } from '../types';
import { ColourField } from '../projectCanvas/ColourField';
import { SIZE_PRESETS } from '../projectCanvas/sizes';
import { requestAreaLabel, requestTargetLabel } from '../projectCanvas/requests';
import { behaviourStateLabel } from '../projectCanvas/behaviours';
import { PROJECT_NAME_MAX_LENGTH } from '../editorShared';
import { PropsSection } from './sections';
import { PropertyField } from './controls';
import { MAX_PROMPT_CHARS, MAX_VIEWPORT_SIDE } from '../generated/limits';

function wholeSize(draft: string, min: number, max: number) {
  const value = Number(draft);
  return Number.isInteger(value) && value >= min && value <= max;
}

export function BoundaryProperties({ page, commit, updateSize, adaptSize, addSize, removeSize, remove, busy, selectSize, selectedSizeId, widthPreview, previewWidth, behaviour }: {
  page: Page;
  selectSize: (sizeId: string) => void;
  selectedSizeId?: string | null;
  widthPreview?: { sizeId: string; width: number } | null;
  previewWidth?: (value: { sizeId: string; width: number } | null) => void;
  commit: (patch: { name?: string; path?: string; background?: string; included?: boolean }) => void;
  adaptSize: (sizeId: string) => void;
  updateSize: (sizeId: string, patch: { label?: string; width?: number; height?: number; kind?: BoundaryKind }) => void;
  addSize: (preset: SizePreset, width?: number, height?: number) => void;
  removeSize: (size: PageSize) => void;
  remove: () => void;
  busy: boolean;
  behaviour?: BehaviourProperty;
}) {
  // People make slides, pages and images; 'boundary' is the app's word, not theirs.
  const kindWord = page.output === 'slide' ? 'Slide' : page.output === 'image' ? 'Image' : 'Page';
  const [customWidth, setCustomWidth] = useState('1280');
  const [customHeight, setCustomHeight] = useState('800');
  const [customOpen, setCustomOpen] = useState(false);
  const customValid = wholeSize(customWidth, 1, MAX_VIEWPORT_SIDE) && wholeSize(customHeight, 1, MAX_VIEWPORT_SIDE);
  const presets = page.output === 'slide'
    ? [{ label: '16:9', width: TEMPLATE_DEFAULTS.slideDeck.width, height: TEMPLATE_DEFAULTS.slideDeck.height }, { label: '4:3', width: 1024, height: 768 }]
    : page.output === 'image' ? [{ label: 'Square', width: TEMPLATE_DEFAULTS.socialImage.width, height: TEMPLATE_DEFAULTS.socialImage.height }, { label: 'Portrait', width: 1080, height: 1350 }, { label: 'Story', width: 1080, height: 1920 }, { label: 'Landscape', width: 1200, height: 630 }] : null;
  const labelForSize = (size: PageSize) => presets
    ? presets.find(preset => preset.width === size.width && preset.height === size.height)?.label ?? `${size.width} × ${size.height}`
    : size.label;
  const hasPath = page.output === 'webpage';
  const selectedSize = page.sizes.find(size => size.id === selectedSizeId) ?? page.sizes[0];
  const minWidth = Math.min(...page.sizes.map(size => size.width));
  const maxWidth = Math.max(...page.sizes.map(size => size.width));
  const previewing = widthPreview?.sizeId === selectedSize.id;
  const shownWidth = previewing ? widthPreview.width : selectedSize.width;
  return <div className="dock-form props boundary-props">
    <div className="dock-layer-title"><strong>{kindWord}</strong></div>
    <PropsSection id="boundary-layout" title="Layout">
      <CompactTextField label="Name" placeholder="Name" value={page.name} disabled={busy} valid={name => name.trim().length > 0} commit={name => commit({ name: name.trim() })} />
      {hasPath && <CompactTextField prefix="URL" label="Path" placeholder="/path" value={page.path} disabled={busy} valid={path => path.trim().startsWith('/')} commit={path => commit({ path: path.trim() })} />}
    </PropsSection>
    {behaviour && <PropsSection id="props-behaviour" title="Behaviour"><BehaviourField {...behaviour} /></PropsSection>}
    {hasPath && minWidth < maxWidth && previewWidth && <PropsSection id="boundary-preview" title="Responsive preview">
      <div className="boundary-preview-heading"><label htmlFor={`preview-width-${selectedSize.id}`}>Width</label><output htmlFor={`preview-width-${selectedSize.id}`}>{shownWidth} px</output>{previewing && <button type="button" className="dock-action" onClick={() => previewWidth(null)}>Reset</button>}</div>
      <input id={`preview-width-${selectedSize.id}`} className="props-range boundary-preview-range" type="range" aria-label="Preview width" min={minWidth} max={maxWidth} step={1} value={shownWidth} disabled={busy}
        onChange={event => previewWidth({ sizeId: selectedSize.id, width: Number(event.target.value) })}
        onKeyDown={event => { event.stopPropagation(); if (event.key === 'Escape') { event.preventDefault(); previewWidth(null); } }} />
    </PropsSection>}
    <PropsSection id="boundary-background" title="Background">
      <ColourField label="Background" value={page.background} disabled={busy} optional={false} commit={background => commit({ background })} />
      <div className="boundary-background-presets">{[['#ffffff', 'White'], ['#f7f7f5', 'Off-white'], ['#e9e9e9', 'Light grey'], ['#1f1f1f', 'Dark grey'], ['#121212', 'Near-black'], ['#000000', 'Black']].map(([colour, name]) =>
        <button key={colour} type="button" className="props-swatch" style={{ background: colour }} aria-label={`${name} background`} aria-pressed={page.background === colour} disabled={busy} onClick={() => commit({ background: colour })} />)}</div>
    </PropsSection>
    <PropsSection id="boundary-sizes" title="Sizes" group add={() => setCustomOpen(open => !open)} addLabel={presets ? 'Add custom size' : 'Add a linked size'} addExpanded={customOpen}>
      {page.sizes.map((size, index) => <div key={size.id} className="dock-size-row" data-size-row={size.id}>
        {index === 0
          ? page.sizes.length > 1 && <span className="dock-size-tag">Base</span>
          : <span className="dock-size-name" title={labelForSize(size)}>{labelForSize(size)}</span>}
        <CompactNumberField prefix="W" label={`Width of ${labelForSize(size)}`} min={1} max={MAX_VIEWPORT_SIDE} unit="px" value={size.width} disabled={busy} valid={width => wholeSize(width, 1, MAX_VIEWPORT_SIDE)}
          commit={width => { const value = Math.round(Number(width)); if (value >= 1 && value <= MAX_VIEWPORT_SIDE) updateSize(size.id, { width: value }); }} />
        <CompactNumberField prefix="H" label={`Height of ${labelForSize(size)}`} min={1} max={MAX_VIEWPORT_SIDE} unit="px" value={size.height} disabled={busy} valid={height => wholeSize(height, 1, MAX_VIEWPORT_SIDE)}
          commit={height => { const value = Math.round(Number(height)); if (value >= 1 && value <= MAX_VIEWPORT_SIDE) updateSize(size.id, { height: value }); }} />
        <div className="dock-size-tail">
          {!presets && <select className="dock-kind" value={size.kind} disabled={busy} title="Height behaviour" aria-label={`Height behaviour of ${labelForSize(size)}`} onChange={event => updateSize(size.id, { kind: event.target.value as BoundaryKind })}>
            <option value="page">Grow</option><option value="screen">Fixed</option>
          </select>}
          {index > 0 && <button type="button" className="icon-button sm dock-row-toggle" aria-label={`Remove ${labelForSize(size)} size`} title={`Remove ${labelForSize(size)} size`} disabled={busy} onClick={() => removeSize(size)}><Icon name="trash" size={13} /></button>}
        </div>
      </div>)}
      {selectedSize.width < page.sizes[0].width && <button type="button" className="dock-action" disabled={busy} title={`Adapt ${labelForSize(selectedSize)} layout`} onClick={() => adaptSize(selectedSize.id)}>Adapt layout</button>}
      <div className="dock-size-add">
        <div className="boundary-add-row">
          {presets ? presets.map(preset => <button key={preset.label} type="button" className="dock-action" disabled={busy} title={`${preset.width} × ${preset.height}`}
            aria-pressed={selectedSize.width === preset.width && selectedSize.height === preset.height}
            onClick={() => updateSize(page.sizes[0].id, preset)}>{preset.label}</button>)
            : SIZE_PRESETS.map(preset => {
              const size = page.sizes.find(size => size.label.toLowerCase() === preset.label.toLowerCase() || size.width === preset.width);
              return <button key={preset.preset} type="button" className="dock-action" disabled={busy} title={`${preset.width} × ${preset.height}`}
                aria-pressed={selectedSize.label.toLowerCase() === preset.label.toLowerCase() || selectedSize.width === preset.width}
                onClick={() => size ? selectSize(size.id) : addSize(preset.preset)}>{preset.label}</button>;
            })}
        </div>
        {customOpen && !presets && <div className="boundary-add-row">{SIZE_PRESETS.map(preset => <button key={preset.preset} type="button" className="dock-action" disabled={busy}
          title={`${preset.width} × ${preset.height}`} onClick={() => { addSize(preset.preset); setCustomOpen(false); }}>Add {preset.label}</button>)}</div>}
        {customOpen && <div className="boundary-custom-row">
          <CompactNumberField prefix="W" label="Custom width" min={1} max={MAX_VIEWPORT_SIDE} unit="px" value={customWidth} disabled={busy} valid={width => wholeSize(width, 1, MAX_VIEWPORT_SIDE)} commit={setCustomWidth} />
          <CompactNumberField prefix="H" label="Custom height" min={1} max={MAX_VIEWPORT_SIDE} unit="px" value={customHeight} disabled={busy} valid={height => wholeSize(height, 1, MAX_VIEWPORT_SIDE)} commit={setCustomHeight} />
          <button type="button" className="dock-action" aria-label="Add custom size" title="Add custom size" disabled={busy || !customValid} onClick={() => addSize('custom', Math.round(Number(customWidth)), Math.round(Number(customHeight)))}>Add</button>
        </div>}
      </div>
    </PropsSection>
    <div className="boundary-footer">
      <button type="button" className="dock-action danger" disabled={busy} onClick={remove}>Delete {kindWord.toLowerCase()}</button>
    </div>
  </div>;
}

const validHttpUrl = (value: string) => {
  try { const url = new URL(value.trim()); return (url.protocol === 'http:' || url.protocol === 'https:') && !!url.hostname; } catch { return false; }
};

export function ReferenceProperties({ item, update, remove, busy }: {
  item: ReferenceItem; update: (patch: { text?: string; url?: string; title?: string; color?: string }) => void;
  remove: () => void; busy: boolean;
}) {
  const noteText = item.kind.kind === 'note' ? item.kind.text : '';
  const [draft, changeDraft, flushDraft] = useDraftText(noteText, (_value, latest) => Promise.resolve(update({ text: latest() })).then(() => latest()), 400);
  return <div className="dock-form">
    {item.kind.kind === 'note' && <label className="dock-field">Note
      <textarea value={draft} rows={4} maxLength={MAX_PROMPT_CHARS} placeholder="Reference note"
        onChange={event => changeDraft(event.target.value)} onBlur={flushDraft}
        onKeyDown={event => { if (event.key === 'Escape') { event.preventDefault(); flushDraft(); event.currentTarget.blur(); } }} />
    </label>}
    {item.kind.kind === 'link' && <>
      <PropertyField label="Address" value={item.kind.url} disabled={busy} valid={validHttpUrl} commit={url => update({ url: url.trim() })} />
      <PropertyField label="Title" value={item.kind.title} disabled={busy} commit={title => update({ title })} />
    </>}
    {item.kind.kind === 'swatch' && <ColourField label="Colour" value={item.kind.color} disabled={busy} optional={false} commit={color => update({ color })} />}
    {item.kind.kind === 'image' && <div className="dock-field">Picture<input value={item.kind.name ?? ''} readOnly /></div>}
    <button type="button" className="dock-action" disabled={busy} onClick={remove}>Remove</button>
  </div>;
}

export function ProjectProperties({ project, rename, setGrid, setGridSettings, busy, save }: {
  project: Project; rename: (name: string) => void;
  setGrid: (enabled: boolean) => void; setGridSettings: (patch: { snap?: boolean }) => void;
  busy: boolean; save?: () => void;
}) {
  return <div className="dock-form">
    <PropertyField label="Project" ariaLabel="Project name" maxLength={PROJECT_NAME_MAX_LENGTH} value={project.name} disabled={busy}
      valid={name => name.trim().length > 0} commit={name => rename(name.trim())} />
    {save && <button type="button" className="dock-action" disabled={busy} onClick={save}>Save project…</button>}
    <div className="dock-checks">
      <label className="check-label"><input type="checkbox" checked={project.showGrid} disabled={busy} onChange={event => setGrid(event.target.checked)} />Dot grid</label>
      <label className="check-label"><input type="checkbox" checked={project.gridSettings.snap} disabled={busy} onChange={event => setGridSettings({ snap: event.target.checked })} />Snap</label>
    </div>
  </div>;
}

export function UntitledSaveForm({ project, location, busy, chooseLocation, save, cancel }: {
  project: Project; location: ProjectLocationView | null; busy: boolean;
  chooseLocation: (name: string) => Promise<string | null>;
  save: (name: string, path: string | null) => Promise<void>;
  cancel: () => void;
}) {
  const [name, setName] = useState(project.name);
  const [path, setPath] = useState<string | null>(null);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState('');
  const locked = busy || working;
  const fail = (reason: unknown) => setError(reason instanceof Error ? reason.message : String(reason));
  async function choose() {
    if (locked) return;
    setError('');
    try {
      const picked = await chooseLocation(name.trim() || project.name);
      if (picked) setPath(picked);
    } catch (reason) { fail(reason); }
  }
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (locked || !name.trim()) return;
    setWorking(true); setError('');
    try {
      // Without a chosen file, the project goes to the default location shown above.
      await save(name.trim(), path);
    } catch (reason) { fail(reason); }
    finally { setWorking(false); }
  }
  return <form className="dialog-body dock-form untitled-save" aria-label="Save untitled project" onSubmit={submit}>
    <label className="dock-field">Project name
      <input autoFocus required maxLength={PROJECT_NAME_MAX_LENGTH} value={name} disabled={locked} autoComplete="off"
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

export function RequestProperties({ request, project, busy, update, remove, attach }: {
  request: PendingRequest; project: Project; busy: boolean;
  update: (request: PendingRequest) => void; remove: () => void; attach: (requestIds: string[], itemIds: string[]) => void;
}) {
  const [draft, changeDraft, flushDraft] = useDraftText(request.text, (_value, latest) => { update({ ...request, text: latest() }); return Promise.resolve(latest()); }, 400);
  const attachable = project.reference.filter(item => !request.attachments.some(attachment => attachment.text && item.kind.kind === 'note' && attachment.text === item.kind.text));
  return <div className="dock-form">
    <div className="dock-layer-title"><strong>Request</strong></div>
    <label className="dock-field">What to make or change
      <textarea value={draft} rows={4} maxLength={MAX_PROMPT_CHARS} placeholder="Describe what should happen here"
        onChange={event => changeDraft(event.target.value)} onBlur={flushDraft}
        onKeyDown={event => { if (event.key === 'Escape') { event.preventDefault(); flushDraft(); event.currentTarget.blur(); } }} />
    </label>
    <div className="dock-field"><span className="dock-field-label">Targets</span>
      {request.targets.length ? <ul className="dock-request-targets">{request.targets.map((target, index) => <li key={`${target.kind}:${index}`}>{target.kind === 'element' ? 'Layer · ' : target.kind === 'boundary' ? 'Boundary · ' : target.kind === 'reference' ? 'Reference · ' : 'Pointer · '}{requestTargetLabel(project, target)}</li>)}</ul>
        : <p className="dock-hint">No targets: this request makes new reference outside every boundary.</p>}
    </div>
    <p className="dock-hint">{requestAreaLabel(request.area ?? null)}</p>
    {attachable.length > 0 && <div className="dock-field"><span className="dock-field-label">Attach reference</span>
      <ul className="dock-request-targets">{attachable.map(item => <li key={item.id}><button type="button" className="dock-action" disabled={busy} onClick={() => attach([request.id], [item.id])}>Attach {requestTargetLabel(project, { kind: 'reference', id: item.id })}</button></li>)}</ul>
    </div>}
    {request.attachments.length > 0 && <div className="dock-field"><span className="dock-field-label">Attachments</span>
      <ul className="dock-history-attachments">{request.attachments.map((attachment: RequestAttachment) => <li key={attachment.id}>{attachment.name}</li>)}</ul>
    </div>}
    <button type="button" className="dock-action danger" disabled={busy} onClick={remove}>Delete request</button>
  </div>;
}

export type PointerClashActions = {
  keepComment: () => void; keepCode: () => void; keepThis: () => void; merge: () => void; reattach: () => void; remove: () => void;
};

export type BehaviourProperty = {
  project: Project; pointer: Pointer | null; state: PointerStateView | null;
  commit: (comment: string) => void; focusToken: number | null;
  busy: boolean; aiConnected: boolean; actions?: PointerClashActions;
};

function PointerClashNote({ pointer, project, state, busy, aiConnected, actions }: {
  pointer: Pointer; project: Project; state: PointerStateView | null; busy: boolean; aiConnected: boolean; actions: PointerClashActions;
}) {
  if (pointerBroken(pointer, project)) return <div className="dock-pointer-broken" role="group" aria-label="Broken pointer">
    <p>Its {pointerEndpointName(pointer.from, project, 'source').startsWith('Missing') ? 'start' : 'target'} no longer exists.</p>
    <div className="form-actions">
      <button type="button" className="dock-action" disabled={busy} onClick={actions.reattach}>Reattach</button>
      <button type="button" className="dock-action danger" disabled={busy} onClick={actions.remove}>Remove</button>
    </div>
  </div>;
  if (state?.state === 'codeChanged') return <div className="dock-pointer-clash" role="group" aria-label="Exported code changed">
    <p>Its exported code changed outside the pointer.</p>
    <div className="form-actions">
      <button type="button" className="dock-action" disabled={busy} onClick={actions.keepComment}>Keep comment</button>
      <button type="button" className="dock-action" disabled={busy || !aiConnected} title={aiConnected ? 'Ask the AI to rewrite the comment' : 'Connect AI first'} onClick={actions.keepCode}>Keep code</button>
    </div>
    {!aiConnected && <p className="muted">Connect AI to rewrite the comment from the code.</p>}
  </div>;
  if (state?.state === 'contradicts') return <div className="dock-pointer-clash" role="group" aria-label="Contradicting pointer">
    <p>Another pointer from the same source says something different.</p>
    <div className="form-actions">
      <button type="button" className="dock-action" disabled={busy} onClick={actions.keepThis}>Keep this one</button>
      <button type="button" className="dock-action" disabled={busy} onClick={actions.merge}>Merge</button>
    </div>
  </div>;
  return null;
}

export function BehaviourField({ project, pointer, state, commit, focusToken, busy, aiConnected, actions }: BehaviourProperty) {
  const saved = pointer?.comment ?? '';
  const [draft, setDraft] = useState(saved);
  const field = useRef<HTMLTextAreaElement>(null);
  useEffect(() => { setDraft(saved); }, [saved, pointer?.id]);
  useEffect(() => { if (focusToken !== null) field.current?.focus({ preventScroll: true }); }, [focusToken]);
  function save() {
    const text = draft.trim();
    if (text === saved.trim()) { setDraft(saved); return; }
    commit(text);
  }
  const clash = !!pointer && (pointerBroken(pointer, project) || state?.state === 'codeChanged' || state?.state === 'contradicts');
  return <>
    <label className="dock-field">
      <textarea ref={field} data-behaviour-field aria-label="Behaviour" value={draft} rows={3} maxLength={MAX_PROMPT_CHARS} disabled={busy}
        placeholder="Describe what it does, like follows the mouse"
        onChange={event => setDraft(event.target.value)}
        onBlur={save}
        onKeyDown={event => {
          if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) { event.preventDefault(); save(); }
          else if (event.key === 'Escape') { event.preventDefault(); setDraft(saved); event.currentTarget.blur(); }
        }} />
    </label>
    {pointer && (clash && actions
      ? <PointerClashNote pointer={pointer} project={project} state={state} busy={busy} aiConnected={aiConnected} actions={actions} />
      : <p className="dock-pointer-note">{behaviourStateLabel(state?.state ?? (pointer.wiring ? 'wired' : 'unwired'))}</p>)}
  </>;
}

export function PointerProperties({ pointer, project, state, change, aiConnected, busy, actions }: {
  pointer: Pointer; project: Project; state: PointerStateView | null; change: (comment: string) => Promise<unknown>;
  aiConnected: boolean; busy: boolean; actions: PointerClashActions;
}) {
  const broken = pointerBroken(pointer, project);
  const [draft, changeDraft, flushDraft] = useDraftText(pointer.comment, (_value, latest) => Promise.resolve(change(latest())).then(() => latest()), 400);
  return <div className="dock-form">
    <dl className="dock-pointer-endpoints">
      <div><dt>From</dt><dd>{pointerEndpointName(pointer.from, project, 'source')}</dd></div>
      <div><dt>To</dt><dd>{pointerEndpointName(pointer.to, project, 'target')}</dd></div>
    </dl>
    <label className="dock-field">Comment
      <textarea value={draft} rows={4} maxLength={MAX_PROMPT_CHARS} placeholder="What should this pointer do?"
        onChange={event => changeDraft(event.target.value)} onBlur={flushDraft}
        onKeyDown={event => { if (event.key === 'Escape') { event.preventDefault(); flushDraft(); event.currentTarget.blur(); } }} />
    </label>
    {!broken && state?.state === 'needsGenerating' && <p className="dock-pointer-note">Needs generating.</p>}
    {!broken && state?.state === 'needsExporting' && <p className="dock-pointer-note">Needs exporting. The next export rebuilds its code.</p>}
    {!broken && state?.state === 'wired' && <p className="dock-pointer-note">Wired to its generated code.</p>}
    <PointerClashNote pointer={pointer} project={project} state={state} busy={busy} aiConnected={aiConnected} actions={actions} />
  </div>;
}
