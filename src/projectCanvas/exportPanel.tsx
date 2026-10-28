import type { ExportJobView, OutputType, Project } from '../types';

const money = (value: number) => new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD', minimumFractionDigits: 2, maximumFractionDigits: 4 }).format(value);

export const OUTPUT_CHOICES: [string, string][] = [
  ['webpage', 'Web'],
  ['appScreen', 'App'],
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
export function ExportPanel({ project, destination, busy, job, sending, error, chooseDestination, runExport, sendExport, cancelExport, retryExport, openFolder, openInBrowser, previewExport, selectBoundary, setIncluded, setOutput, close }: {
  selectBoundary: (pageId: string) => void; setIncluded: (pageId: string, included: boolean) => void; setOutput: (pageId: string, output: OutputType) => void;
  project: Project; destination: string | null; busy: boolean;
  job: ExportJobView | null; sending: boolean; error: string;
  chooseDestination: () => void; runExport: () => void; sendExport: () => void; cancelExport: () => void; retryExport: () => void;
  openFolder: (path: string) => void; openInBrowser: () => void; previewExport: (pageId: string) => void; close: () => void;
}) {
  const boundaries = project.pages;
  const prepared = job?.status === 'prepared';
  const running = job?.status === 'sending';
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
      <input readOnly value={destination ?? 'Choose a folder'} onClick={chooseDestination} />
    </label>
    <button type="button" className="dock-action" disabled={busy || running} onClick={chooseDestination}>Choose folder…</button>
    {error && <p className="dock-error">{error}</p>}
    {job && <div className="export-panel-job" role="status">
      <p><strong>{STATUS_LABELS[job.status] ?? job.status}</strong>{job.status === 'prepared' && job.estimateUsd > 0 ? ` · ${money(job.estimateUsd)} estimate` : ''}</p>
      <p>{job.message}</p>
      {job.kind === 'code' && <p className="muted">Export {job.pages.length} {job.pages.length === 1 ? 'boundary' : 'boundaries'} · {job.sizeCount} {job.sizeCount === 1 ? 'size' : 'sizes'} · {job.pointerCount} {job.pointerCount === 1 ? 'pointer' : 'pointers'}</p>}
      {job.summary && <p className="muted">{job.summary}</p>}
      {job.usage && <p className="muted">{job.usage.inputTokens.toLocaleString()} in · {job.usage.outputTokens.toLocaleString()} out tokens{job.attempts.length > 1 ? ` · ${job.attempts.length} attempts` : ''}</p>}
      {job.pages.length > 0 && <details><summary>Files</summary><ul className="export-panel-files">{job.pages.map(page => <li key={page.pageId}>{page.name} → {page.file || page.output}</li>)}</ul></details>}
      {job.warnings.map(warning => <p key={warning} className="dock-warning">{warning}</p>)}
      {job.result && <div className="form-actions">
        <button type="button" className="dock-action" onClick={() => job.result && openFolder(job.result.path)}>Open folder</button>
        <button type="button" className="dock-action" onClick={openInBrowser}>Open in browser</button>
      </div>}
    </div>}
    <div className="form-actions">
      {!running && <button type="button" className={prepared ? 'dock-action' : 'primary'} disabled={busy || !destination || !boundaries.some(page => page.included)} onClick={runExport}>{prepared ? 'Start a new export' : 'Export'}</button>}
      {prepared && <button type="button" className="primary" disabled={sending} onClick={sendExport}>{sending ? 'Sending…' : job?.kind === 'comment' ? 'Send to rewrite the comment' : 'Generate behaviour'}</button>}
      {prepared && <button type="button" className="dock-action" disabled={sending} onClick={cancelExport}>Cancel</button>}
      {running && <button type="button" className="dock-action" onClick={cancelExport}>Cancel</button>}
      {(job?.status === 'failed' || job?.status === 'cancelled' || job?.status === 'stale') && <button type="button" className="dock-action" disabled={sending} onClick={retryExport}>Try again</button>}
    </div>
  </div>;
}
