import { useEffect, useRef, useState, type CSSProperties } from 'react';
import { native } from './api';
import { liveFrameSource, type CollectedFrame } from './previewFrames';
import type { PreviewDescriptor } from './types';

export type LiveFrameSession = { sessionId: string; url: string };

/// An explicitly interacting frame gets a live session. The canvas removes its
/// temporary capture runner before editing, and keeps no passive live overlays.
export function useLiveFrameSession(pageId: string, revision: string, enabled: boolean): LiveFrameSession | null {
  const [session, setSession] = useState<LiveFrameSession | null>(null);
  useEffect(() => {
    if (!enabled) { setSession(null); return; }
    let active = true;
    let created: string | null = null;
    setSession(null);
    void native<PreviewDescriptor>('create_preview', { pageId, runScripts: true }).then(value => {
      if (!active) { void native('release_preview', { sessionId: value.sessionId }).catch(() => {}); return; }
      created = value.sessionId;
      setSession({ sessionId: value.sessionId, url: value.url });
    }).catch(() => { if (active) setSession(null); });
    return () => { active = false; if (created) void native('release_preview', { sessionId: created }).catch(() => {}); };
  }, [pageId, revision, enabled]);
  return session;
}

/// A live frame sitting exactly over its still copy. `interactive` is the only state in
/// which it takes pointer or keyboard input; otherwise it renders without ever capturing
/// a click, so the Select tool still selects the frame element.
export function FrameOverlay({ frame, live, pageId, staticPageUrl, zoom, interactive, placement, title, stop }: {
  frame: CollectedFrame; live: LiveFrameSession; pageId: string; staticPageUrl: string | null; zoom: number;
  interactive: boolean; placement: CSSProperties; title: string; stop: () => void;
}) {
  const wrapper = useRef<HTMLDivElement>(null);
  const frameNode = useRef<HTMLIFrameElement>(null);
  const stopRef = useRef(stop); stopRef.current = stop;
  useEffect(() => { if (interactive) wrapper.current?.focus({ preventScroll: true }); }, [interactive]);
  // The live session bridge reports Escape from inside the frame, because key events in a
  // cross-origin document never reach the editor. Only this overlay's own window counts.
  useEffect(() => {
    if (!interactive) return;
    const receive = (event: MessageEvent) => {
      if (event.source !== frameNode.current?.contentWindow || !event.data || typeof event.data !== 'object' || Array.isArray(event.data)) return;
      const value = event.data as Record<string, unknown>;
      if (value.type === 'desegnos-preview-escape' && value.sessionId === live.sessionId && value.pageId === pageId) stopRef.current();
    };
    window.addEventListener('message', receive);
    return () => window.removeEventListener('message', receive);
  }, [interactive, live.sessionId, pageId]);
  // Pointer transparency alone does not freeze scripts. An inactive overlay must
  // have no iframe at all, even if a caller still holds a live session.
  if (!interactive) return null;
  return <div ref={wrapper} className={`frame-overlay${interactive ? ' interactive' : ''}`} style={placement}
    tabIndex={interactive ? 0 : -1} aria-label={interactive ? 'Interactive frame. Press Escape to stop interacting.' : undefined} role={interactive ? 'group' : undefined}
    onKeyDown={event => {
      // Editor shortcuts live on the canvas and are only reached from its own keyboard
      // focus. Escape is the one key this wrapper forwards to leave the frame.
      event.stopPropagation();
      if (event.key === 'Escape') { event.preventDefault(); stop(); }
    }}>
    <iframe ref={frameNode} src={liveFrameSource(frame.src, staticPageUrl, live.url)} title={title} sandbox={frame.sandbox}
      allow={frame.allow ?? undefined} allowFullScreen={frame.allowFullscreen} referrerPolicy="no-referrer" tabIndex={interactive ? undefined : -1}
      style={{ width: frame.rect.width, height: frame.rect.height, transform: `scale(${zoom})`, transformOrigin: 'top left' }} />
  </div>;
}
