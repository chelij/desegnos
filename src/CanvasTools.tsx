import { FloatingBar } from './FloatingBar';
import { LayerIcon } from './Layers';
import { MenuButton } from './Menus';
import { steppedZoom, zoomLabel, ZOOM_LEVELS, MIN_ZOOM } from './canvasGeometry';
import type { RegionShape } from './types';
import { RequestShapeIcon } from './RegionOutline';
import { Icon } from './icons';
import { withShortcut } from './shortcuts';

export type CanvasTool = 'select' | 'request' | 'pan';
export type RequestShapeKind = RegionShape['kind'];
export function CanvasTools({ tool, setTool, requestShape, setRequestShape, zoom, setZoom, fit, layersOpen, toggleLayers }: {
  requestShape: RequestShapeKind; setRequestShape: (shape: RequestShapeKind) => void;
  tool: CanvasTool; setTool: (tool: CanvasTool) => void; zoom: number; setZoom: (value: number) => void; fit: () => void; layersOpen: boolean; toggleLayers: () => void;
}) {
  return <FloatingBar className="canvas-tools" role="toolbar" aria-label="Canvas tools">
    {([
      ['select', 'Select', 'M3.5 2 12.5 9 8 10 6.5 14.5Z'],
      ['request', 'Add request', 'M2 4h12v9H2Z M8 1.5v5 M5 4h6'],
      ['pan', 'Pan', 'M4.5 8V3Q4.5 1.5 6 3v4.5V2.5Q6 .5 7.5 2.5V7.5 3Q7.5 1.5 9 3v6 2.5Q9 3.5 10.5 5v4.5L8.5 13H4.5L1.5 9Q1.5 7.5 3 8.5l1.5 1Z'],
    ] as const).map(([value, label, path]) => <button key={value} type="button" aria-pressed={tool === value} title={withShortcut(label, value === 'select' ? 'selectTool' : value === 'request' ? 'requestTool' : 'panTool')} onClick={() => setTool(value)}>
      <svg viewBox="0 0 16 16" aria-hidden="true"><path d={path} /></svg><span>{label}</span>
    </button>)}
    <MenuButton ariaLabel="Request shape" title="Request shape · Shift+R / C / L" label={<><RequestShapeIcon kind={requestShape} /><span>{requestShape === 'rect' ? 'Rectangle' : requestShape === 'ellipse' ? 'Circle' : 'Freehand'}</span></>} items={([
      ['rect', 'Rectangle', 'Shift+R'], ['ellipse', 'Circle', 'C'], ['freehand', 'Freehand', 'L'],
    ] as const).map(([kind, label, shortcut]) => ({ id: kind, label, shortcut, checked: requestShape === kind, action: () => { setRequestShape(kind); setTool('request'); } }))} />
    <button type="button" data-layers-trigger aria-label="Layers" title="Layers" aria-haspopup="dialog" aria-expanded={layersOpen} onClick={toggleLayers}><LayerIcon /><span>Layers</span></button>
    <span className="tool-divider" aria-hidden="true" />
    <button type="button" className="icon-button sm" aria-label="Zoom out" title={withShortcut('Zoom out', 'zoomOut')} disabled={zoom <= MIN_ZOOM} onClick={() => setZoom(steppedZoom(zoom, -1))}><Icon name="minus" /></button>
    <MenuButton label={zoomLabel(zoom)} ariaLabel="Canvas zoom" title="Canvas zoom" items={ZOOM_LEVELS.map(value => ({ id: `zoom-${value}`, label: zoomLabel(value), checked: zoom === value, action: () => setZoom(value) }))} />
    <button type="button" className="icon-button sm" aria-label="Zoom in" title={withShortcut('Zoom in', 'zoomIn')} disabled={zoom === ZOOM_LEVELS.at(-1)} onClick={() => setZoom(steppedZoom(zoom, 1))}><Icon name="plus" /></button>
    <button type="button" title={withShortcut('Fit screen guide', 'fit')} onClick={fit}>Fit</button>
  </FloatingBar>;
}
