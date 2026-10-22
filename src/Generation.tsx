import { useEffect, useState, type ReactNode } from 'react';
import { Modal } from './Modal';
import { MenuButton, type MenuItem } from './Menus';
import { scopeLabel } from './Notes';
import { PagePreview, SourceView } from './Preview';
import type { AiSettingsView, GenerationJobView, Project, ResponseCorrelation, ResponseRejection } from './types';
import type { GenerationController } from './useGeneration';

const money = (value: number) => new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD', minimumFractionDigits: 2, maximumFractionDigits: 4 }).format(value);
const readable = (value: string) => value.replaceAll('_', ' ').replace(/^./, letter => letter.toUpperCase());
const rejectionLabels: Record<ResponseRejection, string> = {
  eventJson: 'Invalid event JSON', eventType: 'Missing or unsupported event type', eventName: 'Event name and type disagree',
  responseIdentity: 'Missing, invalid or changed response identity', textLocation: 'Text does not belong to a completed output part',
  textIdentity: 'Text item identity changed or differs from the completed message', textOrder: 'Text arrived after its part was completed',
  textMismatch: 'Streamed text differs from the completed text', outputShape: 'Missing or unsupported output item',
  messageRole: 'Output message is not from the assistant', messageState: 'Output message is not complete',
  messagePhase: 'Unrecognized message phase', contentShape: 'Missing or unsupported text content',
  answerCount: 'Expected one final answer; intermediate commentary is excluded', credentialEcho: 'Reply contains a credential',
  candidateJson: 'Final answer is not valid JSON', candidateShape: 'Final answer is not a site object',
  jobMismatch: 'Returned job ID differs from the frozen request', requirementsMismatch: 'Returned instructions differ from the saved request',
  usage: 'Reported usage is missing fields or has inconsistent totals',
};
const correlationLabels: Record<ResponseCorrelation['cause'], string> = {
  invalidIndex: 'Invalid stream position', missingIdentity: 'No unambiguous message identity',
  itemAbsent: 'Message missing from completed output', itemDuplicate: 'Repeated message identity',
  identityChanged: 'Message identity changed', contentPartAbsent: 'Text part missing from its message',
  unfinishedItem: 'Unfinished or missing output item', itemChanged: 'Finalized message content changed',
};
const terminalOutputLabels: Record<ResponseCorrelation['terminalOutput'], string> = {
  notReceived: 'Not received', absent: 'Omitted', null: 'Null', array: 'Item list', invalid: 'Invalid',
};

function CorrelationDetails({ value }: { value: ResponseCorrelation }) {
  return <>
    <dt>Stream matching</dt><dd>{correlationLabels[value.cause]} <span className="small muted">({value.cause})</span></dd>
    <dt>Completed output</dt><dd>{terminalOutputLabels[value.terminalOutput]} · {value.terminalItems.toLocaleString()} {value.terminalItems === 1 ? 'item' : 'items'} · {value.terminalTextParts.toLocaleString()} {value.terminalTextParts === 1 ? 'text part' : 'text parts'}</dd>
    <dt>Stream counts</dt><dd>{value.streamedParts.toLocaleString()} {value.streamedParts === 1 ? 'text part' : 'text parts'} · {value.finalizedItems.toLocaleString()} {value.finalizedItems === 1 ? 'finalized item' : 'finalized items'} · {value.emptyAddedItems.toLocaleString()} {value.emptyAddedItems === 1 ? 'empty added item' : 'empty added items'}</dd>
    {(value.outputIndex != null || value.contentIndex != null) && <><dt>Stream position</dt><dd>{value.outputIndex != null && <>Output {value.outputIndex.toLocaleString()}</>}{value.outputIndex != null && value.contentIndex != null && ' · '}{value.contentIndex != null && <>Content {value.contentIndex.toLocaleString()}</>} <span className="small muted">(zero-based)</span></dd></>}
  </>;
}

function requestLabel(project: Project, id: string) {
  for (const page of project.pages) {
    const index = page.regions.findIndex(region => region.id === id);
    if (index >= 0) return `${page.name} · Box ${index + 1}`;
  }
  return id;
}

