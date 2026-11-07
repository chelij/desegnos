import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { native } from './api';
import { Notice } from './ProblemBanner.js';
import { SIZE_PRESETS } from './generated/limits';
import { EXTRACTOR_SCRIPT } from './websiteImportExtract';
import type { WebCapture, WebsiteImportPageView, WebsiteImportResultView, WebsiteImportSourceView } from './types';

type Progress = { done: number; total: number; label: string };

/** The one page being rendered: the frame showing it and the promise waiting for it. */
type Capture = { id: string; src: string; timer: number; resolve: (capture: WebCapture) => void; reject: (reason: Error) => void };

type Phase =
  | { kind: 'idle' }
  | { kind: 'reading'; progress: Progress }
  | { kind: 'rendering'; progress: Progress; capture: Capture | null }
  | { kind: 'applying'; progress: Progress }
  | { kind: 'imported' };

function messageOf(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

/** Read a website, render its pages locally, and turn the captures into canvas boundaries. */
export function useWebsiteImport(busy: boolean, onImported: (result: WebsiteImportResultView) => void) {
  const serial = useRef(0);
  const [source, setSource] = useState<WebsiteImportSourceView | null>(null);
  const [phase, setPhase] = useState<Phase>({ kind: 'idle' });
  const [error, setError] = useState('');
  const [warnings, setWarnings] = useState<string[]>([]);
  const stopped = useRef(false);
  const phaseRef = useRef<Phase>(phase);
  phaseRef.current = phase;
  const working = phase.kind === 'reading' || phase.kind === 'rendering' || phase.kind === 'applying';
  const locked = busy || working;
  const origin = useMemo(() => { try { return source ? new URL(source.baseUrl).origin : ''; } catch { return ''; } }, [source?.baseUrl]);

  const finishCapture = useCallback((id: string, errorMessage?: string, capture?: WebCapture) => {
    const current = phaseRef.current;
    if (current.kind !== 'rendering' || current.capture?.id !== id) return;
    const { timer, resolve, reject } = current.capture;
    window.clearTimeout(timer);
    setPhase(state => state.kind === 'rendering' && state.capture?.id === id ? { ...state, capture: null } : state);
    if (errorMessage || !capture) reject(new Error(errorMessage ?? 'That page could not be read.'));
    else resolve(capture);
  }, []);

  useEffect(() => {
    const receive = (event: MessageEvent) => {
      if (origin && event.origin !== origin) return;
      const data = event.data as { type?: string; id?: string; capture?: WebCapture; message?: string } | null;
      if (!data || !data.id) return;
      if (data.type === 'desegnos-extract' && data.capture) finishCapture(data.id, undefined, data.capture);
      else if (data.type === 'desegnos-extract-error') finishCapture(data.id, data.message || 'That page could not be read.');
    };
    window.addEventListener('message', receive);
    return () => window.removeEventListener('message', receive);
  }, [origin, finishCapture]);

  useEffect(() => () => {
    serial.current += 1;
    stopped.current = true;
    const current = phaseRef.current;
    if (current.kind === 'rendering' && current.capture) {
      window.clearTimeout(current.capture.timer);
      current.capture.reject(new Error('The import was stopped.'));
    }
    if (current.kind !== 'applying') void native('cancel_website_import').catch(error => setError(messageOf(error)));
  }, []);

  async function read(operation: () => Promise<WebsiteImportSourceView | null>) {
    if (locked) return null;
    const request = ++serial.current;
    setError(''); setWarnings([]); setPhase({ kind: 'reading', progress: { done: 0, total: 0, label: 'Reading source…' } });
    try {
      const view = await operation();
      if (request !== serial.current) return null;
      if (view) { setSource(view); setWarnings(view.skipped); }
      return view;
    } catch (reason) {
      if (request === serial.current) setError(messageOf(reason));
    } finally {
      if (request === serial.current) setPhase({ kind: 'idle' });
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
      const src = `${source.extractorUrl}?id=${encodeURIComponent(id)}&url=${encodeURIComponent(page.url)}&width=${width}`;
      const resolveCapture = (capture: WebCapture) => resolve({ ...capture, site: source.name, route: page.route,
        scripts: capture.scripts.map(script => ({ ...script, src: script.src?.startsWith(source.baseUrl) ? script.src.slice(source.baseUrl.length) : script.src })) });
      setPhase(current => current.kind === 'rendering' ? { ...current, capture: { id, src, timer, resolve: resolveCapture, reject } } : current);
    });
  }

  async function runImport() {
    if (!source || locked || phase.kind === 'imported') return null;
    stopped.current = false;
    setError(''); setWarnings(source.skipped);
    const captures: WebCapture[] = [];
    const notes: string[] = [...source.skipped];
    const pages = source.pages.slice(0, 30);
    const total = pages.length * 2;
    const report = (progress: Progress) => setPhase(current => current.kind === 'rendering' ? { ...current, progress } : current);
    setPhase({ kind: 'rendering', progress: { done: 0, total, label: `Rendering pages at ${SIZE_PRESETS.desktop.width} px and ${SIZE_PRESETS.phone.width} px…` }, capture: null });
    try {
      for (const [index, page] of pages.entries()) {
        if (stopped.current) throw new Error('The import was stopped.');
        report({ done: index * 2, total, label: `Rendering ${page.title || page.route}…` });
        try {
          captures.push(await capturePage(page, SIZE_PRESETS.desktop.width));
        } catch (reason) {
          if (stopped.current) throw reason;
          const note = `${page.title || page.route} (${page.route}): skipped because it could not render. ${messageOf(reason)}`;
          notes.push(note);
          setWarnings(items => [...items, note]);
          continue;
        }
        if (stopped.current) throw new Error('The import was stopped.');
        report({ done: index * 2 + 1, total, label: `Rendering ${page.title || page.route} on a phone…` });
        try {
          captures.push(await capturePage(page, SIZE_PRESETS.phone.width));
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
      setPhase({ kind: 'applying', progress: { done: total, total, label: 'Adding pages to the canvas…' } });
      const result = await native<WebsiteImportResultView>('import_website_pages', { captures });
      setWarnings(items => [...items, ...result.warnings]);
      onImported({ ...result, warnings: [...notes, ...result.warnings] });
      setPhase({ kind: 'imported' });
      // What this import left out beyond the files the read already listed.
      return [...notes.slice(source.skipped.length), ...result.warnings];
    } catch (reason) {
      if (!stopped.current) setError(messageOf(reason));
      setPhase({ kind: 'idle' });
      return null;
    }
  }

  async function cancel() {
    const current = phaseRef.current;
    if (current.kind === 'applying') return;
    serial.current += 1;
    stopped.current = true;
    if (current.kind === 'rendering' && current.capture) finishCapture(current.capture.id, 'The import was stopped.');
    await native('cancel_website_import').catch(error => setError(messageOf(error)));
    setSource(null); setPhase({ kind: 'idle' });
  }

  return { source, working, phase, error, warnings,
    readUrl, pick, runImport, cancel, dismissWarning: (index: number) => setWarnings(items => items.filter((_, position) => position !== index)) };
}

export function WebsiteImportStatus({ flow, busy }: { flow: ReturnType<typeof useWebsiteImport>; busy: boolean }) {
  const phase = flow.phase;
  const progress = phase.kind === 'reading' || phase.kind === 'rendering' || phase.kind === 'applying' ? phase.progress : null;
  const capture = phase.kind === 'rendering' ? phase.capture : null;
  return <>
    {capture && <iframe key={capture.id} className="website-import-frame" title="Rendering imported page" src={capture.src}
      sandbox="allow-scripts allow-same-origin" tabIndex={-1} aria-hidden="true" />}
    {progress && <div className="identity-progress" role="status"><span>{progress.label || 'Reading source…'}{progress.total > 0 ? ` ${Math.min(progress.done, progress.total)} of ${progress.total}` : ''}</span>
      {phase.kind !== 'applying' && <button type="button" disabled={busy} onClick={() => { void flow.cancel(); }}>Stop</button>}
    </div>}
    {flow.error && <p className="field-help field-error" role="alert">{flow.error}</p>}
    {flow.warnings.map((warning, index) => <Notice key={`${index}:${warning}`} variant="warning" message={warning} dismiss={() => flow.dismissWarning(index)} />)}
  </>;
}
