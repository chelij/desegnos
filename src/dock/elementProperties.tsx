import { MAX_EDITOR_COORDINATE, MAX_GRID_COLUMNS, MAX_FONT_WEIGHT, TEXT_SIZE_RANGE, IMAGE_MIME_TYPES } from '../generated/limits';
import { linkedDefinition } from '../projectCanvas/links';
import { sizeTarget } from '../projectCanvas/canvasTargets';
import { sharedElementValues } from '../projectCanvas/sharedElementValues';
import { ColourField } from '../projectCanvas/ColourField';
import { FontField } from '../projectCanvas/FontField';
import { useCanvasFont } from '../projectCanvas/fonts';
import { ATTACHMENT_ACCEPT, readRequestAttachments } from '../requestAttachments';
import { useEffect, useMemo, useRef, useState } from 'react';
import { CodeEditor } from '../CodeEditor';
import { CompactNumberField, CompactTextField, fieldKeys, useFieldDraft } from '../CompactField';
import { normalizeAngle } from '../numberExpression';
import { ChoiceRow } from './controls';
import { PropsSection } from './sections';
import { BehaviourField, type BehaviourProperty } from './properties';
import { Icon } from '../icons';
import { projectCodeSource } from '../editorHtml';
import type { Element, ElementPadding, ElementPatch, ElementStyle, InputType, Page, PendingRequest, Project, Rect, RequestAttachment } from '../types';
import { kindFields } from '../types';
import { elementKindLabel, elementPadding, inputTypes, isTextKind, pictureName, pictureUri as projectPictureUri, rememberColour, uniformPadding } from '../projectCanvas/elementGeometry';
import { EditScopeChoice, requestEditScope } from '../projectCanvas/selectionBar';
import { MAX_PROMPT_CHARS, MAX_VIEWPORT_SIDE } from '../generated/limits';

const validNumber = (value: string) => Number.isFinite(Number(value));
const validUrl = (value: string) => {
  try { const url = new URL(value.trim()); return (url.protocol === 'http:' || url.protocol === 'https:') && !!url.hostname; } catch { return false; }
};
/** A hyperlink is a web address or a path to one of this project's boundaries. */
const validLink = (value: string) => !value.trim() || (value.trim().startsWith('/') && !value.trim().startsWith('//')) || validUrl(value);
/** An Embed needs a target: a web address or one of this project's page paths. */
const validEmbedAddress = (value: string) => !!value.trim() && validLink(value);

/** The project boundary an Embed or hyperlink names, as one shared picker. */
function ProjectPageSelect({ label, project, value, disabled, choose }: { label: string; project: Project; value: string | null | undefined; disabled: boolean; choose: (path: string) => void }) {
  return <label className="dock-field">{label}
    <select aria-label="Link to a project page" value={project.pages.some(page => page.path === value) ? value ?? '' : ''} disabled={disabled}
      onChange={event => { if (event.target.value) choose(event.target.value); }}>
      <option value="">Project page…</option>
      {project.pages.map(page => <option key={page.id} value={page.path}>{page.name} · {page.path}</option>)}
    </select>
  </label>;
}

function stylePatches(elements: Element[], change: (style: ElementStyle) => void): ElementPatch[] {
  return elements.map(element => {
    const style = structuredClone(element.style);
    change(style);
    return { id: element.id, style };
  });
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
      placeholder={value === undefined ? 'Mixed' : undefined}
      valid={draft => Number(draft) >= 0 && Number(draft) <= 100}
      commit={draft => commit(Math.min(100, Math.max(0, Number(draft))))} />
    <input className="props-range" type="range" aria-label="Opacity slider" min={0} max={100} step={1} disabled={disabled}
      value={typeof shown === 'number' ? shown : 0}
      onChange={event => setPreview(Number(event.target.value))}
      onPointerUp={settle} onPointerCancel={settle} onKeyUp={settle} onBlur={settle} />
  </div>;
}

