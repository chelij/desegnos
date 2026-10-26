// Frame adoption decides which request boxes belong to the document inside an iframe.
// An explicit whole-page request belongs to the host page instead: adopting a frame would put
// it back inside the framed document and clear the choice, so it is left alone.
import type { Region } from './types';

export function adoptsFrameTarget(region: Region): boolean {
  return !region.completed && !region.wholePage;
}
