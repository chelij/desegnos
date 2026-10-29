import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { native } from './api';
import { Notice } from './ProblemBanner.js';
import { EXTRACTOR_SCRIPT } from './websiteImportExtract';
import type { WebCapture, WebsiteImportPageView, WebsiteImportResultView, WebsiteImportSourceView } from './types';
import './shell.css';

type Phase = 'idle' | 'reading' | 'rendering' | 'applying';

function messageOf(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

/** Read a website, render its pages locally, and turn the captures into canvas boundaries. */
export function useWebsiteImport(busy: boolean, onImported: (result: WebsiteImportResultView) => void) {
  const [imported, setImported] = useState(false);
  const serial = useRef(0);
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
    serial.current += 1;
    stopped.current = true;
    if (pending.current) {
      window.clearTimeout(pending.current.timer);
      pending.current.reject(new Error('The import was stopped.'));
      pending.current = null;
    }
    if (phaseRef.current !== 'applying') void native('cancel_website_import').catch(() => {});
  }, []);

  async function read(operation: () => Promise<WebsiteImportSourceView | null>) {
    if (locked) return null;
    const request = ++serial.current;
    setError(''); setWarnings([]); setProgress({ done: 0, total: 0, label: 'Reading source…' }); setPhase('reading');
    try {
      const view = await operation();
      if (request !== serial.current) return null;
      if (view) { setSource(view); setImported(false); setWarnings(view.skipped); }
      return view;
    } catch (reason) {
      if (request === serial.current) setError(messageOf(reason));
    } finally {
      if (request === serial.current) setPhase('idle');
    }
    return null;
  }

  function readUrl(address: string) {
    if (!address.trim()) return Promise.resolve(null);
    return read(() => native<WebsiteImportSourceView>('prepare_website_import_url', { url: address.trim(), extractor: EXTRACTOR_SCRIPT }));
  }

  function pick(kind: 'folder' | 'zip' | 'file') {
    return read(() => native<WebsiteImportSourceView | null>('pick_website_import_source', { kind, extractor: EXTRACTOR_SCRIPT }));
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
    if (!source || locked || imported) return false;
    stopped.current = false;
    setError(''); setWarnings(source.skipped); setPhase('rendering');
    const captures: WebCapture[] = [];
    const notes: string[] = [...source.skipped];
    const pages = source.pages.slice(0, 30);
    const total = pages.length * 2;
    setProgress({ done: 0, total, label: 'Rendering pages at 1440 px and 390 px…' });
    try {
      for (const [index, page] of pages.entries()) {
        if (stopped.current) throw new Error('The import was stopped.');
        setProgress({ done: index * 2, total, label: `Rendering ${page.title || page.route}…` });
        try {
          captures.push(await capturePage(page, 1440));
        } catch (reason) {
          if (stopped.current) throw reason;
          const note = `${page.title || page.route} (${page.route}): skipped because it could not render. ${messageOf(reason)}`;
          notes.push(note);
          setWarnings(items => [...items, note]);
          continue;
        }
        if (stopped.current) throw new Error('The import was stopped.');
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
      if (stopped.current) throw new Error('The import was stopped.');
      if (!captures.length) throw new Error('No page could be rendered.');
      setPhase('applying');
      setProgress({ done: total, total, label: 'Adding pages to the canvas…' });
      const result = await native<WebsiteImportResultView>('import_website_pages', { captures });
      setWarnings(items => [...items, ...result.warnings]);
      onImported({ ...result, warnings: [...notes, ...result.warnings] });
      setImported(true);
      setPhase('idle');
      return true;
    } catch (reason) {
      if (!stopped.current) setError(messageOf(reason));
      setPhase('idle');
      return false;
    }
  }

  async function cancel() {
    if (phaseRef.current === 'applying') return;
    serial.current += 1;
    stopped.current = true;
    if (pending.current) finishCapture(pending.current.id, 'The import was stopped.');
    await native('cancel_website_import').catch(() => {});
    setSource(null); setPhase('idle');
  }

  return { source, imported, working, phase, progress, error, warnings, frame,
    readUrl, pick, runImport, cancel, dismissWarning: (index: number) => setWarnings(items => items.filter((_, position) => position !== index)) };
}

export function WebsiteImportStatus({ flow, busy }: { flow: ReturnType<typeof useWebsiteImport>; busy: boolean }) {
  return <>
    {flow.frame && <iframe key={flow.frame.id} className="website-import-frame" title="Rendering imported page" src={flow.frame.src}
      sandbox="allow-scripts allow-same-origin" tabIndex={-1} aria-hidden="true" />}
    {flow.working && <div className="identity-progress" role="status"><span>{flow.progress.label || 'Reading source…'}{flow.progress.total > 0 ? ` ${Math.min(flow.progress.done, flow.progress.total)} of ${flow.progress.total}` : ''}</span>
      {flow.phase !== 'applying' && <button type="button" disabled={busy} onClick={() => { void flow.cancel(); }}>Stop</button>}
    </div>}
    {flow.error && <p className="field-help field-error" role="alert">{flow.error}</p>}
    {flow.warnings.map((warning, index) => <Notice key={`${index}:${warning}`} variant="warning" message={warning} dismiss={() => flow.dismissWarning(index)} />)}
  </>;
}
