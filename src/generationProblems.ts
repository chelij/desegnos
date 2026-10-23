import type { GenerationJobView, Page, Rect } from './types';
import { generationError, outcomeReason, technicalReason } from './userErrors.js';

/// A specific place that can resolve an outcome the AI itself declared.
export type OutcomeFix = 'aiSettings' | 'import' | 'brief';
export type Outcome = { retryable: boolean; reason: string; fix?: OutcomeFix };
export type JobOutcome = { retryable: string[]; blocked: { regionId: string; reason: string; fix?: OutcomeFix }[]; retryAll: boolean };
export type AreaProblem = { key: string; regionId: string; draftId?: string; message: string; stale: boolean; retryable: boolean; fix?: OutcomeFix; rect: Rect };

export const fixLabel = (fix: OutcomeFix) => fix === 'aiSettings' ? 'AI settings' : fix === 'import' ? 'Import…' : 'Project brief';

const terminal = ['failed', 'unknown', 'cancelled', 'blocked', 'applied'];

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
    if (typeof block?.regionId === 'string' && typeof block.prompt === 'string') prompts.set(block.regionId, block.prompt);
  }
  return prompts;
}

/// The request text was changed after this job sent its frozen requirements.
export function requestEdited(job: GenerationJobView | null, regionId: string, prompt?: string): boolean {
  const sent = job && sentPrompts(job).get(regionId);
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
export function areaOutcome(job: GenerationJobView | null, regionId: string, prompt?: string): Outcome {
  const retry = (reason = ''): Outcome => ({ retryable: true, reason });
  if (!job || !terminal.includes(job.status)) return retry();
  if (requestEdited(job, regionId, prompt)) return retry();
  const outcome = job.candidate?.regions?.find(item => item.regionId === regionId);
  const unavailable = job.candidate?.componentRequests?.find(item => item.regionId === regionId && item.resolution === 'unavailable');
  // Only an explained, plainly worded conflict is the AI's own request for input.
  const explanation = outcome?.status === 'conflict' ? outcome.explanation?.trim() ?? '' : '';
  const declared = !!explanation && !technicalReason(explanation);
  if (declared || unavailable) {
    const reason = declared ? explanation : '';
    const message = reason || unavailable?.reason || job.message || '';
    return { retryable: false, reason: outcomeReason(message, prompt ?? ''), fix: resolutionAction(`${reason} ${unavailable?.reason ?? ''}`) };
  }
  return retry();
}

/// Classify a whole job, keeping the retryable areas separate from the declared gaps.
export function jobOutcome(job: GenerationJobView | null, promptOf: (regionId: string) => string | undefined = () => undefined): JobOutcome {
  if (!job || !terminal.includes(job.status)) return { retryable: [], blocked: [], retryAll: false };
  const ids = new Set<string>();
  for (const area of job.areas) ids.add(area.regionId);
  for (const draft of job.drafts ?? []) for (const id of draft.regionIds) ids.add(id);
  for (const outcome of job.candidate?.regions ?? []) ids.add(outcome.regionId);
  const retryable: string[] = [];
  const blocked: JobOutcome['blocked'] = [];
  for (const regionId of ids) {
    const outcome = areaOutcome(job, regionId, promptOf(regionId));
    if (!outcome.retryable) { blocked.push({ regionId, reason: outcome.reason, fix: outcome.fix }); continue; }
    const latest = job.drafts?.filter(draft => draft.regionIds.includes(regionId)).at(-1);
    const implemented = job.candidate?.regions?.some(region => region.regionId === regionId && region.status === 'implemented');
    if (!implemented && !latest?.applied) retryable.push(regionId);
  }
  return { retryable, blocked, retryAll: ids.size === 0 && !blocked.length && ['failed', 'unknown', 'cancelled'].includes(job.status) };
}

export function areaProblems(job: GenerationJobView | null, page: Page): AreaProblem[] {
  if (!job || ['prepared', 'generating', 'ready', 'applying'].includes(job.status)) return [];
  return page.regions.flatMap(region => {
    if (!job.areas.some(area => area.pageId === page.id && area.regionId === region.id)) return [];
    const drafts = job.drafts?.filter(draft => draft.pageId === page.id && draft.regionIds.includes(region.id)) ?? [];
    const latest = drafts.at(-1);
    if (latest?.applied || latest?.dismissed) return [];
    const outcome = job.candidate?.regions?.find(item => item.regionId === region.id);
    if (!latest && (!outcome || outcome.status === 'implemented')) return [];
    if (region.completed && (!latest || outcome?.status === 'implemented')) return [];
    const result = areaOutcome(job, region.id, region.prompt);
    const stale = !!latest?.baseContent && JSON.stringify(latest.baseContent) !== JSON.stringify(page.content);
    let reason = latest?.reason || outcome?.explanation || job.message;
    // Resolve a validator's area identity to the user's visible box number.
    const affected = page.regions.findIndex(item => item.id !== region.id && reason.includes(item.id));
    const message = result.retryable
      ? generationError(stale ? 'stale revision' : reason, region.prompt, affected >= 0 ? `Box ${affected + 1}` : undefined)
      : result.reason;
    return [{ key: `${job.jobId}:${region.id}`, regionId: region.id, draftId: latest?.id,
      message, stale, retryable: result.retryable, fix: result.fix, rect: region.rect }];
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