const pageChanged = (raw: string) => /stale|generate new requirements|page source changed|requested area.*(?:changed|removed)|context.*changed/i.test(raw);

export function generationNeedsNewRequest(error: string, job: GenerationJobView | null) {
  const raw = error || job?.message || '';
  return pageChanged(raw) || /cost estimate changed|already applied|twenty-attempt limit|frozen requirements file changed/i.test(raw);
}

export function generationMessage(error: string, job: GenerationJobView | null) {
  const raw = error || job?.message || '';
  let message = 'The AI could not finish this request. Review Details, then try again.';
  if (pageChanged(raw)) message = 'You changed the page while this was building, so the result no longer fits. Generate again to build from your current boxes.';
  else if (/cost estimate changed/i.test(raw)) message = 'The price estimate changed. Generate again to prepare a new request with current pricing.';
  else if (/already applied|twenty-attempt limit|frozen requirements file changed/i.test(raw)) message = 'This saved request cannot be used again. Generate again to build from your current boxes.';
  else if (/Only a current candidate with ready regions|rejected the candidate|rejected.*reply|invalid.*(?:reply|response)/i.test(raw)) message = 'The AI returned a result that could not be used. Try again to build another version.';
  else if (job?.failureKind === 'chatgptUsageLimit') message = 'Your ChatGPT plan or credits limit was reached. Open Details to manage usage before trying again.';
  else if (/AI settings|API key|connect.*account|credential|authentication|unauthorized/i.test(raw)) message = 'The AI connection needs attention. Review AI settings, then try again.';
  else if (/spending|budget|output.*(?:cap|limit)|cost estimate/i.test(raw)) message = 'The request exceeds your current AI limits. Review Model and spending in AI settings, then try again.';
  else if (job?.status === 'unknown') message = 'The result did not arrive, and the provider may still count this request. Review Details before trying again.';
  else if (job?.status === 'cancelled') message = 'Generation stopped, but submitted work may still count toward your bill or plan. Try again when you are ready.';
  else if (job?.status === 'blocked' && job.candidate?.regions.some(region => region.status === 'conflict')) message = 'A request needs your decision. Open Details to review it before generating again.';
  else if (/progress.*refreshed/i.test(raw)) message = 'Progress could not be refreshed. Open Details to check the request before trying again.';
  return `${message} Your work is safe.`;
}

export function GenerationFeedback({ controller, compact = false, children }: {
  controller: GenerationController; compact?: boolean; children?: ReactNode;
}) {
  const { job: currentJob, operation, elapsedSeconds, error } = controller;
  const job = operation === 'Preparing AI instructions…' ? null : currentJob;
  if (!job && !operation && !error && !controller.stopped) return null;
  const minutes = elapsedSeconds ?? 0;
  const headings = { prepared: 'Getting ready…', generating: minutes >= 60 ? 'Still building your page…' : 'Building your page…',
    ready: 'Updating your page…', blocked: 'Couldn’t update the page', failed: 'Couldn’t build the page', unknown: 'Not sure the page was built',
    cancelled: 'Stopped', discarded: 'Result discarded', applying: 'Updating your page…', applied: 'Page updated' };
  const failed = !!error || !!job && ['failed', 'unknown', 'cancelled', 'blocked'].includes(job.status);
  const heading = operation === 'Stopping…' ? 'Stopping…' : controller.stopped ? 'Stopped' : operation ? 'Getting ready…' : job ? headings[job.status] : 'Couldn’t start';
  return <section className={`generation-status${compact ? ' compact' : ''}`} data-job-status={job?.status} aria-label="Generation status">
    <div className="generation-status-copy"><p className="generation-heading" role="status" aria-atomic="true"><strong>{heading}</strong></p>
      {elapsedSeconds !== null && (!compact || job?.status === 'generating' || !!operation) && <p className="generation-activity" aria-live="off">
        <span role="timer" aria-live="off">{Math.floor(elapsedSeconds / 60)}:{String(elapsedSeconds % 60).padStart(2, '0')}</span>
      </p>}
      {compact && failed && <p className="generation-feedback-error" role="alert">{generationMessage(error, job)}</p>}
    </div>{children && <div className="generation-status-actions">{children}</div>}
  </section>;
}