function LongTextField({ label, placeholder, value, commit, rows }: { label: string; placeholder?: string; value: string; commit: (value: string) => void; rows?: number }) {
  const state = useFieldDraft(value, commit);
  return <textarea className="props-textarea" value={state.draft} rows={rows ?? 3} maxLength={MAX_PROMPT_CHARS} aria-label={label} placeholder={placeholder ?? label}
    onChange={event => state.setDraft(event.target.value)}
    onBlur={state.settle}
    onKeyDown={fieldKeys(state.settle, state.cancel)} />;
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

function RequestArea({ element, project, request, setRequest, error }: {
  element: Element; project: Project; request: PendingRequest | null;
  setRequest: (data: { text: string; attachments: RequestAttachment[] } | null) => void;
  error: (message: string) => void;
}) {
  const files = useRef<HTMLInputElement>(null);
  const save = (attachments: RequestAttachment[]) => {
    const text = request?.text ?? '';
    if (!text.trim() && !attachments.length) { setRequest(null); return; }
    setRequest({ text, attachments });
  };
  async function attach(files: File[], links: string[] = []) {
    const result = await readRequestAttachments(request?.attachments ?? [], files, links);
    save(result.attachments);
    if (result.errors.length) error(result.errors.join(' '));
  }
  function drop(event: React.DragEvent<HTMLDivElement>) {
    if (event.dataTransfer.files.length) { event.preventDefault(); void attach(Array.from(event.dataTransfer.files)); return; }
    const text = (event.dataTransfer.getData('text/uri-list') || event.dataTransfer.getData('text/plain') || '').trim();
    if (!text || !validUrl(text)) return;
    event.preventDefault();
    void attach([], [text]);
  }
  return <div className="dock-form dock-layer-meta">
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
      <input ref={files} type="file" hidden multiple accept={ATTACHMENT_ACCEPT} onChange={event => { const chosen = Array.from(event.target.files ?? []); event.target.value = ''; if (chosen.length) void attach(chosen); }} />
      {(request?.attachments.length ?? 0) > 0 ? <ul className="request-attachments">{request!.attachments.map(attachment => <li key={attachment.id} title={attachment.kind === 'link' ? attachment.url ?? undefined : attachment.name}>
        {attachment.kind === 'image' && attachment.dataUri ? <img src={attachment.dataUri} alt="" /> : <span className="attachment-icon" aria-hidden="true"><Icon name={attachment.kind === 'link' ? 'link' : 'text'} size={12} /></span>}
        <span className="attachment-name">{attachment.name}</span>
        <button type="button" className="icon-button sm" aria-label={`Remove ${attachment.name}`} title={`Remove ${attachment.name}`} onClick={() => save(request!.attachments.filter(item => item.id !== attachment.id))}><Icon name="close" /></button>
      </li>)}</ul> : <p className="dock-hint">Drop files or links here</p>}
    </div>
    <p className="dock-hint">{pictureName(project, element.image) || elementKindLabel(element)}</p>
  </div>;
}

function StylePicker({ project, category, label, value, choose, create, disabled }: {
  project: Project; category: 'colour' | 'text'; label: string; value?: string | null; choose: (id: string | null) => void; create: () => void; disabled: boolean;
}) {
  const styles = category === 'colour' ? project.styles?.colours : project.styles?.text;
  return <select aria-label={`${label} style`} title="Style" value={value ?? ''} disabled={disabled} style={{ width: 100, minWidth: 0, flex: 'none' }}
    onChange={event => event.target.value === '__create' ? create() : choose(event.target.value || null)}>
    <option value="">Custom</option>{styles?.map(style => <option key={style.id} value={style.id}>{style.name}</option>)}
    <option value="__create">Create style from this</option>
  </select>;
}

type PaddingSide = keyof ElementPadding;
const PADDING_SIDES: PaddingSide[] = ['top', 'right', 'bottom', 'left'];

/** A compact all-sides padding field that opens onto the four sides, like Figma's auto layout. */
function PaddingFields({ uniform, sides, disabled, change }: {
  uniform: number | undefined;
  sides: Record<PaddingSide, number | undefined>;
  disabled: boolean;
  change: (change: (padding: ElementPadding) => ElementPadding) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  return <>
    <div style={{ display: 'flex', gap: 4 }}>
      <CompactNumberField prefix="P" label="Padding" min={0} unit="px" value={uniform ?? ''} placeholder={uniform === undefined ? 'Mixed' : undefined} disabled={disabled}
        valid={value => Number(value) >= 0}
        commit={value => { const next = Number(value); change(() => ({ top: next, right: next, bottom: next, left: next })); }} />
      <button type="button" className="icon-button sm" aria-label="Padding per side" aria-expanded={expanded} title="Set padding per side"
        onClick={() => setExpanded(value => !value)}><Icon name={expanded ? 'chevron-down' : 'chevron-right'} size={12} /></button>
    </div>
    {expanded && <div className="props-grid">{PADDING_SIDES.map(side => <CompactNumberField key={side} prefix={side[0].toUpperCase()} label={`${side[0].toUpperCase()}${side.slice(1)} padding`}
      min={0} unit="px" value={sides[side] ?? ''} placeholder={sides[side] === undefined ? 'Mixed' : undefined} disabled={disabled}
      valid={value => Number(value) >= 0} commit={value => change(padding => ({ ...padding, [side]: Number(value) }))} />)}</div>}
  </>;
}

export function ElementProperties({ project, page, sizeId, elements, busy, update, rename, link, unlink, canLink, canUnlink, request, setRequest, error, styleCommand, behaviour }: {
  styleCommand?: (command: import('../types').Command) => Promise<unknown>;
  behaviour?: BehaviourProperty;
  project: Project; page: Page; sizeId: string | null; elements: Element[]; busy: boolean;
  update: (patches: ElementPatch[]) => void;
  rename: (name: string | null) => Promise<void>;
  link: () => void; unlink: () => void; canLink: boolean; canUnlink: boolean;
  request: PendingRequest | null;
  setRequest: (data: { text: string; attachments: RequestAttachment[] } | null) => void;
  error: (message: string) => void;
}) {
  const single = elements.length === 1 ? elements[0] : null;
  const multi = elements.length > 1;
  const linked = linkedDefinition(project, single);
  // Every selected layer in one linked group already shares its content; offer the group, not
  // another link command that would only fail as "already linked".
  const shared = useMemo(() => sharedElementValues(elements), [elements]);
  const linkIds = elements.map(element => linkedDefinition(project, element)?.id ?? null);
  const sharedLinkId = multi && linkIds.every(id => id === linkIds[0]) ? linkIds[0] : null;
  const sharedLinked = sharedLinkId ? project.linked?.[sharedLinkId] ?? null : null;
  // Edits in one size become that size's overrides; All sizes changes base values and clears
  // the same fields from every size.
  const sized = <T,>(patch: T): T & { size_id?: string | null; all_sizes?: boolean } =>
    ({ ...patch, ...sizeTarget({ sizeId }) });
  const sameKind = shared.kind !== undefined;
  const kind = shared.kind ?? null;
  const { fill, strokeColour, strokeWidth, radius, opacity, shadow, shadowColour, textStyle, textColour, text, url, layout, x, y, width, height, rotation } = shared;
  const font = useCanvasFont(textStyle);
  const paddingSides = { top: shared.top, right: shared.right, bottom: shared.bottom, left: shared.left };
  const paddingUniform = paddingSides.top !== undefined && paddingSides.top === paddingSides.right && paddingSides.right === paddingSides.bottom && paddingSides.bottom === paddingSides.left ? paddingSides.top : undefined;
  const mixed = (value: unknown) => multi && value === undefined;
  const picture = useRef<HTMLInputElement>(null);
  const [pictureUri, setPictureUri] = useState<string | null>(null);
  const [limitsOpen, setLimitsOpen] = useState(false);
  const [nameError, setNameError] = useState('');
  async function commitName(name: string) {
    setNameError('');
    if (name.trim() === (single?.name ?? '')) return;
    try { await rename(name.trim() || null); }
    catch (reason) { setNameError(reason instanceof Error ? reason.message : String(reason)); }
  }

  function commitStyle(change: (style: ElementStyle) => void) { update(stylePatches(elements, style => {
    const before = JSON.stringify(style.textStyle); change(style);
    const { color: _beforeColor, align: _beforeAlign, ...beforeText } = JSON.parse(before) as ElementStyle['textStyle'];
    const { color: _color, align: _align, ...afterText } = style.textStyle;
    if (JSON.stringify(beforeText) !== JSON.stringify(afterText)) style.references = { ...style.references, text: null };
  }).map(patch => sized(patch))); }
  function picker(reference: 'fill' | 'stroke' | 'textColour' | 'text') {
    const current = shared[`${reference}Reference`];
    const category = reference === 'text' ? 'text' : 'colour';
    const choose = (id: string | null) => commitStyle(style => { style.references = { ...style.references, [reference]: id }; });
    async function create() {
      if (!styleCommand) return;
      const id = crypto.randomUUID();
      try {
        if (reference === 'text') {
          const { color: _color, align: _align, ...properties } = elements[0].style.textStyle;
          await styleCommand({ type: 'add_text_style', style: { id, name: `Text ${(project.styles?.text.length ?? 0) + 1}`, ...properties } });
        } else {
          const style = elements[0].style;
          const value = reference === 'fill' ? style.fill : reference === 'stroke' ? style.stroke.color : style.textStyle.color;
          if (!value) return;
          await styleCommand({ type: 'add_colour_style', style: { id, name: `Colour ${(project.styles?.colours.length ?? 0) + 1}`, value } });
        }
        choose(id);
      } catch (reason) { error(reason instanceof Error ? reason.message : String(reason)); }
    }
    return <StylePicker project={project} category={category} label={reference === 'textColour' ? 'Text colour' : reference === 'text' ? 'Text' : reference === 'fill' ? 'Fill' : 'Stroke'} value={current} choose={choose} create={() => void create()} disabled={busy || !styleCommand} />;
  }
  function commitSizing(change: Partial<NonNullable<ElementStyle['sizing']>>) {
    commitStyle(style => { style.sizing = { width: 'fixed', height: 'fixed', ...style.sizing, ...change }; });
  }
  function commitRects(change: (rect: Rect) => Rect) {
    update(elements.map(element => sized({ id: element.id, rect: change(element.rect) })));
  }
  function commitPadding(change: (padding: ElementPadding) => ElementPadding) {
    update(elements.map(element => {
      if (element.layout.kind === 'free') return sized({ id: element.id });
      const next = change(elementPadding(element));
      const shared = uniformPadding(next);
      return sized({
        id: element.id,
        layout: { ...element.layout, padding: shared ?? next.top },
        style: { ...element.style, padding: shared === null ? next : null },
      });
    }));
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
  const hyperlinkKind = !!single && (single.kind === 'text' || single.kind === 'box' || single.kind === 'button');
  function commitUrl(value: string) {
    if (!single) return;
    update([{ id: single.id, url: value.trim() || null }]);
  }
  // Text and lines keep their box plain until a stroke or radius is actually set.
  const showsStrokeWidth = kind === 'line' || multi || (strokeColour ?? '').trim().length > 0 || (kind !== 'text' && (strokeWidth ?? 0) > 0);
  const showsRadius = multi || (kind !== null && kind !== 'text' && kind !== 'line' ? true : (radius ?? 0) > 0);

  // A layer inside a linked copy is already shared; only a plain layer can be made linked.
  const makeLinkable = !!single && !linked && !canUnlink;
  return <div className="dock-form props">
    <div className="dock-layer-title" style={{ alignItems: 'center' }}><strong>{single ? elementKindLabel(single) : `${elements.length} layers`}</strong>
      {page.sizes.length > 1 && <EditScopeChoice scope={sizeId === null ? 'all' : 'this'} choose={requestEditScope} />}
    </div>
    {single && <>
      <CompactTextField label="Name" placeholder="Name" field="layer-name" value={single.name ?? ''} disabled={busy} commit={name => void commitName(name)} />
      {nameError && <p className="dock-field-error" role="alert">{nameError}</p>}
      {linked && <CompactTextField label="Linked element name" field="linked-name" value={linked.name} disabled={busy}
        valid={name => name.trim().length > 0} commit={name => update([{ id: single.id, linked_name: name }])} />}
    </>}
    {(sharedLinkId || canLink || canUnlink || makeLinkable) && <div className="form-actions">
      {sharedLinkId
        ? <><span className="dock-hint" title="These layers share one linked element">Linked · {sharedLinked?.name ?? 'Linked element'}</span>
          <button type="button" className="dock-action" disabled={busy} title="Unlink the selected layers (Ctrl/Cmd+Alt+B)" onClick={unlink}>Unlink</button></>
        : <>
          {(canLink || makeLinkable) && <button type="button" className="dock-action" disabled={busy} title="Make the selected layers linked (Ctrl/Cmd+Alt+K)" onClick={link}>{single ? 'Make linked' : `Make linked (${elements.length})`}</button>}
          {canUnlink && <button type="button" className="dock-action" disabled={busy} title="Detach this copy (Ctrl/Cmd+Alt+B)" onClick={unlink}>Unlink</button>}
        </>}
    </div>}
    {(single || multi) && <PropsSection id="props-layout" title="Layout">
      <div className="props-grid">
        <CompactNumberField prefix="X" label="X" min={-MAX_EDITOR_COORDINATE} max={MAX_EDITOR_COORDINATE} value={x ?? ''} placeholder={mixed(x) ? 'Mixed' : undefined} disabled={busy}
          valid={validNumber} commit={value => commitRects(rect => ({ ...rect, x: Number(value) }))} />
        <CompactNumberField prefix="Y" label="Y" min={-MAX_EDITOR_COORDINATE} max={MAX_EDITOR_COORDINATE} value={y ?? ''} placeholder={mixed(y) ? 'Mixed' : undefined} disabled={busy}
          valid={validNumber} commit={value => commitRects(rect => ({ ...rect, y: Number(value) }))} />
        {(['width', 'height'] as const).map(axis => {
          // A line's thickness axis may be zero; every other box keeps at least one pixel.
          const minimum = axis === 'height' && single?.kind === 'line' ? 0 : 1;
          const value = axis === 'width' ? width : height;
          return <div key={axis} style={{ display: 'flex', minWidth: 0, gap: 2 }}>
            <CompactNumberField prefix={axis === 'width' ? 'W' : 'H'} label={axis === 'width' ? 'Width' : 'Height'} min={minimum} max={MAX_VIEWPORT_SIDE}
              value={value ?? ''} placeholder={mixed(value) ? 'Mixed' : undefined} disabled={busy} valid={draft => Number(draft) >= minimum && Number(draft) <= MAX_VIEWPORT_SIDE}
              commit={draft => commitRects(rect => ({ ...rect, [axis]: Number(draft) }))} />
            {single && <select aria-label={`${axis} sizing`} style={{ width: 55, minWidth: 0 }} value={single.style.sizing?.[axis] ?? 'fixed'} disabled={busy} onChange={event => commitSizing({ [axis]: event.target.value })}>
              <option value="fixed">Fixed</option><option value="fit">Hug</option><option value="fill">Fill</option>
            </select>}
          </div>;
        })}
        <CompactNumberField prefix="°" label="Rotation" value={rotation ?? ''} placeholder={mixed(rotation) ? 'Mixed' : undefined} disabled={busy} normalise={normalizeAngle}
          valid={validNumber} commit={value => update(elements.map(element => ({ id: element.id, rotation: Number(value) })))} />
      </div>
      {single && <>
        <button type="button" className="dock-action" aria-expanded={limitsOpen} onClick={() => setLimitsOpen(value => !value)}>Min / max</button>
        {limitsOpen && <div className="props-grid">{(['minWidth', 'maxWidth', 'minHeight', 'maxHeight'] as const).map(key => <CompactNumberField key={key} prefix={key.startsWith('min') ? 'Min' : 'Max'} label={key.replace(/([A-Z])/g, ' $1')} min={0} optional
          value={single.style.sizing?.[key] ?? ''} disabled={busy} valid={value => !value.trim() || (Number.isFinite(Number(value)) && Number(value) >= 0)}
          commit={value => commitSizing({ [key]: value.trim() ? Number(value) : null })} />)}</div>}
      </>}
      {sameKind && kind === 'box' && <>
        <div className="props-choice"><ChoiceRow label="Flow" value={layout?.kind ?? 'free'} choices={[['free', 'Free'], ['stack', 'Stack'], ['grid', 'Grid']]} disabled={busy}
          choose={flow => update(elements.map(element => {
            // Switching from Free to Stack hugs a box to its children; an empty box keeps its size.
            const hug = flow === 'stack' && element.layout.kind === 'free' && element.children.length > 0;
            const next = flow === 'free' ? { kind: 'free' as const } : flow === 'stack'
              ? { kind: 'stack' as const, direction: 'row' as const, gap: 8, padding: 0, align: 'start' as const, justify: 'start' as const, wrap: false }
              : { kind: 'grid' as const, columns: 2, gap: 8, padding: 0 };
            return sized({ id: element.id, layout: next, ...(hug ? { style: { ...element.style, sizing: { ...element.style.sizing, width: 'fit' as const, height: 'fit' as const } } } : {}) });
          }))} /></div>
        {layout?.kind === 'stack' && <>
          <div className="props-choice"><ChoiceRow label="Direction" value={layout.direction} choices={[['row', 'Row'], ['column', 'Column']]} disabled={busy}
            choose={direction => update(elements.map(element => sized({ id: element.id, layout: element.layout.kind === 'free' ? element.layout : { ...element.layout, direction } })))} /></div>
          <div className="props-grid">
            <CompactNumberField prefix="G" label="Gap" min={0} unit="px" value={layout.gap} disabled={busy} valid={value => Number(value) >= 0}
              commit={value => update(elements.map(element => sized({ id: element.id, layout: element.layout.kind === 'free' ? element.layout : { ...element.layout, gap: Number(value) } })))} />
          </div>
          <PaddingFields uniform={paddingUniform} sides={paddingSides} disabled={busy} change={commitPadding} />
          <div className="props-choice"><ChoiceRow label="Align" value={layout.align} choices={[['start', 'Start'], ['center', 'Center'], ['end', 'End'], ['stretch', 'Stretch']]} disabled={busy}
            choose={align => update(elements.map(element => sized({ id: element.id, layout: element.layout.kind === 'free' ? element.layout : { ...element.layout, align } })))} /></div>
          <div className="props-choice"><ChoiceRow label="Justify" value={layout.justify} choices={[['start', 'Start'], ['center', 'Center'], ['end', 'End'], ['spaceBetween', 'Between']]} disabled={busy}
            choose={justify => update(elements.map(element => sized({ id: element.id, layout: element.layout.kind === 'free' ? element.layout : { ...element.layout, justify } })))} /></div>
          <label className="check-label"><input type="checkbox" checked={layout.wrap} disabled={busy} onChange={event => update(elements.map(element => sized({ id: element.id, layout: element.layout.kind === 'free' ? element.layout : { ...element.layout, wrap: event.target.checked } })))} />Wrap</label>
        </>}
        {layout?.kind === 'grid' && <>
          <div className="props-grid">
            <CompactNumberField prefix="C" label="Columns" min={1} max={MAX_GRID_COLUMNS} value={layout.columns} disabled={busy} valid={value => Number(value) >= 1 && Number(value) <= MAX_GRID_COLUMNS}
              commit={value => update(elements.map(element => sized({ id: element.id, layout: element.layout.kind === 'free' ? element.layout : { ...element.layout, columns: Number(value) } })))} />
            <CompactNumberField prefix="G" label="Gap" min={0} unit="px" value={layout.gap} disabled={busy} valid={value => Number(value) >= 0}
              commit={value => update(elements.map(element => sized({ id: element.id, layout: element.layout.kind === 'free' ? element.layout : { ...element.layout, gap: Number(value) } })))} />
          </div>
          <PaddingFields uniform={paddingUniform} sides={paddingSides} disabled={busy} change={commitPadding} />
        </>}
      </>}
    </PropsSection>}
    {hyperlinkKind && single && <PropsSection id="props-link" title="Link">
      <ProjectPageSelect label="Link to…" project={project} value={single.url} disabled={busy} choose={commitUrl} />
      <CompactTextField label="Link address" field="element-link" placeholder="https://example.com or /page" value={single.url ?? ''}
        disabled={busy} valid={validLink} commit={commitUrl} />
      {single.url && <button type="button" className="dock-action" disabled={busy} onClick={() => commitUrl('')}>Remove link</button>}
    </PropsSection>}
    {single && behaviour && <PropsSection id="props-behaviour" title="Behaviour"><BehaviourField {...behaviour} /></PropsSection>}
    {visual && <PropsSection id="props-appearance" title="Appearance">
      {kind !== 'line' && <div className="props-colour-style"><ColourField label="Fill" value={fill ?? ''} mixed={mixed(fill)} disabled={busy} commit={value => { rememberColour('fill', value.trim() || null); commitStyle(style => { style.fill = value.trim() || null; style.references = { ...style.references, fill: null }; }); }} />{picker('fill')}</div>}
      <div className="props-stroke">
        <div className="props-colour-style"><ColourField label="Stroke" value={strokeColour ?? ''} mixed={mixed(strokeColour)} fallback="#000000" disabled={busy} commit={value => { rememberColour('stroke', value.trim() || null); commitStyle(style => { style.stroke.color = value.trim() || null; style.references = { ...style.references, stroke: null }; }); }} />{picker('stroke')}</div>
        {showsStrokeWidth && <CompactNumberField prefix={<Icon name="line-weight" size={12} />} label="Stroke width" min={0} unit="px" value={strokeWidth ?? ''} placeholder={mixed(strokeWidth) ? 'Mixed' : undefined} disabled={busy} valid={value => Number(value) >= 0}
          commit={value => commitStyle(style => { style.stroke.width = Number(value); })} />}
      </div>
      {showsRadius && <CompactNumberField prefix={<Icon name="corner-radius" size={12} />} label="Radius" min={0} unit="px" value={radius ?? ''} placeholder={mixed(radius) ? 'Mixed' : undefined} disabled={busy} valid={value => Number(value) >= 0}
        commit={value => commitStyle(style => { style.radius = Number(value); })} />}
      <OpacityField value={opacity} disabled={busy} commit={percent => commitStyle(style => { style.opacity = percent / 100; })} />
    </PropsSection>}
    {((imageKind && single) || mediaKind) && <PropsSection id="props-content" title="Content">
      {imageKind && single && <div className="dock-inline">
        {projectPictureUri(project, single.image) && <img className="dock-picture" src={projectPictureUri(project, single.image)!} alt="" />}
        <span className="dock-value">{pictureName(project, single.image)}</span>
        <button type="button" className="dock-action" disabled={busy} onClick={() => picture.current?.click()}>Replace</button>
        <input ref={picture} type="file" hidden accept={IMAGE_MIME_TYPES.join(',')} onChange={event => { choosePicture(event.target.files?.[0]); event.target.value = ''; }} />
      </div>}
      {kind === 'embed' && single && <ProjectPageSelect label="Embed page…" project={project} value={single.url} disabled={busy} choose={commitUrl} />}
      {mediaKind && <CompactTextField label="Address" value={url ?? ''} disabled={busy} valid={kind === 'embed' ? validEmbedAddress : validUrl} commit={value => update(elements.map(element => ({ id: element.id, url: value.trim() })))} />}
    </PropsSection>}
    {textKind && <PropsSection id="props-text" title="Text">
      {picker('text')}
      <FontField label="Font" value={textStyle?.fontFamily ?? ''} disabled={busy} commit={value => commitStyle(style => { style.textStyle.fontFamily = value; })} />
      {font?.fellBack && <span className="field-help">Not installed — using Archivo</span>}
      <div className="props-grid">
        <CompactNumberField prefix="S" label="Size" min={TEXT_SIZE_RANGE[0]} max={TEXT_SIZE_RANGE[1]} unit="px" value={textStyle?.size ?? ''} placeholder={mixed(textStyle) ? 'Mixed' : undefined} disabled={busy} valid={value => Number(value) >= TEXT_SIZE_RANGE[0] && Number(value) <= TEXT_SIZE_RANGE[1]}
          commit={value => commitStyle(style => { style.textStyle.size = Number(value); })} />
        <CompactNumberField prefix="W" label="Weight" min={1} max={MAX_FONT_WEIGHT} value={textStyle?.weight ?? ''} placeholder={mixed(textStyle) ? 'Mixed' : undefined} disabled={busy} valid={value => Number(value) >= 1 && Number(value) <= MAX_FONT_WEIGHT}
          commit={value => commitStyle(style => { style.textStyle.weight = Number(value); })} />
        <CompactNumberField prefix="LH" label="Line height" min={0} value={textStyle?.lineHeight ?? ''} placeholder={mixed(textStyle) ? 'Mixed' : undefined} disabled={busy} valid={value => Number(value) >= 0}
          commit={value => commitStyle(style => { style.textStyle.lineHeight = Number(value); })} />
        <CompactNumberField prefix="LS" label="Letter spacing" unit="px" value={textStyle?.letterSpacing ?? ''} placeholder={mixed(textStyle) ? 'Mixed' : undefined} disabled={busy} valid={validNumber}
          commit={value => commitStyle(style => { style.textStyle.letterSpacing = Number(value); })} />
      </div>
      <div className="props-colour-style"><ColourField label="Text colour" value={textColour ?? ''} mixed={mixed(textColour)} disabled={busy} commit={value => { rememberColour('text', value.trim() || null); commitStyle(style => { style.textStyle.color = value.trim() || '#1a1a1a'; style.references = { ...style.references, textColour: null }; }); }} />{picker('textColour')}</div>
      <label className="check-label"><input type="checkbox" disabled={busy} checked={!!textStyle?.italic} onChange={event => commitStyle(style => { style.textStyle.italic = event.target.checked; })} />Italic</label>
      <div className="props-choice"><ChoiceRow label="Align" value={textStyle?.align ?? 'left'} choices={[['left', 'Left'], ['center', 'Center'], ['right', 'Right']]} disabled={busy}
        choose={align => commitStyle(style => { style.textStyle.align = align; })} /></div>
      {kind === 'input' && <div className="props-choice"><ChoiceRow label="Input type" value={kindFields(elements[0]).inputType ?? 'text'} choices={inputTypes.map(type => [type, type[0].toUpperCase() + type.slice(1)] as [InputType, string])} disabled={busy}
        choose={input_type => update(elements.map(element => ({ id: element.id, input_type })))} /></div>}
      {kind === 'select' && <LongTextField label="Options" placeholder="One option per line" rows={5} value={(kindFields(elements[0]).options ?? []).join('\n')}
        commit={value => { const options = value.split('\n').map(option => option.trim()).filter(Boolean); if (options.length) update(elements.map(element => ({ id: element.id, options }))); }} />}
      <LongTextField label="Text" placeholder="Text" value={text ?? ''} commit={value => update(elements.map(element => ({ id: element.id, text: value })))} />
    </PropsSection>}
    {codeKind && single && <PropsSection id="props-code" title="Code">
      <CompactTextField label="Language" value={kindFields(single).language ?? ''} disabled={busy} valid={value => value.trim().length > 0} commit={value => update([{ id: single.id, language: value.trim() }])} />
      <CodeField language={kindFields(single).language ?? ''} source={kindFields(single).source ?? ''} commit={source => update([{ id: single.id, source }])} />
    </PropsSection>}
    {visual && <PropsSection id="props-shadow" title="Shadow">
        <ColourField label="Shadow colour" value={shadowColour ?? ''} mixed={mixed(shadowColour)} fallback="#000000" disabled={busy}
          commit={value => commitStyle(style => { style.shadow.color = value.trim() || null; })} />
        <div className="props-grid">
          <CompactNumberField prefix="X" label="Shadow X" unit="px" value={shadow?.x ?? ''} placeholder={mixed(shadow) ? 'Mixed' : undefined} disabled={busy} valid={validNumber}
            commit={value => commitStyle(style => { style.shadow.x = Number(value); })} />
          <CompactNumberField prefix="Y" label="Shadow Y" unit="px" value={shadow?.y ?? ''} placeholder={mixed(shadow) ? 'Mixed' : undefined} disabled={busy} valid={validNumber}
            commit={value => commitStyle(style => { style.shadow.y = Number(value); })} />
          <CompactNumberField prefix="B" label="Shadow blur" min={0} unit="px" value={shadow?.blur ?? ''} placeholder={mixed(shadow) ? 'Mixed' : undefined} disabled={busy} valid={value => Number(value) >= 0}
            commit={value => commitStyle(style => { style.shadow.blur = Number(value); })} />
        </div>
    </PropsSection>}
    {single && <RequestArea element={single} project={project} request={request} setRequest={setRequest} error={error} />}
    {!single && <p className="dock-hint">{elements.map(element => elementKindLabel(element)).filter((label, index, all) => all.indexOf(label) === index).join(', ')}</p>}
  </div>;
}
