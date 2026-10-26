import { Component, useEffect, useState, type ErrorInfo, type ReactNode } from 'react';
import { native } from './api';
import { Notice, ProblemBanner } from './ProblemBanner';
import { CRASH_NOTICE, crashOpenFailure, type PendingCrash } from './crashReportText';

let editorFailed = false;
let reported = false;

function recordEditorCrash(error: unknown, componentStack = '') {
  if (reported) return;
  reported = true;
  const message = error instanceof Error ? error.message : String(error ?? 'The editor failed to render.');
  const stack = [error instanceof Error ? error.stack ?? '' : '', componentStack].filter(Boolean).join('\n');
  void native('record_frontend_crash', { message: message.slice(0, 2000), stack: stack.slice(0, 8000) }).catch(() => {});
}

// Render failures are the only frontend crashes worth a report. Ordinary unhandled
// rejections are ignored unless the editor itself has already failed.
export class EditorCrashBoundary extends Component<{ children: ReactNode }, { failed: boolean }> {
  state: { failed: boolean } = { failed: false };
  static getDerivedStateFromError() { return { failed: true }; }
  componentDidCatch(error: Error, info: ErrorInfo) {
    editorFailed = true;
    recordEditorCrash(error, info.componentStack ?? '');
  }
  render() {
    return this.state.failed
      ? <main className="startup"><h1>Desegnos stopped working.</h1><button type="button" onClick={() => window.location.reload()}>Reload</button></main>
      : this.props.children;
  }
}

if (typeof window !== 'undefined') {
  const afterFailure = (value: unknown) => { if (editorFailed) recordEditorCrash(value); };
  window.addEventListener('error', event => afterFailure(event.error ?? event.message));
  window.addEventListener('unhandledrejection', event => afterFailure(event.reason));
}

export function CrashReportNotice() {
  const [report, setReport] = useState<PendingCrash | null>(null);
  const [failure, setFailure] = useState<{ url: string | null; message: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    let live = true;
    native<PendingCrash | null>('pending_crash_report')
      .then(value => { if (live) setReport(value); })
      .catch(() => {});
    return () => { live = false; };
  }, []);
  const dismiss = () => {
    setReport(null); setFailure(null);
    void native('dismiss_crash_report').catch(() => {});
  };
  const open = async () => {
    setBusy(true);
    try { await native('open_crash_report'); setReport(null); setFailure(null); }
    catch (error) { setFailure(crashOpenFailure(error)); }
    finally { setBusy(false); }
  };
  const copy = () => {
    void native('copy_text', { text: failure?.url }).then(() => setCopied(true)).catch(() => {});
  };
  const url = failure?.url;
  return <>
    {report && <Notice
      message={CRASH_NOTICE}
      action={{ label: 'Report on GitHub', disabled: busy, onClick: () => { void open(); } }}
      secondaryAction={{ label: 'Dismiss', onClick: dismiss }}
      showDismiss={false}
      details={{ summary: "What's included", body: report.body }}
    />}
    {failure && url && <Notice
      variant="warning"
      title="Couldn't open your browser"
      message={url}
      action={{ label: copied ? 'Copied' : 'Copy link', disabled: copied, onClick: copy }}
      dismiss={() => setFailure(null)}
    />}
    {failure && !url && <ProblemBanner message={failure.message} dismiss={() => setFailure(null)} />}
  </>;
}
