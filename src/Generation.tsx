import { Notice, needsAiSettings, type ProblemAction } from './ProblemBanner.js';
import { userError } from './userErrors.js';
import { fixLabel, jobOutcome, requestText, staleResult } from './generationProblems.js';
import type { Element, Project } from './types';
import type { GenerationController } from './useGeneration';

function allElements(elements: Element[]): Element[] {
  return elements.flatMap(element => [element, ...allElements(element.children)]);
}

export function projectElement(project: Project, id: string): Element | null {
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
  const { job: currentJob, operation, elapsedSeconds, error } = controller;
  const job = operation === 'Preparing AI instructions…' ? null : currentJob;
  if (!job && !operation && !error && !controller.stopped) return null;
  const minutes = elapsedSeconds ?? 0;
  const review = jobOutcome(job, id => project && projectElement(project, id) ? requestText(project, id) : undefined);
  const needsInput = review.blocked.length > 0 && review.retryable.length === 0 && !review.retryAll;
  const retry = review.retryable.length > 0 || review.retryAll || review.retryBatches || (controller.stopped && !review.blocked.length);
  const retryRequests = () => {
    if (review.retryBatches) void controller.retry().catch(() => {});
    else void controller.prepare(review.retryable.length ? review.retryable : undefined).catch(() => {});
  };
  const blocked = review.blocked[0];
  const headings = { prepared: 'Getting ready…', generating: minutes >= 60 ? 'Still building your layers…' : 'Building your layers…',
    ready: 'Updating your layers…', blocked: staleResult(job) ? 'Design changed — not applied' : needsInput ? 'Needs your input' : 'Needs attention', failed: 'Couldn’t build the layers', unknown: 'Not sure the layers were built',
    cancelled: 'Stopped', discarded: 'Result discarded', applying: 'Updating your layers…', applied: 'Layers updated' };
  const heading = operation === 'Stopping…' ? 'Stopping…' : controller.stopped ? 'Stopped' : operation ? 'Getting ready…' : job ? headings[job.status] : 'Couldn’t start';
  const fix = blocked?.fix ? () => {
    const open = blocked.fix === 'aiSettings' ? openSettings : blocked.fix === 'import' ? openImport : openBrief;
    open?.();
  } : undefined;
  const fixAction = blocked?.fix && fix ? { label: fixLabel(blocked.fix), onClick: fix } : undefined;
  const change = blocked && changeRequest ? { label: 'Change request', onClick: () => changeRequest(blocked.elementId) } : undefined;
  const changeAction = change ?? fixAction;
  const secondaryAction = needsInput && change && fixAction ? fixAction : details ? { label: 'Details', onClick: details } : undefined;
  return <Notice className={`generation-notice status-${job?.status ?? 'preparing'}`} variant={error || job && ['failed', 'blocked', 'unknown'].includes(job.status) ? 'error' : job?.status === 'applied' ? 'success' : 'info'} priority="result" title={heading} dismissKey={`${job?.jobId}:${job?.status}:${operation}:${error}`}
    message={error ? userError(error) : elapsedSeconds !== null && controller.canStop ? `${Math.floor(minutes / 60)}:${String(minutes % 60).padStart(2, '0')}` : blocked?.reason ?? ''}
    action={action ?? (needsAiSettings(error || job?.message || '') && openSettings ? { label: 'AI settings', onClick: openSettings } : controller.canStop ? { label: 'Stop', onClick: () => { void controller.cancel(); } } : changeAction ?? (retry ? { label: 'Try again', onClick: retryRequests } : undefined))}
    secondaryAction={secondaryAction} />;
}
