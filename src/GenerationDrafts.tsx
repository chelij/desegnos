import { ProblemBanner } from './ProblemBanner.js';
import { userError, generationError } from './userErrors.js';
import { useEffect, useState } from 'react';
import { native } from './api';
import { pendingDrafts } from './generationDraftState';
import { WorkspaceSurface } from './WorkspaceSurface';
import { PagePreview } from './Preview';
import type { DraftReview, GenerationDraft, GenerationJobView, Project } from './types';
import type { GenerationController } from './useGeneration';

export function GenerationDrafts({ job, project, controller, pageId, compact = false, changed }: {
  job: GenerationJobView; project: Project; controller: GenerationController; pageId?: string;
  compact?: boolean; changed?: (job: GenerationJobView) => void;
}) {
  if (compact) return null;
  return <>{pendingDrafts(job).filter(draft => !pageId || draft.pageId === pageId).map(draft =>
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
    }, error => { if (active) setError('Could not review this draft. It is still saved.'); });
    return () => { active = false; };
  }, [job.jobId, draft.id, project.revision]);
  async function act(action: 'dismiss' | 'apply') {
    setBusy(true); setError('');
    try {
      const value = await controller.draftAction(job.jobId, draft.id, action, review?.revision);
      changed?.(value.job); setConfirm(false); setPreview(false);
    } catch { setError('Could not apply this action. Review the draft and try again.'); setConfirm(false); }
    finally { setBusy(false); }
  }
  async function repair() {
    setBusy(true); setError('');
    try { await controller.repairDraft(job.jobId, draft.id); }
    catch { setError('Could not start the repair. Your draft is still saved.'); }
    finally { setBusy(false); }
  }
  const reasons = [...new Set([draft.reason, review?.blocked].filter(Boolean).map(value => plainReason(value!)))];
  const body = <>
    {reasons.map(reason => <ProblemBanner key={reason} message={reason} action={{ label: 'Try again', disabled: busy || !!controller.operation || controller.canStop, onClick: () => { void controller.prepare().catch(() => {}); } }} />)}
  </>;
  return <section className="generation-draft" aria-label={`${areas || page?.name || 'Reply'} draft`}>
    <span className="log-meta">Draft · {page?.name ?? 'Reply'}{areas && ` · ${areas}`}</span>
    {body}
    <div className="draft-actions">
      {review?.previewable && page && <button type="button" disabled={busy} onClick={() => setPreview(true)}>Preview</button>}
      {review && !review.blocked && <button type="button" disabled={busy || !!controller.operation || controller.canStop} onClick={() => setConfirm(true)}>Apply anyway</button>}
      <button type="button" disabled={busy || !!controller.operation || controller.canStop} onClick={() => { void repair(); }}>Ask AI to fix</button>
      <button type="button" disabled={busy} onClick={() => { void act('dismiss'); }}>Dismiss</button>
    </div>
    {confirm && !preview && review && <div className="field-help" role="group" aria-label="Confirm draft application">
      <p>Replace this page with the saved draft? You can undo this change.</p><button type="button" className="primary" disabled={busy} onClick={() => { void act('apply'); }}>Confirm apply</button>{' '}
      <button type="button" disabled={busy} onClick={() => setConfirm(false)}>Cancel</button>
    </div>}
    {error && <ProblemBanner message={userError(error)} instruction="Review the saved draft and try again." dismiss={() => setError('')} />}
    {preview && page && <WorkspaceSurface place="canvas" title="Previewing draft" showClose={false} close={() => setPreview(false)} headerActions={<div className="draft-preview-bar"><button type="button" className="primary" disabled={busy || !!review?.blocked || !!controller.operation || controller.canStop} onClick={() => setConfirm(true)}>Apply anyway</button><button type="button" onClick={() => setPreview(false)}>Back</button></div>}>
      {confirm && review && <div className="draft-preview-confirm" role="group" aria-label="Confirm draft application"><p>Replace this page with the saved draft? You can undo this change.</p><button disabled={busy} onClick={() => { void act('apply'); }}>Confirm apply</button><button onClick={() => setConfirm(false)}>Cancel</button></div>}
      {error && <ProblemBanner message={userError(error)} instruction="Review the saved draft and try again." dismiss={() => setError('')} />}<PagePreview page={page} revision={project.revision} jobId={job.jobId} draftId={draft.id} /></WorkspaceSurface>}
  </section>;
}

export const plainReason = generationError;
