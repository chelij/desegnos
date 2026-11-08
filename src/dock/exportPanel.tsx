import { useEffect, useState } from 'react';
import { userError, providerProblem } from '../userErrors.js';
import type { OutputType, Project } from '../types';
import type { ExportJobs } from '../exportJobs/useExportJobs';

const money = (value: number) => new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD', minimumFractionDigits: 2, maximumFractionDigits: 4 }).format(value);

const OUTPUT_CHOICES: [string, string][] = [
  ['webpage', 'Page'],
  ['slide', 'Slide'],
  ['image', 'Image'],
];

const STATUS_LABELS: Record<string, string> = {
  prepared: 'Ready',
  sending: 'Exporting…',
  ready: 'Exported',
  failed: 'Could not export',
  cancelled: 'Cancelled',
  stale: 'Out of date',
};

/** The inline export panel the dock shows when Export is chosen. No modal dialog. */
export function ExportPanel({ project, exports, busy, selectBoundary, setIncluded, setOutput, close, openSettings }: {
  project: Project; exports: ExportJobs; busy: boolean;
  selectBoundary: (pageId: string) => void; setIncluded: (pageId: string, included: boolean) => void; setOutput: (pageId: string, output: OutputType) => void;
  close: () => void; openSettings: () => void;
}) {
  const { destination, job, sending, error, setDestination, openFolder } = exports;
  // Every action reports its own failure in the panel; a rejection needs no further handling.
  const act = (action: () => Promise<unknown>) => () => { void action(); };
  const chooseDestination = act(exports.chooseDestination);
  const runExport = act(() => exports.run());
  const sendExport = act(() => exports.send());
  const cancelExport = act(exports.cancel);
  const openInBrowser = act(exports.openInBrowser);
  const previewExport = (pageId: string) => { void exports.run(pageId); };
  const boundaries = project.pages;
  const prepared = job?.status === 'prepared';
  const running = job?.status === 'sending';
  const [folder, setFolder] = useState(destination ?? '');
  useEffect(() => { setFolder(destination ?? ''); }, [destination]);
  const commitFolder = () => { if (folder.trim() && folder.trim() !== destination) setDestination(folder); };
  return <div className="dock-form export-panel">
    <div className="export-panel-heading">
      <span className="dock-field-label">Export</span>
      <button type="button" className="dock-action" onClick={close}>Close</button>
    </div>
    <div className="export-panel-list" role="group" aria-label="Export boundaries">
      {boundaries.map(page => <div key={page.id} className="export-panel-row">
        <input type="checkbox" aria-label={`Include ${page.name} in export`} checked={page.included} disabled={busy || running} onChange={event => setIncluded(page.id, event.target.checked)} />
        <button type="button" className="dock-action export-panel-name" onClick={() => selectBoundary(page.id)}>{page.name}</button>
        <select aria-label={`Export ${page.name} as`} value={page.output} disabled={busy || running} onChange={event => setOutput(page.id, event.target.value as OutputType)}>
          {OUTPUT_CHOICES.map(([value, label]) => <option key={value} value={value}>{label}</option>)}
        </select>
        {page.output === 'slide' || page.output === 'image'
          ? null
          : <button type="button" className="dock-action" disabled={busy || running} onClick={() => previewExport(page.id)}>Preview</button>}
      </div>)}
    </div>
    <label className="dock-field">Folder
      <input value={folder} placeholder="Choose a folder" spellCheck={false} disabled={busy || running}
        onChange={event => setFolder(event.target.value)} onBlur={commitFolder}
        onKeyDown={event => { if (event.key === 'Enter') { event.preventDefault(); commitFolder(); } else if (event.key === 'Escape') setFolder(destination ?? ''); }} />
    </label>
    <button type="button" className="dock-action" disabled={busy || running} onClick={chooseDestination}>Choose folder…</button>
    {error && !job?.providerError && <p className="dock-error">{userError(error)}</p>}
    {job && <div className="export-panel-job" role="status">
      <p><strong>{prepared && sending ? 'Writing behaviour…' : STATUS_LABELS[job.status] ?? job.status}</strong>{job.status === 'prepared' && job.estimateUsd > 0 ? ` · ${money(job.estimateUsd)} estimate` : ''}</p>
      {!(prepared && sending) && <p>{job.providerError ? userError(job) : job.message}</p>}
      {job.kind === 'code' && <p className="muted">Export {job.pages.length} {job.pages.length === 1 ? 'boundary' : 'boundaries'} · {job.sizeCount} {job.sizeCount === 1 ? 'size' : 'sizes'} · {job.pointerCount} {job.pointerCount === 1 ? 'behaviour' : 'behaviours'}{job.reusedBehaviourCount ? ` · ${job.reusedBehaviourCount} ${job.reusedBehaviourCount === 1 ? 'behaviour' : 'behaviours'} reused` : ''}</p>}
      {job.summary && <details className="export-panel-notes"><summary>AI notes</summary><p className="muted">{job.summary}</p></details>}
      {job.usage && <p className="muted">{job.usage.inputTokens.toLocaleString()} in · {job.usage.outputTokens.toLocaleString()} out tokens{job.attempts.length > 1 ? ` · ${job.attempts.length} attempts` : ''}</p>}
      {job.pages.length > 0 && <details><summary>Files</summary><ul className="export-panel-files">{job.pages.map(page => <li key={page.pageId}>{page.name} → {page.file || page.output}</li>)}</ul></details>}
      {job.warnings.map(warning => <p key={warning} className="dock-warning">{warning}</p>)}
      {job.result && <div className="form-actions">
        <button type="button" className="dock-action" onClick={() => job.result && openFolder(job.result.path)}>Open folder</button>
        <button type="button" className="dock-action" onClick={openInBrowser}>Open in browser</button>
      </div>}
    </div>}
    <div className="form-actions export-panel-actions">
      {!running && <button type="button" className={prepared ? 'dock-action' : 'primary'} disabled={busy || !destination || !boundaries.some(page => page.included)} onClick={runExport}>{prepared ? 'Start a new export' : 'Export'}</button>}
      {prepared && <button type="button" className="primary" disabled={sending} onClick={sendExport}>{sending ? 'Sending…' : job?.kind === 'comment' ? 'Send to rewrite the comment' : 'Generate behaviour'}</button>}
      {prepared && <button type="button" className="dock-action" disabled={sending} onClick={cancelExport}>Cancel</button>}
      {running && <button type="button" className="dock-action" onClick={cancelExport}>Cancel</button>}
      {(job?.status === 'failed' || job?.status === 'cancelled' || job?.status === 'stale') && <button type="button" className="dock-action" disabled={sending} onClick={providerProblem(job.providerError, job.provider)?.settings ? openSettings : sendExport}>{providerProblem(job.providerError, job.provider)?.action ?? 'Try again'}</button>}
    </div>
  </div>;
}
