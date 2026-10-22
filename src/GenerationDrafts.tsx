import { useEffect, useState } from 'react';
import { native } from './api';
import { WorkspaceSurface } from './WorkspaceSurface';
import { PagePreview } from './Preview';
import type { DraftReview, GenerationDraft, GenerationJobView, Project } from './types';
import type { GenerationController } from './useGeneration';

export function GenerationDrafts({ job, project, controller, pageId, compact = false, changed }: {
  job: GenerationJobView; project: Project; controller: GenerationController; pageId?: string;
  compact?: boolean; changed?: (job: GenerationJobView) => void;
}) {
  return <>{job.drafts?.filter(draft => !draft.dismissed && !draft.applied && (!pageId || draft.pageId === pageId)).map(draft =>
    <Draft compact={compact} key={draft.id} draft={draft} job={job} project={project} controller={controller} changed={changed} />)}</>;
}

function Draft({ draft, job, project, controller, compact, changed }: {
  draft: GenerationDraft; job: GenerationJobView; project: Project; controller: GenerationController;
  compact?: boolean; changed?: (job: GenerationJobView) => void;
}) {
  const [review, setReview] = useState<DraftReview | null>(null);
  const [confirm, setConfirm] = useState(false);
  const [preview, setPreview] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [copied, setCopied] = useState(false);
  const page = project.pages.find(page => page.id === draft.pageId);
  const areas = job.areas.filter(area => draft.regionIds.includes(area.regionId)).map(area => {
    const index = page?.regions.findIndex(region => region.id === area.regionId) ?? -1;
    return index >= 0 ? `Box ${index + 1}` : 'Area';
  }).join(', ');
  useEffect(() => {
    let active = true;
    setReview(null); setConfirm(false);
    void native<DraftReview>('generation_draft', { jobId: job.jobId, draftId: draft.id, action: 'inspect' }).then(value => {
      if (active) setReview(value);
    }, error => { if (active) setError(String(error)); });
    return () => { active = false; };
  }, [job.jobId, draft.id, project.revision]);
  async function act(action: 'dismiss' | 'apply') {
    setBusy(true); setError('');
    try {
      const value = await controller.draftAction(job.jobId, draft.id, action, review?.revision);
      changed?.(value.job); setConfirm(false); setPreview(false);
    } catch (error) { setError(String(error)); setConfirm(false); }
    finally { setBusy(false); }
  }
  async function copy() {
    try {
      if (navigator.clipboard?.writeText) await navigator.clipboard.writeText(draft.code);
      else {
        const text = document.createElement('textarea'); text.value = draft.code;
        text.style.position = 'fixed'; text.style.opacity = '0'; document.body.append(text);
        try { text.select(); if (!document.execCommand('copy')) throw new Error('Copy failed. Select the retained code below.'); }
        finally { text.remove(); }
      }
      setCopied(true);
    } catch (error) { setError(String(error)); }
  }
  const reason = plainReason(draft.reason);
  const body = <>
    <p className="log-text">{reason}</p>
    {review?.blocked && <p className="field-help">{review.blocked}</p>}
  </>;
  return <section className={compact ? 'generation-status compact' : 'log-outcomes'} aria-label={`${areas || page?.name || 'Reply'} draft`}>
    <span className="log-meta">Draft · {page?.name ?? 'Reply'}{areas && ` · ${areas}`}</span>
    {compact ? <details><summary>{reason}</summary>{body}</details> : body}
    <div className="form-actions">
      {review?.previewable && page && <button type="button" disabled={busy} onClick={() => setPreview(true)}>Preview</button>}
      {review && !review.blocked && <button type="button" disabled={busy || !!controller.operation || controller.canStop} onClick={() => setConfirm(true)}>Apply anyway</button>}
      <button type="button" onClick={() => { void copy(); }}>{copied ? 'Copied' : 'Copy code'}</button>
      <button type="button" disabled={busy} onClick={() => { void act('dismiss'); }}>Dismiss</button>
    </div>
    {confirm && !preview && review && <div className="field-help" role="group" aria-label="Confirm draft application">
      <p>{review.confirmation}</p><button type="button" className="primary" disabled={busy} onClick={() => { void act('apply'); }}>Confirm apply</button>{' '}
      <button type="button" disabled={busy} onClick={() => setConfirm(false)}>Cancel</button>
    </div>}
    {error && <p role="alert">{error}</p>}
    {!compact && <details><summary>Retained code</summary><pre tabIndex={0}>{draft.code}</pre></details>}
    {preview && page && <WorkspaceSurface place="canvas" title="Previewing draft" close={() => setPreview(false)} headerActions={<div className="draft-preview-bar"><button type="button" disabled={busy || !!review?.blocked || !!controller.operation || controller.canStop} onClick={() => setConfirm(true)}>Apply anyway</button><button type="button" onClick={() => setPreview(false)}>Back</button></div>}>
      {confirm && review && <div className="draft-preview-confirm" role="group" aria-label="Confirm draft application"><p>{review.confirmation}</p><button disabled={busy} onClick={() => { void act('apply'); }}>Confirm apply</button><button onClick={() => setConfirm(false)}>Cancel</button></div>}
      {error && <p role="alert">{error}</p>}<PagePreview page={page} revision={project.revision} jobId={job.jobId} draftId={draft.id} /></WorkspaceSurface>}
  </section>;
}

function plainReason(reason: string) {
  const text = reason.replace(/Area [0-9a-f-]+(?:, page [0-9a-f-]+)?:\s*/gi, '').split('Excerpt:')[0].trim();
  if (/CSS outside|CSS selector|stylesheet|sharedCss/i.test(text)) return 'Styles affect content outside this area.';
  if (/JavaScript|shared behavior/i.test(text)) return 'Behavior changes affect content outside this area.';
  return text.split('\n')[0].replace(/ Your page is unchanged\./g, '').slice(0, 240);
}
