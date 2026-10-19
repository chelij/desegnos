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
  jobMismatch: 'Returned job ID differs from the frozen request', requirementsMismatch: 'Returned requirements digest differs from the frozen request',
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
    if (index >= 0) return `${page.name} · Request ${index + 1}`;
  }
  return id;
}

export function GenerationFeedback({ controller, compact = false, children }: {
  controller: GenerationController; compact?: boolean; children?: ReactNode;
}) {
  const { job: currentJob, operation, elapsedSeconds, error } = controller;
  const job = operation === 'Preparing AI instructions…' ? null : currentJob;
  if (!job && !operation && !error) return null;
  const headings = { prepared: 'Preparing your design', generating: job?.receivedBytes ? 'Receiving AI response' : (elapsedSeconds ?? 0) >= 30 ? 'Still waiting for OpenAI' : 'Waiting for OpenAI',
    ready: 'Updating your canvas', blocked: 'Let’s finish your draft', failed: 'Generation failed', unknown: 'Generation outcome uncertain',
    cancelled: 'Generation cancelled', discarded: 'Result discarded', applying: 'Updating your canvas…', applied: 'Changes applied' };
  const heading = operation ?? (job ? headings[job.status] : 'Generation could not start');
  const detail = operation ? 'Your changes will appear on the canvas.'
    : job?.status === 'prepared' ? 'Checking your requests and project choices.'
    : job?.status === 'generating' ? 'Your changes will appear here when ready.'
    : job?.status === 'ready' ? 'Your changes are being put in place.'
    : job?.status === 'applied' ? 'Your site and project brief are up to date.'
    : job?.status === 'blocked' ? 'Edit your requests or try a draft with clearly labelled placeholders.'
    : job?.message ?? 'Read the error, then review AI settings or prepare the request again.';
  return <section className={`generation-status${compact ? ' compact' : ''}`} data-job-status={job?.status} aria-label="Generation status">
    <div className="generation-status-copy"><p className="generation-heading" role="status" aria-atomic="true"><strong>{heading}</strong></p>
      <p className="generation-detail">{detail}</p>
      {elapsedSeconds !== null && <p className="generation-activity" aria-live="off">
        {elapsedSeconds !== null && <span role="timer" aria-live="off">{Math.floor(elapsedSeconds / 60)}:{String(elapsedSeconds % 60).padStart(2, '0')} elapsed</span>}
      </p>}
      {compact && error && <p className="generation-feedback-error" role="alert">{error}</p>}
    </div>{children && <div className="generation-status-actions">{children}</div>}
  </section>;
}

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
  const source = candidate?.pages.find(page => page.pageId === pageId) ?? candidate?.pages[0];
  const page = project.pages.find(page => page.id === source?.pageId);
  const canRetry = !!job && ['failed', 'unknown', 'cancelled', 'blocked'].includes(job.status);
  const canDiscard = !!job && ['prepared', 'ready', 'blocked', 'failed', 'unknown', 'cancelled'].includes(job.status);
  const alreadyApplied = !!job && project.appliedJobIds.includes(job.jobId);
  const title = job?.status === 'prepared' ? 'Generation details' : job?.status === 'ready' ? 'Generation details' : 'Generation';
  const pricing = settings?.catalogue.find(model => model.id === job?.settings.model);
  const usesChatgpt = job?.authentication === 'chatgpt';
  const frozenAccount = settings?.chatgpt.accounts.find(account => account.id === job?.chatgptAccountId);
  const usageLimit = usesChatgpt && job?.failureKind === 'chatgptUsageLimit';
  const providerError = job?.providerError;
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
        {job?.status === 'generating' && <button disabled={!!operation} onClick={() => { void controller.cancel().catch(() => {}); }}>Cancel generation</button>}
      </GenerationFeedback>}><div className="generation-dialog">
      {error && <p className="message error" role="alert">{error}</p>}
      {usageError && <p className="message error" role="alert">{usageError}</p>}
      {job?.dossierError && <p className="message warning" role="alert">Your result is saved, but the project brief needs attention: {job.dossierError}</p>}
      {!job ? <><p>{operation ? 'Desegnos is freezing your current requests and project requirements.' : 'No generation request is prepared.'}</p><div className="form-actions">
        <button onClick={openSettings}>AI settings</button>{!operation && <button onClick={() => { void controller.prepare().catch(() => {}); }}>Prepare request again</button>}
      </div></> : <>
        {job.status === 'unknown' && <p className="message warning">OpenAI may have processed this request. Retrying {usesChatgpt ? 'uses your plan again' : 'may incur another charge'}.</p>}
        {job.status === 'cancelled' && <p className="small muted">{usesChatgpt ? 'Submitted work may still count toward your plan.' : 'Submitted work may still be charged.'}</p>}
        {usageLimit && <p className="message warning"><strong>ChatGPT usage limit reached.</strong> Review your plan or this app’s limit in ChatGPT settings.</p>}
        {job.status === 'applied' && !alreadyApplied && <p className="message warning">This job was applied earlier. The current project does not include that application; it may have been undone.</p>}
        <div className="generation-review-toolbar"><p>{job.requirements.regionCount} {job.requirements.regionCount === 1 ? 'request' : 'requests'} · {affectedPages.join(', ')} · {readable(job.generationMode)}</p><MenuButton label="More" ariaLabel="Generation options" items={more} /></div>
        <p className="small muted">{usesChatgpt ? `Using ${frozenAccount?.label ?? 'your original ChatGPT account'} · counts toward your plan or credits.` : `Estimated cost ${money(job.estimateUsd)} · final charges may vary.`}</p>
        {tab === 'details' && <section className="generation-section"><h3>Request details</h3><dl className="generation-facts"><dt>Provider</dt><dd>OpenAI</dd><dt>Authentication</dt><dd>{usesChatgpt ? 'ChatGPT plan' : 'API key'}</dd><dt>Model</dt><dd>{job.settings.model}</dd><dt>Generation mode</dt><dd>{readable(job.generationMode)}</dd>
          {usesChatgpt ? <><dt>Account</dt><dd>{frozenAccount?.label ?? job.chatgptAccountId ?? 'No account recorded'}{frozenAccount && <span className="small muted"> · {job.chatgptAccountId}</span>}</dd><dt>Reasoning effort</dt><dd>{job.settings.reasoningEffort}</dd></> : <>
            <dt>Estimated job cost</dt><dd>{money(job.estimateUsd)}</dd><dt>Estimate allowance</dt><dd>{money(job.settings.spendingLimitUsd)}</dd>
            <dt>Output cap</dt><dd>{job.settings.maxOutputTokens.toLocaleString()} tokens · {job.settings.reasoningEffort} reasoning</dd></>}</dl>
        {usesChatgpt && <div className="plan-disclosure"><p>Using ChatGPT plan. Eligible usage counts toward this account’s plan or credits balance. This job keeps the account and authentication method shown above, including when retried.</p>{!usageLimit && <button disabled={openingUsage} onClick={() => { void usage(); }}>Manage usage</button>}</div>}
        {!usesChatgpt && pricing && <p className="small muted">Catalogue verified {pricing.verifiedAt}. Estimates may include higher long-context rates and are not guaranteed billing caps.</p>}
        <p className="small muted">Sending attaches these requirements, including current affected-page source, directly to OpenAI. Desegnos uses <code>store: false</code>; this does not guarantee zero retention. OpenAI’s data controls still apply: <span className="source-url">https://developers.openai.com/api/docs/guides/your-data</span></p>
        <p className="small muted">Received {job.receivedBytes.toLocaleString()} bytes from OpenAI.</p>
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
        {tab === 'requirements' && <section className="generation-section"><h3>REQUIREMENTS.md</h3><p className="small">The exact requirements sent to OpenAI. Credentials stay in Desegnos’s secure storage.</p>
          <p className="digest">SHA-256: <code>{job.requirements.sha256}</code></p><pre className="markdown-source" tabIndex={0} aria-label="Generation REQUIREMENTS.md source">{job.requirements.markdown}</pre>
        </section>}
        {tab === 'attempts' && <section className="generation-section"><h3>Actual attempts</h3>{job.attempts.length === 0 ? <p className="muted">No provider attempt has been recorded.</p> : <ol className="attempt-list">{job.attempts.map(attempt => <li key={attempt.ordinal}>
          <strong>Attempt {attempt.ordinal} · {readable(attempt.status)}</strong><p className="small">Started {attempt.startedAt}{attempt.finishedAt ? ` · Finished ${attempt.finishedAt}` : ''}</p>
          {attempt.responseId && <p className="small response-id">Response: {attempt.responseId}</p>}{attempt.usage && <p className="small">{attempt.usage.inputTokens} input · {attempt.usage.outputTokens} output · {attempt.usage.totalTokens} total tokens</p>}
        </li>)}</ol>}</section>}
        {tab === 'review' && <section className="generation-section">{!candidate ? <p className="muted">{job.status === 'prepared' ? 'Your requests and project brief are ready to send.' : job.status === 'generating' ? 'Your site will appear here when it is ready.' : 'No completed site to review.'}</p> : <>
          {job.status === 'blocked' ? <section aria-label="Finish your draft"><h3>Your page is unchanged</h3>
            <p>Missing pictures, links or copy can use clearly labelled placeholders. Desegnos can use font fallbacks and download online images when available.</p>
            <p>Try a new draft, or change a request on the canvas first.</p>
            <ul className="generation-list">{candidate.regions.filter(region => region.status !== 'implemented').map(region => {
              const request = project.pages.flatMap(page => page.regions).find(regionItem => regionItem.id === region.regionId);
              return <li key={region.regionId}><strong>{requestLabel(project, region.regionId)}</strong><p className="preserve-text">{request?.prompt}</p>
                {request && !request.completed && <button onClick={() => editRequest(region.regionId)}>Change request</button>}</li>;
            })}</ul>
            <details><summary>Why this attempt stopped</summary><p>{job.message}</p><p>{candidate.summary}</p><ul className="generation-list">{candidate.regions.filter(region => region.status !== 'implemented').map(region => <li key={region.regionId}><strong>{requestLabel(project, region.regionId)}</strong><p>{region.explanation}</p></li>)}</ul></details>
          </section> : <><h3>Your website</h3><p>{candidate.summary}</p></>}
          {source && <><div className="candidate-page-picker"><label>Result page<select value={source.pageId} onChange={event => setPageId(event.target.value)}>{candidate.pages.map(item => <option key={item.pageId} value={item.pageId}>{project.pages.find(page => page.id === item.pageId)?.name ?? item.pageId}</option>)}</select></label></div>
            {page && <PagePreview key={`${job.jobId}:${source.pageId}`} page={page} revision={job.baseRevision} jobId={job.jobId} />}
            {showSource && <section className="candidate-source"><h3>Source code</h3><SourceView key={source.pageId} content={source.content} label="Generated" /></section>}
          </>}
          {candidate.decisionProposals.length > 0 && <details className="proposal-list"><summary>Project choices ({candidate.decisionProposals.length})</summary><p className="small muted">{alreadyApplied ? 'These suggestions are recorded. Add a choice in Project brief to guide future generations.' : 'Choose which suggestions to keep in your project brief.'}</p>{candidate.decisionProposals.map((proposal, index) => <label key={index} className="proposal">
            <input type="checkbox" checked={selected.includes(index)} disabled={job.status !== 'ready' || alreadyApplied || !!operation} onChange={event => setSelected(previous => event.target.checked ? [...previous, index] : previous.filter(value => value !== index))} />
            <span><strong>{proposal.topic}</strong><span className="preserve-text">{proposal.value}</span><span className="small muted">{scopeLabel(project, proposal.scope)} · {proposal.rationale}</span></span>
          </label>)}</details>}
          {candidate.criticism.length > 0 && <details className="generation-feedback"><summary>Design feedback ({candidate.criticism.length})</summary><p className="small muted">Suggestions, not rules. The creative choice is yours.</p><ul className="generation-list">{candidate.criticism.map((criticism, index) => <li key={index}><strong>{criticism.regionId ? requestLabel(project, criticism.regionId) : 'Whole project'}</strong><p>{criticism.message}</p><p className="muted">{criticism.suggestion}</p></li>)}</ul></details>}
          <details className="generation-coverage"><summary>Request details</summary><div className="table-scroll"><table className="coverage-table"><thead><tr><th>Request</th><th>Result</th><th>Explanation</th></tr></thead><tbody>{candidate.regions.map(region => <tr key={region.regionId}><th>{requestLabel(project, region.regionId)}</th><td>{readable(region.status)}</td><td>{region.explanation}</td></tr>)}</tbody></table></div>
            {candidate.componentRequests.length > 0 && <ul className="generation-list">{candidate.componentRequests.map((component, index) => <li key={index}><strong>{component.capability}</strong><p>{component.resolution === 'native' ? 'Built in' : 'Unavailable'}: {component.reason}</p></li>)}</ul>}
          </details>
        </>}</section>}
        <div className="generation-actions form-actions">
          {usageLimit && <button className="primary" disabled={openingUsage} onClick={() => { void usage(); }}>Manage usage</button>}
          {job.status === 'ready' && !alreadyApplied && <button className="primary" disabled={!!operation} onClick={() => { void controller.apply([...selected].sort((a, b) => a - b)).catch(() => {}); }}>Apply changes</button>}
          {canRetry && <button className="primary" disabled={!!operation || !project.pages.some(page => page.regions.some(region => !region.completed && region.prompt.trim()))} onClick={() => { void controller.prepare().catch(() => {}); }}>{job.status === 'blocked' ? 'Generate draft' : 'Generate again'}</button>}
        </div>
      </>}
    </div></Modal>
    {confirmRetry && job && <Modal title="Try the same requests again?" close={() => setConfirmRetry(false)}><div className="dialog-body"><p>{usesChatgpt ? `This uses ${frozenAccount?.label ?? 'the original ChatGPT account'} again.` : 'This sends another paid request.'}</p>
      <p>{usesChatgpt ? 'The retry counts toward that account’s ChatGPT plan or credits balance. Earlier attempts may also have used your plan or credits.' : `Estimated job cost: ${money(job.estimateUsd)}. Earlier attempts may also have been charged.`}</p>
      {usageError && <p className="message error" role="alert">{usageError}</p>}
      <div className="form-actions"><button className="primary" disabled={!!operation} onClick={() => { setConfirmRetry(false); void controller.retry().catch(() => {}); }}>{usesChatgpt ? 'Retry using ChatGPT plan' : 'Send paid retry'}</button>{usesChatgpt && <button disabled={openingUsage} onClick={() => { void usage(); }}>Manage usage</button>}<button onClick={() => setConfirmRetry(false)}>Keep current job</button></div></div></Modal>}
  </>;
}


export function GenerationDialog(props: {
  controller: GenerationController; project: Project; settings: AiSettingsView | null; openSettings: () => void; manageUsage: () => Promise<void>; editRequest: (id: string) => void; close: () => void;
}) {
  return <GenerationDetails {...props} />;
}
