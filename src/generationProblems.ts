import type { GenerationJobView, Project, GenerationValidationEvent } from './types';
import { outcomeReason, technicalReason, providerFailureMessage } from './userErrors.js';

/// A specific place that can resolve an outcome the AI itself declared.
export type OutcomeFix = 'aiSettings' | 'import' | 'brief';
export type Outcome = { retryable: boolean; reason: string; fix?: OutcomeFix };
export type JobOutcome = { reason: string; retryable: string[]; blocked: { elementId: string; reason: string; fix?: OutcomeFix }[]; retryAll: boolean; retryBatches: boolean };

export function requestText(project: Project | null, elementId: string): string {
  const request = (project?.requests ?? []).find(item => item.targets.some(target => target.kind === 'element' && target.id === elementId));
  return request?.text ?? '';
}

export const fixLabel = (fix: OutcomeFix) => fix === 'aiSettings' ? 'AI settings' : fix === 'import' ? 'Import…' : 'Project brief';

const terminal = ['failed', 'unknown', 'cancelled', 'blocked', 'applied'];

export const staleResultMessage = "The boundary changed while the AI was working, so this result wasn't applied.";
export function staleResult(job: GenerationJobView | null): boolean {
  return job?.status === 'blocked' && job.problemKind === 'stale';
}

/// The provider stopped because the reply reached the request's output-token limit: a
/// finish_reason of "length" and a provider's own incomplete marker both arrive as `incomplete`.
export const replyCutOffMessage = 'The AI reached its output limit before finishing. Raise Maximum output tokens in AI settings or split the request.';
export function replyCutOff(job: GenerationJobView | null): boolean {
  if (!job || !['failed', 'unknown'].includes(job.status) || job.failureKind !== 'incomplete') return false;
  // Chat providers can report the stop before publishing usage, so a missing total still counts.
  return !job.usage || job.usage.outputTokens >= job.settings.maxOutputTokens;
}

