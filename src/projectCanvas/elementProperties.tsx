import { useEffect, useMemo, useRef, useState } from 'react';
import { CodeEditor } from '../CodeEditor';
import { ChoiceRow, PropertyField } from '../EditorDock';
import { Icon } from '../icons';
import { projectCodeSource } from '../editorHtml';
import type { Element, ElementLayout, ElementPatch, ElementStyle, InputType, Page, PendingRequest, Project, RequestAttachment } from '../types';
import { commonValue, elementKindLabel, inputTypes, isTextKind, pictureName, pictureUri as projectPictureUri } from './elementGeometry';

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

function TextEditorField({ value, commit, rows, label }: { value: string; commit: (value: string) => void; rows?: number; label: string }) {
  const [draft, setDraft] = useState(value);
  const cancelled = useRef(false);
  useEffect(() => { setDraft(value); }, [value]);
  return <label className="dock-field">{label}
    <textarea value={draft} rows={rows ?? 3} maxLength={16384}
      onChange={event => setDraft(event.target.value)}
      onBlur={() => { if (cancelled.current) { cancelled.current = false; setDraft(value); return; } if (draft !== value) commit(draft); }}
      onKeyDown={event => {
        if (event.key === 'Escape') { event.preventDefault(); cancelled.current = true; setDraft(value); event.currentTarget.blur(); }
        else if (event.key === 'Enter' && !event.shiftKey) { event.preventDefault(); if (draft !== value) commit(draft); event.currentTarget.blur(); }
      }} />
  </label>;
}

