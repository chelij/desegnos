import { Notice, ProblemBanner, needsAiSettings, type ProblemAction } from './ProblemBanner.js';
import { userError, generationError } from './userErrors.js';
import { useEffect, useRef, useState } from 'react';
import { MenuButton, type MenuItem } from './Menus';
import { scopeLabel } from './Notes';
import { SourceView } from './Preview';
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
    if (index >= 0) return `${page.name} · Box ${index + 1}`;
  }
  return 'Request';
}

export function GenerationFeedback({ controller, action, openSettings, details }: {
  details?: () => void;
  controller: GenerationController; action?: ProblemAction; openSettings?: () => void;
}) {
  const { job: currentJob, operation, elapsedSeconds, error } = controller;
  const job = operation === 'Preparing AI instructions…' ? null : currentJob;
  if (!job && !operation && !error && !controller.stopped) return null;
  const minutes = elapsedSeconds ?? 0;
  const headings = { prepared: 'Getting ready…', generating: minutes >= 60 ? 'Still building your page…' : 'Building your page…',
    ready: 'Updating your page…', blocked: 'Needs attention', failed: 'Couldn’t build the page', unknown: 'Not sure the page was built',
    cancelled: 'Stopped', discarded: 'Result discarded', applying: 'Updating your page…', applied: 'Page updated' };
  const heading = operation === 'Stopping…' ? 'Stopping…' : controller.stopped ? 'Stopped' : operation ? 'Getting ready…' : job ? headings[job.status] : 'Couldn’t start';
  return <Notice className={`generation-notice status-${job?.status ?? 'preparing'}`} variant={error || job && ['failed', 'blocked', 'unknown'].includes(job.status) ? 'error' : job?.status === 'applied' ? 'success' : 'info'} priority="result" title={heading} dismissKey={`${job?.jobId}:${job?.status}:${operation}:${error}`}
    message={error ? userError(error) : elapsedSeconds !== null && controller.canStop ? `${Math.floor(minutes / 60)}:${String(minutes % 60).padStart(2, '0')}` : ''}
    action={action ?? (needsAiSettings(error || job?.message || '') && openSettings ? { label: 'AI settings', onClick: openSettings } : controller.canStop ? { label: 'Stop', onClick: () => { void controller.cancel(); } } : controller.stopped || job && ['cancelled', 'failed', 'unknown', 'blocked'].includes(job.status) ? { label: 'Try again', onClick: () => { void controller.prepare().catch(() => {}); } } : undefined)}
    secondaryAction={details ? { label: 'Details', onClick: details } : undefined} />;
}

