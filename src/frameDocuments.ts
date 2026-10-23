// Editable documents shown inside a page's iframes.
//
// The static canvas renders a framed document as a same-origin `srcdoc` iframe, so elements
// inside it can be hit-tested and measured. This module holds the plain geometry those
// measurements need: how a point or box inside the frame maps onto the page, which frame a
// page point falls in, and how a request box on the page maps into the framed document.
// Everything here is pure; Preview.tsx reads live DOM values and passes them in.
import type { DocumentTarget, ElementTarget, Rect } from './types';

export type FrameViewport = { width: number; height: number };

/// Where the frame's content box sits on the page. `frame` is the iframe's border box in
/// page CSS pixels, already carrying any CSS transform. `scale` and `inset` convert inner
/// coordinates, which the browser reports in the frame's own layout pixels.
export type FrameMapping = {
  frame: Rect;
  scaleX: number;
  scaleY: number;
  insetX: number;
  insetY: number;
  scrollX: number;
  scrollY: number;
};

export type FrameBoxGeometry = { mapping: FrameMapping; viewport: FrameViewport };

/// A framed document the canvas can edit, in page coordinates.
export type EditableFrame = FrameBoxGeometry & {
  iframe: ElementTarget;
  selector: string;
  document: DocumentTarget;
  sourceSha256: string;
  /// The iframe's content box on the page, for deciding whether a request belongs here.
  contentRect: Rect;
};

export type FramedRequest = { frame: EditableFrame; rect: Rect };

export function frameMapping(input: {
  frame: Rect;
  layoutWidth: number;
  layoutHeight: number;
  borderLeft?: number;
  borderTop?: number;
  paddingLeft?: number;
  paddingTop?: number;
  scrollX?: number;
  scrollY?: number;
}): FrameMapping {
  // A transformed iframe reports its transformed box while layout sizes stay untransformed.
  const scaleX = input.layoutWidth > 0 && Number.isFinite(input.frame.width / input.layoutWidth) ? input.frame.width / input.layoutWidth : 1;
  const scaleY = input.layoutHeight > 0 && Number.isFinite(input.frame.height / input.layoutHeight) ? input.frame.height / input.layoutHeight : 1;
  return {
    frame: input.frame,
    scaleX: scaleX || 1,
    scaleY: scaleY || 1,
    insetX: (input.borderLeft ?? 0) + (input.paddingLeft ?? 0),
    insetY: (input.borderTop ?? 0) + (input.paddingTop ?? 0),
    scrollX: input.scrollX ?? 0,
    scrollY: input.scrollY ?? 0,
  };
}

/// A point in the frame's visible content, as `elementFromPoint` and `getBoundingClientRect`
/// report it, to page coordinates.
export function innerViewportToPage(mapping: FrameMapping, point: { x: number; y: number }): { x: number; y: number } {
  return { x: mapping.frame.x + (mapping.insetX + point.x) * mapping.scaleX, y: mapping.frame.y + (mapping.insetY + point.y) * mapping.scaleY };
}

export function pageToInnerViewport(mapping: FrameMapping, point: { x: number; y: number }): { x: number; y: number } {
  return { x: (point.x - mapping.frame.x) / mapping.scaleX - mapping.insetX, y: (point.y - mapping.frame.y) / mapping.scaleY - mapping.insetY };
}

/// A point in the framed document, including its own scroll offset (which is what a stored
/// request box uses), to page coordinates.
export function innerDocumentToPage(mapping: FrameMapping, point: { x: number; y: number }): { x: number; y: number } {
  return innerViewportToPage(mapping, { x: point.x - mapping.scrollX, y: point.y - mapping.scrollY });
}

export function pageToInnerDocument(mapping: FrameMapping, point: { x: number; y: number }): { x: number; y: number } {
  const inner = pageToInnerViewport(mapping, point);
  return { x: inner.x + mapping.scrollX, y: inner.y + mapping.scrollY };
}

/// A measured inner box, in page coordinates.
/// A point inside the frame's document (scroll included) to canvas pixels. The canvas offsets
/// and scales the whole page, so this composes the frame mapping with its view transform.
export function innerToCanvas(point: { x: number; y: number }, mapping: FrameMapping, view: { origin: { x: number; y: number }; zoom: number }): { x: number; y: number } {
  const page = innerDocumentToPage(mapping, point);
  return { x: (page.x - view.origin.x) * view.zoom, y: (page.y - view.origin.y) * view.zoom };
}

