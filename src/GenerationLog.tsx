import { useEffect, useRef, useState } from 'react';
import { GenerationDrafts } from './GenerationDrafts';
import type { GenerationController } from './useGeneration';
import { native } from './api';
import type { GenerationHistory, GenerationJobView, Project } from './types';

// Current and locally saved conversations share the same draft review actions.
const SERVICE = { openai: 'OpenAI', anthropic: 'Anthropic', google: 'Google', openrouter: 'OpenRouter', local: 'Local model' } as const;
const kb = (bytes: number) => bytes < 1024 ? `${bytes} bytes` : `${(bytes / 1024).toFixed(1)} KB`;

export function GenerationLog({ job: currentJob, operation, error, project, controller, close }: {
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
    return { key: area.regionId, page: sent?.pageName || page?.name || 'Removed page', box: index + 1, prompt: sent?.prompt || region?.prompt || '', attachments: sent?.attachments ?? (past ? [] : region?.attachments ?? []), phone: !!(sent?.mobileOnly ?? (!past && region?.mobileOnly)) };
  });
  const working = !past && (!!operation || job?.status === 'prepared' || job?.status === 'generating' || job?.status === 'ready' || job?.status === 'applying');
  const candidate = job?.candidate;
  const failed = job && ['failed', 'unknown', 'cancelled', 'blocked'].includes(job.status);
  const raw = candidate ? JSON.stringify({ ...candidate, pages: candidate.pages.map(page => ({ pageId: page.pageId, html: `${page.content.html.length} characters`, css: `${page.content.css.length} characters`, javascript: `${page.content.javascript.length} characters` })) }, null, 2) : '';
  return <aside className="generation-log" aria-label="AI conversation">
    <header><div><strong>AI conversation</strong><span>{past ? 'past generation' : 'this generation'}</span></div><button type="button" aria-label="Hide AI conversation" title="Hide" onClick={close}>×</button></header>
    <div className="log-messages">
      {past && <button type="button" onClick={() => { historyEpoch.current += 1; setLoadingPast(false); setPast(null); }}>Back to current generation</button>}
      {!job && !operation && !error && <p className="log-empty">No conversation yet.</p>}
      {(job || operation) && <section className="log-message from-you" aria-label="Sent to the AI">
        <p className="log-who">You → {job ? `${SERVICE[job.provider ?? 'openai']} · ${job.settings.model}` : 'AI'}</p>
        {job ? <>
          <p>{requests.length} {requests.length === 1 ? 'request' : 'requests'} · {job.generationMode === 'strict' ? 'Precise' : 'Flexible'}</p>
          <ol className="log-requests">{requests.map(item => <li key={item.key}>
            <span className="log-meta">{item.page} · Box {item.box}{item.phone ? ' · Phone fix' : ''}</span>
            <p className="log-text">{item.prompt || <em>No description</em>}</p>
            {item.attachments.length > 0 && <p className="log-meta">Attached: {item.attachments.map((attachment: { name: string }) => attachment.name).join(', ')}</p>}
          </li>)}</ol>
          <details><summary>Full instructions sent</summary><pre tabIndex={0} aria-label="Instructions sent">{job.requirements.markdown}</pre></details>
        </> : <p>{operation}</p>}
      </section>}
      {job?.earlierContexts?.map((context, index) => <section key={index} className="log-message from-you"><p className="log-who">You → {SERVICE[job.provider ?? 'openai']}</p><details><summary>Requested context sent</summary><pre tabIndex={0}>{context.markdown}</pre></details></section>)}
      {job?.context && (job.repairRounds ?? 0) > 0 && <>
        <section className="log-message from-ai" aria-label="First AI reply"><p className="log-who">{SERVICE[job.provider ?? 'openai']}</p><p>{job.firstExchange?.summary}</p><ul>{job.firstExchange?.regions.map(region => <li key={region.regionId}>{region.status === 'implemented' ? 'Done' : 'Needs correction'} · {region.explanation}</li>)}</ul>{job.firstExchange?.usage && <p className="log-meta">{job.firstExchange.usage.inputTokens.toLocaleString()} tokens sent · {job.firstExchange.usage.outputTokens.toLocaleString()} received</p>}{job.rawReply && <details><summary>Original reply</summary><pre tabIndex={0}>{job.rawReply}</pre></details>}</section>
        <section className="log-message from-you" aria-label="Repair sent to the AI"><p className="log-who">You → {SERVICE[job.provider ?? 'openai']}</p><p>Asked the AI to fix {job.repairAreaCount ?? job.areas.length} areas</p><details><summary>Repair instructions sent</summary><pre tabIndex={0}>{job.context.markdown}</pre></details></section>
      </>}
      {job?.context && !(job.repairRounds ?? 0) && <>
        <section className="log-message from-ai" aria-label="AI context request"><p className="log-who">{SERVICE[job.provider ?? 'openai']}</p><p>Asked for {context.length} more {context.length === 1 ? 'element' : 'elements'}</p><ul>{context.map(item => <li key={item.id}><code>{item.html?.match(/<([a-z][a-z0-9-]*)/i)?.[1] ?? 'element'} · {item.id}</code>{item.missing ? ' · unavailable' : ''}</li>)}</ul></section>
        <section className="log-message from-you" aria-label="Context sent to the AI"><p className="log-who">You → {SERVICE[job.provider ?? 'openai']}</p><details><summary>Context sent</summary><pre tabIndex={0} aria-label="Context sent">{job.context.markdown}</pre></details></section>
      </>}
      {job && <section className={`log-message from-ai${failed ? ' failed' : ''}`} aria-label="AI reply" aria-live="polite">
        <p className="log-who">{SERVICE[job.provider ?? 'openai']}</p>
        {working && !candidate ? <p className="log-working">{job.receivedBytes ? `Writing… ${kb(job.receivedBytes)} received` : 'Thinking…'}</p> : null}
        {candidate && <>
          <p className="log-text">{job.status === 'applied' && candidate.regions.some(region => region.status === 'conflict') ? `Applied ${candidate.regions.filter(region => region.status === 'implemented').length} of ${candidate.regions.length} areas` : candidate.summary}</p>
          <ul className="log-outcomes">{candidate.regions.map(outcome => { const item = requests.find(request => request.key === outcome.regionId); return <li key={outcome.regionId} className={outcome.status}>
            <span className="log-meta"><span className="log-outcome-status">{outcome.status === 'implemented' ? 'Done' : 'Needs a decision'}</span> · {item ? `${item.page} · Box ${item.box}` : 'Request'}</span>
            <p className="log-text">{outcome.explanation}</p>
          </li>; })}</ul>
          {candidate.criticism.length > 0 && <details><summary>Suggestions ({candidate.criticism.length})</summary><ul>{candidate.criticism.map((item, index) => <li key={index}><p className="log-text">{item.message}</p><p className="log-meta">{item.suggestion}</p></li>)}</ul></details>}
          <details><summary>Raw reply</summary><pre tabIndex={0} aria-label="Raw reply">{raw}</pre></details>
        </>}
        {failed && <details><summary>Show details</summary><p className="log-text">{job.status === 'cancelled' ? 'Stopped.' : job.message || 'The AI could not finish this request.'}{job.providerError?.responseFormat?.providerMessage ? ` ${job.providerError.responseFormat.providerMessage}` : ''}</p></details>}
        {failed && job.rawReply && <details><summary>Raw reply</summary><pre tabIndex={0} aria-label="Failed raw reply">{job.rawReply}</pre></details>}
        {!!job.validationEvents?.length && <details><summary>Validation notes</summary><ul>{job.validationEvents.map((event, index) => <li key={index}>{event.message}</li>)}</ul></details>}
        <GenerationDrafts job={job} project={project} controller={controller} changed={value => { if (past) setPast(value); }} />
        {job.usage && <p className="log-meta">{job.usage.inputTokens.toLocaleString()} tokens sent · {job.usage.outputTokens.toLocaleString()} received</p>}
      </section>}
      <details className="log-message" aria-label="Past generations"><summary>Past generations · {history?.jobs.length ?? 0}</summary>
        {history?.jobs.map(item => <p key={item.jobId}><button type="button" disabled={loadingPast} aria-pressed={past?.jobId === item.jobId} onClick={() => { void showPast(item.jobId); }}>
          <span className="log-meta">{item.createdAt ? new Date(item.createdAt).toLocaleString() : 'Earlier generation'} · {statusLabel(item.status)}</span><br />
          <span style={{ display: 'block', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{(item.firstRequest || item.summary).split('\n')[0].replace(/\s*·\s*[a-f0-9]{8,64}\b/gi, '').slice(0, 160)}</span>{item.usage && <><br /><span className="log-meta">{item.usage.inputTokens.toLocaleString()} tokens sent · {item.usage.outputTokens.toLocaleString()} received</span></>}
        </button></p>)}
        {history?.skipped ? <p className="field-help">{history.skipped} saved records could not be read.</p> : null}
        {history?.truncated && <p className="field-help">Showing the latest 100 saved generations.</p>}
        {historyError && <p role="alert">{historyError}</p>}
      </details>
      {error && <section className="log-message from-ai failed"><p className="log-who">Desegnos</p><details><summary>Show details</summary><p className="log-text">{error}</p></details></section>}
    </div>
  </aside>;
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
