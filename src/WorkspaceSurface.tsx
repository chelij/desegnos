import { createContext, useCallback, useContext, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import './dialogs.css';

type Place = 'panel' | 'banner' | 'start' | 'canvas';
type Entry = { place: Place; close: () => void; opener: HTMLElement | null };
const Surfaces = createContext<{
  hosts: Partial<Record<Place, HTMLElement>>;
  host: (place: Place, node: HTMLElement | null) => void;
  enter: (entry: Entry) => () => void;
} | null>(null);

export function WorkspaceSurfaces({ children }: { children: ReactNode }) {
  const [hosts, setHosts] = useState<Partial<Record<Place, HTMLElement>>>({});
  const entries = useRef<Entry[]>([]);
  const host = useCallback((place: Place, node: HTMLElement | null) => {
    setHosts(previous => { if (previous[place] === (node ?? undefined)) return previous; return { ...previous, [place]: node ?? undefined }; });
  }, []);
  const enter = useCallback((entry: Entry) => {
    // One view occupies the shared side panel. Banners and canvas previews are independent.
    const previous = entry.place === 'banner' ? undefined : entries.current.find(item => item.place === entry.place);
    if (previous) { if (entry.opener?.closest('.workspace-surface')) entry.opener = previous.opener; previous.close(); }
    entries.current.push(entry);
    return () => {
      const wasTop = entries.current.at(-1) === entry;
      entries.current = entries.current.filter(item => item !== entry);
      const remaining = entries.current.at(-1);
      if (wasTop) queueMicrotask(() => {
        if (entries.current.at(-1) !== remaining) return;
        const target = entry.opener;
        if (target?.isConnected && !target.matches(':disabled')) target.focus({ preventScroll: true });
      });
    };
  }, []);
  useLayoutEffect(() => {
    const escape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || event.defaultPrevented) return;
      // Let open menus and native select controls handle their own Escape first.
      if (document.querySelector('.editor-menu')) return;
      const entry = entries.current.at(-1);
      if (entry) { event.preventDefault(); event.stopImmediatePropagation(); entry.close(); }
    };
    window.addEventListener('keydown', escape, true);
    return () => window.removeEventListener('keydown', escape, true);
  }, []);
  return <Surfaces.Provider value={{ hosts, host, enter }}>{children}</Surfaces.Provider>;
}

export function SurfaceHost({ place }: { place: Place }) {
  const context = useContext(Surfaces)!;
  const ref = useCallback((node: HTMLDivElement | null) => context.host(place, node), [context.host, place]);
  return <div ref={ref} className={`surface-host surface-host-${place}`} />;
}

export function WorkspaceSurface({ title, close, children, place = 'panel', headerActions }: {
  title: string; close: () => void; children: ReactNode; place?: Place; headerActions?: ReactNode;
}) {
  const context = useContext(Surfaces)!;
  const opener = useRef(document.activeElement instanceof HTMLElement ? document.activeElement : null);
  const closeRef = useRef(close); closeRef.current = close;
  const ref = useRef<HTMLElement>(null);
  const target = context.hosts[place];
  useLayoutEffect(() => {
    if (!target) return;
    const leave = context.enter({ place, close: () => closeRef.current(), opener: opener.current });
    // Focus enters the view without trapping keyboard navigation or disabling the canvas.
    if (!ref.current?.contains(document.activeElement)) (ref.current?.querySelector<HTMLElement>('[autofocus], input, textarea, select, button') ?? ref.current)?.focus({ preventScroll: true });
    return leave;
  }, [context.enter, target, place]);
  if (!target) return null;
  return createPortal(<section ref={ref} tabIndex={-1} className={`workspace-surface ${place === 'panel' ? 'generation-log' : `workspace-${place}`}`} aria-label={title} onKeyDown={event => event.stopPropagation()} onPointerDown={event => event.stopPropagation()}>
    <header><strong>{title}</strong>{headerActions}<button type="button" aria-label={`Close ${title}`} title="Close" onClick={close}>×</button></header>
    <div className="surface-content">{children}</div>
  </section>, target);
}