export function canvasToInner(point: { x: number; y: number }, mapping: FrameMapping, view: { origin: { x: number; y: number }; zoom: number }): { x: number; y: number } {
  return pageToInnerDocument(mapping, { x: point.x / view.zoom + view.origin.x, y: point.y / view.zoom + view.origin.y });
}

export function innerRectToPage(mapping: FrameMapping, rect: Rect): Rect {
  const at = innerViewportToPage(mapping, rect);
  return { x: at.x, y: at.y, width: rect.width * mapping.scaleX, height: rect.height * mapping.scaleY };
}

export function pageRectToInnerViewport(mapping: FrameMapping, rect: Rect): Rect {
  const at = pageToInnerViewport(mapping, rect);
  return { x: at.x, y: at.y, width: rect.width / mapping.scaleX, height: rect.height / mapping.scaleY };
}

export function pageRectToInnerDocument(mapping: FrameMapping, rect: Rect): Rect {
  const at = pageToInnerDocument(mapping, rect);
  return { x: at.x, y: at.y, width: rect.width / mapping.scaleX, height: rect.height / mapping.scaleY };
}

/// The frame's visible content box on the page. A point inside it belongs to the frame,
/// not to the iframe element drawn behind it.
export function frameContentRect(frame: FrameBoxGeometry): Rect {
  return innerRectToPage(frame.mapping, { x: 0, y: 0, width: frame.viewport.width, height: frame.viewport.height });
}

function inside(inner: { x: number; y: number }, viewport: FrameViewport): boolean {
  return inner.x >= 0 && inner.y >= 0 && inner.x <= viewport.width && inner.y <= viewport.height;
}

/// Which frame a page point falls in, with the point in that frame's content. Later frames
/// paint on top, so they win.
export function frameAt(frames: readonly FrameBoxGeometry[], point: { x: number; y: number }): { index: number; point: { x: number; y: number } } | null {
  for (let index = frames.length - 1; index >= 0; index--) {
    const frame = frames[index];
    const inner = pageToInnerViewport(frame.mapping, point);
    if (inside(inner, frame.viewport)) return { index, point: inner };
  }
  return null;
}

/// The frame a request box belongs to: the frame under the box's centre. A box that only
/// clips a frame corner stays a page request.
export function requestFrame(frames: readonly EditableFrame[], rect: Rect): FramedRequest | null {
  const hit = frameAt(frames, { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 });
  if (!hit) return null;
  const frame = frames[hit.index];
  return { frame, rect: pageRectToInnerDocument(frame.mapping, rect) };
}

/// A request box that already belongs to a frame, re-mapped after it moves on the page.
/// Returns null once the box's centre leaves the frame.
export function remapFramedRequest(frames: readonly EditableFrame[], frame: { document: DocumentTarget; iframeId: string }, rect: Rect): Rect | null {
  const match = frames.find(item => item.iframe.id === frame.iframeId && sameFrameDocument(item.document, frame.document));
  if (!match) return null;
  const hit = frameAt([match], { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 });
  return hit ? pageRectToInnerDocument(match.mapping, rect) : null;
}

function sameFrameDocument(a: DocumentTarget | undefined, b: DocumentTarget | undefined): boolean {
  if (!a || !b) return !a && !b;
  return a.kind === b.kind && (a.kind === 'page' ? b.kind === 'page' && a.pageId === b.pageId : b.kind === 'siteFile' && a.path === b.path);
}

/// The session URL of a framed document, used by the live overlay when Interact plays the
/// frame. Imported websites serve site files at their own path; generated pages are served
/// under their page ID in the session directory.
export function frameDocumentSource(descriptor: { website?: boolean; url: string; sessionId: string } | null, pageId: string | null, path?: string | null): string | null {
  if (!descriptor) return null;
  try {
    const url = new URL(descriptor.url);
    if (descriptor.website && path) return `${url.protocol}//${url.host}/${path.replace(/^\/+/, '')}`;
    if (!descriptor.website && pageId) return `${url.protocol}//${url.host}${url.pathname.replace(/\/[^/]+\/[^/]*$/, '/')}${pageId}/index.html`;
  } catch { return null; }
  return null;
}

/// The base a framed document resolves its relative resources against. Imported websites
/// keep their original site paths; generated pages keep the app's document, like their host.
export function frameDocumentBase(descriptor: { website?: boolean; url: string } | null, path?: string | null): string | null {
  if (!descriptor?.website || !path) return null;
  try {
    const url = new URL(descriptor.url);
    return `${url.protocol}//${url.host}/${path.replace(/^\/+/, '')}`;
  } catch { return null; }
}
