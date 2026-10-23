// Live overlays for frames inside the static canvas.
//
// The static canvas document never runs scripts and the sandbox it inherits cannot be
// widened, so a game or embed inside it stays still. These helpers describe each framed
// element the canvas shows and place a live copy of it over the still one. Everything
// here is pure: Preview.tsx reads the static document and passes plain values in, and
// Canvas.tsx positions the overlays.

export type FrameBox = { x: number; y: number; width: number; height: number };
export type FrameOrigin = { x: number; y: number };

/// What the static document reports about one <iframe>.
export type FrameCandidate = {
  rect: FrameBox;
  display: string;
  visibility: string;
  src: string | null;
  sandbox: string | null;
  allow: string | null;
  allowFullscreen: boolean;
};

/// One frame the canvas can show live. `src` is resolved against the page's session URL,
/// so a relative frame points at the file the live session will serve.
export type CollectedFrame = {
  key: string;
  rect: FrameBox;
  src: string;
  sandbox: string;
  allow: string | null;
  allowFullscreen: boolean;
  selector: string | null;
};

export type FramePlacement = { left: number; top: number; width: number; height: number };

const ALLOWED_PROTOCOLS = new Set(['desegni-preview:', 'http:', 'https:']);

/// A frame's identity is its document order plus the file it loads. Both stay the same
/// across the two measurements of one promoted document, so React keeps one overlay.
export function frameKey(index: number, src: string | null): string {
  return `${index}:${src ?? ''}`;
}

/// The document may sit above the page's session (framed site files) or be an external
/// https embed. Anything else is not a frame the live preview can serve.
export function resolveFrameSource(src: string | null, pageUrl: string | null): string | null {
  const value = src?.trim(); if (!value) return null;
  let url: URL;
  try { url = pageUrl ? new URL(value, pageUrl) : new URL(value); } catch { return null; }
  if (!ALLOWED_PROTOCOLS.has(url.protocol)) return null;
  return url.href;
}

export function frameVisible(candidate: Pick<FrameCandidate, 'rect' | 'display' | 'visibility'>): boolean {
  return candidate.display !== 'none' && candidate.visibility !== 'hidden' && candidate.visibility !== 'collapse' &&
    candidate.rect.width >= 1 && candidate.rect.height >= 1 &&
    [candidate.rect.x, candidate.rect.y, candidate.rect.width, candidate.rect.height].every(Number.isFinite);
}

/// The live copy is sandboxed without same-origin, exactly like the live page preview, so
/// it cannot reach the editor. The author's narrowing is kept, minus the tokens that only
/// make sense in a frame that is itself inside a sandboxed page: same-origin (which would
/// end the opaque origin) and top navigation (which would let the frame replace the app).
export function overlaySandbox(author: string | null): string {
  const tokens = (author ?? '').toLowerCase().split(/\s+/).filter(Boolean)
    .filter(token => token !== 'allow-scripts' && token !== 'allow-same-origin' && token !== 'allow-popups-to-escape-sandbox' && !token.startsWith('allow-top-navigation'));
  return ['allow-scripts', ...new Set(tokens)].join(' ');
}

export function collectFrame(candidate: FrameCandidate, pageUrl: string | null, index: number): CollectedFrame | null {
  if (!frameVisible(candidate)) return null;
  const src = resolveFrameSource(candidate.src, pageUrl); if (!src) return null;
  return {
    key: frameKey(index, src),
    rect: { ...candidate.rect },
    src,
    sandbox: overlaySandbox(candidate.sandbox),
    allow: candidate.allow,
    allowFullscreen: candidate.allowFullscreen,
    selector: null,
  };
}

/// A frame the static page loaded from the page session belongs to the same site in the
/// live session. The frame's path relative to the page is applied to the live page's
/// directory, which covers both URL forms: a session in the host (an imported site) and a
/// session in the path (a generated page). External https frames stay as they are.
export function liveFrameSource(src: string, staticPageUrl: string | null, livePageUrl: string): string {
  let frame: URL; let live: URL; let staticPage: URL;
  try { frame = new URL(src); live = new URL(livePageUrl); staticPage = new URL(staticPageUrl ?? ''); } catch { return src; }
  if (frame.protocol !== staticPage.protocol || frame.host !== staticPage.host) return src;
  const directory = live.pathname.slice(0, live.pathname.lastIndexOf('/') + 1);
  try { return new URL(relativePath(staticPage.pathname, frame.pathname) + frame.search + frame.hash, `${live.protocol}//${live.host}${directory}`).href; }
  catch { return src; }
}

/// The reference that reaches `target` from the directory holding `page`.
function relativePath(page: string, target: string): string {
  const directory = page.slice(0, page.lastIndexOf('/') + 1).split('/').filter(Boolean);
  const parts = target.split('/').filter(Boolean);
  let common = 0;
  while (common < directory.length && common < parts.length && directory[common] === parts[common]) common++;
  const rest = [...Array(directory.length - common).fill('..'), ...parts.slice(common)].join('/');
  return rest ? `${rest}${target.endsWith('/') ? '/' : ''}` : './';
}

export function intersectsFrame(rect: FrameBox, viewport: FrameBox): boolean {
  return rect.x < viewport.x + viewport.width && rect.x + rect.width > viewport.x &&
    rect.y < viewport.y + viewport.height && rect.y + rect.height > viewport.y;
}

/// Only frames on screen get an overlay. Creation stays lazy: a page with many frames
/// pays for at most `limit` of them, and an overlay is removed with the frame it follows.
export function placeOverlays(frames: readonly CollectedFrame[], viewport: FrameBox, limit = 8): CollectedFrame[] {
  return frames.filter(frame => intersectsFrame(frame.rect, viewport)).slice(0, Math.max(0, limit));
}

/// Position of the overlay's outer box in canvas pixels. The canvas is scaled by `zoom`
/// around the page origin, so a box is offset and scaled the same way as the still page.
/// The live document itself is laid out at its page size and scaled inside this box.
export function overlayPlacement(rect: FrameBox, origin: FrameOrigin, zoom: number): FramePlacement {
  return { left: (rect.x - origin.x) * zoom, top: (rect.y - origin.y) * zoom, width: rect.width * zoom, height: rect.height * zoom };
}