export function RetryConfirmation({ job, settings, fresh = false, undo = false, busy, send, close }: {
  job: GenerationJobView | null; settings: AiSettingsView | null; fresh?: boolean; undo?: boolean; busy: boolean; send: () => void; close: () => void;
}) {
  const plan = (fresh ? settings?.authentication : job?.authentication) === 'chatgpt';
  const provider = (fresh ? settings?.provider : job?.provider) ?? settings?.provider ?? 'openai';
  const service = providerName(provider);
  const accountId = fresh ? settings?.chatgpt.activeAccountId : job?.chatgptAccountId;
  const account = settings?.chatgpt.accounts.find(account => account.id === accountId);
  return <Modal title={fresh ? 'Build another version?' : 'Try the same requests again?'} close={close}><div className="dialog-body">
    <p>{fresh ? `${undo ? 'This undoes the result and sends' : 'This sends'} a new request using your current boxes and AI settings.` : `This sends the original saved request to ${plan ? account?.label ?? 'the original ChatGPT account' : service} again.`}</p>
    <p>{plan ? 'This counts toward your ChatGPT plan or credits. Earlier attempts may also have used your plan or credits.' : `${service} may charge for another request.${!fresh && job ? ` Estimated job cost: ${money(job.estimateUsd)}.` : ''} Earlier attempts may also have been charged.`}</p>
    <div className="form-actions"><button className="primary" disabled={busy} onClick={send}>{plan ? 'Send using plan or credits' : 'Send paid request'}</button><button onClick={close}>Keep editing</button></div>
  </div></Modal>;
}

export const providerName = (provider: string) => ({ openai: 'OpenAI', anthropic: 'Anthropic', google: 'Google', openrouter: 'OpenRouter', local: 'Local model' } as Record<string, string>)[provider] ?? 'AI provider';

