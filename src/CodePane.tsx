import { useEffect, useMemo, useRef, useState } from 'react';
import type { ElementTarget, SiteContent } from './types';

export type CodeView = 'design' | 'split' | 'code';
type Tab = keyof SiteContent;
const TABS: [Tab, string][] = [['html', 'HTML'], ['css', 'CSS'], ['javascript', 'JavaScript']];

function escape(text: string) { return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;'); }

// Light, dependency-free colouring. Each pass is a single regex over escaped text,
// so inserted spans are never re-scanned and page source can never inject markup.
const span = (kind: string, text: string) => `<span class="tok-${kind}">${text}</span>`;
const KEYWORDS = /^(const|let|var|function|return|if|else|for|while|new|class|import|export|await|async)$/;
function colour(line: string, tab: Tab) {
  if (tab === 'css') {
    const rule = line.match(/^(\s*[^{}]+)(\{\s*)$/);
    if (rule) return span('tag', escape(rule[1])) + escape(rule[2]);
    return escape(line).replace(/(\/\*.*?\*\/)|^(\s*)([\w-]+)(\s*:)/g, (match, comment, indent, property, colon) =>
      comment ? span('comment', comment) : `${indent}${span('attr', property)}${colon}`);
  }
  if (tab === 'html') return escape(line).replace(/(&lt;!--.*?--&gt;)|(&lt;\/?)([a-zA-Z][\w-]*)|([\w:-]+)=("[^"]*"|'[^']*')/g,
    (match, comment, open, tag, attribute, value) => comment ? span('comment', comment) : open ? open + span('tag', tag) : attribute.startsWith('data-desegnos-') ? span('meta', `${attribute}=${value}`) : `${span('attr', attribute)}=${span('string', value)}`);
  return escape(line).replace(/(\/\/.*$)|("(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|`(?:\\.|[^`\\])*`)|\b([a-z]+)\b/g,
    (match, comment, string, word) => comment ? span('comment', comment) : string ? span('string', string) : KEYWORDS.test(word) ? span('tag', word) : word);
}

// Display-only formatting for minified output. The saved source is never changed.
const CONTAINERS = new Set(['html', 'head', 'body', 'main', 'header', 'footer', 'section', 'article', 'nav', 'aside', 'div', 'ul', 'ol', 'form', 'table', 'thead', 'tbody', 'tr', 'figure', 'fieldset', 'select']);
const LINES = new Set(['p', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'li', 'button', 'a', 'img', 'input', 'label', 'textarea', 'td', 'th', 'meta', 'link', 'title', 'script', 'style', 'figcaption', 'option', 'video', 'iframe', 'blockquote', 'pre', 'br', 'hr']);
const INLINE = new Set(['a', 'img', 'input', 'label', 'button', 'br']);
function formatHtml(html: string) {
  if (html.split('\n').length > 8) return html;
  const out: string[] = []; let depth = 0; let current = ''; let raw = '';
  const flush = () => { if (current.trim()) out.push('  '.repeat(depth) + current.trim()); current = ''; };
  for (const part of html.split(/(<[^>]+>)/)) {
    if (!part) continue;
    if (raw) { current += part; if (part.toLowerCase() === `</${raw}>`) { raw = ''; flush(); } continue; }
    const tag = part.match(/^<(\/?)([a-zA-Z][\w-]*)/);
    if (!tag) { current += part; continue; }
    const closing = !!tag[1], name = tag[2].toLowerCase();
    if (CONTAINERS.has(name)) {
      flush();
      if (closing) { depth = Math.max(0, depth - 1); out.push('  '.repeat(depth) + part); }
      else { out.push('  '.repeat(depth) + part); if (!part.endsWith('/>')) depth += 1; }
    } else if (!closing && LINES.has(name) && !(INLINE.has(name) && current.trim())) { flush(); current = part; if (['script', 'style', 'pre', 'textarea'].includes(name)) raw = name; }
    else { current += part; if (closing && LINES.has(name) && !INLINE.has(name)) flush(); }
  }
  flush();
  return out.join('\n');
}
function formatCss(css: string) {
  if (css.split('\n').length > 8) return css;
  return css.replace(/\s*\{\s*/g, ' {\n  ').replace(/;\s*(?!\s*\})/g, ';\n  ').replace(/;?\s*\}\s*/g, ';\n}\n').replace(/\n  \n/g, '\n').trim();
}

// Best-effort location of the selected element until the native source map exists:
// match its id, then its first class, then its tag.
function findLine(lines: string[], target: ElementTarget | null): number | null {
  if (!target || target.selector === 'body') return null;
  const id = target.selector.match(/#([\w-]+)(?![^[]*\])/)?.[1];
  const className = target.selector.match(/\.([\w-]+)/)?.[1];
  const patterns = [
    id && new RegExp(`id=["']${id}["']`),
    className && new RegExp(`<${target.tag}\\b[^>]*class=["'][^"']*\\b${className}\\b`, 'i'),
    new RegExp(`<${target.tag}\\b`, 'i'),
  ].filter(Boolean) as RegExp[];
  for (const pattern of patterns) {
    const index = lines.findIndex(line => pattern.test(line));
    if (index >= 0) return index;
  }
  return null;
}

export type SourceDiagnostic = { language: string; line: number; column: number; endLine: number; endColumn: number; message: string };
export type SourceRange = { selector: string; startLine: number; endLine: number; startColumn: number; endColumn: number; implicit: boolean };
export type SourceMapView = { html: string; ranges: SourceRange[] };
type SaveError = { message?: string; diagnostics?: SourceDiagnostic[]; conflict?: boolean };
type Status = { kind: 'idle' | 'saving' | 'saved' | 'invalid' | 'conflict' | 'error'; message?: string; diagnostics?: SourceDiagnostic[] };
const LINE = 19;

// The code shown and edited: the saved text when it already has lines, otherwise a tidy layout of compacted output.
function displayText(content: SiteContent | null, tab: Tab) {
  const raw = content?.[tab] ?? '';
  return tab === 'html' ? formatHtml(raw) : tab === 'css' ? formatCss(raw) : raw;
}
function cursorAt(text: string, index: number) {
  const before = text.slice(0, index); const line = before.split('\n').length;
  return { line, column: index - before.lastIndexOf('\n') };
}

export function CodePane({ content, target, map, save, pick }: {
  content: SiteContent | null; target: ElementTarget | null; map: SourceMapView | null;
  save?: (language: Tab, text: string, expected: string) => Promise<void>; pick?: (selector: string) => void;
}) {
  const [tab, setTab] = useState<Tab>('html');
  const [query, setQuery] = useState('');
  const [hit, setHit] = useState(0);
  const [drafts, setDrafts] = useState<Partial<Record<Tab, string>>>({});
  const [status, setStatus] = useState<Status>({ kind: 'idle' });
  const [showMeta, setShowMeta] = useState(() => { try { return localStorage.getItem('desegnos.code-show-meta') === 'yes'; } catch { return false; } });
  const base = useRef<Partial<Record<Tab, string>>>({});
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLTextAreaElement>(null);
  const shown = displayText(content, tab);
  const text = drafts[tab] ?? shown;
  const lines = useMemo(() => text.split('\n'), [text]);
  const width = useMemo(() => lines.reduce((max, line) => Math.max(max, line.length), 0), [lines]);
  const exact = !!content && shown === content[tab] && drafts[tab] === undefined;
  const mapped = tab === 'html' && exact && map?.html === content?.html ? map : null;
  const range = mapped && target ? mapped.ranges.filter(item => item.selector === target.selector).sort((a, b) => (a.endLine - a.startLine) - (b.endLine - b.startLine))[0] : undefined;
  const selected = range ? [range.startLine - 1, range.endLine - 1] : (() => { const line = tab === 'html' ? findLine(lines, target) : null; return line === null ? null : [line, line]; })();
  const errors = new Map((status.kind === 'invalid' ? status.diagnostics ?? [] : []).filter(item => item.language === tab || (tab === 'html' && item.language === 'html')).map(item => [item.line - 1, item.message]));
  const matches = useMemo(() => query.trim() ? lines.flatMap((line, index) => line.toLowerCase().includes(query.toLowerCase()) ? [index] : []) : [], [lines, query]);
  const focusLine = matches.length ? matches[hit % matches.length] : selected?.[0] ?? null;
  const reveal = (line: number) => { const node = scroller.current; if (node) node.scrollTop = Math.max(0, line * LINE - node.clientHeight / 2); };
  useEffect(() => { setHit(0); }, [query, tab]);
  useEffect(() => { if (focusLine !== null && document.activeElement !== input.current) reveal(focusLine); }, [focusLine, tab, content]);
  // A saved draft comes back as the page's source; a change made elsewhere while drafting is a conflict.
  useEffect(() => {
    setDrafts(current => {
      const next = { ...current };
      for (const key of Object.keys(current) as Tab[]) {
        if ((content?.[key] ?? '') === current[key]) { delete next[key]; delete base.current[key]; }
        else if ((content?.[key] ?? '') !== base.current[key]) setStatus({ kind: 'conflict', message: 'This page changed elsewhere while you were editing.' });
      }
      return next;
    });
  }, [content]);
  async function commit(language: Tab, value: string) {
    if (!save || base.current[language] === undefined) return;
    if (value === displayText(content, language)) { setDrafts(current => { const next = { ...current }; delete next[language]; return next; }); return; }
    setStatus({ kind: 'saving' });
    try { await save(language, value, base.current[language]!); setStatus({ kind: 'saved' }); }
    catch (raw) {
      const error = (raw ?? {}) as SaveError;
      if (error.conflict) setStatus({ kind: 'conflict', message: error.message });
      else if (error.diagnostics?.length) setStatus({ kind: 'invalid', message: error.message, diagnostics: error.diagnostics });
      else setStatus({ kind: 'error', message: typeof raw === 'string' ? raw : error.message ?? String(raw) });
    }
  }
  function change(value: string) {
    if (base.current[tab] === undefined) base.current[tab] = content?.[tab] ?? '';
    setDrafts(current => ({ ...current, [tab]: value }));
    if (status.kind !== 'conflict') setStatus({ kind: 'idle' });
    if (timer.current) clearTimeout(timer.current);
    const language = tab;
    timer.current = setTimeout(() => { void commit(language, value); }, 700);
  }
  function flush() { if (timer.current) { clearTimeout(timer.current); timer.current = null; } const value = drafts[tab]; if (value !== undefined && status.kind !== 'conflict') void commit(tab, value); }
  function reload() { base.current = {}; setDrafts({}); setStatus({ kind: 'idle' }); }
  // Clicking in the code selects the matching element on the canvas.
  function follow() {
    const node = input.current; if (!node || !mapped || !pick) return;
    const at = cursorAt(text, node.selectionStart);
    const inside = mapped.ranges.filter(item => (at.line > item.startLine || (at.line === item.startLine && at.column >= item.startColumn)) && (at.line < item.endLine || (at.line === item.endLine && at.column < item.endColumn)));
    const best = inside.sort((a, b) => (a.endLine - a.startLine) - (b.endLine - b.startLine) || (a.endColumn - a.startColumn) - (b.endColumn - b.startColumn))[0];
    if (best && best.selector !== target?.selector) pick(best.selector);
  }
  const editing = !!save && !!content;
  const note = status.kind === 'saving' ? 'Saving…' : status.kind === 'saved' ? 'Saved. Undo works as usual.' : status.kind === 'invalid' ? 'Not saved yet. Fix the highlighted line.' : status.kind === 'error' ? status.message : status.kind === 'conflict' ? status.message : drafts[tab] !== undefined ? 'Saving after you pause…' : editing && shown !== content?.[tab] ? 'This code was compacted. It’s shown tidied, and your first edit saves that tidy layout.' : editing ? 'Edits save automatically when you pause.' : '';
  return <section className={`code-pane${showMeta ? '' : ' quiet-meta'}`} aria-label="Page code">
    <header className="code-pane-bar">
      <div className="code-tabs" role="tablist" aria-label="Code files">{TABS.map(([value, label]) => <button key={value} role="tab" aria-selected={tab === value} onClick={() => { flush(); setTab(value); }}>{label}{drafts[value] !== undefined ? ' •' : ''}</button>)}</div>
      <input className="code-search" type="search" placeholder="Find in code" aria-label="Find in code" value={query} onChange={event => setQuery(event.target.value)}
        onKeyDown={event => { if (event.key === 'Enter' && matches.length) { event.preventDefault(); setHit(value => value + (event.shiftKey ? matches.length - 1 : 1)); } }} />
      {tab === 'html' && content?.html.includes('data-desegnos-') && <label className="code-meta-toggle" title="Desegnos uses these to keep track of elements. They are removed when you export."><input type="checkbox" checked={showMeta} onChange={event => { setShowMeta(event.target.checked); try { localStorage.setItem('desegnos.code-show-meta', event.target.checked ? 'yes' : 'no'); } catch {} }} />Show editor attributes</label>}
      {query.trim() && <span className="code-count" aria-live="polite">{matches.length ? `${(hit % matches.length) + 1}/${matches.length}` : '0'}</span>}
    </header>
    {!content ? <p className="code-empty">Generate the page to see its code here.</p> : <div ref={scroller} className="code-scroll">
      <div className="code-sheet" style={{ height: lines.length * LINE + 12, minWidth: `calc(${width + 4}ch + 52px)` }}>
        <div className="code-gutter" aria-hidden="true">{lines.map((_, index) => <span key={index} className={errors.has(index) ? 'has-error' : undefined}>{index + 1}</span>)}</div>
        <div className="code-text">
          <div className="code-highlight" aria-hidden="true">{lines.map((line, index) => <div key={index} data-line={index} title={errors.get(index)} className={`code-line${selected && index >= selected[0] && index <= selected[1] ? ' selected' : ''}${matches.includes(index) ? ' match' : ''}${index === focusLine && matches.length ? ' current' : ''}${errors.has(index) ? ' error' : ''}`}><code dangerouslySetInnerHTML={{ __html: colour(line, tab) || ' ' }} /></div>)}</div>
          <textarea ref={input} className="code-input" aria-label={`${TABS.find(([value]) => value === tab)![1]} code`} value={text} readOnly={!editing} wrap="off" spellCheck={false} autoCapitalize="off" autoComplete="off"
            onChange={event => change(event.target.value)} onBlur={flush} onClick={follow} onKeyUp={event => { if (event.key.startsWith('Arrow') || event.key === 'Home' || event.key === 'End') follow(); }}
            onKeyDown={event => {
              if (event.key === 'Tab' && !event.shiftKey && editing) { event.preventDefault(); const node = event.currentTarget; const start = node.selectionStart; const value = `${text.slice(0, start)}  ${text.slice(node.selectionEnd)}`; change(value); requestAnimationFrame(() => { node.selectionStart = node.selectionEnd = start + 2; }); }
              if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 's') { event.preventDefault(); flush(); }
            }} />
        </div>
      </div>
    </div>}
    <footer className={`code-pane-note status-${status.kind}`} role="status">
      <span>{note}</span>
      {status.kind === 'invalid' && <ul className="code-errors">{(status.diagnostics ?? []).slice(0, 3).map((item, index) => <li key={index}><button type="button" className="link-button" onClick={() => { setTab(item.language === 'css' ? 'css' : item.language === 'javascript' ? 'javascript' : 'html'); reveal(item.line - 1); }}>Line {item.line}</button> {item.message}</li>)}</ul>}
      {status.kind === 'conflict' && <button type="button" onClick={reload}>Reload latest code</button>}
    </footer>
  </section>;
}
