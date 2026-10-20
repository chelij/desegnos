import type { GenerationJobView, Project } from './types';

// A read-only conversation view of the current generation: what was sent, then what came back.
// It is temporary: it shows only the latest job and is not saved with the project.
const SERVICE = { openai: 'OpenAI', anthropic: 'Anthropic', google: 'Google', openrouter: 'OpenRouter', local: 'Local model' } as const;
const kb = (bytes: number) => bytes < 1024 ? `${bytes} bytes` : `${(bytes / 1024).toFixed(1)} KB`;

export function GenerationLog({ job, operation, error, project, close }: {
  job: GenerationJobView | null; operation: string | null; error: string | null; project: Project; close: () => void;
}) {
  const requests = (job?.areas ?? []).map(area => {
    const page = project.pages.find(item => item.id === area.pageId);
    const index = page?.regions.findIndex(region => region.id === area.regionId) ?? -1;
    const region = index >= 0 ? page!.regions[index] : undefined;
    return { key: area.regionId, page: page?.name ?? 'Removed page', box: index + 1, prompt: region?.prompt ?? '', attachments: region?.attachments ?? [], phone: !!region?.mobileOnly };
  });
  const working = !!operation || job?.status === 'prepared' || job?.status === 'generating' || job?.status === 'ready' || job?.status === 'applying';
  const candidate = job?.candidate;
  const failed = job && ['failed', 'unknown', 'cancelled', 'blocked'].includes(job.status);
  const raw = candidate ? JSON.stringify({ ...candidate, pages: candidate.pages.map(page => ({ pageId: page.pageId, html: `${page.content.html.length} characters`, css: `${page.content.css.length} characters`, javascript: `${page.content.javascript.length} characters` })) }, null, 2) : '';
  return <aside className="generation-log" aria-label="AI conversation">
    <header><div><strong>AI conversation</strong><span>Read only · this generation</span></div><button type="button" aria-label="Hide AI conversation" title="Hide" onClick={close}>×</button></header>
    <div className="log-messages">
      {!job && !operation && !error && <p className="log-empty">Press Generate to see what is sent to the AI and what it replies.</p>}
      {(job || operation) && <section className="log-message from-you" aria-label="Sent to the AI">
        <p className="log-who">You → {job ? `${SERVICE[job.provider ?? 'openai']} · ${job.settings.model}` : 'AI'}</p>
        {job ? <>
          <p>{requests.length} {requests.length === 1 ? 'request' : 'requests'} · {job.generationMode === 'strict' ? 'Precise' : 'Flexible'}</p>
          <ol className="log-requests">{requests.map(item => <li key={item.key}>
            <span className="log-meta">{item.page} · Box {item.box}{item.phone ? ' · Phone fix' : ''}</span>
            <p className="log-text">{item.prompt || <em>No description</em>}</p>
            {item.attachments.length > 0 && <p className="log-meta">Attached: {item.attachments.map(attachment => attachment.name).join(', ')}</p>}
          </li>)}</ol>
          <details><summary>Full instructions sent</summary><pre tabIndex={0} aria-label="Instructions sent">{job.requirements.markdown}</pre></details>
        </> : <p>{operation}</p>}
      </section>}
      {job && <section className={`log-message from-ai${failed ? ' failed' : ''}`} aria-label="AI reply" aria-live="polite">
        <p className="log-who">{SERVICE[job.provider ?? 'openai']}</p>
        {working && !candidate ? <p className="log-working"><span className="log-dots" aria-hidden="true"><i /><i /><i /></span>{job.receivedBytes ? `Writing… ${kb(job.receivedBytes)} received` : 'Thinking…'}</p> : null}
        {candidate && <>
          <p className="log-text">{candidate.summary}</p>
          <ul className="log-outcomes">{candidate.regions.map(outcome => { const item = requests.find(request => request.key === outcome.regionId); return <li key={outcome.regionId} className={outcome.status}>
            <span className="log-meta">{item ? `${item.page} · Box ${item.box}` : 'Request'} · {outcome.status === 'implemented' ? 'Done' : 'Needs a decision'}</span>
            <p className="log-text">{outcome.explanation}</p>
          </li>; })}</ul>
          {candidate.criticism.length > 0 && <details><summary>Suggestions ({candidate.criticism.length})</summary><ul>{candidate.criticism.map((item, index) => <li key={index}><p className="log-text">{item.message}</p><p className="log-meta">{item.suggestion}</p></li>)}</ul></details>}
          <details><summary>Raw reply</summary><pre tabIndex={0} aria-label="Raw reply">{raw}</pre></details>
        </>}
        {failed && <p className="log-text">{job.status === 'cancelled' ? 'Stopped.' : job.message || 'The AI could not finish this request.'}{job.providerError?.responseFormat?.providerMessage ? ` ${job.providerError.responseFormat.providerMessage}` : ''}</p>}
        {job.usage && <p className="log-meta">{job.usage.inputTokens.toLocaleString()} tokens sent · {job.usage.outputTokens.toLocaleString()} received</p>}
      </section>}
      {error && <section className="log-message from-ai failed" role="alert"><p className="log-who">Desegnos</p><p className="log-text">{error}</p></section>}
    </div>
  </aside>;
}
