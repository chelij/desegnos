import { ProblemBanner } from './ProblemBanner.js';
import { summarizeReportedOutsideChanges } from './noticeSummary.js';
import { userError, generationError } from './userErrors.js';
import { areaOutcome, declaredConflict, fixLabel, jobOutcome, jsonBlocks, staleResult, staleResultMessage } from './generationProblems.js';
import { isWholePage } from './wholePageScope.js';
import { useEffect, useRef, useState } from 'react';
import { WorkspaceSurface } from './WorkspaceSurface';
import { Disclosure } from './Disclosure';
import type { AiSettingsView } from './types';
import type { GenerationController } from './useGeneration';
import { native } from './api';
import { scopeLabel } from './Notes';
import type { GenerationHistory, GenerationJobView, Project } from './types';

// Current and locally saved conversations share the same draft review actions.
const SERVICE = { openai: 'OpenAI', anthropic: 'Anthropic', google: 'Google', openrouter: 'OpenRouter', local: 'Local model' } as const;
const kb = (bytes: number) => bytes < 1024 ? `${bytes} bytes` : `${(bytes / 1024).toFixed(1)} KB`;

export function GenerationLog({ job: currentJob, operation, error, project, controller, close, settings, openSettings, openImport, openBrief, manageUsage, editRequest, allowWholePage, resultApplied, requestEditorOpen, reply }: {
  resultApplied?: boolean; requestEditorOpen?: (regionId: string) => boolean; allowWholePage?: (regionId: string) => Promise<void>;
  settings: AiSettingsView | null; openSettings: () => void; openImport?: () => void; openBrief?: () => void; manageUsage: () => Promise<void>; editRequest: (id: string) => void;
  job: GenerationJobView | null; operation: string | null; error: string | null; project: Project; controller: GenerationController; close: () => void;
  reply: (targets: { pageId: string; regionId: string }[], text: string) => Promise<void>;
}) {
  const [history, setHistory] = useState<GenerationHistory | null>(null);
  const [past, setPast] = useState<GenerationJobView | null>(null);
  const [historyError, setHistoryError] = useState('');
  const [loadingPast, setLoadingPast] = useState(false);
  const [replyText, setReplyText] = useState('');
  const [replyError, setReplyError] = useState('');
  const [replySending, setReplySending] = useState(false);
  const replyBox = useRef<HTMLTextAreaElement>(null);
  useEffect(() => { const node = replyBox.current; if (!node) return; node.style.height = 'auto'; node.style.height = `${node.scrollHeight + node.offsetHeight - node.clientHeight}px`; node.style.overflowY = node.scrollHeight > node.clientHeight ? 'auto' : 'hidden'; }, [replyText]);
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
  const stale = staleResult(job);
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
    return { done, key: area.regionId, pageId: region ? page!.id : '', exists: !!region, completed: region?.completed, wholePage: !!page && !!region && isWholePage(region, page), page: sent?.pageName || page?.name || 'Removed page', box: index + 1, prompt: sent?.prompt || region?.prompt || '', current, outcome: job ? areaOutcome(job, area.regionId, current) : null, attachments: sent?.attachments ?? (past ? [] : region?.attachments ?? []), phone: !!(sent?.mobileOnly ?? (!past && region?.mobileOnly)) };
  });
  // Only areas whose result can still change are resent; declared gaps need a different request.
  const review = jobOutcome(job, id => requests.find(item => item.key === id)?.current);
  const openFix = (fix: Parameters<typeof fixLabel>[0]) => fix === 'aiSettings' ? openSettings : fix === 'import' ? openImport : openBrief;
  const applyFix = (fix: Parameters<typeof fixLabel>[0]) => { openFix(fix)?.(); };
  const working = !past && (!!operation || job?.status === 'prepared' || job?.status === 'generating' || job?.status === 'ready' || job?.status === 'applying');
  const busy = !!operation || working;
  const targets = requests.filter(item => item.exists).map(item => ({ pageId: item.pageId, regionId: item.key }));
  async function sendReply() {
    const text = replyText.trim();
    if (!text || !targets.length || busy || replySending) return;
    setReplySending(true); setReplyError('');
    try {
      await reply(targets, text);
      setReplyText('');
      if (past) { historyEpoch.current += 1; setLoadingPast(false); setPast(null); }
    } catch (error) { setReplyError(String(error)); }
    finally { setReplySending(false); }
  }
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
            <span className="log-meta">{label(item.key)}{item.phone ? ' · Phone fix' : ''} · {stale ? 'Not applied' : item.done ? 'Done' : working ? 'Working' : item.outcome?.retryable === false ? 'Needs your input' : 'Needs attention'}</span>
            {item.prompt && <p className="log-text">{item.prompt}</p>}
            {item.attachments.length > 0 && <p className="log-meta">Attached: {item.attachments.map((attachment: { name: string }) => attachment.name).join(', ')}</p>}
          </li>)}</ol>
        </section>
        <section className="log-message from-ai" aria-label="AI reply" aria-live="polite">
          <p className="log-who">{SERVICE[job.provider ?? 'openai']}</p>
          {working && !candidate && <p className="log-working"><span className="log-dots" aria-hidden="true"><i /><i /><i /></span>{job.receivedBytes ? `Writing… ${kb(job.receivedBytes)} received` : 'Thinking…'}</p>}
          {!past && !working && !resultApplied && stale && <div className="log-stale">
            <p className="log-text">{staleResultMessage}</p>
            <button className="primary" disabled={busy} onClick={() => { const ids = requests.filter(item => item.exists).map(item => item.key); void controller.prepare(undefined, ids.length ? ids : undefined).catch(() => {}); }}>Try again</button>
          </div>}
          {!past && !working && !resultApplied && !stale && (review.retryable.length > 0 || review.retryAll) && <button disabled={!!operation} onClick={() => { void controller.prepare(undefined, review.retryable.length ? review.retryable : undefined).catch(() => {}); }}>Try again</button>}
          {candidate?.outsideChanges && candidate.outsideChanges.length > 0 && <p className="log-text">{summarizeReportedOutsideChanges(candidate.outsideChanges)}</p>}
          {candidate && candidate.criticism.length > 0 && <Disclosure className="generation-feedback" summary={`Design feedback (${candidate.criticism.length})`}><ul>{candidate.criticism.map((item, index) => <li key={index}><p className="log-text">{humanText(item.message)}</p><p className="log-meta">{humanText(item.suggestion)}</p></li>)}</ul></Disclosure>}
          {!past && review.blocked.length > 0 && <section className="log-needs-input" aria-label="Needs your input">
            <p className="log-who">Needs your input</p>
            {review.blocked.map(item => <div key={item.regionId}>
              <span className="log-meta">{label(item.regionId)}</span>
              <p className="log-text">{item.reason}</p>
              {(!requestEditorOpen?.(item.regionId) || (item.fix && openFix(item.fix)) || allowWholePage && declaredConflict(job, item.regionId)) && <div className="log-actions">
                {allowWholePage && declaredConflict(job, item.regionId) && !requests.find(request => request.key === item.regionId)?.wholePage && <button type="button" className="primary" disabled={busy} onClick={() => { void allowWholePage(item.regionId).catch(() => {}); }}>Allow whole page and retry</button>}
                {!requestEditorOpen?.(item.regionId) && <button type="button" onClick={() => editRequest(item.regionId)}>Change request</button>}
                {item.fix && openFix(item.fix) && <button type="button" onClick={() => applyFix(item.fix!)}>{fixLabel(item.fix)}</button>}
              </div>}
            </div>)}
          </section>}
          {candidate && candidate.decisionProposals.length > 0 && <Disclosure summary={`Suggested rules (${candidate.decisionProposals.length})`}><ul>{candidate.decisionProposals.map((proposal, index) => <li key={index}><strong>{humanText(proposal.topic)}</strong><p className="log-text">{humanText(proposal.value)}</p><p className="log-meta">{scopeLabel(project, proposal.scope)}</p></li>)}</ul></Disclosure>}

          <Disclosure className="conversation-request-details" summary="Request details"><dl className="conversation-facts">
            <dt>AI style</dt><dd>{job.generationMode === 'strict' ? 'Precise' : 'Flexible'}</dd>
            <dt>Model</dt><dd>{settings?.catalogue.find(model => model.id === job.settings.model)?.name ?? job.settings.model}</dd>
            {job.repairRounds ? <><dt>Automatic corrections</dt><dd>{job.repairRounds}</dd></> : null}
            {context.length > 0 && <><dt>Additional context</dt><dd>{context.length} page elements</dd></>}
          </dl></Disclosure>
          {job.usage && <p className="log-meta token-footer">{job.usage.inputTokens.toLocaleString()} tokens sent · {job.usage.outputTokens.toLocaleString()} received</p>}
          {!working && targets.length > 0 && <div className="log-reply">
            <textarea ref={replyBox} rows={1} value={replyText} aria-label="Reply to the AI" placeholder="Reply to the AI…" maxLength={16384} onChange={event => setReplyText(event.target.value)} onKeyDown={event => { if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) { event.preventDefault(); void sendReply(); } }} />
            <button type="button" className="primary" disabled={busy || replySending || !replyText.trim()} onClick={() => void sendReply()}>Send</button>
          </div>}
          {replyError && <ProblemBanner message={userError(replyError)} dismiss={() => setReplyError('')} />}
        </section>
      </>}
      <Disclosure className="past-generations" ariaLabel="Past generations" summary={`Past generations · ${history?.jobs.length ?? 0}`}>
        {history?.jobs.map(item => <p key={item.jobId}><button type="button" disabled={loadingPast} aria-pressed={past?.jobId === item.jobId} onClick={() => { void showPast(item.jobId); }}>
          <span className="log-meta past-when">{item.createdAt ? new Date(item.createdAt).toLocaleString() : 'Earlier generation'} · {statusLabel(item.status)}</span>
          <span className="past-request">{humanText(item.firstRequest || item.summary).replace(/\s+/g, ' ')}</span>
          {item.usage && <span className="log-meta">{item.usage.inputTokens.toLocaleString()} sent · {item.usage.outputTokens.toLocaleString()} received</span>}
        </button></p>)}
        {history?.skipped ? <p className="field-help">{history.skipped} saved records could not be read.</p> : null}
        {history?.truncated && <p className="field-help">Showing the latest 100 saved generations.</p>}
        {historyError && <ProblemBanner message={userError(historyError)} dismiss={() => setHistoryError('')} />}
      </Disclosure>
      {(error || job?.message || job?.candidate) && <Disclosure summary="Technical details"><pre>{JSON.stringify({ error, message: job?.message, providerError: job?.providerError, validationEvents: job?.validationEvents, outcomes: job?.candidate?.regions, drafts: job?.drafts?.map(({ reason }) => ({ reason })) }, null, 2)}</pre></Disclosure>}
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

// The AI's own feedback is prose: only raw markup or editor attributes need a plain stand-in.
function humanText(value: string) {
  if (/data-desegnos-|<\/?[a-z][^>]*>/i.test(value)) return generationError(value);
  if (/kept the validated changes for area/i.test(value)) return 'Kept the validated changes for this request.';
  if (/dynamic or executable|Copy code/i.test(value)) return generationError(value);
  return value.replace(/\b[0-9a-f]{8}-[0-9a-f-]{27,}\b/gi, 'request').replace(/\barea [0-9a-f]{8,}[^\s]*/gi, 'request');
}