function CodeField({ language, source, commit }: { language: string; source: string; commit: (source: string) => void }) {
  const [text, setText] = useState(source);
  useEffect(() => { setText(source); }, [source]);
  const projection = useMemo(() => projectCodeSource(text), [text]);
  const tab = language === 'css' ? 'css' : language === 'html' ? 'html' : 'javascript';
  return <div className="dock-code">
    <span className="dock-field-label">Code</span>
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

export function ElementProperties({ project, page, pageId, sizeId, elements, busy, update, rename, link, unlink, canLink, canUnlink, request, setRequest, updateContext, generate, error }: {
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
  const editingAllSizes = sizeId === null;
  const sameKind = elements.every(element => element.kind === elements[0]?.kind);
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
  const linked = elements.some(element => !!element.link);
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
  const textVisual = sameKind && elements[0].kind !== 'group';
  const textKind = sameKind && isTextKind(elements[0].kind);
  const codeKind = sameKind && elements[0].kind === 'code';
  const mediaKind = sameKind && (elements[0].kind === 'link' || elements[0].kind === 'video' || elements[0].kind === 'audio' || elements[0].kind === 'embed');

  return <div className="dock-form">
    <div className="dock-layer-title"><strong>{single ? elementKindLabel(single) : `${elements.length} layers`}</strong></div>
    {page.sizes.length > 1 && <p className="dock-hint">{editingAllSizes ? 'Editing all linked sizes.' : 'Editing this size only.'}</p>}
    {single && <>
      <PropertyField label="Name" value={single.name ?? ''} disabled={busy} field="layer-name" commit={name => void commitName(name)} />
      <p className="dock-hint">{linked ? 'Every copy of this linked element changes together.' : 'A name makes this layer a linked element; its copies change together.'}</p>
      {nameError && <p className="dock-field-error" role="alert">{nameError}</p>}
    </>}
    {(canLink || canUnlink) && <div className="form-actions">
      {canLink && <button type="button" className="dock-action" disabled={busy} title="Link the selected layers (Ctrl/Cmd+Alt+K)" onClick={link}>Link {elements.length} layers</button>}
      {canUnlink && <button type="button" className="dock-action" disabled={busy} title="Detach this copy (Ctrl/Cmd+Alt+B)" onClick={unlink}>Unlink</button>}
    </div>}
    {single && <>
      <div className="dock-pair">
        <PropertyField label="X" type="number" value={single.rect.x} disabled={busy} valid={validNumber} commit={value => commitRect({ x: Number(value) })} />
        <PropertyField label="Y" type="number" value={single.rect.y} disabled={busy} valid={validNumber} commit={value => commitRect({ y: Number(value) })} />
        <PropertyField label="Width" type="number" min={0} value={single.rect.width} disabled={busy} valid={value => Number(value) >= 0} commit={value => commitRect({ width: Number(value) })} />
        <PropertyField label="Height" type="number" min={0} value={single.rect.height} disabled={busy} valid={value => Number(value) >= 0} commit={value => commitRect({ height: Number(value) })} />
      </div>
      <PropertyField label="Rotation" type="number" value={single.rotation} disabled={busy} valid={validNumber} commit={value => update([{ id: single.id, rotation: Number(value) }])} />
    </>}
    {visual && <div className="dock-pair">
      <PropertyField label="Fill" value={fill ?? ''} disabled={busy} valid={validColour} commit={value => commitStyle(style => { style.fill = value.trim() || null; })} />
      <PropertyField label="Stroke" value={strokeColour ?? ''} disabled={busy} valid={validColour} commit={value => commitStyle(style => { style.stroke.color = value.trim() || null; })} />
      <PropertyField label="Stroke width" type="number" min={0} value={strokeWidth ?? ''} disabled={busy} valid={value => Number(value) >= 0} commit={value => commitStyle(style => { style.stroke.width = Number(value); })} />
      <PropertyField label="Radius" type="number" min={0} value={radius ?? ''} disabled={busy} valid={value => Number(value) >= 0} commit={value => commitStyle(style => { style.radius = Number(value); })} />
      <PropertyField label="Opacity" type="number" min={0} max={1} value={opacity ?? ''} disabled={busy} valid={value => Number(value) >= 0 && Number(value) <= 1} commit={value => commitStyle(style => { style.opacity = Number(value); })} />
      <PropertyField label="Shadow colour" value={shadow?.color ?? ''} disabled={busy} valid={validColour} commit={value => commitStyle(style => { style.shadow.color = value.trim() || null; })} />
      <PropertyField label="Shadow X" type="number" value={shadow?.x ?? ''} disabled={busy} valid={validNumber} commit={value => commitStyle(style => { style.shadow.x = Number(value); })} />
      <PropertyField label="Shadow Y" type="number" value={shadow?.y ?? ''} disabled={busy} valid={validNumber} commit={value => commitStyle(style => { style.shadow.y = Number(value); })} />
      <PropertyField label="Shadow blur" type="number" min={0} value={shadow?.blur ?? ''} disabled={busy} valid={value => Number(value) >= 0} commit={value => commitStyle(style => { style.shadow.blur = Number(value); })} />
    </div>}
    {textVisual && <>
      <div className="dock-pair">
        <PropertyField label="Font" value={textStyle?.fontFamily ?? ''} disabled={busy} commit={value => commitStyle(style => { style.textStyle.fontFamily = value; })} />
        <PropertyField label="Size" type="number" min={1} value={textStyle?.size ?? ''} disabled={busy} valid={value => Number(value) >= 1} commit={value => commitStyle(style => { style.textStyle.size = Number(value); })} />
        <PropertyField label="Weight" type="number" min={1} max={1000} value={textStyle?.weight ?? ''} disabled={busy} valid={value => Number(value) >= 1 && Number(value) <= 1000} commit={value => commitStyle(style => { style.textStyle.weight = Number(value); })} />
        <PropertyField label="Line height" type="number" min={0} value={textStyle?.lineHeight ?? ''} disabled={busy} valid={value => Number(value) >= 0} commit={value => commitStyle(style => { style.textStyle.lineHeight = Number(value); })} />
        <PropertyField label="Letter spacing" type="number" value={textStyle?.letterSpacing ?? ''} disabled={busy} valid={validNumber} commit={value => commitStyle(style => { style.textStyle.letterSpacing = Number(value); })} />
        <PropertyField label="Text colour" value={textStyle?.color ?? ''} disabled={busy} valid={validColour} commit={value => commitStyle(style => { style.textStyle.color = value.trim() || '#1a1a1a'; })} />
      </div>
      <label className="check-label"><input type="checkbox" disabled={busy} checked={!!textStyle?.italic} onChange={event => commitStyle(style => { style.textStyle.italic = event.target.checked; })} />Italic</label>
      <ChoiceRow label="Align" value={textStyle?.align ?? 'left'} choices={[['left', 'Left'], ['center', 'Center'], ['right', 'Right']]} disabled={busy}
        choose={align => commitStyle(style => { style.textStyle.align = align; })} />
    </>}
    {textKind && sameKind && <TextEditorField label="Text" value={text ?? ''} commit={value => update(elements.map(element => ({ id: element.id, text: value })))} />}
    {sameKind && (elements[0].kind === 'image' || elements[0].kind === 'link' || elements[0].kind === 'video' || elements[0].kind === 'audio' || elements[0].kind === 'embed') && <>
      {elements[0].kind === 'image' && single && <div className="dock-field">Picture
        <div className="dock-inline">{projectPictureUri(project, single.image) && <img className="dock-picture" src={projectPictureUri(project, single.image)!} alt="" />}<span className="dock-value">{pictureName(project, single.image)}</span>
          <button type="button" className="dock-action" disabled={busy} onClick={() => picture.current?.click()}>Replace</button></div>
        <input ref={picture} type="file" hidden accept="image/png,image/jpeg,image/webp,image/gif" onChange={event => { choosePicture(event.target.files?.[0]); event.target.value = ''; }} />
      </div>}
      {mediaKind && <PropertyField label="Address" value={url ?? ''} disabled={busy} valid={validUrl} commit={value => update(elements.map(element => ({ id: element.id, url: value.trim() })))} />}
    </>}
    {sameKind && elements[0].kind === 'box' && <>
      <ChoiceRow label="Layout" value={layout?.kind ?? 'free'} choices={[['free', 'Free'], ['stack', 'Stack'], ['grid', 'Grid']]} disabled={busy}
        choose={kind => update(elements.map(element => sized({ id: element.id, layout: kind === 'free' ? { kind: 'free' } : kind === 'stack'
          ? { kind: 'stack', direction: 'row', gap: 8, padding: 0, align: 'start', justify: 'start', wrap: false }
          : { kind: 'grid', columns: 2, gap: 8, padding: 0 } })))} />
      {layout?.kind === 'stack' && <>
        <ChoiceRow label="Direction" value={layout.direction} choices={[['row', 'Row'], ['column', 'Column']]} disabled={busy}
          choose={direction => update(elements.map(element => sized({ id: element.id, layout: element.layout.kind === 'free' ? element.layout : { ...element.layout, direction } })))} />
        <div className="dock-pair">
          <PropertyField label="Gap" type="number" min={0} value={layout.gap} disabled={busy} valid={value => Number(value) >= 0}
            commit={value => update(elements.map(element => sized({ id: element.id, layout: element.layout.kind === 'free' ? element.layout : { ...element.layout, gap: Number(value) } })))} />
          <PropertyField label="Padding" type="number" min={0} value={layout.padding} disabled={busy} valid={value => Number(value) >= 0}
            commit={value => update(elements.map(element => sized({ id: element.id, layout: element.layout.kind === 'free' ? element.layout : { ...element.layout, padding: Number(value) } })))} />
        </div>
        <ChoiceRow label="Align" value={layout.align} choices={[['start', 'Start'], ['center', 'Center'], ['end', 'End'], ['stretch', 'Stretch']]} disabled={busy}
          choose={align => update(elements.map(element => sized({ id: element.id, layout: element.layout.kind === 'free' ? element.layout : { ...element.layout, align } })))} />
        <ChoiceRow label="Justify" value={layout.justify} choices={[['start', 'Start'], ['center', 'Center'], ['end', 'End'], ['spaceBetween', 'Between']]} disabled={busy}
          choose={justify => update(elements.map(element => sized({ id: element.id, layout: element.layout.kind === 'free' ? element.layout : { ...element.layout, justify } })))} />
        <label className="check-label"><input type="checkbox" checked={layout.wrap} disabled={busy} onChange={event => update(elements.map(element => sized({ id: element.id, layout: element.layout.kind === 'free' ? element.layout : { ...element.layout, wrap: event.target.checked } })))} />Wrap</label>
      </>}
      {layout?.kind === 'grid' && <div className="dock-pair">
        <PropertyField label="Columns" type="number" min={1} max={64} value={layout.columns} disabled={busy} valid={value => Number(value) >= 1 && Number(value) <= 64}
          commit={value => update(elements.map(element => sized({ id: element.id, layout: element.layout.kind === 'free' ? element.layout : { ...element.layout, columns: Number(value) } })))} />
        <PropertyField label="Gap" type="number" min={0} value={layout.gap} disabled={busy} valid={value => Number(value) >= 0}
          commit={value => update(elements.map(element => sized({ id: element.id, layout: element.layout.kind === 'free' ? element.layout : { ...element.layout, gap: Number(value) } })))} />
        <PropertyField label="Padding" type="number" min={0} value={layout.padding} disabled={busy} valid={value => Number(value) >= 0}
          commit={value => update(elements.map(element => sized({ id: element.id, layout: element.layout.kind === 'free' ? element.layout : { ...element.layout, padding: Number(value) } })))} />
      </div>}
    </>}
    {sameKind && elements[0].kind === 'input' && <ChoiceRow label="Input type" value={elements[0].inputType ?? 'text'} choices={inputTypes.map(type => [type, type[0].toUpperCase() + type.slice(1)] as [InputType, string])} disabled={busy}
      choose={input_type => update(elements.map(element => ({ id: element.id, input_type })))} />}
    {sameKind && elements[0].kind === 'select' && <TextEditorField label="Options, one per line" value={(elements[0].options ?? []).join('\n')} rows={5}
      commit={value => { const options = value.split('\n').map(option => option.trim()).filter(Boolean); if (options.length) update(elements.map(element => ({ id: element.id, options }))); }} />}
    {codeKind && single && <>
      <PropertyField label="Language" value={single.language ?? ''} disabled={busy} valid={value => value.trim().length > 0} commit={value => update([{ id: single.id, language: value.trim() }])} />
      <CodeField language={single.language ?? ''} source={single.source ?? ''} commit={source => update([{ id: single.id, source }])} />
    </>}
    {single && <RequestArea element={single} project={project} request={request} setRequest={setRequest} generate={generate} error={error} />}
    {!single && <p className="dock-hint">{elements.map(element => elementKindLabel(element)).filter((label, index, all) => all.indexOf(label) === index).join(', ')}</p>}
  </div>;
}

