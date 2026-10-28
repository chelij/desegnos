import { useEffect, useMemo, useRef, useState } from 'react';
import { CodeEditor } from '../CodeEditor';
import { CompactNumberField, CompactTextField } from '../CompactField';
import { ChoiceRow, PropsSection } from '../EditorDock';
import { Icon } from '../icons';
import { projectCodeSource } from '../editorHtml';
import type { Element, ElementLayout, ElementPatch, ElementStyle, InputType, Page, PendingRequest, Project, RequestAttachment } from '../types';
import { commonValue, elementKindLabel, inputTypes, isTextKind, pictureName, pictureUri as projectPictureUri, rememberColour } from './elementGeometry';

const validColour = (value: string) => !value.trim() || /^#([0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/i.test(value.trim()) || value.trim().toLowerCase() === 'transparent';
const validNumber = (value: string) => Number.isFinite(Number(value));
const validUrl = (value: string) => {
  try { const url = new URL(value.trim()); return (url.protocol === 'http:' || url.protocol === 'https:') && !!url.hostname; } catch { return false; }
};

function stylePatches(elements: Element[], change: (style: ElementStyle) => void): ElementPatch[] {
  return elements.map(element => {
    const style = structuredClone(element.style);
    change(style);
    return { id: element.id, style };
  });
}

function useDraft(value: string) {
  const [draft, setDraft] = useState(value);
  const cancelled = useRef(false);
  useEffect(() => { setDraft(value); }, [value]);
  const settle = (commit: (draft: string) => void, valid?: (draft: string) => boolean) => {
    if (cancelled.current) { cancelled.current = false; setDraft(value); return; }
    if (draft === value) return;
    if (!valid || valid(draft)) commit(draft); else setDraft(value);
  };
  return { draft, setDraft, settle, cancel: () => { cancelled.current = true; setDraft(value); } };
}

function keyDown(settle: () => void, cancel: () => void) {
  return (event: React.KeyboardEvent<HTMLInputElement | HTMLTextAreaElement>) => {
    if (event.key === 'Enter' && !(event.currentTarget instanceof HTMLTextAreaElement && event.shiftKey)) {
      event.preventDefault(); settle(); event.currentTarget.blur();
    } else if (event.key === 'Escape') {
      event.preventDefault(); cancel(); event.currentTarget.blur();
    }
  };
}

/** Colour swatch plus hex text in one row; an empty value means no colour. */
function ColourField({ label, value, commit, fallback = '#ffffff', disabled }: {
  label: string; value: string; commit: (value: string) => void; fallback?: string; disabled?: boolean;
}) {
  const state = useDraft(value);
  const trimmed = value.trim();
  const hex = /^#[0-9a-f]{3}$/i.test(trimmed) ? `#${trimmed.slice(1).split('').map(part => part + part).join('')}` : trimmed;
  const picker = /^#[0-9a-f]{6}$/i.test(hex) ? hex : fallback;
  return <div className="props-colour">
    <label className={`props-swatch${trimmed ? '' : ' is-empty'}`} title={`${label} colour`} style={trimmed ? { background: trimmed } : undefined}>
      <input type="color" value={picker} disabled={disabled} aria-label={`${label} colour`} onChange={event => commit(event.target.value)} />
    </label>
    <input className="props-hex" type="text" aria-label={label} placeholder="None" spellCheck={false}
      value={state.draft} disabled={disabled} onChange={event => state.setDraft(event.target.value)}
      onBlur={() => state.settle(commit, validColour)} onKeyDown={keyDown(() => state.settle(commit, validColour), state.cancel)} />
  </div>;
}