/// Read the fenced JSON blocks a generation snapshot carries.
export function jsonBlocks(markdown: string): any[] {
  const blocks: any[] = [];
  let fence = ''; let lines: string[] = [];
  for (const line of markdown.split('\n')) {
    if (!fence) { const match = /^(`{3,})json$/.exec(line); if (match) { fence = match[1]; lines = []; } }
    else if (line === fence) { try { blocks.push(JSON.parse(lines.join('\n'))); } catch { /* A partial diagnostic is still readable as text. */ } fence = ''; }
    else lines.push(line);
  }
  return blocks;
}

function sentPrompts(job: GenerationJobView): Map<string, string> {
  const prompts = new Map<string, string>();
  for (const block of jsonBlocks(job.requirements?.markdown ?? '')) {
    for (const request of Array.isArray(block?.requests) ? block.requests : []) {
      // The current request packet names each request and lists its layer targets.
      if (typeof request?.text === 'string' && Array.isArray(request.targets)) {
        for (const target of request.targets) {
          if (target?.kind === 'element' && typeof target.elementId === 'string') prompts.set(target.elementId, request.text);
        }
      }
    }
  }
  return prompts;
}

/// The request text was changed after this job sent its frozen requirements.
function requestEdited(job: GenerationJobView | null, elementId: string, prompt?: string): boolean {
  const sent = job && sentPrompts(job).get(elementId);
  return typeof sent === 'string' && typeof prompt === 'string' && sent.trim() !== prompt.trim();
}

/// Map a plain-language reason to the surface that can resolve it, when one applies.
export function resolutionAction(reason: string): OutcomeFix | undefined {
  const text = reason.toLowerCase();
  if (/\b(?:api|credential|sign[ -]?in|log[ -]?in|account|connect|integrat|service|capabilit|payment|booking|database|server|backend|hosting|subscription|component|package|library|plugin|dependency|module)\b/.test(text)) return 'aiSettings';
  if (/\b(?:which|choose|prefer|decide|decision|direction)\b/.test(text) || /\b(?:identity|brand|business|company|contact|phone|address|email|audience|tone|legal|copyright)\b/.test(text)) return 'brief';
  if (/\b(?:file|asset|photo|image|logo|font|icon|screenshot|document|pdf|upload|template|source)\b/.test(text)) return 'import';
  return undefined;
}

/// Classify one unfinished area: only an AI-declared gap is not worth resending unchanged.
export function areaOutcome(job: GenerationJobView | null, elementId: string, prompt?: string): Outcome {
  const retry = (reason = ''): Outcome => ({ retryable: true, reason });
  if (!job || !terminal.includes(job.status)) return retry();
  if (requestEdited(job, elementId, prompt)) return retry();
  const outcome = job.candidate?.requests?.find(item => item.elementId === elementId);
  // Only an explained, plainly worded conflict is the AI's own request for input.
  const explanation = outcome?.status === 'conflict' ? outcome.explanation?.trim() ?? '' : '';
  const declared = !!explanation && !technicalReason(explanation);
  if (declared) {
    return { retryable: false, reason: outcomeReason(explanation, prompt ?? ''), fix: resolutionAction(explanation) };
  }
  return retry();
}

function jobReason(job: GenerationJobView): string {
  if (staleResult(job)) return staleResultMessage;
  const events = [...(job.validationEvents ?? []), ...(job.batches ?? []).flatMap(batch => batch.validationEvents ?? [])];
  if (job.status === 'applied') {
    const skipped = warningEvents(job);
    const kinds = new Set(skipped.map(event => event.kind === 'image' ? 'image changes'
      : event.kind === 'pointer' ? 'interaction changes' : event.kind === 'pagePatch' ? 'page changes' : ['element', 'operation'].includes(event.kind) ? 'layer changes' : 'changes'));
    if (kinds.size) return `Some ${[...kinds].join(' and ')} were skipped. The rest was applied.`;
    if (job.candidate?.requests.some(request => request.status === 'conflict')) return 'Some requests were skipped. The rest was applied.';
    if (job.problemKind === 'partial') return 'Some changes were skipped. The rest was applied.';
    return '';
  }
  if (!['blocked', 'failed', 'unknown'].includes(job.status)) return '';
  const provider = providerFailureMessage(job.providerError, job.message);
  if (provider) return provider;
  const reply = events.find(event => event.elementId == null && event.kind === 'replyStructure');
  if (reply) return "The AI's reply was incomplete, so nothing was changed.";
  if (job.problemKind === 'rejected') return "The AI's reply couldn't be used, so nothing was changed.";
  return job.status === 'unknown' ? "Desegnos couldn't confirm whether the changes were applied." : "The AI couldn't finish these requests, so nothing was changed.";
}

/// Classify a whole job, keeping the retryable areas separate from the declared gaps.
export function jobOutcome(job: GenerationJobView | null, promptOf: (elementId: string) => string | undefined = () => undefined): JobOutcome {
  if (!job || !terminal.includes(job.status)) return { reason: '', retryable: [], blocked: [], retryAll: false, retryBatches: false };
  const ids = new Set<string>();
  for (const area of job.areas ?? []) ids.add(area.elementId);
  for (const outcome of job.candidate?.requests ?? []) if (outcome.elementId) ids.add(outcome.elementId);
  const retryable: string[] = [];
  const blocked: JobOutcome['blocked'] = [];
  // A stale result was never applied, so even its finished areas are worth resending.
  const stale = staleResult(job);
  const rejected = ['blocked', 'failed', 'unknown'].includes(job.status) && ((job.validationEvents ?? []).some(event => event.elementId == null && event.kind === 'replyStructure') || job.problemKind === 'rejected');
  for (const elementId of ids) {
    const outcome = areaOutcome(job, elementId, promptOf(elementId));
    if (!rejected && !outcome.retryable) { blocked.push({ elementId, reason: outcome.reason, fix: outcome.fix }); continue; }
    const implemented = job.candidate?.requests?.some(request => request.elementId === elementId && request.status === 'implemented');
    if (stale || ['blocked', 'failed', 'unknown'].includes(job.status) || !implemented) retryable.push(elementId);
  }
  // A batched job keeps its finished parts; a failed part is retried on its own.
  const retryBatches = (job.batches ?? []).some(batch => ['failed', 'unknown', 'cancelled', 'blocked'].includes(batch.status));
  return { reason: jobReason(job), retryable, blocked,
    retryAll: ids.size === 0 && !blocked.length && ['blocked', 'failed', 'unknown', 'cancelled'].includes(job.status),
    retryBatches };
}

/// The AI declared a scope conflict for this request: its fix belongs outside the area.
export function declaredConflict(job: GenerationJobView | null, elementId: string): boolean {
  return !!job?.candidate?.requests.some(request => request.elementId === elementId && request.status === 'conflict');
}

/// Job and batch snapshots can carry the same diagnostics; show each warning once.
function warningEvents(job: GenerationJobView | null): GenerationValidationEvent[] {
  const events = [...(job?.validationEvents ?? []), ...(job?.batches ?? []).flatMap(batch => batch.validationEvents ?? [])];
  const seen = new Set<string>();
  return events.filter(event => {
    if (!['dropped', 'skipped', 'conflicted', 'warning'].includes(event.disposition) || event.kind === 'telemetryStorage' || event.kind === 'replyStructure') return false;
    const key = JSON.stringify([event.kind, event.disposition, event.elementId, event.message]);
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

/// Recoverable model mistakes remain visible after the valid operations have applied.
export function generationWarnings(job: GenerationJobView | null): string[] {
  return warningEvents(job).map(event => event.message);
}
