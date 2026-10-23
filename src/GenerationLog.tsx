import { needsAiSettings, ProblemBanner } from './ProblemBanner.js';
import { userError } from './userErrors.js';
import { useEffect, useRef, useState } from 'react';
import { WorkspaceSurface } from './WorkspaceSurface';
import type { AiSettingsView } from './types';
import { plainReason, GenerationDrafts } from './GenerationDrafts';
import type { GenerationController } from './useGeneration';
import { native } from './api';
import { scopeLabel } from './Notes';
import type { GenerationHistory, GenerationJobView, Project } from './types';

// Current and locally saved conversations share the same draft review actions.
const SERVICE = { openai: 'OpenAI', anthropic: 'Anthropic', google: 'Google', openrouter: 'OpenRouter', local: 'Local model' } as const;
const kb = (bytes: number) => bytes < 1024 ? `${bytes} bytes` : `${(bytes / 1024).toFixed(1)} KB`;

export function GenerationLog({ job: currentJob, operation, error, project, controller, close, settings, openSettings, manageUsage, editRequest }: {
  settings: AiSettingsView | null; openSettings: () => void; manageUsage: () => Promise<void>; editRequest: (id: string) => void;
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
    return { key: area.regionId, completed: region?.completed, page: sent?.pageName || page?.name || 'Removed page', box: index + 1, prompt: sent?.prompt || region?.prompt || '', attachments: sent?.attachments ?? (past ? [] : region?.attachments ?? []), phone: !!(sent?.mobileOnly ?? (!past && region?.mobileOnly)) };
  });
  const working = !past && (!!operation || job?.status === 'prepared' || job?.status === 'generating' || job?.status === 'ready' || job?.status === 'applying');
  const candidate = job?.candidate;
  const needsDetails = candidate?.regions.filter(outcome => outcome.status !== 'implemented') ?? [];
  const label = (id: string) => {
    const item = requests.find(request => request.key === id);
    return item ? `${item.page} · ${item.box > 0 ? `Box ${item.box}` : item.prompt || 'Request'}` : 'Request';
  };
  const notes = [...new Set(job?.validationEvents?.filter(event => !job.drafts?.some(draft => !draft.dismissed && !draft.applied && (!event.regionId || draft.regionIds.includes(event.regionId)))).map(event => plainReason(event.message)))];
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
            <span className="log-meta">{label(item.key)}{item.phone ? ' · Phone fix' : ''}</span>
            {item.prompt && <p className="log-text">{item.prompt}</p>}
            {item.attachments.length > 0 && <p className="log-meta">Attached: {item.attachments.map((attachment: { name: string }) => attachment.name).join(', ')}</p>}
          </li>)}</ol>
        </section>
        <section className="log-message from-ai" aria-label="AI reply" aria-live="polite">
          <p className="log-who">{SERVICE[job.provider ?? 'openai']}</p>
          {working && !candidate && <p className="log-working"><span className="log-dots" aria-hidden="true"><i /><i /><i /></span>{job.receivedBytes ? `Writing… ${kb(job.receivedBytes)} received` : 'Thinking…'}</p>}
          {candidate && <>
            <p className="log-text">{job.status === 'applied' && needsDetails.length ? `Applied ${candidate.regions.length - needsDetails.length} of ${candidate.regions.length} requests` : humanText(candidate.summary)}</p>
            <ul className="log-outcomes">{candidate.regions.map(outcome => <li key={outcome.regionId} className={outcome.status}>
              <span className="log-meta">{label(outcome.regionId)} · {outcome.status === 'implemented' ? 'Done' : 'Needs details'}</span>
              {outcome.explanation && (outcome.status === 'conflict' ? <ProblemBanner message={plainReason(outcome.explanation)} action={{ label: 'Try again', disabled: working, onClick: () => { void controller.prepare().catch(() => {}); } }} /> : <p className="log-text">{humanText(outcome.explanation)}</p>)}
            </li>)}</ul>
          </>}
          {!working && !candidate && ['failed', 'unknown'].includes(job.status) && <ProblemBanner message={userError(job.message || 'The AI could not finish this request. Try again.')} action={needsAiSettings(job.message || '') ? { label: 'AI settings', onClick: openSettings } : { label: 'Try again', onClick: () => { void controller.prepare().catch(() => {}); } }} />}
          {needsDetails.length > 0 && <section className="generation-needs-details" aria-label="Finish your draft">
            <h3>Needs details</h3>
            <ul className="log-outcomes">{needsDetails.map(outcome => <li key={outcome.regionId}>
              <span className="log-meta">{label(outcome.regionId)}</span>
              {!past && requests.some(item => item.key === outcome.regionId && !item.completed) && <button onClick={() => editRequest(outcome.regionId)}>Change request</button>}
            </li>)}</ul>
            {!past && <button disabled={working || !!operation} onClick={() => { void controller.prepare().catch(() => {}); }}>Build remaining draft</button>}
          </section>}
          <GenerationDrafts job={job} project={project} controller={controller} changed={value => { if (past) setPast(value); }} />
          {candidate && candidate.criticism.length > 0 && <details className="generation-feedback"><summary>Design feedback ({candidate.criticism.length})</summary><ul>{candidate.criticism.map((item, index) => <li key={index}><p className="log-text">{humanText(item.message)}</p><p className="log-meta">{humanText(item.suggestion)}</p></li>)}</ul></details>}
          {candidate && candidate.decisionProposals.length > 0 && <details><summary>Suggested rules ({candidate.decisionProposals.length})</summary><ul>{candidate.decisionProposals.map((proposal, index) => <li key={index}><strong>{humanText(proposal.topic)}</strong><p className="log-text">{humanText(proposal.value)}</p><p className="log-meta">{scopeLabel(project, proposal.scope)}</p></li>)}</ul></details>}
          {notes.length > 0 && <details><summary>Things to check</summary><ul>{notes.map(reason => <li key={reason}>{reason}</li>)}</ul></details>}
          <details className="conversation-request-details"><summary>Request details</summary><dl className="conversation-facts">
            <dt>AI style</dt><dd>{job.generationMode === 'strict' ? 'Precise' : 'Flexible'}</dd>
            <dt>Model</dt><dd>{settings?.catalogue.find(model => model.id === job.settings.model)?.name ?? job.settings.model}</dd>
            {job.repairRounds ? <><dt>Automatic corrections</dt><dd>{job.repairRounds}</dd></> : null}
            {context.length > 0 && <><dt>Additional context</dt><dd>{context.length} page elements</dd></>}
          </dl></details>
          {!past && !working && !candidate && job.status === 'cancelled' && <button onClick={() => { void controller.prepare().catch(() => {}); }}>Try again</button>}
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
        {historyError && <ProblemBanner message={userError(historyError)} dismiss={() => setHistoryError('')} instruction="Open the conversation again to reload saved generations." />}
      </details>
      {error && <ProblemBanner message={userError(error)} action={needsAiSettings(error) ? { label: 'AI settings', onClick: openSettings } : { label: 'Try again', disabled: working, onClick: () => { void controller.prepare().catch(() => {}); } }} />}
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

function jsonBlocks(markdown: string): any[] {
  const blocks: any[] = [];
  let fence = ''; let lines: string[] = [];
  for (const line of markdown.split('\n')) {
    if (!fence) { const match = /^(`{3,})json$/.exec(line); if (match) { fence = match[1]; lines = []; } }
    else if (line === fence) { try { blocks.push(JSON.parse(lines.join('\n'))); } catch { /* A partial diagnostic is still readable as text. */ } fence = ''; }
    else lines.push(line);
  }
  return blocks;
}

function humanText(value: string) {
  if (/region|candidate|schema|data-desegnos-|excerpt|selector|conflict|validation|<\/?[a-z][^>]*>/i.test(value)) return plainReason(value);
  if (/kept the validated changes for area/i.test(value)) return 'Kept the validated changes for this request.';
  if (/dynamic or executable|Copy code/i.test(value)) return plainReason(value);
  return value.replace(/\b[0-9a-f]{8}-[0-9a-f-]{27,}\b/gi, 'request').replace(/\barea [0-9a-f]{8,}[^\s]*/gi, 'request');
}
