import type { Element, GenerationJobView, Page, Project, Rect, GenerationValidationEvent } from './types';
import { generationError, outcomeReason, technicalReason, userError } from './userErrors.js';

/// A specific place that can resolve an outcome the AI itself declared.
export type OutcomeFix = 'aiSettings' | 'import' | 'brief';
export type Outcome = { retryable: boolean; reason: string; fix?: OutcomeFix };
export type JobOutcome = { reason: string; retryable: string[]; blocked: { elementId: string; reason: string; fix?: OutcomeFix }[]; retryAll: boolean; retryBatches: boolean };
export type AreaProblem = { key: string; elementId: string; message: string; stale: boolean; retryable: boolean; fix?: OutcomeFix; rect: Rect };

export function requestText(project: Project | null, elementId: string): string {
  const request = (project?.requests ?? []).find(item => item.targets.some(target => target.kind === 'element' && target.id === elementId));
  if (request) return request.text;
  // Read a legacy per-layer request still present in an in-memory fixture.
  const pending = (element: Element): string | null => {
    const legacy = (element.meta as { pending?: { prompt?: string } }).pending;
    if (legacy?.prompt) return legacy.prompt;
    for (const child of element.children) { const found = pending(child); if (found) return found; }
    return null;
  };
  for (const page of project?.pages ?? []) for (const element of page.elements) { const found = pending(element); if (found) return found; }
  return '';
}

export const fixLabel = (fix: OutcomeFix) => fix === 'aiSettings' ? 'AI settings' : fix === 'import' ? 'Import…' : 'Project brief';

const terminal = ['failed', 'unknown', 'cancelled', 'blocked', 'applied'];

