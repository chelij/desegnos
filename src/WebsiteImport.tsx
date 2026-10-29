import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent } from 'react';
import { native } from './api';
import { Notice } from './ProblemBanner.js';
import { EXTRACTOR_SCRIPT } from './websiteImportExtract';
import type { WebCapture, WebsiteImportPageView, WebsiteImportResultView, WebsiteImportSourceView } from './types';
import './shell.css';

type Props = {
  busy: boolean;
  onImported: (result: WebsiteImportResultView) => void;
  close: () => void;
};

type Phase = 'idle' | 'reading' | 'rendering' | 'applying';

function messageOf(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

/** Read a website, render its pages locally, and turn the captures into canvas boundaries. */
export function WebsiteImport({ busy, onImported, close }: Props) {
  const [address, setAddress] = useState('');
  const [source, setSource] = useState<WebsiteImportSourceView | null>(null);
  const [phase, setPhase] = useState<Phase>('idle');
  const [progress, setProgress] = useState({ done: 0, total: 0, label: '' });
  const [error, setError] = useState('');
  const [warnings, setWarnings] = useState<string[]>([]);
  const [frame, setFrame] = useState<{ id: string; src: string } | null>(null);
  const pending = useRef<{ id: string; resolve: (capture: WebCapture) => void; reject: (reason: Error) => void; timer: number } | null>(null);
  const stopped = useRef(false);
  const phaseRef = useRef<Phase>('idle');
  phaseRef.current = phase;
  const working = phase !== 'idle';
  const locked = busy || working;
  const origin = useMemo(() => { try { return source ? new URL(source.baseUrl).origin : ''; } catch { return ''; } }, [source?.baseUrl]);

  const finishCapture = useCallback((id: string, errorMessage?: string, capture?: WebCapture) => {
    const request = pending.current;
    if (!request || request.id !== id) return;
    pending.current = null;
    window.clearTimeout(request.timer);
    setFrame(null);
    if (errorMessage || !capture) request.reject(new Error(errorMessage ?? 'That page could not be read.'));
    else request.resolve(capture);
  }, []);

  useEffect(() => {
    const receive = (event: MessageEvent) => {
      if (origin && event.origin !== origin) return;
      const data = event.data as { type?: string; id?: string; capture?: WebCapture; message?: string } | null;
      if (!data || !data.id || !pending.current || data.id !== pending.current.id) return;
      if (data.type === 'desegnos-extract' && data.capture) finishCapture(data.id, undefined, data.capture);
      else if (data.type === 'desegnos-extract-error') finishCapture(data.id, data.message || 'That page could not be read.');
    };
    window.addEventListener('message', receive);
    return () => window.removeEventListener('message', receive);
  }, [origin, finishCapture]);

  useEffect(() => () => {
    if (pending.current) {
      window.clearTimeout(pending.current.timer);
      pending.current.reject(new Error('The import was stopped.'));
      pending.current = null;
    }
    if (phaseRef.current !== 'applying') void native('cancel_website_import').catch(() => {});
  }, []);

  async function readUrl(event: FormEvent) {
    event.preventDefault();
    if (!address.trim() || locked) return;
    setError(''); setWarnings([]); setSource(null); setPhase('reading');
    try {
      const view = await native<WebsiteImportSourceView>('prepare_website_import_url', { url: address.trim(), extractor: EXTRACTOR_SCRIPT });
      setSource(view);
    } catch (reason) {
      setError(messageOf(reason));
    } finally {
      setPhase('idle');
    }
  }

  async function pick(kind: 'folder' | 'zip') {
    if (locked) return;
    setError(''); setWarnings([]); setSource(null); setPhase('reading');
    try {
      const view = await native<WebsiteImportSourceView | null>('pick_website_import_source', { kind, extractor: EXTRACTOR_SCRIPT });
      if (view) setSource(view);
    } catch (reason) {
      setError(messageOf(reason));
    } finally {
      setPhase('idle');
    }
  }

  function capturePage(page: WebsiteImportPageView, width: number): Promise<WebCapture> {
    if (!source) return Promise.reject(new Error('Read a website first.'));
    const id = crypto.randomUUID();
    return new Promise<WebCapture>((resolve, reject) => {
      const timer = window.setTimeout(() => finishCapture(id, 'That page took too long to render.'), 32000);
      pending.current = { id, resolve: capture => resolve({ ...capture, site: source.name, route: page.route,
        scripts: capture.scripts.map(script => ({ ...script, src: script.src?.startsWith(source.baseUrl) ? script.src.slice(source.baseUrl.length) : script.src })) }), reject, timer };
      setFrame({ id, src: `${source.extractorUrl}?id=${encodeURIComponent(id)}&url=${encodeURIComponent(page.url)}&width=${width}` });
    });
  }

  async function runImport() {
    if (!source || locked) return;
    stopped.current = false;
    setError(''); setWarnings([]); setPhase('rendering');
    const captures: WebCapture[] = [];
    const notes: string[] = [];
    const pages = source.pages.slice(0, 30);
    const total = pages.length * 2;
    setProgress({ done: 0, total, label: 'Rendering pages at 1440 px and 390 px…' });
    try {
      for (const [index, page] of pages.entries()) {
        setProgress({ done: index * 2, total, label: `Rendering ${page.title || page.route}…` });
        const desktop = await capturePage(page, 1440);
        captures.push(desktop);
        setProgress({ done: index * 2 + 1, total, label: `Rendering ${page.title || page.route} on a phone…` });
        try {
          captures.push(await capturePage(page, 390));
        } catch (reason) {
          if (stopped.current) throw reason;
          // A page that cannot render at phone width still imports at its base size.
          const note = `${page.title || page.route}: the phone size was skipped.`;
          notes.push(note);
          setWarnings(items => [...items, note]);
        }
      }
      setPhase('applying');
      setProgress({ done: total, total, label: 'Adding pages to the canvas…' });
      const result = await native<WebsiteImportResultView>('import_website_pages', { captures });
      setWarnings(items => [...items, ...result.warnings]);
      onImported({ ...result, warnings: [...notes, ...result.warnings] });
      setPhase('idle');
      close();
    } catch (reason) {
      if (!stopped.current) setError(messageOf(reason));
      setPhase('idle');
    }
  }

  async function cancel() {
    if (phaseRef.current === 'applying') return;
    stopped.current = true;
    if (pending.current) finishCapture(pending.current.id, 'The import was stopped.');
    await native('cancel_website_import').catch(() => {});
    setSource(null); setPhase('idle');
  }

  return <div className="dialog-body website-import">
    {frame && <iframe key={frame.id} className="website-import-frame" title="Rendering imported page" src={frame.src}
      sandbox="allow-scripts allow-same-origin" tabIndex={-1} aria-hidden="true" />}
    <form className="website-import-source" onSubmit={readUrl}>
      <label>Website address
        <input value={address} onChange={event => setAddress(event.target.value)} placeholder="example.com" disabled={locked} spellCheck={false} />
      </label>
      <div className="form-actions">
        <button className="primary" disabled={locked || !address.trim()}>Read website</button>
        <button type="button" disabled={locked} onClick={() => { void pick('folder'); }}>Choose folder…</button>
        <button type="button" disabled={locked} onClick={() => { void pick('zip'); }}>Choose ZIP…</button>
      </div>
      <p className="field-help">Pages are read from your computer or the public website. Their code does not run during the import.</p>
    </form>
    {source && <section className="website-import-pages" aria-label="Pages to import">
      <div className="website-import-heading"><div><strong>{source.name}</strong><p className="field-help">{source.pages.length} {source.pages.length === 1 ? 'page' : 'pages'} found. Each becomes a webpage boundary.</p></div>
        <button type="button" disabled={locked} onClick={() => { void cancel(); }}>Choose another</button></div>
      <ul className="website-import-list">{source.pages.slice(0, 30).map(page => <li key={page.route}>{page.title ? <><span>{page.title}</span> <span className="muted">{page.route}</span></> : <span>{page.route}</span>}</li>)}</ul>
      {source.pages.length > 30 && <p className="field-help">Only the first 30 pages are imported.</p>}
      <div className="form-actions"><button className="primary" disabled={locked} onClick={() => { void runImport(); }}>Import {Math.min(source.pages.length, 30) > 1 ? `${Math.min(source.pages.length, 30)} pages` : 'page'}</button>
        {working && phase !== 'applying' && <button type="button" disabled={busy} onClick={() => { void cancel(); }}>Stop</button>}</div>
    </section>}
    {working && <div className="identity-progress" role="status"><span>{progress.label}{progress.total > 0 ? ` ${Math.min(progress.done, progress.total)} of ${progress.total}` : ''}</span></div>}
    {/* A failed import stays in the panel, beside the button that retries it. */}
    {error && <p className="field-help field-error" role="alert">{error}</p>}
    {warnings.map((warning, index) => <Notice key={`${index}:${warning}`} variant="warning" message={warning} dismiss={() => setWarnings(items => items.filter((_, position) => position !== index))} />)}
  </div>;
}
