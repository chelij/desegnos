import { useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type PointerEvent as ReactPointerEvent } from 'react';
import { native } from './api';
import { MIN_ZOOM, steppedZoom, wheelZoom, zoomLabel } from './canvasGeometry';
import { boundaryHeight, cachedSnapshotState, fitCamera, layoutBoundaries, MAX_ZOOM, panCamera, planSnapshots, rememberSnapshot, sameBoundaries, snapshotContentKey, unionBounds, updateSnapshotHeight, zoomCameraAt, type Boundary, type CachedSnapshot, type CanvasCamera, type Point, type SnapshotCapture } from './projectCanvasGeometry';
import { capturePreview, freezeSnapshot } from './frozenPreview';
import { FloatingBar } from './FloatingBar';
import { Icon } from './icons';
import { isTextField } from './WorkspaceSurface';
import type { PreviewDescriptor, Project } from './types';

type Snapshot = { html: string };
type Drag = { pointerId: number; start: Point; camera: CanvasCamera; moved: boolean; pageId: string | null };

// Survives opening a page and coming back, and revision bumps that leave page content alone.
const snapshotCache: Map<string, CachedSnapshot> = new Map();
const cachedState = (project: Project) => cachedSnapshotState(snapshotCache, project.id, project.pages);

// Every page of the project on one pan/zoom surface, drawn from a frozen snapshot.
export function ProjectCanvas({ project, openPage }: { project: Project; openPage: (pageId: string) => void }) {
  const surface = useRef<HTMLDivElement>(null);
  const sessions = useRef(new Set<string>());
  const drag = useRef<Drag | null>(null);
  const space = useRef(false);
  const touched = useRef(false);
  const [snapshots, setSnapshots] = useState<Record<string, Snapshot>>(() => cachedState(project).snapshots);
  const [heights, setHeights] = useState<Record<string, number>>(() => cachedState(project).heights);
  const [boundaries, setBoundaries] = useState<Boundary[]>([]);
  const [camera, setCamera] = useState<CanvasCamera>({ x: 0, y: 0, zoom: 1 });
  const [selected, setSelected] = useState<string | null>(null);
  const [panning, setPanning] = useState(false);
  const [spaceHeld, setSpaceHeld] = useState(false);

  function release(sessionId: string) {
    if (!sessions.current.delete(sessionId)) return;
    void native('release_preview', { sessionId }).catch(() => {});
  }

  const pageKey = project.pages.map(page => `${page.id}:${page.viewport.width}`).join('|');
  const contentKeys = project.pages.map(page => `${page.id}:${snapshotContentKey(page.content, page.viewport)}`).join('|');
  useEffect(() => {
    const laid = layoutBoundaries(project.pages.map(page => ({ id: page.id, width: page.viewport.width, height: heights[page.id] ?? page.viewport.height })), project.homePageId);
    setBoundaries(current => sameBoundaries(current, laid) ? current : laid);
  }, [pageKey, heights, project.homePageId]);

  useEffect(() => {
    touched.current = false; setSelected(null);
    const cached = cachedState(project);
    setSnapshots(cached.snapshots); setHeights(cached.heights);
  }, [project.id]);

  // Show cached snapshots at once; capture only pages whose own content or viewport changed.
  useEffect(() => {
    const plan = planSnapshots(snapshotCache, project.id, project.pages);
    setSnapshots(plan.snapshots);
    setHeights(current => ({ ...current, ...plan.heights }));
    if (!plan.pending.length) return;
    const controller = new AbortController();
    const queue = [...plan.pending];
    let active = 0;
    async function capture({ page, key }: SnapshotCapture) {
      // Each page gets its own viewport-sized host, so vw/vh and responsive layout match its boundary.
      const element = document.createElement('div');
      element.style.cssText = `position:absolute;left:-20000px;top:0;width:${page.viewport.width}px;height:${page.viewport.height}px;visibility:hidden;pointer-events:none;overflow:hidden`;
      element.setAttribute('aria-hidden', 'true');
      document.body.append(element);
      let sessionId: string | null = null;
      try {
        const descriptor = await native<PreviewDescriptor>('create_preview', { pageId: page.id, runScripts: true });
        sessionId = descriptor.sessionId;
        sessions.current.add(sessionId);
        if (controller.signal.aborted) return;
        const frozen = freezeSnapshot(await capturePreview(descriptor.url, descriptor.sessionId, element, controller.signal));
        if (controller.signal.aborted) return;
        rememberSnapshot(snapshotCache, project.id, page.id, { key, html: frozen, height: 0 });
        setSnapshots(current => ({ ...current, [page.id]: { html: frozen } }));
      } catch { /* A page that cannot render keeps its cached snapshot or an empty boundary. */ }
      finally { element.remove(); if (sessionId) release(sessionId); }
    }
    function next() {
      while (!controller.signal.aborted && active < 2 && queue.length) {
        const item = queue.shift()!;
        active++;
        void capture(item).finally(() => { active--; next(); });
      }
    }
    next();
    return () => controller.abort();
  }, [project.id, contentKeys]);

  useEffect(() => () => { for (const sessionId of sessions.current) release(sessionId); sessions.current.clear(); }, []);

  // Fit until the first pan or zoom, so measured snapshot heights still settle the view.
  useEffect(() => {
    const node = surface.current;
    if (touched.current || !node || !boundaries.length) return;
    const box = node.getBoundingClientRect();
    if (box.width < 1 || box.height < 1) return;
    setCamera(fitCamera(unionBounds(boundaries.map(item => item.rect)), { width: box.width, height: box.height }));
  }, [boundaries, project.id]);

  useEffect(() => {
    const node = surface.current;
    if (!node) return;
    const wheel = (event: WheelEvent) => {
      const box = node.getBoundingClientRect();
      const point = { x: event.clientX - box.left, y: event.clientY - box.top };
      event.preventDefault();
      touched.current = true;
      setCamera(current => event.ctrlKey || event.metaKey ? zoomCameraAt(current, point, wheelZoom(current.zoom, event.deltaY)) : panCamera(current, { x: -event.deltaX, y: -event.deltaY }));
    };
    node.addEventListener('wheel', wheel, { passive: false });
    return () => node.removeEventListener('wheel', wheel);
  }, []);

  useEffect(() => {
    const down = (event: KeyboardEvent) => {
      if (event.code !== 'Space' || event.repeat || isTextField(event.target)) return;
      event.preventDefault();
      space.current = true; setSpaceHeld(true);
    };
    const up = (event: KeyboardEvent) => { if (event.code !== 'Space') return; space.current = false; setSpaceHeld(false); };
    const blur = () => { space.current = false; setSpaceHeld(false); };
    window.addEventListener('keydown', down);
    window.addEventListener('keyup', up);
    window.addEventListener('blur', blur);
    return () => { window.removeEventListener('keydown', down); window.removeEventListener('keyup', up); window.removeEventListener('blur', blur); };
  }, []);

  function fit() {
    const node = surface.current;
    if (!node || !boundaries.length) return;
    const box = node.getBoundingClientRect();
    touched.current = false;
    setCamera(fitCamera(unionBounds(boundaries.map(item => item.rect)), { width: box.width, height: box.height }));
  }

  function zoomStep(direction: number) {
    const node = surface.current;
    if (!node) return;
    const box = node.getBoundingClientRect();
    touched.current = true;
    setCamera(current => zoomCameraAt(current, { x: box.width / 2, y: box.height / 2 }, steppedZoom(current.zoom, direction)));
  }

  function pointerDown(event: ReactPointerEvent<HTMLDivElement>) {
    const pageId = (event.target as Element).closest<HTMLElement>('[data-boundary]')?.dataset.boundary ?? null;
    const pans = event.button === 1 || (event.button === 0 && (space.current || !pageId));
    if (!pans) {
      if (pageId && event.button === 0) setSelected(pageId);
      return;
    }
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { pointerId: event.pointerId, start: { x: event.clientX, y: event.clientY }, camera, moved: false, pageId };
    setPanning(true);
  }

  function pointerMove(event: ReactPointerEvent<HTMLDivElement>) {
    const active = drag.current;
    if (!active || active.pointerId !== event.pointerId) return;
    const delta = { x: event.clientX - active.start.x, y: event.clientY - active.start.y };
    if (!active.moved && Math.abs(delta.x) < 2 && Math.abs(delta.y) < 2) return;
    active.moved = true;
    touched.current = true;
    setCamera(panCamera(active.camera, delta));
  }

  function pointerUp(event: ReactPointerEvent<HTMLDivElement>) {
    const active = drag.current;
    if (!active || active.pointerId !== event.pointerId) return;
    drag.current = null;
    setPanning(false);
    if (!active.moved && !active.pageId && event.button === 0) setSelected(null);
  }

  function keyDown(event: ReactKeyboardEvent<HTMLDivElement>) {
    if (event.key === 'Enter' && selected) { event.preventDefault(); openPage(selected); }
    else if (event.key === 'Escape' && selected) { event.preventDefault(); setSelected(null); }
  }

  function measure(pageId: string, node: HTMLIFrameElement) {
    const doc = node.contentDocument;
    if (!doc) return;
    const height = boundaryHeight(Math.max(doc.body?.scrollHeight ?? 0, doc.documentElement?.scrollHeight ?? 0));
    updateSnapshotHeight(snapshotCache, project.id, pageId, height);
    setHeights(current => current[pageId] === height ? current : { ...current, [pageId]: height });
  }

  return <div ref={surface} className={`project-canvas${panning ? ' panning' : ''}${spaceHeld ? ' pan-ready' : ''}`} tabIndex={0}
    onPointerDown={pointerDown} onPointerMove={pointerMove} onPointerUp={pointerUp} onPointerCancel={pointerUp} onKeyDown={keyDown}>
    <div className="project-canvas-layer" style={{ transform: `translate(${camera.x}px, ${camera.y}px) scale(${camera.zoom})` }}>
      {boundaries.map(({ pageId, rect }) => {
        const page = project.pages.find(item => item.id === pageId);
        const snapshot = snapshots[pageId];
        if (!page) return null;
        return <div key={pageId} className={`page-boundary${selected === pageId ? ' selected' : ''}`} data-boundary={pageId}
          style={{ left: rect.x, top: rect.y, width: rect.width, height: rect.height }} onDoubleClick={() => openPage(pageId)}>
          <span className="page-boundary-name">{page.name}</span>
          {snapshot && <iframe title={`${page.name} snapshot`} srcDoc={snapshot.html} sandbox="allow-same-origin" tabIndex={-1} aria-hidden="true"
            style={{ width: rect.width, height: rect.height }} onLoad={event => measure(pageId, event.currentTarget)} />}
        </div>;
      })}
    </div>
    <FloatingBar className="canvas-tools" role="toolbar" aria-label="Project canvas">
      <button type="button" className="icon-button sm" aria-label="Zoom out" disabled={camera.zoom <= MIN_ZOOM} onClick={() => zoomStep(-1)}><Icon name="minus" /></button>
      <span className="floating-bar-label">{zoomLabel(camera.zoom)}</span>
      <button type="button" className="icon-button sm" aria-label="Zoom in" disabled={camera.zoom >= MAX_ZOOM} onClick={() => zoomStep(1)}><Icon name="plus" /></button>
      <button type="button" title="Fit all pages" onClick={fit}>Fit</button>
    </FloatingBar>
  </div>;
}
