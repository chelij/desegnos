import { ProblemBanner } from './ProblemBanner.js';
import { userError, generationError } from './userErrors.js';
import { areaOutcome, declaredConflict, fixLabel, jobOutcome, jsonBlocks, staleResult, staleResultMessage } from './generationProblems.js';
import { useEffect, useRef, useState } from 'react';
import { WorkspaceSurface } from './WorkspaceSurface';
import { Disclosure } from './Disclosure';
import type { AiSettingsView, Element, GenerationHistory, GenerationJobView, Project } from './types';
import type { GenerationController } from './useGeneration';
import { native } from './api';
import { scopeLabel } from './Notes';

const SERVICE = { openai: 'OpenAI', anthropic: 'Anthropic', google: 'Google', openrouter: 'OpenRouter', deepseek: 'DeepSeek', local: 'Local model' } as const;
const kb = (bytes: number) => bytes < 1024 ? `${bytes} bytes` : `${(bytes / 1024).toFixed(1)} KB`;

function allElements(elements: Element[]): Element[] {
  return elements.flatMap(element => [element, ...allElements(element.children)]);
}

function elementName(element: Element): string {
  return element.name?.trim() || element.text.trim().split('\n')[0] || element.kind;
}

export function GenerationLog({ job: currentJob, operation, error, project, controller, close, settings, openSettings, openImport, openBrief, editRequest, reply }: {
  settings: AiSettingsView | null; openSettings: () => void; openImport?: () => void; openBrief?: () => void; editRequest: (id: string) => void;
  job: GenerationJobView | null; operation: string | null; error: string | null; project: Project; controller: GenerationController; close: () => void;
  reply: (targets: { pageId: string; elementId: string }[], text: string) => Promise<void>;
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
  const frozen = jsonBlocks(job?.requirements.markdown ?? '').filter(value => value?.elementId && value?.pageId);
  const requests = (job?.areas ?? []).map(area => {
    const page = project.pages.find(item => item.id === area.pageId);
    const element = page ? allElements(page.elements).find(item => item.id === area.elementId) : undefined;
    const sent = frozen.find(item => item.elementId === area.elementId);
    const outcome = job?.candidate?.requests.find(item => item.elementId === area.elementId);
    const done = outcome?.status === 'implemented';
    const current = project.requests?.find(request => request.targets.some(target => target.kind === 'element' && target.id === area.elementId))?.text;
    return { done, key: area.elementId, pageId: page?.id ?? '', exists: !!element, page: sent?.pageName || page?.name || 'Removed boundary',
      label: element ? elementName(element) : 'Removed layer', prompt: sent?.request || current || '', current,
      outcome: job ? areaOutcome(job, area.elementId, current) : null,
      attachments: sent?.attachments ?? (past ? [] : project.requests?.find(request => request.targets.some(target => target.kind === 'element' && target.id === area.elementId))?.attachments ?? []) };
  });
  const review = jobOutcome(job, id => requests.find(item => item.key === id)?.current);
  const openFix = (fix: Parameters<typeof fixLabel>[0]) => fix === 'aiSettings' ? openSettings : fix === 'import' ? openImport : openBrief;
  const applyFix = (fix: Parameters<typeof fixLabel>[0]) => { openFix(fix)?.(); };
  const working = !past && (!!operation || job?.status === 'prepared' || job?.status === 'generating' || job?.status === 'ready' || job?.status === 'applying');
  const busy = !!operation || working;
  const targets = requests.filter(item => item.exists).map(item => ({ pageId: item.pageId, elementId: item.key }));
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
  // A boundary-only request has no layer to list, so the frozen requirements own the count.
  const sentCount = job?.requirements.requestCount ?? requests.length;
  const label = (id: string) => {
    const item = requests.find(request => request.key === id);
    return item ? `${item.page} · ${item.label}` : 'Layer';
  };
  return <WorkspaceSurface title="AI conversation" close={close}>
    <div className="log-messages">
      {past && <button type="button" onClick={() => { historyEpoch.current += 1; setLoadingPast(false); setPast(null); }}>Back to current generation</button>}
      {!job && !operation && !error && <p className="log-empty">No conversation yet.</p>}
      {!job && operation && <p role="status">{operation}</p>}
      {job && <>
        <section className="log-message from-you" aria-label="Sent to the AI">
          <p className="log-who">You → {SERVICE[job.provider ?? 'openai']}</p>
          <p>{sentCount} {sentCount === 1 ? 'request' : 'requests'}</p>
          <ol className="log-requests">{requests.map(item => <li key={item.key}>
            <span className="log-meta">{label(item.key)} · {stale ? 'Not applied' : item.done ? 'Done' : working ? 'Working' : item.outcome?.retryable === false ? 'Needs your input' : 'Needs attention'}</span>
            {item.prompt && <p className="log-text">{item.prompt}</p>}
            {item.attachments.length > 0 && <p className="log-meta">Attached: {item.attachments.map((attachment: { name: string }) => attachment.name).join(', ')}</p>}
          </li>)}</ol>
        </section>
        <section className="log-message from-ai" aria-label="AI reply" aria-live="polite">
          <p className="log-who">{SERVICE[job.provider ?? 'openai']}</p>
          {working && !candidate && <p className="log-working"><span className="log-dots" aria-hidden="true"><i /><i /><i /></span>{job.receivedBytes ? `Writing… ${kb(job.receivedBytes)} received` : 'Thinking…'}</p>}
          {!past && !working && stale && <div className="log-stale">
            <p className="log-text">{staleResultMessage}</p>
            {(review.retryable.length > 0 || review.retryAll) && <button className="primary" disabled={busy} onClick={() => { const ids = requests.filter(item => item.exists).map(item => item.key); void controller.prepare(ids.length ? ids : undefined).catch(() => {}); }}>Try again</button>}
          </div>}
          {!working && !stale && (!review.blocked.length || job.status === 'applied') && review.reason && <p className="log-text">{review.reason}</p>}
          {!past && !working && !stale && (review.retryable.length > 0 || review.retryAll || review.retryBatches) && <button disabled={!!operation} onClick={() => { void (review.retryBatches ? controller.retry() : controller.prepare(review.retryable.length ? review.retryable : undefined)).catch(() => {}); }}>Try again</button>}
          {candidate && <p className="log-text">{candidate.summary}</p>}
          {candidate?.requests.flatMap((request, requestIndex) => request.operations.map((operation, operationIndex) => operation.kind === 'setBehaviour' ? <p className="log-text" key={`${requestIndex}-${operationIndex}`}>Behaviour: {operation.behaviour}</p> : null))}
          {candidate && candidate.criticism.length > 0 && <Disclosure className="generation-feedback" summary={`Design feedback (${candidate.criticism.length})`}><ul>{candidate.criticism.map((item, index) => <li key={index}><p className="log-text">{humanText(item.message)}</p><p className="log-meta">{humanText(item.suggestion)}</p></li>)}</ul></Disclosure>}
          {!past && review.blocked.length > 0 && <section className="log-needs-input" aria-label="Needs your input">
            <p className="log-who">Needs your input</p>
            {review.blocked.map(item => <div key={item.elementId}>
              <span className="log-meta">{label(item.elementId)}</span>
              <p className="log-text">{item.reason}</p>
              <div className="log-actions">
                {declaredConflict(job, item.elementId) && <button type="button" className="primary" disabled={busy} onClick={() => { void controller.prepare([item.elementId]).catch(() => {}); }}>Resend this request</button>}
                <button type="button" onClick={() => editRequest(item.elementId)}>Change request</button>
                {item.fix && openFix(item.fix) && <button type="button" onClick={() => applyFix(item.fix!)}>{fixLabel(item.fix)}</button>}
              </div>
            </div>)}
          </section>}
          {candidate && candidate.decisionProposals.length > 0 && <Disclosure summary={`Suggested rules (${candidate.decisionProposals.length})`}><ul>{candidate.decisionProposals.map((proposal, index) => <li key={index}><strong>{humanText(proposal.topic)}</strong><p className="log-text">{humanText(proposal.value)}</p><p className="log-meta">{scopeLabel(project, proposal.scope)}</p></li>)}</ul></Disclosure>}
          <Disclosure className="conversation-request-details" summary="Request details"><dl className="conversation-facts">
            <dt>AI style</dt><dd>{job.generationMode === 'strict' ? 'Precise' : 'Flexible'}</dd>
            <dt>Model</dt><dd>{settings?.catalogue.find(model => model.id === job.settings.model)?.name ?? job.settings.model}</dd>
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
      {(error || job?.message || job?.candidate || !!job?.validationEvents?.length) && <Disclosure summary="Technical details"><pre>{JSON.stringify({ error, message: job?.message, providerError: job?.providerError, validationEvents: job?.validationEvents, outcomes: job?.candidate?.requests }, null, 2)}</pre></Disclosure>}
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
  if (/data-desegnos-|<\/?[a-z][^>]*>/i.test(value)) return generationError(value);
  return value.replace(/\b[0-9a-f]{8}-[0-9a-f-]{27,}\b/gi, 'request');
}
