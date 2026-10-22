import type { Page, Region } from './types';

// Keep the coverage thresholds in sync with Region::auto_whole_page.
export function autoWholePage(region: Region, page: Page): boolean {
  if (region.mobileOnly || region.element || region.elements?.length || region.shape && region.shape.kind !== 'rect') return false;
  const width = Math.max(page.viewport.width, ...(region.areaBounds ?? []).map(bound => bound.rect.x + bound.rect.width));
  const height = region.areaBounds?.length ? Math.max(...region.areaBounds.map(bound => bound.rect.y + bound.rect.height)) : page.viewport.height;
  const overlap = (start: number, length: number, extent: number) => Math.max(0, Math.min(start + length, extent) - Math.max(start, 0));
  const base = page.regions.map((region, index) => ({ region, index })).filter(item => !item.region.completed)
    .sort((a, b) => a.region.zIndex - b.region.zIndex || a.index - b.index)[0]?.region.id === region.id;
  const prompt = ` ${region.prompt.toLowerCase().split(/[^\p{L}\p{N}]+/u).filter(Boolean).join(' ')} `;
  const layout = ['one page one screen', 'one screen only', 'single screen', 'fit the page to', 'fit this page to',
    'fit on one screen', 'fit in one screen', 'page height', 'page layout', 'viewport height'].some(phrase => prompt.includes(` ${phrase} `));
  return overlap(region.rect.x, region.rect.width, width) >= width * 0.95
    && (layout || base && (Math.abs(region.rect.y) <= page.viewport.height * 0.1 && overlap(region.rect.y, region.rect.height, page.viewport.height) >= page.viewport.height * 0.9
      || overlap(region.rect.y, region.rect.height, height) >= height * 0.9));
}

export function isWholePage(region: Region, page: Page): boolean {
  return !!region.wholePage || !region.wholePageDisabled && autoWholePage(region, page);
}