function GenerationDetails({ controller, project, settings, openSettings, manageUsage, editRequest, close }: {
  controller: GenerationController; project: Project; settings: AiSettingsView | null; openSettings: () => void; manageUsage: () => Promise<void>; editRequest: (id: string) => void; close: () => void;
}) {
  const { job, error, operation } = controller;
  const [tab, setTab] = useState<'review' | 'requirements' | 'attempts' | 'details'>('review');
  const [showSource, setShowSource] = useState(false);
  const [pageId, setPageId] = useState('');
  const [selected, setSelected] = useState<number[]>([]);
  const [confirmRetry, setConfirmRetry] = useState(false);
  const [usageError, setUsageError] = useState('');
  const [openingUsage, setOpeningUsage] = useState(false);
  useEffect(() => { setSelected(job?.acceptedDecisionIndices ?? []); setPageId(''); setTab('review'); setShowSource(false); }, [job?.jobId]);
  useEffect(() => { if (job?.status === 'ready' || job?.status === 'blocked' || job?.status === 'applied') setTab('review'); }, [job?.status]);
  useEffect(() => { if (job?.status === 'ready') setSelected(job.recommendedDecisionIndices); }, [job?.jobId, job?.status]);
  useEffect(() => { if (job?.status === 'applied') setSelected(job.acceptedDecisionIndices); }, [job?.status, job?.acceptedDecisionIndices]);
  const candidate = job?.candidate;
  const conflicts = candidate?.regions.filter(region => region.status === 'conflict') ?? [];
  const generatedImages = (candidate?.pages ?? []).flatMap(page => {
    const template = document.createElement('template');
    template.innerHTML = page.content.html;
    return Array.from(template.content.querySelectorAll('img[data-desegnos-generated-prompt]')).filter(node => node.getAttribute('data-desegnos-generated-job') === job?.jobId).map(node => ({
      prompt: node.getAttribute('data-desegnos-generated-prompt') ?? '',
      src: node.getAttribute('src') ?? '',
      regionId: node.closest('[data-desegnos-region]')?.getAttribute('data-desegnos-region')
        ?? project.pages.find(item => item.id === page.pageId)?.regions.find(region =>
          [region.element, ...(region.elements ?? [])].some(target => target && node.closest('[data-desegnos-id]')?.getAttribute('data-desegnos-id') === target.id))?.id,
      pageId: page.pageId,
      failed: node.hasAttribute('data-desegnos-placeholder'),
    }));
  });
  const source = candidate?.pages.find(page => page.pageId === pageId) ?? candidate?.pages[0];
  const page = project.pages.find(page => page.id === source?.pageId);
  const newRequest = generationNeedsNewRequest(error, job);
  const canRetry = !!job && ['failed', 'unknown', 'cancelled', 'blocked'].includes(job.status);
  const canDiscard = !!job && ['prepared', 'ready', 'blocked', 'failed', 'unknown', 'cancelled'].includes(job.status);
  const alreadyApplied = !!job && project.appliedJobIds.includes(job.jobId);
  const title = job?.status === 'prepared' ? 'Generation details' : job?.status === 'ready' ? 'Generation details' : 'Generation';
  const pricing = settings?.catalogue.find(model => model.id === job?.settings.model);
  const usesChatgpt = job?.authentication === 'chatgpt';
  const frozenAccount = settings?.chatgpt.accounts.find(account => account.id === job?.chatgptAccountId);
  const usageLimit = usesChatgpt && job?.failureKind === 'chatgptUsageLimit';
  const providerError = job?.providerError;
  const service = ({ openai: 'OpenAI', anthropic: 'Anthropic', google: 'Google', openrouter: 'OpenRouter', local: 'Local model' } as const)[job?.provider ?? 'openai'];
  const bodyKinds = { unavailable: 'Not read completely', eventStream: 'Generation event stream', empty: 'Empty', jsonError: 'JSON error', jsonDetail: 'JSON admission detail', jsonResponse: 'JSON response without a stream', jsonOther: 'Other JSON', html: 'HTML', text: 'Text', binary: 'Binary' };
  const affectedPageIds = new Set(Object.keys(job?.requirements.dependencies ?? {}).filter(key => key.startsWith('source:')).map(key => key.slice(7)));
  const affectedPages = project.pages.filter(page => affectedPageIds.has(page.id)).map(page => page.name);
  async function usage() {
    setOpeningUsage(true); setUsageError('');
    try { await manageUsage(); }
    catch (error) { setUsageError(error instanceof Error ? error.message : String(error)); }
    finally { setOpeningUsage(false); }
  }
  const more: MenuItem[] = [
    { id: 'result', label: 'Overview', action: () => setTab('review') },
    { id: 'technical', label: 'Advanced', children: [
      { id: 'requirements', label: job?.status === 'prepared' ? 'What will be sent' : 'AI instructions sent', action: () => setTab('requirements') },
      { id: 'request-details', label: 'Request details', action: () => setTab('details') },
      { id: 'source', label: showSource ? 'Hide source code' : 'Source code', disabled: !candidate?.pages.length, action: () => { setTab('review'); setShowSource(value => !value); } },
      { id: 'attempts', label: 'Attempts', action: () => setTab('attempts') },
      { id: 'refresh', label: 'Refresh job', disabled: !!operation, action: () => { void controller.refresh(); } },
      ...(canRetry ? [{ id: 'retry-snapshot', label: 'Retry original request', disabled: !!operation, action: () => setConfirmRetry(true) }] : []),
    ] },
    { id: 'settings', label: 'AI settings', action: openSettings },
    ...(canDiscard ? [{ id: 'discard', label: job?.status === 'prepared' ? 'Discard request' : 'Discard result', disabled: !!operation, action: () => { void controller.discard().catch(() => {}); } }] : []),
  ];
  return <>
    <Modal title={title} wide close={close} headerContent={<GenerationFeedback controller={controller}>
        {job?.status === 'prepared' && <button className="primary" disabled={!!operation} onClick={() => { void controller.start().catch(() => {}); }}>Send and generate</button>}
      </GenerationFeedback>}><div className="generation-dialog">
      {error && <details><summary>Show details</summary><p className="preserve-text">{error}</p></details>}
      {usageError && <p className="message error" role="alert">{usageError}</p>}
      {job?.dossierError && <p className="message warning" role="alert">Your result is saved, but the project brief needs attention: {job.dossierError}</p>}
      {!job ? <><p>{operation ? 'Preparing…' : 'No generation request is prepared.'}</p><div className="form-actions">
        <button onClick={openSettings}>AI settings</button>
      </div></> : <>
        {job.status === 'unknown' && <p className="message warning">The AI may have finished this request, but the result didn’t arrive. Trying again {usesChatgpt ? 'uses your plan again' : 'may cost again'}.</p>}
        {job.status === 'cancelled' && <p className="small muted">{usesChatgpt ? 'Submitted work may still count toward your plan.' : 'Submitted work may still be charged.'}</p>}
        {usageLimit && <p className="message warning"><strong>ChatGPT usage limit reached.</strong> Review your plan or this app’s limit in ChatGPT settings.</p>}
        {job.status === 'applied' && !alreadyApplied && <p className="message warning">This job was applied earlier. The current project does not include that application; it may have been undone.</p>}
        <div className="generation-review-toolbar"><p>{job.requirements.regionCount} {job.requirements.regionCount === 1 ? 'request' : 'requests'} · {affectedPages.join(', ')} · {job.generationMode === 'strict' ? 'Precise' : 'Flexible'}</p><MenuButton label="More" ariaLabel="Generation options" items={more} /></div>
        {tab === 'details' && <section className="generation-section"><h3>Request details</h3><dl className="generation-facts"><dt>Cost</dt><dd>{usesChatgpt ? `Counts toward ${frozenAccount?.label ?? 'your ChatGPT'} plan or credits` : `About ${money(job.estimateUsd)}; final charges may vary`}</dd><dt>Provider</dt><dd>{service}</dd><dt>Authentication</dt><dd>{usesChatgpt ? 'ChatGPT plan' : 'API key'}</dd><dt>Model</dt><dd>{job.settings.model}</dd><dt>Generation mode</dt><dd>{job.generationMode === 'strict' ? 'Precise' : 'Flexible'}</dd>
          {usesChatgpt ? <><dt>Account</dt><dd>{frozenAccount?.label ?? job.chatgptAccountId ?? 'No account recorded'}{frozenAccount && <span className="small muted"> · {job.chatgptAccountId}</span>}</dd><dt>Reasoning effort</dt><dd>{job.settings.reasoningEffort}</dd></> : <>
            <dt>Estimated job cost</dt><dd>{money(job.estimateUsd)}</dd><dt>Estimate allowance</dt><dd>{money(job.settings.spendingLimitUsd)}</dd>
            <dt>Output cap</dt><dd>{job.settings.maxOutputTokens.toLocaleString()} tokens · {job.settings.reasoningEffort} reasoning</dd></>}</dl>
        {usesChatgpt && <div className="plan-disclosure"><p>Counts toward this account’s ChatGPT plan or credits.</p>{!usageLimit && <button disabled={openingUsage} onClick={() => { void usage(); }}>Manage usage</button>}</div>}
        {!usesChatgpt && pricing && <p className="small muted">Catalogue verified {pricing.verifiedAt}. Estimates may include higher long-context rates and are not guaranteed billing caps.</p>}
        <p className="small muted">Sent to {usesChatgpt ? `ChatGPT (${frozenAccount?.label ?? 'current account'})` : `${service} (API key)`}</p>
        <p className="small muted">Received {job.receivedBytes.toLocaleString()} bytes from {service}.</p>
        {providerError && <section aria-label="Provider response details"><h4>Provider response</h4><dl className="generation-facts">
          <dt>Outcome</dt><dd>{providerError.outcome === 'unknown' ? 'No completed generation stream; usage is unknown' : providerError.outcome === 'received' ? 'Error received; no site was applied' : readable(providerError.outcome)}</dd>
          {providerError.httpStatus !== null && <><dt>HTTP status</dt><dd>{providerError.httpStatus}</dd></>}
          {providerError.responseFormat && <><dt>Content type</dt><dd>{providerError.responseFormat.contentType ?? 'Missing or unrecognized'}</dd><dt>Body format</dt><dd>{bodyKinds[providerError.responseFormat.bodyKind]}</dd><dt>Body size</dt><dd>{providerError.responseFormat.bodyTruncated ? 'At least ' : ''}{providerError.responseFormat.bodyBytes.toLocaleString()} bytes{providerError.responseFormat.bodyTruncated ? ' (partial reply)' : ''}</dd>
            {providerError.responseFormat.limitBytes != null && <><dt>Read limit</dt><dd>{providerError.responseFormat.limitBytes.toLocaleString()} bytes</dd></>}</>}
          {providerError.responseFormat?.rejection && <><dt>Validation check</dt><dd>{rejectionLabels[providerError.responseFormat.rejection]} <span className="small muted">({providerError.responseFormat.rejection})</span></dd></>}
          {providerError.responseFormat?.correlation && <CorrelationDetails value={providerError.responseFormat.correlation} />}
          {providerError.responseFormat?.providerMessage && <><dt>Provider explanation</dt><dd>{providerError.responseFormat.providerMessage}</dd></>}
          {providerError.code && <><dt>Error code</dt><dd>{providerError.code}</dd></>}
          {providerError.param && <><dt>Request parameter</dt><dd>{providerError.param}</dd></>}
          {providerError.requestId && <><dt>Request ID</dt><dd>{providerError.requestId}</dd></>}
        </dl></section>}
        {job.usage && <p className="token-usage">Reported usage: {job.usage.inputTokens.toLocaleString()} input · {job.usage.outputTokens.toLocaleString()} output · {job.usage.totalTokens.toLocaleString()} total tokens</p>}</section>}
        {tab === 'requirements' && <section className="generation-section"><h3>AI instructions</h3>
          <p className="digest">SHA-256: <code>{job.requirements.sha256}</code></p><pre className="markdown-source" tabIndex={0} aria-label="AI instructions source">{job.requirements.markdown}</pre>
        </section>}
        {tab === 'attempts' && <section className="generation-section"><h3>Actual attempts</h3>{job.attempts.length === 0 ? <p className="muted">No provider attempt has been recorded.</p> : <ol className="attempt-list">{job.attempts.map(attempt => <li key={attempt.ordinal}>
          <strong>Attempt {attempt.ordinal} · {readable(attempt.status)}</strong><p className="small">Started {attempt.startedAt}{attempt.finishedAt ? ` · Finished ${attempt.finishedAt}` : ''}</p>
          {attempt.responseId && <p className="small response-id">Response: {attempt.responseId}</p>}{attempt.usage && <p className="small">{attempt.usage.inputTokens} input · {attempt.usage.outputTokens} output · {attempt.usage.totalTokens} total tokens</p>}
        </li>)}</ol>}</section>}
        {tab === 'review' && <section className="generation-section">{!candidate ? <>{job.status === 'prepared' ? <p className="muted">Ready to send.</p> : job.status === 'generating' ? <p className="muted">Generating…</p> : <><p>Couldn’t build the page.</p>{job.message && <details><summary>Show details</summary><p className="small">{job.message}</p></details>}</>}</> : <>
          {(job.status === 'blocked' || conflicts.length > 0) ? <section aria-label="Finish your draft"><h3>Requests needing a decision</h3>
            <ul className="generation-list">{candidate.regions.filter(region => region.status !== 'implemented').map(region => {
              const request = project.pages.flatMap(page => page.regions).find(regionItem => regionItem.id === region.regionId);
              return <li key={region.regionId}><strong>{requestLabel(project, region.regionId)}</strong><p className="preserve-text">{request?.prompt}</p>
                {request && !request.completed && <button onClick={() => editRequest(region.regionId)}>Change request</button>}</li>;
            })}</ul>
            <details><summary>Show details</summary><p>{job.message}</p><p>{candidate.summary}</p><ul className="generation-list">{candidate.regions.filter(region => region.status !== 'implemented').map(region => <li key={region.regionId}><strong>{requestLabel(project, region.regionId)}</strong><p>{region.explanation}</p></li>)}</ul></details>
          </section> : <><h3>Your page</h3><p>{candidate.summary}</p></>}
          {generatedImages.length > 0 && <section aria-label="Generated images"><h3>Generated images</h3>{generatedImages.map((image, index) => <figure key={index}>
            {image.src.startsWith('data:image/png;base64,') && !image.failed && <img src={image.src} alt="Generated result" width={144} height={144} style={{ objectFit: 'contain' }} />}
            <figcaption><strong>{image.regionId ? requestLabel(project, image.regionId) : project.pages.find(page => page.id === image.pageId)?.name}</strong><p>{image.prompt}</p>{image.failed && <p>Image generation failed. Replace this image or try again.</p>}</figcaption>
          </figure>)}</section>}
          {source && <><div className="candidate-page-picker"><label>Result page<select value={source.pageId} onChange={event => setPageId(event.target.value)}>{candidate.pages.map(item => <option key={item.pageId} value={item.pageId}>{project.pages.find(page => page.id === item.pageId)?.name ?? item.pageId}</option>)}</select></label></div>
            {page && <PagePreview key={`${job.jobId}:${source.pageId}`} page={page} revision={job.baseRevision} jobId={job.jobId} />}
            {showSource && <section className="candidate-source"><h3>Source code</h3><SourceView key={source.pageId} content={source.content} label="Generated" /></section>}
          </>}
          {candidate.decisionProposals.length > 0 && <details className="proposal-list"><summary>Suggested rules ({candidate.decisionProposals.length})</summary>{candidate.decisionProposals.map((proposal, index) => <label key={index} className="proposal">
            <input type="checkbox" checked={selected.includes(index)} disabled={job.status !== 'ready' || alreadyApplied || !!operation} onChange={event => setSelected(previous => event.target.checked ? [...previous, index] : previous.filter(value => value !== index))} />
            <span><strong>{proposal.topic}</strong><span className="preserve-text">{proposal.value}</span><span className="small muted">{scopeLabel(project, proposal.scope)} · {proposal.rationale}</span></span>
          </label>)}</details>}
          {candidate.criticism.length > 0 && <details className="generation-feedback"><summary>Design feedback ({candidate.criticism.length})</summary><ul className="generation-list">{candidate.criticism.map((criticism, index) => <li key={index}><strong>{criticism.regionId ? requestLabel(project, criticism.regionId) : 'Whole site'}</strong><p>{criticism.message}</p><p className="muted">{criticism.suggestion}</p></li>)}</ul></details>}
          <details className="generation-coverage"><summary>Request details</summary><div className="table-scroll"><table className="coverage-table"><thead><tr><th>Request</th><th>Result</th><th>Explanation</th></tr></thead><tbody>{candidate.regions.map(region => <tr key={region.regionId}><th>{requestLabel(project, region.regionId)}</th><td>{readable(region.status)}</td><td>{region.explanation}</td></tr>)}</tbody></table></div>
            {candidate.componentRequests.length > 0 && <ul className="generation-list">{candidate.componentRequests.map((component, index) => <li key={index}><strong>{component.capability}</strong><p>{component.resolution === 'native' ? 'Built in' : 'Unavailable'}: {component.reason}</p></li>)}</ul>}
          </details>
        </>}</section>}
        <div className="generation-actions form-actions">
          {usageLimit && <button className="primary" disabled={openingUsage} onClick={() => { void usage(); }}>Manage usage</button>}
          {job.status === 'ready' && !alreadyApplied && <button className="primary" disabled={!!operation} onClick={() => { void controller.apply([...selected].sort((a, b) => a - b)).catch(() => {}); }}>{conflicts.length > 0 ? 'Apply completed changes' : 'Apply changes'}</button>}
          {canRetry && <button className="primary" disabled={!!operation} onClick={() => setConfirmRetry(true)}>{newRequest ? 'Generate again' : 'Try again'}</button>}
        </div>
      </>}
    </div></Modal>
    {confirmRetry && job && <RetryConfirmation job={job} settings={settings} fresh={newRequest} busy={!!operation} close={() => setConfirmRetry(false)} send={() => { setConfirmRetry(false); void (newRequest ? controller.prepare() : controller.retry()).catch(() => {}); }} />}
  </>;
}


export function GenerationDialog(props: {
  controller: GenerationController; project: Project; settings: AiSettingsView | null; openSettings: () => void; manageUsage: () => Promise<void>; editRequest: (id: string) => void; close: () => void;
}) {
  return <GenerationDetails {...props} />;
}
