import type { Page, Region } from './types';

// Keep the coverage thresholds in sync with Region::auto_whole_page.
export function autoWholePage(region: Region, page: Page): boolean {
  if (region.mobileOnly || region.element || region.elements?.length || region.shape && region.shape.kind !== 'rect') return false;
  const width = Math.max(page.viewport.width, ...(region.areaBounds ?? []).map(bound => bound.rect.x + bound.rect.width));
  const height = region.areaBounds?.length ? Math.max(...region.areaBounds.map(bound => bound.rect.y + bound.rect.height)) : page.viewport.height;
  const overlap = (start: number, length: number, extent: number) => Math.max(0, Math.min(start + length, extent) - Math.max(start, 0));
  return overlap(region.rect.x, region.rect.width, width) >= width * 0.95
    && (Math.abs(region.rect.y) <= page.viewport.height * 0.1 && overlap(region.rect.y, region.rect.height, page.viewport.height) >= page.viewport.height * 0.9
      || overlap(region.rect.y, region.rect.height, height) >= height * 0.9);
}

export function isWholePage(region: Region, page: Page): boolean {
  return !!region.wholePage || !region.wholePageDisabled && autoWholePage(region, page);
}
