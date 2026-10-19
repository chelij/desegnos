import { LayerIcon } from './Layers';
import { MenuButton } from './Menus';
import { steppedZoom, zoomLabel, ZOOM_LEVELS, MIN_ZOOM } from './canvasGeometry';

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