/** Opacity reads as a percentage, stays stored as 0-1, and previews on a slim slider beside it. */
function OpacityField({ value, disabled, commit }: { value: number | undefined; disabled: boolean; commit: (percent: number) => void }) {
  const percent = value === undefined ? '' : Math.round(value * 100);
  const [preview, setPreview] = useState<number | null>(null);
  useEffect(() => { setPreview(null); }, [percent]);
  const shown = preview ?? percent;
  function settle() {
    if (preview === null) return;
    setPreview(null);
    commit(preview);
  }
  return <div className="props-opacity">
    <CompactNumberField prefix="O" label="Opacity" min={0} max={100} unit="%" value={shown} disabled={disabled}
      valid={draft => Number(draft) >= 0 && Number(draft) <= 100}
      commit={draft => commit(Math.min(100, Math.max(0, Number(draft))))} />
    <input className="props-range" type="range" aria-label="Opacity slider" min={0} max={100} step={1} disabled={disabled}
      value={typeof shown === 'number' ? shown : 0}
      onChange={event => setPreview(Number(event.target.value))}
      onPointerUp={settle} onPointerCancel={settle} onKeyUp={settle} onBlur={settle} />
  </div>;
}

function LongTextField({ label, placeholder, value, commit, rows }: { label: string; placeholder?: string; value: string; commit: (value: string) => void; rows?: number }) {
  const state = useDraft(value);
  return <textarea className="props-textarea" value={state.draft} rows={rows ?? 3} maxLength={16384} aria-label={label} placeholder={placeholder ?? label}
    onChange={event => state.setDraft(event.target.value)}
    onBlur={() => state.settle(commit)}
    onKeyDown={keyDown(() => state.settle(commit), state.cancel)} />;
}

function CodeField({ language, source, commit }: { language: string; source: string; commit: (source: string) => void }) {
  const [text, setText] = useState(source);
  useEffect(() => { setText(source); }, [source]);
  const projection = useMemo(() => projectCodeSource(text), [text]);
  const tab = language === 'css' ? 'css' : language === 'html' ? 'html' : 'javascript';
  return <div className="dock-code">
    <CodeEditor key={tab} language={tab} label="Code block" text={text} projection={projection} hiddenMetadata={false} readOnly={false} active
      marks={[]} query="" focusLine={null} change={value => setText(value)} cursor={() => {}} flush={() => { if (text !== source) commit(text); }}
      saveProject={() => {}} find={() => {}} />
  </div>;
}