/// A finished result the boundary changed out from under; both app messages contain "is stale".
export const staleResultMessage = "The boundary changed while the AI was working, so this result wasn't applied.";
export function staleResult(job: GenerationJobView | null): boolean {
  return job?.status === 'blocked' && /\bis stale\b/i.test(job.message ?? '');
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
    // The current requirements packet wraps requests in one object; older saved jobs carried
    // one object per request. Read both so a restored job still detects an edited prompt.
    if (typeof block?.elementId === 'string' && typeof block.request === 'string') prompts.set(block.elementId, block.request);
    for (const request of Array.isArray(block?.requests) ? block.requests : []) {
      if (typeof request?.elementId === 'string' && typeof request.request === 'string') prompts.set(request.elementId, request.request);
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
export function requestEdited(job: GenerationJobView | null, elementId: string, prompt?: string): boolean {
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
    const kinds = new Set(skipped.map(event => /image/i.test(event.kind) ? 'image changes'
      : /pointer/i.test(event.kind) ? 'interaction changes' : /page/i.test(event.kind) ? 'page changes' : /operation|element|layer/i.test(event.kind) ? 'layer changes' : 'changes'));
    if (kinds.size) return `Some ${[...kinds].join(' and ')} were skipped. The rest was applied.`;
    if (job.candidate?.requests.some(request => request.status === 'conflict')) return 'Some requests were skipped. The rest was applied.';
    if (/\b(?:skipped|dropped|warnings?)\b/i.test(job.message ?? '')) return 'Some changes were skipped. The rest was applied.';
    return '';
  }
  if (!['blocked', 'failed', 'unknown'].includes(job.status)) return '';
  const reply = events.find(event => event.elementId == null && event.kind === 'replyStructure');
  const message = (job.message?.trim() || reply?.message || '').replace(/^Desegnos rejected the candidate:\s*/i, '');
  if (reply || /missing field|incomplete|unexpected end|requirementsSha256/i.test(message)) return "The AI's reply was incomplete, so nothing was changed.";
  if (/generation result|candidate|schema|JSON|deserializ|invalid.*(?:reply|response|format)/i.test(message)) return "The AI's reply couldn't be used, so nothing was changed.";
  if (!message) return job.status === 'unknown' ? "Desegnos couldn't confirm whether the changes were applied." : "The AI couldn't finish these requests, so nothing was changed.";
  const translated = userError(message);
  return translated === message && technicalReason(message) ? generationError(message) : translated;
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
  const rejected = ['blocked', 'failed', 'unknown'].includes(job.status) && ((job.validationEvents ?? []).some(event => event.elementId == null && event.kind === 'replyStructure') || /^Desegnos rejected the candidate:/i.test(job.message ?? ''));
  for (const elementId of ids) {
    const outcome = areaOutcome(job, elementId, promptOf(elementId));
    if (!rejected && !outcome.retryable) { blocked.push({ elementId, reason: outcome.reason, fix: outcome.fix }); continue; }
    const implemented = job.candidate?.requests?.some(request => request.elementId === elementId && request.status === 'implemented');
    if (stale || ['blocked', 'failed', 'unknown'].includes(job.status) || !implemented) retryable.push(elementId);
  }
  // A batched job keeps its finished parts; a failed part is retried on its own.
  const retryBatches = (job.batches ?? []).some(batch => ['failed', 'unknown', 'cancelled', 'blocked'].includes(batch.status));
  return { reason: jobReason(job), retryable: job.retryable === false ? [] : retryable, blocked,
    retryAll: job.retryable !== false && ids.size === 0 && !blocked.length && ['blocked', 'failed', 'unknown', 'cancelled'].includes(job.status),
    retryBatches: job.retryable !== false && retryBatches };
}

/// The AI declared a scope conflict for this request: its fix belongs outside the area.
export function declaredConflict(job: GenerationJobView | null, elementId: string): boolean {
  return !!job?.candidate?.requests.some(request => request.elementId === elementId && request.status === 'conflict');
}

export function areaProblems(job: GenerationJobView | null, page: Page, project: Project | null = null): AreaProblem[] {
  if (!job || ['prepared', 'generating', 'ready', 'applying'].includes(job.status)) return [];
  const stale = staleResult(job);
  const elements: Element[] = page.elements.flatMap(function walk(element): Element[] { return [element, ...element.children.flatMap(walk)]; });
  return elements.flatMap((element): AreaProblem[] => {
    if (!job.areas.some(area => area.pageId === page.id && area.elementId === element.id)) return [];
    const outcome = job.candidate?.requests?.find(item => item.elementId === element.id);
    const prompt = project ? requestText(project, element.id) : undefined;
    const result = areaOutcome(job, element.id, prompt);
    // A stale result did nothing to the canvas; say so where the visible layer is.
    if (stale && result.retryable && (project ? !!prompt?.trim() : true)) return [{ key: `${job.jobId}:${element.id}`, elementId: element.id,
      message: staleResultMessage, stale: true, retryable: true, rect: element.rect }];
    if (!outcome || outcome.status === 'implemented') return [];
    const reason = outcome.explanation || job.message;
    const message = result.retryable ? generationError(reason, prompt ?? '') : result.reason;
    return [{ key: `${job.jobId}:${element.id}`, elementId: element.id,
      message, stale: false, retryable: result.retryable, fix: result.fix, rect: element.rect }];
  });
}

export type Callout = { key: string; rect: Rect };
const overlaps = (a: Rect, b: Rect) => a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y;
// Coordinates are in painted canvas pixels; cards keep readable type at every zoom.
export function placeCallouts(items: Callout[], areas: Rect[], origin: { x: number; y: number }, zoom: number, dismissed: readonly string[] = []) {
  const paint = (r: Rect): Rect => ({ x: (r.x-origin.x)*zoom, y: (r.y-origin.y)*zoom, width:r.width*zoom, height:r.height*zoom });
  const obstacles = areas.map(paint);
  const placed: (Rect & { key: string })[] = [];
  for (const item of items) {
    if (dismissed.includes(item.key)) continue;
    const area = paint(item.rect);
    const card = { key:item.key, x:area.x, y:area.y-168, width:360, height:132 };
    // Move above every intersecting area/card, retaining the area anchor horizontally.
    while (true) {
      const hit = [...obstacles, ...placed].filter(r => overlaps(card, r));
      if (!hit.length) break;
      card.y = Math.min(...hit.map(r => r.y))-card.height-36;
    }
    placed.push(card);
  }
  return placed;
}

/// Job and batch snapshots can carry the same diagnostics; show each warning once.
function warningEvents(job: GenerationJobView | null): GenerationValidationEvent[] {
  const events = [...(job?.validationEvents ?? []), ...(job?.batches ?? []).flatMap(batch => batch.validationEvents ?? [])];
  const seen = new Set<string>();
  return events.filter(event => {
    if (!/^(?:dropped|skipped|conflicted|warning)$/.test(event.disposition) || event.kind === 'telemetryStorage' || event.kind === 'replyStructure') return false;
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
