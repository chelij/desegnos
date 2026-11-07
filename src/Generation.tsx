import { Notice, needsAiSettings, type ProblemAction } from './ProblemBanner.js';
import { userError, providerFailureMessage } from './userErrors.js';
import { fixLabel, jobOutcome, replyCutOff, replyCutOffMessage, requestText, staleResult } from './generationProblems.js';
import type { Element, Project } from './types';
import type { GenerationController } from './useGeneration';

function allElements(elements: Element[]): Element[] {
  return elements.flatMap(element => [element, ...allElements(element.children)]);
}

function projectElement(project: Project, id: string): Element | null {
  for (const page of project.pages) {
    const found = allElements(page.elements).find(element => element.id === id);
    if (found) return found;
  }
  return null;
}

export function GenerationFeedback({ controller, action, openSettings, openImport, openBrief, changeRequest, project, details }: {
  details?: () => void;
  controller: GenerationController; action?: ProblemAction; openSettings?: () => void; openImport?: () => void; openBrief?: () => void;
  changeRequest?: (elementId: string) => void; project?: Project;
}) {
  const { job: currentJob, phase, operation, elapsedSeconds, error } = controller;
  // While fresh instructions are still being prepared, the loaded job is not what the notice is about.
  const job = phase.name === 'preparing' && phase.step === 'reading' ? null : currentJob;
  if (!job && !operation && !error && !controller.stopped) return null;
  const minutes = elapsedSeconds ?? 0;
  const review = jobOutcome(job, id => project && projectElement(project, id) ? requestText(project, id) : undefined);
  const cutOff = replyCutOff(job);
  const partial = job?.status === 'applied' && !!review.reason;
  const needsInput = review.blocked.length > 0 && review.retryable.length === 0 && !review.retryAll;
  const retry = review.retryable.length > 0 || review.retryAll || review.retryBatches || (job?.retryable !== false && controller.stopped && !review.blocked.length);
  const retryRequests = () => {
    if (review.retryBatches) void controller.retry();
    else void controller.prepare(review.retryable.length ? review.retryable : undefined);
  };
  const blocked = review.blocked[0];
  // Live progress from the stream: the page being written now, and how many have started.
  const started = job?.status === 'generating' ? job.pagesStarted ?? [] : [];
  const buildingPage = started.length ? project?.pages.find(page => page.id === started[started.length - 1]) : undefined;
  const pageTotal = Math.max(job?.requirements?.pageCount ?? 0, started.length);
  const pageProgress = started.length && pageTotal > 1 ? ` · ${started.length} of ${pageTotal} pages` : '';
  const headings = { prepared: 'Getting ready…', generating: buildingPage ? `Building ${buildingPage.name}…` : minutes >= 60 ? 'Still building your layers…' : 'Building your layers…',
    ready: 'Updating your layers…', blocked: staleResult(job) ? 'Design changed — not applied' : needsInput ? 'Needs your input' : 'Needs attention', failed: cutOff ? 'Reply cut off' : 'Couldn’t build the layers', unknown: cutOff ? 'Reply cut off' : 'Not sure the layers were built',
    cancelled: 'Stopped', discarded: 'Result discarded', applying: 'Updating your layers…', applied: partial ? 'Layers updated with warnings' : 'Layers updated' };
  const providerMessage = job && !cutOff && ['failed', 'unknown'].includes(job.status) ? providerFailureMessage(job.providerError, job.provider, job.message) : null;
  const heading = phase.name === 'stopping' ? 'Stopping…' : controller.stopped ? 'Stopped' : operation ? 'Getting ready…' : providerMessage && job?.providerError?.httpStatus && job.providerError.httpStatus >= 400 ? 'Couldn’t build the layers' : job ? headings[job.status] : 'Couldn’t start';
  const fix = blocked?.fix ? () => {
    const open = blocked.fix === 'aiSettings' ? openSettings : blocked.fix === 'import' ? openImport : openBrief;
    open?.();
  } : undefined;
  const fixAction = blocked?.fix && fix ? { label: fixLabel(blocked.fix), onClick: fix } : undefined;
  const change = blocked && changeRequest ? { label: 'Change request', onClick: () => changeRequest(blocked.elementId) } : undefined;
  const changeAction = change ?? fixAction;
  const secondaryAction = details ? { label: 'Details', onClick: details } : needsInput && change && fixAction ? fixAction : undefined;
  return <Notice className={`generation-notice status-${job?.status ?? 'preparing'}`} variant={error || job && ['failed', 'blocked', 'unknown'].includes(job.status) ? 'error' : partial ? 'warning' : job?.status === 'applied' ? 'success' : 'info'} priority="result" title={heading} dismissKey={`${job?.jobId}:${job?.status}:${operation}:${error}`}
    message={providerMessage || (job?.status === 'generating' && job.providerRetry ? job.message : error ? userError(error) : elapsedSeconds !== null && controller.canStop ? `${Math.floor(minutes / 60)}:${String(minutes % 60).padStart(2, '0')}${pageProgress}` : cutOff ? replyCutOffMessage : job?.status === 'applied' ? review.reason : blocked?.reason || review.reason)}
    action={action ?? (cutOff && openSettings ? { label: 'AI settings', onClick: openSettings } : needsAiSettings(error || job?.message || '', job?.providerError, job?.provider) && openSettings ? { label: 'AI settings', onClick: openSettings } : controller.canStop ? { label: 'Stop', onClick: () => { void controller.cancel(); } } : changeAction ?? (retry ? { label: 'Try again', onClick: retryRequests } : undefined))}
    secondaryAction={secondaryAction} />;
}
