import { forwardRef, lazy, Suspense, useImperativeHandle, useEffect, useMemo, useRef, useState } from 'react';
import { synchronizeCodeDrafts, type PendingCodeDraft } from './codeDraftLifecycle';
import type { CodeEditorHandle, CodeLineMark } from './CodeEditor';
const CodeEditor = lazy(() => import('./CodeEditor').then(module => ({ default: module.CodeEditor })));
import { projectCodeSource, projectEditorHtml, restoreEditorHtml, sourceOffset, sourcePosition, type HtmlProjection } from './editorHtml';
import type { ElementTarget, SiteContent } from './types';

export type CodeView = 'design' | 'split' | 'code';
type Tab = keyof SiteContent;
const TABS: [Tab, string][] = [['html', 'HTML'], ['css', 'CSS'], ['javascript', 'JavaScript']];
export type SourceDiagnostic = { language: string; line: number; column: number; endLine: number; endColumn: number; message: string };
export type SourceRange = { id?: string; selector: string; startLine: number; endLine: number; startColumn: number; endColumn: number; implicit: boolean };
export type SourceMapView = { html: string; ranges: SourceRange[] };
type SaveError = { message?: string; diagnostics?: SourceDiagnostic[]; conflict?: boolean };
type Status = { kind: 'idle' | 'saving' | 'saved' | 'invalid' | 'conflict' | 'error'; message?: string; diagnostics?: SourceDiagnostic[] };
type Draft = { text: string; base: string; projection: HtmlProjection };
const plain = projectCodeSource;

