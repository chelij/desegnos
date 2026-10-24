// Live view shows the real, interactive page instead of the static copy. These helpers decide
// the size of that page and what the editing canvas still paints around it, so the canvas and
// its tests share one description of the live frame.

import type { Viewport } from './types';

export type LiveDevice = 'desktop' | 'tablet' | 'phone';
export type LiveFrame = { width: number; height: number };

/// Device viewports in CSS pixels. Desktop follows the page instead.
export const LIVE_DEVICE_SIZES: Record<Exclude<LiveDevice, 'desktop'>, LiveFrame> = {
  tablet: { width: 768, height: 1024 },
  phone: { width: 390, height: 844 },
};

/// What the canvas paints besides the live page. Both the static editing page and its screen
/// guide are hidden while Live is on, so no second layout or white filler shows through. Requests
/// are stored against the page width, so they are only drawn when the live frame is the page.
export type LiveVisibility = { staticPage: boolean; screenGuide: boolean; requests: boolean };

export function liveFrame(device: LiveDevice, page: Viewport, scrollHeight: number): LiveFrame {
  if (device === 'desktop') return { width: page.width, height: Math.max(page.height, Math.ceil(Number.isFinite(scrollHeight) ? scrollHeight : 0)) };
  return { ...LIVE_DEVICE_SIZES[device] };
}

export function liveVisibility(frame: LiveFrame | null, pageWidth: number): LiveVisibility {
  if (!frame) return { staticPage: true, screenGuide: true, requests: true };
  return { staticPage: false, screenGuide: false, requests: frame.width === pageWidth };
}