function RequestArea({ element, project, request, setRequest, generate, error }: {
  element: Element; project: Project; request: PendingRequest | null;
  setRequest: (data: { text: string; attachments: RequestAttachment[] } | null) => void;
  generate: () => void; error: (message: string) => void;
}) {
  const files = useRef<HTMLInputElement>(null);
  const [prompt, setPrompt] = useState(request?.text ?? '');
  useEffect(() => { setPrompt(request?.text ?? ''); }, [request?.text, element.id]);
  const save = (patch: { text?: string; attachments?: RequestAttachment[] }) => {
    const text = patch.text ?? prompt;
    const attachments = patch.attachments ?? request?.attachments ?? [];
    if (!text.trim() && !attachments.length) { setRequest(null); return; }
    setRequest({ text, attachments });
  };
  function commit() { save({ text: prompt }); }
  async function askAi() {
    const text = prompt.trim();
    if (!text) return;
    try { save({ text }); } catch (reason) { error(String(reason)); return; }
    generate();
  }
  function add(attachments: RequestAttachment[]) {
    const next = [...(request?.attachments ?? [])];
    for (const attachment of attachments) {
      if (next.length >= 32) { error('A request supports at most 32 attachments.'); break; }
      next.push(attachment);
    }
    save({ attachments: next });
  }
  async function attach(files: File[]) {
    const attachments: RequestAttachment[] = [];
    for (const file of files) {
      const id = crypto.randomUUID().replace(/-/g, '').slice(0, 16);
      if (file.type.startsWith('image/')) {
        const dataUri = await new Promise<string>((resolve, reject) => {
          const reader = new FileReader();
          reader.onload = () => resolve(String(reader.result));
          reader.onerror = () => reject(new Error(`${file.name} could not be read.`));
          reader.readAsDataURL(file);
        });
        attachments.push({ id, name: file.name.slice(0, 120), kind: 'image', dataUri });
      } else if (file.type.startsWith('text/') || /\.(txt|md|csv|json|html?)$/i.test(file.name)) {
        const text = await file.text();
        if (text.trim()) attachments.push({ id, name: file.name.slice(0, 120), kind: 'text', text: text.slice(0, 65536) });
      } else error(`${file.name} is not a picture or a text file.`);
    }
    add(attachments);
  }
  function drop(event: React.DragEvent<HTMLDivElement>) {
    if (event.dataTransfer.files.length) { event.preventDefault(); void attach(Array.from(event.dataTransfer.files)); return; }
    const text = (event.dataTransfer.getData('text/uri-list') || event.dataTransfer.getData('text/plain') || '').trim();
    if (!text || !validUrl(text)) return;
    event.preventDefault();
    add([{ id: crypto.randomUUID().replace(/-/g, '').slice(0, 16), name: new URL(text).hostname.slice(0, 120), kind: 'link', url: text }]);
  }
  return <div className="dock-form dock-layer-meta">
    <label className="dock-field">Ask AI on this layer
      <textarea data-element-request value={prompt} rows={3} maxLength={16384} placeholder="Describe a change"
        onChange={event => setPrompt(event.target.value)}
        onBlur={commit}
        onKeyDown={event => { if (event.key === 'Enter' && !event.shiftKey) { event.preventDefault(); askAi(); } }} />
    </label>
    <button type="button" className="dock-action" disabled={!prompt.trim()} onClick={() => void askAi()}>Ask AI</button>
    {element.meta.requests.length > 0 && <div className="dock-history">
      <span className="dock-field-label">Past requests</span>
      {[...element.meta.requests].reverse().map(item => <details key={item.id} className="dock-history-entry">
        <summary title={item.prompt}>{item.prompt.split('\n')[0]}</summary>
        <p>{item.prompt}</p>
        {item.outcome && <p className="dock-hint">{item.outcome}</p>}
        {item.attachments.length > 0 && <ul className="dock-history-attachments">{item.attachments.map(attachment => <li key={attachment.id}>{attachment.name}</li>)}</ul>}
      </details>)}
    </div>}
    <div className="dock-context" onDragOver={event => event.preventDefault()} onDrop={drop}>
      <div className="dock-attachment-head"><span>Attachments</span>
        <button type="button" className="icon-button sm" aria-label="Attach files" title="Attach pictures or text files" onClick={() => files.current?.click()}><Icon name="paperclip" /></button>
      </div>
      <input ref={files} type="file" hidden multiple accept="image/png,image/jpeg,image/webp,image/gif,.txt,.md,.csv,.json,.html" onChange={event => { const chosen = Array.from(event.target.files ?? []); event.target.value = ''; if (chosen.length) void attach(chosen); }} />
      {(request?.attachments.length ?? 0) > 0 ? <ul className="request-attachments">{request!.attachments.map(attachment => <li key={attachment.id} title={attachment.kind === 'link' ? attachment.url : attachment.name}>
        {attachment.kind === 'image' && attachment.dataUri ? <img src={attachment.dataUri} alt="" /> : <span className="attachment-icon" aria-hidden="true"><Icon name={attachment.kind === 'link' ? 'link' : 'text'} size={12} /></span>}
        <span className="attachment-name">{attachment.name}</span>
        <button type="button" className="icon-button sm" aria-label={`Remove ${attachment.name}`} title={`Remove ${attachment.name}`} onClick={() => save({ attachments: request!.attachments.filter(item => item.id !== attachment.id) })}><Icon name="close" /></button>
      </li>)}</ul> : <p className="dock-hint">Drop files or links here</p>}
    </div>
    <p className="dock-hint">{pictureName(project, element.image) || elementKindLabel(element)}</p>
  </div>;
}

