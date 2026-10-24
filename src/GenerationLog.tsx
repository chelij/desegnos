import { ProblemBanner } from './ProblemBanner.js';
import { userError, generationError } from './userErrors.js';
import { areaOutcome, fixLabel, jobOutcome, jsonBlocks } from './generationProblems.js';
import { useEffect, useRef, useState } from 'react';
import { WorkspaceSurface } from './WorkspaceSurface';
import type { AiSettingsView } from './types';
import type { GenerationController } from './useGeneration';
import { native } from './api';
import { scopeLabel } from './Notes';
import type { GenerationHistory, GenerationJobView, Project } from './types';

// Current and locally saved conversations share the same draft review actions.
const SERVICE = { openai: 'OpenAI', anthropic: 'Anthropic', google: 'Google', openrouter: 'OpenRouter', local: 'Local model' } as const;
const kb = (bytes: number) => bytes < 1024 ? `${bytes} bytes` : `${(bytes / 1024).toFixed(1)} KB`;

export function GenerationLog({ job: currentJob, operation, error, project, controller, close, settings, openSettings, openImport, openBrief, manageUsage, editRequest, resultApplied }: {
  resultApplied?: boolean;
  settings: AiSettingsView | null; openSettings: () => void; openImport?: () => void; openBrief?: () => void; manageUsage: () => Promise<void>; editRequest: (id: string) => void;
  job: GenerationJobView | null; operation: string | null; error: string | null; project: Project; controller: GenerationController; close: () => void;
}) {
  const [history, setHistory] = useState<GenerationHistory | null>(null);
  const [past, setPast] = useState<GenerationJobView | null>(null);
  const [historyError, setHistoryError] = useState('');
  const [loadingPast, setLoadingPast] = useState(false);
  const historyEpoch = useRef(0);
  useEffect(() => {
    let active = true;
    historyEpoch.current += 1; setPast(null); setHistory(null); setLoadingPast(false); setHistoryError('');
    void native<GenerationHistory>('list_generations', { projectId: project.id }).then(value => { if (active) setHistory(value); }, error => { if (active) setHistoryError(String(error)); });
    return () => { active = false; };
  }, [project.id, currentJob?.jobId, currentJob?.status]);
  async function showPast(jobId: string) {
    const epoch = ++historyEpoch.current;
    setLoadingPast(true); setHistoryError('');
    try { const value = await native<GenerationJobView>('get_past_generation', { projectId: project.id, jobId }); if (epoch === historyEpoch.current) setPast(value); }
    catch (error) { if (epoch === historyEpoch.current) setHistoryError(String(error)); }
    finally { if (epoch === historyEpoch.current) setLoadingPast(false); }
  }
  const job = past ?? currentJob;
  const frozen = jsonBlocks(job?.requirements.markdown ?? '').filter(value => value?.regionId && value?.pageId);
  const context = jsonBlocks(job?.context?.markdown.split('\n## Requested context\n')[1] ?? '').flatMap(value => Array.isArray(value) ? value : []).filter(value => value?.id);
  const requests = (job?.areas ?? []).map(area => {
    const page = project.pages.find(item => item.id === area.pageId);
    const index = page?.regions.findIndex(region => region.id === area.regionId) ?? -1;
    const region = index >= 0 ? page!.regions[index] : undefined;
    const sent = frozen.find(item => item.regionId === area.regionId);
    const draft = job?.drafts?.filter(draft => draft.regionIds.includes(area.regionId)).at(-1);
    const done = region?.completed || draft?.applied || job?.candidate?.regions.find(outcome => outcome.regionId === area.regionId)?.status === 'implemented';
    const current = region?.prompt;
    return { done, key: area.regionId, completed: region?.completed, page: sent?.pageName || page?.name || 'Removed page', box: index + 1, prompt: sent?.prompt || region?.prompt || '', current, outcome: job ? areaOutcome(job, area.regionId, current) : null, attachments: sent?.attachments ?? (past ? [] : region?.attachments ?? []), phone: !!(sent?.mobileOnly ?? (!past && region?.mobileOnly)) };
  });
  // Only areas whose result can still change are resent; declared gaps need a different request.
  const review = jobOutcome(job, id => requests.find(item => item.key === id)?.current);
  const openFix = (fix: Parameters<typeof fixLabel>[0]) => fix === 'aiSettings' ? openSettings : fix === 'import' ? openImport : openBrief;
  const applyFix = (fix: Parameters<typeof fixLabel>[0]) => { openFix(fix)?.(); };
  const working = !past && (!!operation || job?.status === 'prepared' || job?.status === 'generating' || job?.status === 'ready' || job?.status === 'applying');
  const candidate = job?.candidate;
  const label = (id: string) => {
    const item = requests.find(request => request.key === id);
    return item ? `${item.page} · ${item.box > 0 ? `Box ${item.box}` : item.prompt || 'Request'}` : 'Request';
  };
  return <WorkspaceSurface title="AI conversation" close={close}>
    <div className="log-messages">
      {past && <button type="button" onClick={() => { historyEpoch.current += 1; setLoadingPast(false); setPast(null); }}>Back to current generation</button>}
      {!job && !operation && !error && <p className="log-empty">No conversation yet.</p>}
      {!job && operation && <p role="status">{operation}</p>}
      {job && <>
        <section className="log-message from-you" aria-label="Sent to the AI">
          <p className="log-who">You → {SERVICE[job.provider ?? 'openai']}</p>
          <p>{requests.length} {requests.length === 1 ? 'request' : 'requests'}</p>
          <ol className="log-requests">{requests.map(item => <li key={item.key}>
            <span className="log-meta">{label(item.key)}{item.phone ? ' · Phone fix' : ''} · {item.done ? 'Done' : working ? 'Working' : item.outcome?.retryable === false ? 'Needs your input' : 'Needs attention'}</span>
            {item.prompt && <p className="log-text">{item.prompt}</p>}
            {item.attachments.length > 0 && <p className="log-meta">Attached: {item.attachments.map((attachment: { name: string }) => attachment.name).join(', ')}</p>}
          </li>)}</ol>
        </section>
        <section className="log-message from-ai" aria-label="AI reply" aria-live="polite">
          <p className="log-who">{SERVICE[job.provider ?? 'openai']}</p>
          {working && !candidate && <p className="log-working"><span className="log-dots" aria-hidden="true"><i /><i /><i /></span>{job.receivedBytes ? `Writing… ${kb(job.receivedBytes)} received` : 'Thinking…'}</p>}
          {!past && !working && !resultApplied && (review.retryable.length > 0 || review.retryAll) && <button disabled={!!operation} onClick={() => { void controller.prepare(undefined, review.retryable.length ? review.retryable : undefined).catch(() => {}); }}>Try again</button>}
          {candidate && candidate.criticism.length > 0 && <details className="generation-feedback"><summary>Design feedback ({candidate.criticism.length})</summary><ul>{candidate.criticism.map((item, index) => <li key={index}><p className="log-text">{humanText(item.message)}</p><p className="log-meta">{humanText(item.suggestion)}</p></li>)}</ul></details>}
          {!past && review.blocked.length > 0 && <section className="log-needs-input" aria-label="Needs your input">
            <p className="log-who">Needs your input</p>
            {review.blocked.map(item => <div key={item.regionId}>
              <span className="log-meta">{label(item.regionId)}</span>
              <p className="log-text">{item.reason}</p>
              <div className="log-actions">
                <button type="button" onClick={() => editRequest(item.regionId)}>Change request</button>
                {item.fix && openFix(item.fix) && <button type="button" onClick={() => applyFix(item.fix!)}>{fixLabel(item.fix)}</button>}
              </div>
            </div>)}
          </section>}
          {candidate && candidate.decisionProposals.length > 0 && <details><summary>Suggested rules ({candidate.decisionProposals.length})</summary><ul>{candidate.decisionProposals.map((proposal, index) => <li key={index}><strong>{humanText(proposal.topic)}</strong><p className="log-text">{humanText(proposal.value)}</p><p className="log-meta">{scopeLabel(project, proposal.scope)}</p></li>)}</ul></details>}

          <details className="conversation-request-details"><summary>Request details</summary><dl className="conversation-facts">
            <dt>AI style</dt><dd>{job.generationMode === 'strict' ? 'Precise' : 'Flexible'}</dd>
            <dt>Model</dt><dd>{settings?.catalogue.find(model => model.id === job.settings.model)?.name ?? job.settings.model}</dd>
            {job.repairRounds ? <><dt>Automatic corrections</dt><dd>{job.repairRounds}</dd></> : null}
            {context.length > 0 && <><dt>Additional context</dt><dd>{context.length} page elements</dd></>}
          </dl></details>
          {job.usage && <p className="log-meta token-footer">{job.usage.inputTokens.toLocaleString()} tokens sent · {job.usage.outputTokens.toLocaleString()} received</p>}
        </section>
      </>}
      <details className="log-message" aria-label="Past generations"><summary>Past generations · {history?.jobs.length ?? 0}</summary>
        {history?.jobs.map(item => <p key={item.jobId}><button type="button" disabled={loadingPast} aria-pressed={past?.jobId === item.jobId} onClick={() => { void showPast(item.jobId); }}>
          <span className="log-meta">{item.createdAt ? new Date(item.createdAt).toLocaleString() : 'Earlier generation'} · {statusLabel(item.status)}</span><br />
          <span style={{ display: 'block', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{humanText(item.firstRequest || item.summary).split('\n')[0].slice(0, 160)}</span>{item.usage && <><br /><span className="log-meta">{item.usage.inputTokens.toLocaleString()} sent · {item.usage.outputTokens.toLocaleString()} received</span></>}
        </button></p>)}
        {history?.skipped ? <p className="field-help">{history.skipped} saved records could not be read.</p> : null}
        {history?.truncated && <p className="field-help">Showing the latest 100 saved generations.</p>}
        {historyError && <ProblemBanner message={userError(historyError)} dismiss={() => setHistoryError('')} />}
      </details>
      {(error || job?.message || job?.candidate) && <details><summary>Technical details</summary><pre>{JSON.stringify({ error, message: job?.message, providerError: job?.providerError, validationEvents: job?.validationEvents, outcomes: job?.candidate?.regions, drafts: job?.drafts?.map(({ reason }) => ({ reason })) }, null, 2)}</pre></details>}
    </div>
  </WorkspaceSurface>;
}

function statusLabel(status: string) {
  if (['applied', 'ready'].includes(status)) return 'Done';
  if (['cancelled', 'discarded'].includes(status)) return 'Stopped';
  if (['failed', 'unknown'].includes(status)) return 'Failed';
  if (status === 'blocked') return 'Needs a decision';
  return 'In progress';
}

function humanText(value: string) {
  if (/region|candidate|schema|data-desegnos-|excerpt|selector|conflict|validation|<\/?[a-z][^>]*>/i.test(value)) return generationError(value);
  if (/kept the validated changes for area/i.test(value)) return 'Kept the validated changes for this request.';
  if (/dynamic or executable|Copy code/i.test(value)) return generationError(value);
  return value.replace(/\b[0-9a-f]{8}-[0-9a-f-]{27,}\b/gi, 'request').replace(/\barea [0-9a-f]{8,}[^\s]*/gi, 'request');
}
