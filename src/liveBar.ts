// The Live bar drives the page inside the preview iframe through its bridge. The editor
// never navigates that iframe itself, so these helpers describe the small message
// protocol and map a site path back to the project page that shows it.

export type LiveHistoryAction = 'back' | 'forward' | 'reload';

export type LiveBridgeCommand = { type: 'desegnos-preview-command'; sessionId: string; action: LiveHistoryAction };

/// Back, Forward and Reload carry the session they belong to; the bridge ignores a
/// command for any other session.
export function liveCommand(sessionId: string, action: LiveHistoryAction): LiveBridgeCommand {
  return { type: 'desegnos-preview-command', sessionId, action };
}

/// A path reported by a live bridge after load, a hash change or a history move.
export function liveLocation(data: unknown, sessionId: string): string | null {
  if (!data || typeof data !== 'object' || Array.isArray(data)) return null;
  const value = data as Record<string, unknown>;
  if (value.type !== 'desegnos-preview-location' || value.sessionId !== sessionId) return null;
  if (typeof value.path !== 'string' || !value.path.startsWith('/') || value.path.length > 2048) return null;
  return value.path;
}

/// A URL path without its query or fragment. A directory index folds onto its folder, so
/// `/projects/papan/index.html` and `/projects/papan/` name the same site page.
export function sitePath(path: string): string | null {
  const raw = path.split(/[?#]/, 1)[0] ?? '';
  let decoded = raw;
  try { decoded = decodeURIComponent(raw); } catch { /* keep the encoded path */ }
  if (!decoded.startsWith('/')) return null;
  const folder = decoded.replace(/\/index\.html?$/i, '/').replace(/\/+$/, '');
  return folder || '/';
}

/// The project page a live path shows, when the site has one. Only a mapped path offers
/// to leave Live for the editor; every other navigation stays inside the preview.
export function pathPage<T extends { path: string }>(path: string, pages: readonly T[]): T | null {
  const wanted = sitePath(path);
  if (!wanted) return null;
  const key = wanted.toLowerCase();
  const extensionless = key.replace(/\.html?$/, '') || '/';
  return pages.find(page => {
    const route = sitePath(page.path);
    if (!route) return false;
    const candidate = route.toLowerCase();
    return candidate === key || candidate === extensionless;
  }) ?? null;
}