export function GenerationDetails({ controller, project, settings, openSettings, manageUsage, editRequest }: {
  controller: GenerationController; project: Project; settings: AiSettingsView | null; openSettings: () => void; manageUsage: () => Promise<void>; editRequest: (id: string) => void;
}) {
  const { job, operation } = controller;
  const [tab, setTab] = useState<'review' | 'requirements' | 'attempts' | 'details'>('review');
  const [showSource, setShowSource] = useState(false);
  const [pageId, setPageId] = useState('');
  const [selected, setSelected] = useState<number[]>([]);
  const [confirmRetry, setConfirmRetry] = useState(false);
  const retryConfirmation = useRef<HTMLElement>(null);
  const retryTrigger = useRef<HTMLElement | null>(null);
  useEffect(() => { if (confirmRetry) { retryConfirmation.current?.scrollIntoView({ block: 'nearest' }); retryConfirmation.current?.querySelector('button')?.focus({ preventScroll: true }); } }, [confirmRetry]);
  function dismissRetry() { setConfirmRetry(false); if (retryTrigger.current?.isConnected) retryTrigger.current.focus({ preventScroll: true }); }
  const [usageError, setUsageError] = useState('');
  const [openingUsage, setOpeningUsage] = useState(false);
  useEffect(() => { setConfirmRetry(false); setSelected(job?.acceptedDecisionIndices ?? []); setPageId(''); setTab('review'); setShowSource(false); }, [job?.jobId]);
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
  const canRetry = !!job && (['failed', 'unknown', 'cancelled', 'blocked'].includes(job.status) || (job.status === 'applied' && conflicts.length > 0));
  const canDiscard = !!job && ['prepared', 'ready', 'blocked', 'failed', 'unknown', 'cancelled'].includes(job.status);
  const alreadyApplied = !!job && project.appliedJobIds.includes(job.jobId);
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
      ...(canRetry ? [{ id: 'retry-snapshot', label: 'Retry with current page', disabled: !!operation, action: () => { retryTrigger.current = document.activeElement instanceof HTMLElement ? document.activeElement : null; setConfirmRetry(true); } }] : []),
    ] },
    { id: 'settings', label: 'AI settings', action: openSettings },
    ...(canDiscard ? [{ id: 'discard', label: job?.status === 'prepared' ? 'Discard request' : 'Discard result', disabled: !!operation, action: () => { void controller.discard().catch(() => {}); } }] : []),
  ];
  return <>
    <GenerationFeedback controller={controller} openSettings={openSettings}
      action={job?.status === 'prepared' ? { label: 'Send and generate', disabled: !!operation, onClick: () => { void controller.start().catch(() => {}); } } : undefined} /><div className="generation-dialog">
      {usageError && <ProblemBanner message={userError(usageError)} dismiss={() => setUsageError('')} />}
      {job?.dossierError && <ProblemBanner message={userError(job.dossierError)} />}
      {!job ? <><p>{operation ? 'Preparing…' : 'No generation request is prepared.'}</p><div className="form-actions">
        <button onClick={openSettings}>AI settings</button>{!operation && <button onClick={() => { void controller.prepare().catch(() => {}); }}>Prepare request again</button>}
      </div></> : <>
        {job.status === 'unknown' && <Notice variant="warning" message={`The result did not arrive. Trying again ${usesChatgpt ? 'uses your plan again' : 'may cost again'}.`} />}
        {job.status === 'cancelled' && <p className="small muted">{usesChatgpt ? 'Submitted work may still count toward your plan.' : 'Submitted work may still be charged.'}</p>}
        {usageLimit && <Notice variant="warning" title="ChatGPT usage limit reached" message="Review your limit in ChatGPT settings." />}
        {job.status === 'applied' && !alreadyApplied && <Notice variant="warning" message="These changes are no longer on the page. They may have been undone." />}
        <div className="generation-review-toolbar"><p>{job.requirements.regionCount} {job.requirements.regionCount === 1 ? 'request' : 'requests'} · {affectedPages.join(', ')} · {readable(job.generationMode)}</p><MenuButton label="More" ariaLabel="Generation options" items={more} /></div>
        <details className="generation-section" open={tab === 'details' || undefined}><summary>Request details</summary><dl className="generation-facts"><dt>Cost</dt><dd>{usesChatgpt ? `Counts toward ${frozenAccount?.label ?? 'your ChatGPT'} plan or credits` : `About ${money(job.estimateUsd)}; final charges may vary`}</dd><dt>Provider</dt><dd>{service}</dd><dt>Authentication</dt><dd>{usesChatgpt ? 'ChatGPT plan' : 'API key'}</dd><dt>Model</dt><dd>{job.settings.model}</dd><dt>Generation mode</dt><dd>{readable(job.generationMode)}</dd>
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
        {job.usage && <p className="token-usage">Reported usage: {job.usage.inputTokens.toLocaleString()} input · {job.usage.outputTokens.toLocaleString()} output · {job.usage.totalTokens.toLocaleString()} total tokens</p>}</details>
        {tab === 'requirements' && <section className="generation-section"><h3>REQUIREMENTS.md</h3>
          <p className="digest">SHA-256: <code>{job.requirements.sha256}</code></p><pre className="markdown-source" tabIndex={0} aria-label="Generation REQUIREMENTS.md source">{job.requirements.markdown}</pre>
        </section>}
        {tab === 'attempts' && <section className="generation-section"><h3>Actual attempts</h3>{job.attempts.length === 0 ? <p className="muted">No provider attempt has been recorded.</p> : <ol className="attempt-list">{job.attempts.map(attempt => <li key={attempt.ordinal}>
          <strong>Attempt {attempt.ordinal} · {readable(attempt.status)}</strong><p className="small">Started {attempt.startedAt}{attempt.finishedAt ? ` · Finished ${attempt.finishedAt}` : ''}</p>
          {attempt.responseId && <p className="small response-id">Response: {attempt.responseId}</p>}{attempt.usage && <p className="small">{attempt.usage.inputTokens} input · {attempt.usage.outputTokens} output · {attempt.usage.totalTokens} total tokens</p>}
        </li>)}</ol>}</section>}
        <section className="generation-section">{!candidate ? <>{job.status === 'prepared' ? <p className="muted">Ready to send.</p> : job.status === 'generating' ? <p className="muted">Generating…</p> : <><p>Needs attention</p>{job.message && <details><summary>Technical details</summary><p className="small">{job.message}</p></details>}</>}</> : <>
          {job.status === 'applied' && conflicts.length > 0 && <p>Applied {candidate.regions.length - conflicts.length} of {candidate.regions.length} areas</p>}
          {(job.status === 'blocked' || conflicts.length > 0) ? <section aria-label="Finish your draft">
            <ul className="generation-list">{candidate.regions.filter(region => region.status !== 'implemented').map(region => {
              const request = project.pages.flatMap(page => page.regions).find(regionItem => regionItem.id === region.regionId);
              return <li key={region.regionId}><strong>{requestLabel(project, region.regionId)}</strong><p className="preserve-text">{request?.prompt}</p>
                <span>Needs attention</span></li>;
            })}</ul>
            <details><summary>Technical details</summary><p>{job.message}</p><p>{candidate.summary}</p><ul className="generation-list">{candidate.regions.filter(region => region.status !== 'implemented').map(region => <li key={region.regionId}><strong>{requestLabel(project, region.regionId)}</strong><p>{region.explanation}</p></li>)}</ul></details>
          </section> : <><h3>Your page</h3><p>{candidate.summary}</p></>}
          {generatedImages.length > 0 && <section aria-label="Generated images"><h3>Generated images</h3>{generatedImages.map((image, index) => <figure key={index}>
            {image.src.startsWith('data:image/png;base64,') && !image.failed && <img src={image.src} alt="Generated result" width={144} height={144} style={{ objectFit: 'contain' }} />}
            <figcaption><strong>{image.regionId ? requestLabel(project, image.regionId) : project.pages.find(page => page.id === image.pageId)?.name}</strong><p>{image.prompt}</p>{image.failed && <p>Image generation failed. Replace this image or try again.</p>}</figcaption>
          </figure>)}</section>}
          {source && <><div className="candidate-page-picker"><label>Result page<select value={source.pageId} onChange={event => setPageId(event.target.value)}>{candidate.pages.map(item => <option key={item.pageId} value={item.pageId}>{project.pages.find(page => page.id === item.pageId)?.name ?? item.pageId}</option>)}</select></label></div>
            {showSource && <section className="candidate-source"><h3>Source code</h3><SourceView key={source.pageId} content={source.content} label="Generated" /></section>}
          </>}
          {candidate.decisionProposals.length > 0 && <details className="proposal-list"><summary>Suggested rules ({candidate.decisionProposals.length})</summary>{candidate.decisionProposals.map((proposal, index) => <label key={index} className="proposal">
            <input type="checkbox" checked={selected.includes(index)} disabled={job.status !== 'ready' || alreadyApplied || !!operation} onChange={event => setSelected(previous => event.target.checked ? [...previous, index] : previous.filter(value => value !== index))} />
            <span><strong>{proposal.topic}</strong><span className="preserve-text">{proposal.value}</span><span className="small muted">{scopeLabel(project, proposal.scope)} · {proposal.rationale}</span></span>
          </label>)}</details>}
          {candidate.criticism.length > 0 && <details className="generation-feedback"><summary>Design feedback ({candidate.criticism.length})</summary><ul className="generation-list">{candidate.criticism.map((criticism, index) => <li key={index}><strong>{criticism.regionId ? requestLabel(project, criticism.regionId) : 'Whole site'}</strong><p>{criticism.message}</p><p className="muted">{criticism.suggestion}</p></li>)}</ul></details>}
          <details className="generation-coverage"><summary>Request details</summary><div className="table-scroll"><table className="coverage-table"><thead><tr><th>Request</th><th>Result</th><th>Explanation</th></tr></thead><tbody>{candidate.regions.map(region => <tr key={region.regionId}><th>{requestLabel(project, region.regionId)}</th><td>{readable(region.status)}</td><td>{region.status === 'conflict' ? generationError(region.explanation) : region.explanation}</td></tr>)}</tbody></table></div>
            {candidate.componentRequests.length > 0 && <ul className="generation-list">{candidate.componentRequests.map((component, index) => <li key={index}><strong>{component.capability}</strong><p>{component.resolution === 'native' ? 'Built in' : 'Unavailable'}: {component.reason}</p></li>)}</ul>}
          </details>
        </>}</section>
        <div className="generation-actions form-actions">
          {usageLimit && <button className="primary" disabled={openingUsage} onClick={() => { void usage(); }}>Manage usage</button>}
          {job.status === 'ready' && !alreadyApplied && <button className="primary" disabled={!!operation} onClick={() => { void controller.apply([...selected].sort((a, b) => a - b)).catch(() => {}); }}>{conflicts.length > 0 ? 'Apply ready changes' : 'Apply changes'}</button>}
          {canRetry && <button className="primary" disabled={!!operation || !project.pages.some(page => page.regions.some(region => !region.completed && region.prompt.trim()))} onClick={() => { void controller.prepare().catch(() => {}); }}>Try again</button>}
        </div>
      </>}
    </div>
    {confirmRetry && job && <section ref={retryConfirmation} aria-label="Confirm retry" className="dialog-body"><strong>Try these requests with the current page?</strong><p>{usesChatgpt ? `This uses ${frozenAccount?.label ?? 'the original ChatGPT account'} again.` : 'This sends another paid request.'}</p>
      <p>{usesChatgpt ? 'The retry counts toward that account’s ChatGPT plan or credits balance. Earlier attempts may also have used your plan or credits.' : `Estimated job cost: ${money(job.estimateUsd)}. Earlier attempts may also have been charged.`}</p>
      {usageError && <ProblemBanner message={userError(usageError)} dismiss={() => setUsageError('')} />}
      <div className="form-actions"><button className="primary" disabled={!!operation} onClick={() => { dismissRetry(); void controller.retry().catch(() => {}); }}>{usesChatgpt ? 'Retry using ChatGPT plan' : 'Send paid retry'}</button>{usesChatgpt && <button disabled={openingUsage} onClick={() => { void usage(); }}>Manage usage</button>}<button onClick={dismissRetry}>Keep current job</button></div></section>}
  </>;
}