export type CodePaneHandle = { synchronize: () => Promise<void> };
export const CodePane = forwardRef<CodePaneHandle, {
  content: SiteContent | null; target: ElementTarget | null; map: SourceMapView | null;
  save?: (language: Tab, text: string, expected: string) => Promise<SiteContent>; pick?: (selector: string, id?: string) => void; saveProject?: () => void;
  tidy?: () => void;
}>(function CodePane({ content, target, map, save, pick, saveProject, tidy }, ref) {
  const [tab, setTab] = useState<Tab>('html');
  const [query, setQuery] = useState('');
  const [hit, setHit] = useState(0);
  const [drafts, setDraftState] = useState<Partial<Record<Tab, Draft>>>({});
  const draftRef = useRef(drafts);
  const [statuses, setStatuses] = useState<Partial<Record<Tab, Status>>>({});
  const statusRef = useRef(statuses);
  const lastApplied = useRef<Partial<Record<Tab, { previous: string; source: string }>>>({});
  const synchronizing = useRef<Promise<void> | null>(null);
  const [showMeta, setShowMeta] = useState(() => { try { return localStorage.getItem('desegnos.code-show-meta') === 'yes'; } catch { return false; } });
  const current = useRef({ content, save, showMeta }); current.current = { content, save, showMeta };
  const editors = useRef<Partial<Record<Tab, CodeEditorHandle>>>({});
  const timers = useRef<Partial<Record<Tab, ReturnType<typeof setTimeout>>>>({});
  const flights = useRef<Partial<Record<Tab, Promise<void>>>>({});
  const queued = useRef(new Set<Tab>());
  const searchInput = useRef<HTMLInputElement>(null);
  const status = statuses[tab] ?? { kind: 'idle' };
  const projectText = (language: Tab, raw: string) => language === 'html' ? projectEditorHtml(raw, current.current.showMeta) : plain(raw);
  const projections = useMemo(() => ({ html: projectEditorHtml(content?.html ?? '', showMeta), css: plain(content?.css ?? ''), javascript: plain(content?.javascript ?? '') }), [content, showMeta]);
  const text = drafts[tab]?.text ?? projections[tab].text;
  const exact = !drafts.html || drafts.html.text === projections.html.text && drafts.html.base === content?.html;
  const mapped = useMemo(() => {
    if (!exact || !map || map.html !== content?.html) return null;
    if (projections.html.text === map.html) return map;
    return { ...map, ranges: map.ranges.map(item => {
      const start = sourcePosition(projections.html.text, projections.html.rawToVisible(sourceOffset(map.html, item.startLine, item.startColumn)));
      const end = sourcePosition(projections.html.text, projections.html.rawToVisible(sourceOffset(map.html, item.endLine, item.endColumn)));
      return { ...item, startLine: start.line, startColumn: start.column, endLine: end.line, endColumn: end.column };
    }) };
  }, [exact, map, content?.html, showMeta, projections.html]);
  const range = mapped && target ? mapped.ranges.filter(item => target.id ? item.id === target.id : item.selector === target.selector).sort((a, b) => (a.endLine - a.startLine) - (b.endLine - b.startLine) || a.endColumn - a.startColumn - (b.endColumn - b.startColumn))[0] : undefined;
  const matches = useMemo(() => {
    const result: { offset: number; line: number }[] = [];
    if (!query) return result;
    const lower = text.toLowerCase(), find = query.toLowerCase();
    let at = 0;
    while ((at = lower.indexOf(find, at)) >= 0) { result.push({ offset: at, line: sourcePosition(text, at).line }); at += find.length; }
    return result;
  }, [text, query]);
  const focusLine = matches.length ? matches[hit % matches.length].line : tab === 'html' ? range?.startLine ?? null : null;
  useEffect(() => { setHit(0); }, [query, tab]);
  function draft(language: Tab, value: Draft | undefined) {
    const next = { ...draftRef.current };
    if (value) next[language] = value; else delete next[language];
    draftRef.current = next; setDraftState(next);
  }
  function setStatus(language: Tab, value: Status) { const next = { ...statusRef.current, [language]: value }; statusRef.current = next; setStatuses(next); }
  function sourceFor(language: Tab) { const raw = current.current.content?.[language] ?? ''; const own = lastApplied.current[language]; return own?.previous === raw ? own.source : raw; }
  function clearTimer(language: Tab) { if (timers.current[language]) clearTimeout(timers.current[language]); delete timers.current[language]; }
  useEffect(() => () => { for (const timer of Object.values(timers.current)) clearTimeout(timer); }, []);
  // A native save may add IDs without changing visible text. Clear only the draft
  // that actually matches the applied source; keep later typing and rejected text.
  useEffect(() => {
    for (const [language] of TABS) {
      if (lastApplied.current[language]?.source === (content?.[language] ?? '')) delete lastApplied.current[language];
      const value = draftRef.current[language];
      if (!value) { editors.current[language]?.replaceSource(projectText(language, sourceFor(language))); continue; }
      if (flights.current[language]) continue;
      const source = sourceFor(language);
      if (source === value.base) { if (projectText(language, source).text === value.text) draft(language, undefined); }
      else { clearTimer(language); setStatus(language, { kind: 'conflict', message: 'This page changed elsewhere while you were editing.' }); }
    }
  }, [content, drafts]);
  async function commit(language: Tab): Promise<void> {
    clearTimer(language);
    if (['invalid', 'conflict', 'error'].includes(statusRef.current[language]?.kind ?? '')) return;
    if (flights.current[language]) { queued.current.add(language); return flights.current[language]; }
    const value = draftRef.current[language], save = current.current.save;
    if (!value || !save) return;
    const source = sourceFor(language);
    if (source !== value.base) { setStatus(language, { kind: 'conflict', message: 'This page changed elsewhere while you were editing.' }); return; }
    if (value.text === projectText(language, source).text) { draft(language, undefined); return; }
    const editor = editors.current[language]; if (!editor) return;
    const snapshot = editor.snapshot();
    const hidden = language === 'html' && !current.current.showMeta;
    const rawValue = restoreEditorHtml(snapshot.projection);
    setStatus(language, { kind: 'saving' });
    const operation = async () => {
      try {
        const applied = await save(language, rawValue, value.base);
        lastApplied.current[language] = { previous: value.base, source: applied[language] };
        const projection = projectText(language, applied[language]);
        const rebased = editor.rebase(projection, snapshot);
        const latest = draftRef.current[language];
        if (latest) draft(language, { text: rebased.text, base: applied[language], projection: rebased });
        setStatus(language, { kind: 'saved' });
      } catch (raw) {
        const error = (raw ?? {}) as SaveError;
        if (error.conflict) setStatus(language, { kind: 'conflict', message: error.message });
        else if (error.diagnostics?.length) {
          const projection = language === 'html' ? projectEditorHtml(rawValue, !hidden) : projectCodeSource(rawValue);
          const diagnostics = error.diagnostics.map(item => {
            if (item.language !== language) return item;
            const start = sourcePosition(snapshot.text, projection.rawToVisible(sourceOffset(rawValue, item.line, item.column)));
            const end = sourcePosition(snapshot.text, projection.rawToVisible(sourceOffset(rawValue, item.endLine, item.endColumn)));
            return { ...item, line: start.line, column: start.column, endLine: end.line, endColumn: end.column };
          });
          setStatus(language, { kind: 'invalid', message: error.message, diagnostics });
        } else setStatus(language, { kind: 'error', message: typeof raw === 'string' ? raw : error.message ?? String(raw) });
      } finally {
        editor.release(snapshot); delete flights.current[language];
        // A queued pause saves the newer text once; a rejected unchanged draft is
        // never retried automatically.
        const again = queued.current.delete(language);
        const latest = draftRef.current[language];
        if (latest && sourceFor(language) === latest.base && projectText(language, latest.base).text === latest.text) draft(language, undefined);
        else if (again && latest && latest.text !== snapshot.text) void commit(language);
      }
    };
    flights.current[language] = operation();
    return flights.current[language];
  }
  function change(language: Tab, text: string, projection: HtmlProjection) {
    const previous = draftRef.current[language];
    draft(language, { text, projection, base: previous?.base ?? sourceFor(language) });
    if (statusRef.current[language]?.kind !== 'conflict') setStatus(language, { kind: 'idle' });
    clearTimer(language); timers.current[language] = setTimeout(() => { void commit(language); }, 700);
  }
  function follow(offset: number) {
    if (!mapped || !pick || tab !== 'html') return;
    const at = sourcePosition(text, offset);
    const inside = mapped.ranges.filter(item => (at.line > item.startLine || at.line === item.startLine && at.column >= item.startColumn) && (at.line < item.endLine || at.line === item.endLine && at.column < item.endColumn));
    const best = inside.sort((a, b) => a.endLine - a.startLine - (b.endLine - b.startLine) || a.endColumn - a.startColumn - (b.endColumn - b.startColumn))[0];
    if (best && (best.id ? best.id !== target?.id : best.selector !== target?.selector)) pick(best.selector, best.id);
  }
  function discard() { clearTimer(tab); queued.current.delete(tab); editors.current[tab]?.replaceSource(projectText(tab, sourceFor(tab))); draft(tab, undefined); setStatus(tab, { kind: 'idle' }); }
  function pendingDrafts(): PendingCodeDraft[] {
    return TABS.flatMap<PendingCodeDraft>(([language, label]) => {
      const value = draftRef.current[language]; if (!value) return [];
      const status = statusRef.current[language]?.kind;
      if (status === 'invalid' || status === 'conflict' || status === 'error') return [{ language: label, status }];
      return value.text === projectText(language, value.base).text ? [] : [{ language: label, status: flights.current[language] ? 'saving' : 'dirty' }];
    });
  }
  function synchronize() {
    if (synchronizing.current) return synchronizing.current;
    synchronizing.current = synchronizeCodeDrafts({
      clearTimers: () => { for (const [language] of TABS) clearTimer(language); },
      pending: pendingDrafts,
      flush: async () => {
        for (const [language, label] of TABS) {
          if (flights.current[language]) await flights.current[language];
          if (!pendingDrafts().some(draft => draft.language === label && (draft.status === 'dirty' || draft.status === 'saving'))) continue;
          await commit(language);
        }
      },
    }).catch(error => {
      const issue = pendingDrafts().find(draft => ['invalid', 'conflict', 'error'].includes(draft.status));
      const language = TABS.find(([, label]) => label === issue?.language)?.[0];
      if (language) { setTab(language); const line = statusRef.current[language]?.diagnostics?.[0]?.line; if (line) requestAnimationFrame(() => editors.current[language]?.reveal(line)); }
      throw error;
    }).finally(() => { synchronizing.current = null; });
    return synchronizing.current;
  }
  useImperativeHandle(ref, () => ({ synchronize }));
  const note = status.kind === 'saving' ? 'Saving…' : status.kind === 'saved' ? 'Saved. Undo works as usual.' : status.kind === 'invalid' ? 'Not saved yet. Fix the highlighted line.' : status.kind === 'error' || status.kind === 'conflict' ? status.message : drafts[tab] ? 'Saving after you pause…' : save && content ? 'Edits save automatically when you pause.' : '';
  return <section className="code-pane" aria-label="Page code">
    <header className="code-pane-bar">
      <div className="code-tabs" role="tablist" aria-label="Code files">{TABS.map(([value, label]) => <button key={value} role="tab" aria-selected={tab === value} onClick={() => { void commit(tab); setTab(value); }}>{label}{drafts[value] && drafts[value]!.text !== projections[value].text ? ' •' : ''}</button>)}</div>
      {tidy && content && <button type="button" className="code-tidy" title="Tidy code" disabled={Object.values(statuses).some(item => item?.kind === 'saving')} onClick={tidy}>Tidy code</button>}
      <input ref={searchInput} className="code-search" type="search" placeholder="Find in code" aria-label="Find in code" value={query} onChange={event => setQuery(event.target.value)} onKeyDown={event => { if (event.key === 'Enter' && matches.length) { event.preventDefault(); setHit(value => value + (event.shiftKey ? matches.length - 1 : 1)); } }} />
      {content?.html.includes('data-desegnos-') && tab === 'html' && <label className="code-meta-toggle"><input type="checkbox" checked={showMeta} disabled={!!drafts.html || statuses.html?.kind === 'saving'} onChange={event => { setShowMeta(event.target.checked); try { localStorage.setItem('desegnos.code-show-meta', event.target.checked ? 'yes' : 'no'); } catch {} }} />Show editor attributes</label>}
      {query && <span className="code-count" aria-live="polite">{matches.length ? `${hit % matches.length + 1}/${matches.length}` : '0'}</span>}
    </header>
    {!content ? <p className="code-empty">No generated code.</p> : <Suspense fallback={<p className="code-empty" role="status">Opening code…</p>}>{TABS.map(([language, label]) => {
      const value = drafts[language]?.text ?? projections[language].text;
      const marks = new Map<number, CodeLineMark>();
      const mark = (line: number, className: string, title?: string) => { const old = marks.get(line); marks.set(line, { line, className: [old?.className, className].filter(Boolean).join(' '), title: title ?? old?.title }); };
      if (language === 'html' && range) for (let line = range.startLine; line < range.startLine + 2000 && line <= range.endLine - (range.endColumn === 1 && range.endLine > range.startLine ? 1 : 0); line++) mark(line, 'selected');
      if (language === tab) {
        for (const match of matches.slice(0, 2000)) mark(match.line, 'match');
        if (matches.length) mark(matches[hit % matches.length].line, 'current');
      }
      for (const error of statuses[language]?.diagnostics ?? []) if (error.language === language) mark(error.line, 'error', error.message);
      return <CodeEditor key={language} ref={node => { if (node) editors.current[language] = node; else delete editors.current[language]; }} language={language} label={`${label} code`} text={value} projection={drafts[language]?.projection ?? projections[language]} hiddenMetadata={language === 'html' && !showMeta} readOnly={!save} active={language === tab} marks={[...marks.values()]} query={language === tab ? query : ''} focusLine={language === tab ? focusLine : null} change={(text, projection) => change(language, text, projection)} cursor={language === 'html' ? follow : () => {}} flush={() => { void commit(language); }} saveProject={saveProject ?? (() => { void synchronize().catch(() => {}); })} find={() => searchInput.current?.focus()} />;
    })}</Suspense>}
    <footer className={`code-pane-note status-${status.kind}`} role="status"><span>{note}</span>
      {status.kind === 'invalid' && <ul className="code-errors">{(status.diagnostics ?? []).slice(0, 3).map((item, index) => <li key={index}><button type="button" className="link-button" onClick={() => { const language = item.language === 'css' ? 'css' : item.language === 'javascript' ? 'javascript' : 'html'; setTab(language); requestAnimationFrame(() => editors.current[language]?.reveal(item.line)); }}>Line {item.line}</button> {item.message}</li>)}</ul>}
      {drafts[tab] && status.kind === 'error' && <button type="button" onClick={() => { setStatus(tab, { kind: 'idle' }); void commit(tab); }}>Try saving again</button>}
      {drafts[tab] && ['invalid', 'conflict', 'error'].includes(status.kind) && <button type="button" disabled={!!flights.current[tab]} onClick={discard}>Discard code changes</button>}
    </footer>
  </section>;
});