export function ElementProperties({ project, pageId, sizeId, elements, busy, update, rename, link, unlink, canLink, canUnlink, request, setRequest, updateContext, generate, error }: {
  project: Project; page: Page; pageId: string; sizeId: string | null; elements: Element[]; busy: boolean;
  update: (patches: ElementPatch[]) => void;
  rename: (name: string | null) => Promise<void>;
  link: () => void; unlink: () => void; canLink: boolean; canUnlink: boolean;
  request: PendingRequest | null;
  setRequest: (data: { text: string; attachments: RequestAttachment[] } | null) => void;
  updateContext: (elementId: string, context: RequestAttachment[]) => void;
  generate: () => void;
  error: (message: string) => void;
}) {
  const single = elements.length === 1 ? elements[0] : null;
  // Edits in one size become that size's overrides; double-click editing changes base values
  // and clears the same fields from every size.
  const sized = <T,>(patch: T): T & { size_id?: string | null; all_sizes?: boolean } =>
    sizeId === null ? { ...patch, all_sizes: true } : { ...patch, size_id: sizeId };
  const sameKind = elements.every(element => element.kind === elements[0]?.kind);
  const kind = sameKind ? elements[0].kind : null;
  const fill = commonValue(elements.map(element => element.style.fill));
  const strokeColour = commonValue(elements.map(element => element.style.stroke.color));
  const strokeWidth = commonValue(elements.map(element => element.style.stroke.width));
  const radius = commonValue(elements.map(element => element.style.radius));
  const opacity = commonValue(elements.map(element => element.style.opacity));
  const shadow = commonValue(elements.map(element => element.style.shadow));
  const textStyle = commonValue(elements.map(element => element.style.textStyle));
  const text = commonValue(elements.map(element => element.text));
  const url = commonValue(elements.map(element => element.url));
  const layout = commonValue(elements.map(element => element.layout));
  const picture = useRef<HTMLInputElement>(null);
  const [pictureUri, setPictureUri] = useState<string | null>(null);
  const [nameError, setNameError] = useState('');
  async function commitName(name: string) {
    setNameError('');
    if (name.trim() === (single?.name ?? '')) return;
    try { await rename(name.trim() || null); }
    catch (reason) { setNameError(reason instanceof Error ? reason.message : String(reason)); }
  }

  function commitStyle(change: (style: ElementStyle) => void) { update(stylePatches(elements, change).map(patch => sized(patch))); }
  function commitRect(rect: { x?: number; y?: number; width?: number; height?: number }) {
    if (!single) return;
    update([sized({ id: single.id, rect: { ...single.rect, ...rect } })]);
  }
  function choosePicture(file: File | undefined) {
    if (!file || !single) return;
    const reader = new FileReader();
    reader.onload = () => setPictureUri(String(reader.result));
    reader.readAsDataURL(file);
  }
  useEffect(() => {
    if (!pictureUri || !single) return;
    update([{ id: single.id, picture_data_uri: pictureUri }]);
    setPictureUri(null);
  }, [pictureUri, single?.id]);
  const visual = elements.every(element => element.kind !== 'group');
  const textKind = sameKind && kind !== null && isTextKind(kind);
  const codeKind = kind === 'code';
  const mediaKind = kind === 'link' || kind === 'video' || kind === 'audio' || kind === 'embed';
  const imageKind = kind === 'image';
  // Text and lines keep their box plain until a stroke or radius is actually set.
  const showsStrokeWidth = kind === 'line' || (strokeColour ?? '').trim().length > 0 || (kind !== 'text' && (strokeWidth ?? 0) > 0);
  const showsRadius = kind !== null && kind !== 'text' && kind !== 'line' ? true : (radius ?? 0) > 0;

  return <div className="dock-form props">
    <div className="dock-layer-title"><strong>{single ? elementKindLabel(single) : `${elements.length} layers`}</strong></div>
    {single && <>
      <CompactTextField label="Name" placeholder="Name" field="layer-name" value={single.name ?? ''} disabled={busy} commit={name => void commitName(name)} />
      {nameError && <p className="dock-field-error" role="alert">{nameError}</p>}
    </>}
    {(canLink || canUnlink) && <div className="form-actions">
      {canLink && <button type="button" className="dock-action" disabled={busy} title="Link the selected layers (Ctrl/Cmd+Alt+K)" onClick={link}>Link {elements.length} layers</button>}
      {canUnlink && <button type="button" className="dock-action" disabled={busy} title="Detach this copy (Ctrl/Cmd+Alt+B)" onClick={unlink}>Unlink</button>}
    </div>}
    {(single || (sameKind && kind === 'box')) && <PropsSection id="props-layout" title="Layout">
      {single && <div className="props-grid">
        <CompactNumberField prefix="X" label="X" value={single.rect.x} disabled={busy} valid={validNumber} commit={value => commitRect({ x: Number(value) })} />
        <CompactNumberField prefix="Y" label="Y" value={single.rect.y} disabled={busy} valid={validNumber} commit={value => commitRect({ y: Number(value) })} />
        <CompactNumberField prefix="W" label="Width" min={0} value={single.rect.width} disabled={busy} valid={value => Number(value) >= 0} commit={value => commitRect({ width: Number(value) })} />
        <CompactNumberField prefix="H" label="Height" min={0} value={single.rect.height} disabled={busy} valid={value => Number(value) >= 0} commit={value => commitRect({ height: Number(value) })} />
        <CompactNumberField prefix="°" label="Rotation" value={single.rotation} disabled={busy} valid={validNumber} commit={value => update([{ id: single.id, rotation: Number(value) }])} />
      </div>}
      {sameKind && kind === 'box' && <>
        <div className="props-choice"><ChoiceRow label="Flow" value={layout?.kind ?? 'free'} choices={[['free', 'Free'], ['stack', 'Stack'], ['grid', 'Grid']]} disabled={busy}
          choose={flow => update(elements.map(element => sized({ id: element.id, layout: flow === 'free' ? { kind: 'free' } : flow === 'stack'
            ? { kind: 'stack', direction: 'row', gap: 8, padding: 0, align: 'start', justify: 'start', wrap: false }
            : { kind: 'grid', columns: 2, gap: 8, padding: 0 } })))} /></div>
        {layout?.kind === 'stack' && <>
          <div className="props-choice"><ChoiceRow label="Direction" value={layout.direction} choices={[['row', 'Row'], ['column', 'Column']]} disabled={busy}
            choose={direction => update(elements.map(element => sized({ id: element.id, layout: element.layout.kind === 'free' ? element.layout : { ...element.layout, direction } })))} /></div>
          <div className="props-grid">
            <CompactNumberField prefix="G" label="Gap" min={0} unit="px" value={layout.gap} disabled={busy} valid={value => Number(value) >= 0}
              commit={value => update(elements.map(element => sized({ id: element.id, layout: element.layout.kind === 'free' ? element.layout : { ...element.layout, gap: Number(value) } })))} />
            <CompactNumberField prefix="P" label="Padding" min={0} unit="px" value={layout.padding} disabled={busy} valid={value => Number(value) >= 0}
              commit={value => update(elements.map(element => sized({ id: element.id, layout: element.layout.kind === 'free' ? element.layout : { ...element.layout, padding: Number(value) } })))} />
          </div>
          <div className="props-choice"><ChoiceRow label="Align" value={layout.align} choices={[['start', 'Start'], ['center', 'Center'], ['end', 'End'], ['stretch', 'Stretch']]} disabled={busy}
            choose={align => update(elements.map(element => sized({ id: element.id, layout: element.layout.kind === 'free' ? element.layout : { ...element.layout, align } })))} /></div>
          <div className="props-choice"><ChoiceRow label="Justify" value={layout.justify} choices={[['start', 'Start'], ['center', 'Center'], ['end', 'End'], ['spaceBetween', 'Between']]} disabled={busy}
            choose={justify => update(elements.map(element => sized({ id: element.id, layout: element.layout.kind === 'free' ? element.layout : { ...element.layout, justify } })))} /></div>
          <label className="check-label"><input type="checkbox" checked={layout.wrap} disabled={busy} onChange={event => update(elements.map(element => sized({ id: element.id, layout: element.layout.kind === 'free' ? element.layout : { ...element.layout, wrap: event.target.checked } })))} />Wrap</label>
        </>}
        {layout?.kind === 'grid' && <div className="props-grid">
          <CompactNumberField prefix="C" label="Columns" min={1} max={64} value={layout.columns} disabled={busy} valid={value => Number(value) >= 1 && Number(value) <= 64}
            commit={value => update(elements.map(element => sized({ id: element.id, layout: element.layout.kind === 'free' ? element.layout : { ...element.layout, columns: Number(value) } })))} />
          <CompactNumberField prefix="G" label="Gap" min={0} unit="px" value={layout.gap} disabled={busy} valid={value => Number(value) >= 0}
            commit={value => update(elements.map(element => sized({ id: element.id, layout: element.layout.kind === 'free' ? element.layout : { ...element.layout, gap: Number(value) } })))} />
          <CompactNumberField prefix="P" label="Padding" min={0} unit="px" value={layout.padding} disabled={busy} valid={value => Number(value) >= 0}
            commit={value => update(elements.map(element => sized({ id: element.id, layout: element.layout.kind === 'free' ? element.layout : { ...element.layout, padding: Number(value) } })))} />
        </div>}
      </>}
    </PropsSection>}
    {visual && <PropsSection id="props-appearance" title="Appearance">
      {kind !== 'line' && <ColourField label="Fill" value={fill ?? ''} disabled={busy} commit={value => { rememberColour('fill', value.trim() || null); commitStyle(style => { style.fill = value.trim() || null; }); }} />}
      <div className="props-stroke">
        <ColourField label="Stroke" value={strokeColour ?? ''} fallback="#000000" disabled={busy} commit={value => { rememberColour('stroke', value.trim() || null); commitStyle(style => { style.stroke.color = value.trim() || null; }); }} />
        {showsStrokeWidth && <CompactNumberField prefix={<Icon name="line-weight" size={12} />} label="Stroke width" min={0} unit="px" value={strokeWidth ?? ''} disabled={busy} valid={value => Number(value) >= 0}
          commit={value => commitStyle(style => { style.stroke.width = Number(value); })} />}
      </div>
      {showsRadius && <CompactNumberField prefix={<Icon name="corner-radius" size={12} />} label="Radius" min={0} unit="px" value={radius ?? ''} disabled={busy} valid={value => Number(value) >= 0}
        commit={value => commitStyle(style => { style.radius = Number(value); })} />}
      <OpacityField value={opacity} disabled={busy} commit={percent => commitStyle(style => { style.opacity = percent / 100; })} />
    </PropsSection>}
    {((imageKind && single) || mediaKind) && <PropsSection id="props-content" title="Content">
      {imageKind && single && <div className="dock-inline">
        {projectPictureUri(project, single.image) && <img className="dock-picture" src={projectPictureUri(project, single.image)!} alt="" />}
        <span className="dock-value">{pictureName(project, single.image)}</span>
        <button type="button" className="dock-action" disabled={busy} onClick={() => picture.current?.click()}>Replace</button>
        <input ref={picture} type="file" hidden accept="image/png,image/jpeg,image/webp,image/gif" onChange={event => { choosePicture(event.target.files?.[0]); event.target.value = ''; }} />
      </div>}
      {mediaKind && <CompactTextField label="Address" value={url ?? ''} disabled={busy} valid={validUrl} commit={value => update(elements.map(element => ({ id: element.id, url: value.trim() })))} />}
    </PropsSection>}
    {textKind && <PropsSection id="props-text" title="Text">
      <CompactTextField label="Font" value={textStyle?.fontFamily ?? ''} disabled={busy} commit={value => commitStyle(style => { style.textStyle.fontFamily = value; })} />
      <div className="props-grid">
        <CompactNumberField prefix="S" label="Size" min={1} unit="px" value={textStyle?.size ?? ''} disabled={busy} valid={value => Number(value) >= 1}
          commit={value => commitStyle(style => { style.textStyle.size = Number(value); })} />
        <CompactNumberField prefix="W" label="Weight" min={1} max={1000} value={textStyle?.weight ?? ''} disabled={busy} valid={value => Number(value) >= 1 && Number(value) <= 1000}
          commit={value => commitStyle(style => { style.textStyle.weight = Number(value); })} />
        <CompactNumberField prefix="LH" label="Line height" min={0} value={textStyle?.lineHeight ?? ''} disabled={busy} valid={value => Number(value) >= 0}
          commit={value => commitStyle(style => { style.textStyle.lineHeight = Number(value); })} />
        <CompactNumberField prefix="LS" label="Letter spacing" unit="px" value={textStyle?.letterSpacing ?? ''} disabled={busy} valid={validNumber}
          commit={value => commitStyle(style => { style.textStyle.letterSpacing = Number(value); })} />
      </div>
      <ColourField label="Text colour" value={textStyle?.color ?? ''} disabled={busy} commit={value => { rememberColour('text', value.trim() || null); commitStyle(style => { style.textStyle.color = value.trim() || '#1a1a1a'; }); }} />
      <label className="check-label"><input type="checkbox" disabled={busy} checked={!!textStyle?.italic} onChange={event => commitStyle(style => { style.textStyle.italic = event.target.checked; })} />Italic</label>
      <div className="props-choice"><ChoiceRow label="Align" value={textStyle?.align ?? 'left'} choices={[['left', 'Left'], ['center', 'Center'], ['right', 'Right']]} disabled={busy}
        choose={align => commitStyle(style => { style.textStyle.align = align; })} /></div>
      {kind === 'input' && <div className="props-choice"><ChoiceRow label="Input type" value={elements[0].inputType ?? 'text'} choices={inputTypes.map(type => [type, type[0].toUpperCase() + type.slice(1)] as [InputType, string])} disabled={busy}
        choose={input_type => update(elements.map(element => ({ id: element.id, input_type })))} /></div>}
      {kind === 'select' && <LongTextField label="Options" placeholder="One option per line" rows={5} value={(elements[0].options ?? []).join('\n')}
        commit={value => { const options = value.split('\n').map(option => option.trim()).filter(Boolean); if (options.length) update(elements.map(element => ({ id: element.id, options }))); }} />}
      <LongTextField label="Text" placeholder="Text" value={text ?? ''} commit={value => update(elements.map(element => ({ id: element.id, text: value })))} />
    </PropsSection>}
    {codeKind && single && <PropsSection id="props-code" title="Code">
      <CompactTextField label="Language" value={single.language ?? ''} disabled={busy} valid={value => value.trim().length > 0} commit={value => update([{ id: single.id, language: value.trim() }])} />
      <CodeField language={single.language ?? ''} source={single.source ?? ''} commit={source => update([{ id: single.id, source }])} />
    </PropsSection>}
    {visual && <PropsSection id="props-shadow" title="Shadow">
        <ColourField label="Shadow colour" value={shadow?.color ?? ''} fallback="#000000" disabled={busy}
          commit={value => commitStyle(style => { style.shadow.color = value.trim() || null; })} />
        <div className="props-grid">
          <CompactNumberField prefix="X" label="Shadow X" unit="px" value={shadow?.x ?? ''} disabled={busy} valid={validNumber}
            commit={value => commitStyle(style => { style.shadow.x = Number(value); })} />
          <CompactNumberField prefix="Y" label="Shadow Y" unit="px" value={shadow?.y ?? ''} disabled={busy} valid={validNumber}
            commit={value => commitStyle(style => { style.shadow.y = Number(value); })} />
          <CompactNumberField prefix="B" label="Shadow blur" min={0} unit="px" value={shadow?.blur ?? ''} disabled={busy} valid={value => Number(value) >= 0}
            commit={value => commitStyle(style => { style.shadow.blur = Number(value); })} />
        </div>
    </PropsSection>}
    {single && <RequestArea element={single} project={project} request={request} setRequest={setRequest} generate={generate} error={error} />}
    {!single && <p className="dock-hint">{elements.map(element => elementKindLabel(element)).filter((label, index, all) => all.indexOf(label) === index).join(', ')}</p>}
  </div>;
}

