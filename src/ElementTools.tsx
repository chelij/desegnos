import { useEffect, useState, type FormEvent } from 'react';
import { Modal } from './Modal';
import { escapeHtml, imageFile } from './elementEditing';
import type { ElementDetails, ElementEdit, ElementTarget, ElementVisual } from './types';

export type ElementDialogMode = 'edit' | 'replace' | 'position' | 'remove';
const titles = { edit: 'Edit element', replace: 'Replace element', position: 'Move element', remove: 'Delete element' };
const color = (value: string) => { const rgb = value.match(/[\d.]+/g)?.slice(0, 3); return rgb?.length === 3 ? `#${rgb.map(value => Math.max(0, Math.min(255, Math.round(Number(value)))).toString(16).padStart(2, '0')).join('')}` : '#000000'; };
const styleFields = [['fontSize', 'Font size'], ['width', 'Width'], ['height', 'Height'], ['padding', 'Padding'], ['borderRadius', 'Corner radius']] as const;

export function ElementDialog({ mode, target, visual, inspect, apply, move, close }: {
  mode: ElementDialogMode; target: ElementTarget; visual: ElementVisual | null;
  inspect: (target: ElementTarget) => Promise<ElementDetails>; apply: (change: ElementEdit) => Promise<void>;
  move: (x: number, y: number) => Promise<void>; close: () => void;
}) {
  const [details, setDetails] = useState<ElementDetails | null>(null); const [error, setError] = useState('');
  useEffect(() => { let active = true; void inspect(target).then(value => { if (active) setDetails(value); }, error => { if (active) setError(String(error)); }); return () => { active = false; }; }, [target.sourceSha256, target.selector]);
  return <Modal title={titles[mode]} close={close}><div className="dialog-body element-dialog">
    {error && <p className="message error" role="alert">{error}</p>}
    {!details && !error && <p role="status">Opening this item…</p>}
    {details && <ElementForm key={`${target.sourceSha256}:${target.selector}:${mode}`} mode={mode} details={details} visual={visual} apply={apply} move={move} close={close} />}
  </div></Modal>;
}
function ElementForm({ mode, details, visual, apply, move, close }: { mode: ElementDialogMode; details: ElementDetails; visual: ElementVisual | null; apply: (change: ElementEdit) => Promise<void>; move: (x: number, y: number) => Promise<void>; close: () => void }) {
  const [texts, setTexts] = useState(details.texts); const [attributes, setAttributes] = useState(details.attributes);
  const [styles, setStyles] = useState<Record<string, string>>({ ...visual });
  const [kind, setKind] = useState(['p', 'h1', 'h2', 'h3', 'button', 'a', 'img', 'section', 'div'].includes(details.target.tag) ? details.target.tag : 'p');
  const [replacementText, setReplacementText] = useState(details.texts.map(item => item.text).join(' ').trim()); const [html, setHtml] = useState(details.html);
  const [keepStyle, setKeepStyle] = useState(true); const [image, setImage] = useState(details.attributes.src ?? ''); const [fileName, setFileName] = useState('');
  const [x, setX] = useState(0); const [y, setY] = useState(0); const [busy, setBusy] = useState(false); const [error, setError] = useState('');
  const attr = (name: string, value: string) => setAttributes(current => ({ ...current, [name]: value }));
  const style = (name: string, value: string) => setStyles(current => ({ ...current, [name]: value }));
  async function file(value: File | undefined) { if (!value) return; setError(''); setBusy(true); try { const data = await imageFile(value); setImage(data); attr('src', data); setFileName(value.name); } catch (error) { setError(String(error)); } finally { setBusy(false); } }
  async function submit(event: FormEvent) {
    event.preventDefault(); setError(''); setBusy(true);
    try {
      if (mode === 'remove') await apply({ kind: 'remove' });
      else if (mode === 'position') { if (x || y) await move(x, y); }
      else if (mode === 'replace') {
        let source = html;
        if (kind === 'img') { if (!image) throw new Error('Choose an image first.'); const css = document.createElement('div').style; if (keepStyle) css.cssText = details.inlineStyle; css.setProperty('display','inline-block','important'); source = `<img src="${escapeHtml(image)}" alt="${escapeHtml(attributes.alt ?? '')}" style="${escapeHtml(css.cssText)}">`; }
        else if (kind !== 'html') source = `<${kind}${kind === 'a' ? ` href="${escapeHtml(attributes.href ?? '')}"` : kind === 'button' ? ' type="button"' : ''}>${escapeHtml(replacementText)}</${kind}>`;
        await apply({ kind: 'replace', html: source, keepStyle });
      } else {
        const changed = texts.filter(text => text.text !== details.texts.find(item => item.index === text.index)?.text);
        const attrs: Record<string, string | null> = {};
        for (const [key, value] of Object.entries(attributes)) if (value !== (details.attributes[key] ?? '')) attrs[key] = value || null;
        const css = document.createElement('div').style; css.cssText = details.inlineStyle; let changedStyle = false;
        for (const [name, value] of Object.entries(styles)) {
          if (name === 'translate' || value === visual?.[name as keyof ElementVisual]) continue;
          const property = name.replace(/[A-Z]/g, char => `-${char.toLowerCase()}`);
          if (value && !CSS.supports(property, value)) throw new Error(`Check the ${name.replace(/[A-Z]/g, char => ` ${char.toLowerCase()}`)} value.`);
          if (value) css.setProperty(property, value, 'important'); else css.removeProperty(property);
          changedStyle = true;
        }
        if (changed.length || Object.keys(attrs).length || changedStyle) await apply({ kind: 'update', texts: changed, attributes: attrs, style: changedStyle ? css.cssText : null });
      }
      close();
    } catch (error) { setError(error instanceof Error ? error.message : String(error)); }
    finally { setBusy(false); }
  }
  const imageControl = <label>Image file<input type="file" accept="image/png,image/jpeg,image/webp,image/gif,image/svg+xml" onChange={event => { void file(event.target.files?.[0]); }} />{fileName && <span className="field-help">{fileName}</span>}</label>;
  return <form onSubmit={event => { void submit(event); }}>
    {error && <p className="message error" role="alert">{error}</p>}
    {mode === 'remove' ? <p>Delete this item and everything inside it? You can undo this.</p> : mode === 'position' ? <><p>Drag the selected element on the canvas, or enter an offset.</p><div className="form-row"><label>Move horizontally (px)<input type="number" value={x} min={-100000} max={100000} step="any" onChange={event => setX(event.target.valueAsNumber)} required /></label><label>Move vertically (px)<input type="number" value={y} min={-100000} max={100000} step="any" onChange={event => setY(event.target.valueAsNumber)} required /></label></div></> : mode === 'replace' ? <>
      <label>Replace with<select value={kind} onChange={event => setKind(event.target.value)}>{[['p','Paragraph'],['h1','Main heading'],['h2','Heading'],['h3','Small heading'],['button','Button'],['a','Link'],['img','Image'],['section','Section'],['div','Container'],['html','Custom HTML']].map(([value,label]) => <option key={value} value={value}>{label}</option>)}</select></label>
      {kind === 'html' ? <label>Replacement HTML<textarea className="code-input" value={html} onChange={event => setHtml(event.target.value)} rows={10} maxLength={1048576} required /></label> : kind === 'img' ? <>{imageControl}<label>Image description<input value={attributes.alt ?? ''} onChange={event => attr('alt', event.target.value)} /></label></> : <label>Text<textarea value={replacementText} onChange={event => setReplacementText(event.target.value)} rows={3} /></label>}
      {kind === 'a' && <label>Link address<input value={attributes.href ?? ''} onChange={event => attr('href',event.target.value)} /></label>}
      <label className="check-label"><input type="checkbox" checked={keepStyle} onChange={event => setKeepStyle(event.target.checked)} />Keep existing styles</label>
    </> : <>
      {texts.map((item,index) => <label key={item.index}>{texts.length === 1 ? 'Text' : `Text ${index + 1}`}<textarea rows={Math.min(5, Math.max(1, item.text.split('\n').length))} value={item.text} onChange={event => setTexts(current => current.map(text => text.index === item.index ? { ...text, text:event.target.value } : text))} /></label>)}
      {details.textsTruncated && <p className="field-help">Select a smaller item to edit the rest of its text.</p>}
      {details.target.tag === 'a' && <><label>Link address<input value={attributes.href ?? ''} onChange={event => attr('href',event.target.value)} /></label><label className="check-label"><input type="checkbox" checked={attributes.target === '_blank'} onChange={event => { attr('target',event.target.checked ? '_blank' : ''); if (event.target.checked) attr('rel',[...new Set(`${attributes.rel ?? ''} noopener noreferrer`.trim().split(/\s+/))].join(' ')); }} />Open in a new tab</label></>}
      {details.target.tag === 'img' && <>{imageControl}<label>Image description<input value={attributes.alt ?? ''} onChange={event => attr('alt',event.target.value)} /></label></>}
      {['input','textarea'].includes(details.target.tag) && <label>Hint text<input value={attributes.placeholder ?? ''} onChange={event => attr('placeholder',event.target.value)} /></label>}
      {visual && <details className="element-appearance"><summary>Appearance</summary><div className="form-row"><label>Text color<input type="color" value={color(styles.color ?? '')} onChange={event => style('color',event.target.value)} /></label><label>Background color<input type="color" value={color(styles.backgroundColor ?? '')} onChange={event => style('backgroundColor',event.target.value)} /></label></div><label className="check-label"><input type="checkbox" checked={styles.backgroundColor === 'transparent' || /rgba\([^)]*,\s*0\)/.test(styles.backgroundColor ?? '')} onChange={event => style('backgroundColor',event.target.checked ? 'transparent' : color(styles.backgroundColor ?? ''))} />Transparent background</label>
        <div className="element-style-fields">{styleFields.map(([name,label]) => <label key={name}>{label}<input value={styles[name] ?? ''} onChange={event => style(name,event.target.value)} /></label>)}</div><p className="field-help">Sizes accept px, % and other CSS units. Leave a value empty to use the page’s style.</p>
      </details>}
      <details><summary>More options</summary><label>Accessible name<input value={attributes['aria-label'] ?? ''} onChange={event => attr('aria-label',event.target.value)} /></label><label>Hover text<input value={attributes.title ?? ''} onChange={event => attr('title',event.target.value)} /></label></details>
    </>}
    <div className="form-actions"><button type="button" onClick={close} disabled={busy}>Cancel</button><button className={mode === 'remove' ? 'danger' : 'primary'} disabled={busy}>{busy ? 'Saving…' : mode === 'remove' ? 'Delete element' : mode === 'replace' ? 'Replace element' : mode === 'position' ? 'Move element' : 'Save changes'}</button></div>
  </form>;
}
