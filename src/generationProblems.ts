import type { GenerationJobView, Page, Rect } from './types';
import { generationError } from './userErrors.js';
export type AreaProblem = { key: string; regionId: string; draftId?: string; message: string; stale: boolean; rect: Rect };
export function areaProblems(job: GenerationJobView | null, page: Page): AreaProblem[] {
  if (!job || ['prepared', 'generating', 'ready', 'applying'].includes(job.status)) return [];
  return page.regions.flatMap(region => {
    if (!job.areas.some(area => area.pageId === page.id && area.regionId === region.id)) return [];
    const drafts = job.drafts?.filter(draft => draft.pageId === page.id && draft.regionIds.includes(region.id)) ?? [];
    const latest = drafts.at(-1);
    if (latest?.applied || latest?.dismissed) return [];
    const outcome = job.candidate?.regions.find(item => item.regionId === region.id);
    if (!latest && (!outcome || outcome.status === 'implemented')) return [];
    if (region.completed && (!latest || outcome?.status === 'implemented')) return [];
    const stale = !!latest?.baseContent && JSON.stringify(latest.baseContent) !== JSON.stringify(page.content);
    let reason = latest?.reason || outcome?.explanation || job.message;
    // Resolve a validator's area identity to the user's visible box number.
    const affected = page.regions.findIndex(item => item.id !== region.id && reason.includes(item.id));
    return [{ key: `${job.jobId}:${region.id}`, regionId: region.id, draftId: latest?.id,
      message: generationError(stale ? 'stale revision' : reason, region.prompt, affected >= 0 ? `Box ${affected + 1}` : undefined), stale, rect: region.rect }];
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
