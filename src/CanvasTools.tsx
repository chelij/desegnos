import { useEffect, useRef, useState } from 'react';
import { LayerIcon } from './Layers';
import { MenuButton } from './Menus';
import { steppedZoom, zoomLabel, ZOOM_LEVELS, MIN_ZOOM } from './canvasGeometry';
import type { Project } from './types';

export type CanvasTool = 'select' | 'request' | 'pan';
export function CanvasTools({ tool, setTool, zoom, setZoom, fit, layersOpen, toggleLayers }: {
  tool: CanvasTool; setTool: (tool: CanvasTool) => void; zoom: number; setZoom: (value: number) => void; fit: () => void; layersOpen: boolean; toggleLayers: () => void;
}) {
  return <div className="canvas-tools" role="toolbar" aria-label="Canvas tools">
    {([
      ['select', 'Select', 'V', 'M4 2 14 10 9 11 7 16Z'],
      ['request', 'Add request', 'R', 'M2 4H14V14H2ZM8 1V7M5 4H11'],
      ['pan', 'Pan', 'H', 'M5 9V3Q5 1 7 3V8 2Q7 0 9 2V8 3Q9 1 11 3V9 5Q11 3 13 5V12L10 16H5L1 10Q1 8 3 9L5 11Z'],
    ] as const).map(([value, label, shortcut, path]) => <button key={value} type="button" aria-pressed={tool === value} title={`${label} · ${shortcut}${value === 'request' ? ' · Click or drag to place' : ''}`} onClick={() => setTool(value)}>
      <svg viewBox="0 0 18 18" aria-hidden="true"><path d={path} /></svg><span>{label}</span>
    </button>)}
    <button type="button" data-layers-trigger aria-label="Layers" title="Layers" aria-haspopup="dialog" aria-expanded={layersOpen} onClick={toggleLayers}><LayerIcon /><span>Layers</span></button>
    <span className="tool-divider" aria-hidden="true" />
    <button type="button" aria-label="Zoom out" title="Zoom out · Ctrl/Cmd+−" disabled={zoom <= MIN_ZOOM} onClick={() => setZoom(steppedZoom(zoom, -1))}>−</button>
    <MenuButton label={zoomLabel(zoom)} ariaLabel="Canvas zoom" items={ZOOM_LEVELS.map(value => ({ id: `zoom-${value}`, label: zoomLabel(value), checked: zoom === value, action: () => setZoom(value) }))} />
    <button type="button" aria-label="Zoom in" title="Zoom in · Ctrl/Cmd++" disabled={zoom === ZOOM_LEVELS.at(-1)} onClick={() => setZoom(steppedZoom(zoom, 1))}>+</button>
    <button type="button" title="Fit screen guide · Ctrl/Cmd+0" onClick={fit}>Fit</button>
  </div>;
}

export function RequestOverview({ project, choose }: { project: Project; choose: (pageId: string, regionId: string | null) => void }) {
  const container = useRef<HTMLDetailsElement>(null);
  const [completed, setCompleted] = useState(false);
  useEffect(() => {
    const outside = (event: PointerEvent) => { if (event.target instanceof Node && !container.current?.contains(event.target)) container.current?.removeAttribute('open'); };
    document.addEventListener('pointerdown', outside, true); return () => document.removeEventListener('pointerdown', outside, true);
  }, []);
  const pendingCount = project.pages.reduce((count, page) => count + page.regions.filter(region => !region.completed).length, 0);
  const doneCount = project.pages.reduce((count, page) => count + page.regions.filter(region => region.completed).length, 0);
  function visit(pageId: string, regionId: string | null) { container.current?.removeAttribute('open'); choose(pageId, regionId); }
  return <details ref={container} className="request-overview" onKeyDown={event => { if (event.key === 'Escape') { container.current?.removeAttribute('open'); container.current?.querySelector('summary')?.focus(); } }}>
    <summary title="Find requests on every page">Requests ({pendingCount})</summary>
    <section className="request-popover" aria-label="Project requests">
      <div className="segmented"><button type="button" aria-pressed={!completed} onClick={() => setCompleted(false)}>Pending ({pendingCount})</button><button type="button" aria-pressed={completed} onClick={() => setCompleted(true)}>Completed ({doneCount})</button></div>
      {(completed ? doneCount : pendingCount) === 0 && <p className="muted">{completed ? 'Completed requests will appear here.' : 'No pending requests. Add one on the canvas.'}</p>}
      {project.pages.map(page => {
        const requests = page.regions.filter(region => !!region.completed === completed);
        return requests.length > 0 && <section key={page.id}><h3>{page.name}</h3>{requests.map(region => completed
          ? <details key={region.id}><summary>{region.prompt.slice(0, 80) || 'Empty request'}</summary><p className="request-full-text">{region.prompt}</p><button type="button" onClick={() => visit(page.id, null)}>View page</button></details>
          : <button key={region.id} className="request-list-item" type="button" onClick={() => visit(page.id, region.id)}>{region.prompt || 'Empty request — add a description'}</button>)}</section>;
      })}
    </section>
  </details>;
}
