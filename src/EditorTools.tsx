import { type BoundaryToolKind, boundaryName, boundaryIcon } from './projectCanvas/boundaryDefaults';
import { useEffect } from 'react';
import { MenuButton } from './Menus';
import { zoomLabel, ZOOM_LEVELS, MIN_ZOOM } from './canvasGeometry';
import { Icon } from './icons';
import { INSERT_GROUPS, ELEMENT_KIND_LABELS } from './projectCanvas/elementGeometry';
import { shortcutLabel, withShortcut, type ShortcutAction } from './shortcuts';
import type { ElementKind } from './types';

export type CanvasTool = 'select' | 'request' | 'pan';
export type EditorTool = 'select' | 'request' | 'boundary' | 'pointer' | 'text' | 'pan' | 'rect' | 'ellipse' | 'line' | 'image' | 'note' | 'insert';
export type RequestToolShape = 'pick' | 'rect' | 'ellipse' | 'polygon' | 'freehand';
export type ProjectTool = EditorTool;

const ICONS: Record<EditorTool, string> = {
  select: 'M3.5 2 12.5 9 8 10 6.5 14.5Z',
  request: 'M2.5 2.5h11v11h-11Z M6 6h4M6 9h2.5',
  boundary: 'M2 3h12v10H2Z M2 6.5h12',
  pointer: 'M3 13 13 3 M13 3H8.5 M13 3v4.5',
  text: 'M3 3.5h10M8 3.5v9M5.5 12.5h5',
  rect: 'M2.2 3.2h11.6v9.6H2.2Z',
  ellipse: 'M2.2 8a5.8 4.8 0 1 0 11.6 0 5.8 4.8 0 1 0-11.6 0Z',
  line: 'M2.5 13.2 13.5 2.8',
  image: 'M2.2 3h11.6v10H2.2Z M3 12 7 8l2.5 2.5L12 8l2 2.5 M6 6.4a1.1 1.1 0 1 0 0 .01',
  note: 'M2.6 2.8h10.8v7.6l-3 3H2.6Z M13.4 10.4h-3v3',
  insert: 'M8 2.6v10.8M2.6 8h10.8',
  pan: 'M4.5 8V3Q4.5 1.5 6 3v4.5V2.5Q6 .5 7.5 2.5V7.5 3Q7.5 1.5 9 3v6 2.5Q9 3.5 10.5 5v4.5L8.5 13H4.5L1.5 9Q1.5 7.5 3 8.5l1.5 1Z',
};

const LABELS: Record<EditorTool, string> = {
  select: 'Select', request: 'Request', boundary: 'Boundary', pointer: 'Pointer', text: 'Text', rect: 'Rectangle',
  ellipse: 'Ellipse', line: 'Line', image: 'Image', note: 'Note', insert: 'Insert', pan: 'Pan',
};
const SHORTCUTS: Record<EditorTool, ShortcutAction> = {
  select: 'selectTool', request: 'requestTool', boundary: 'boundaryTool', pointer: 'pointerTool', text: 'textTool',
  rect: 'rectTool', ellipse: 'ellipseTool', line: 'lineTool', image: 'imageTool', note: 'noteTool', insert: 'insert', pan: 'panTool',
};

// Strip order follows the order tools are reached for in a normal design pass; Pan sits last
// beside the zoom controls.
const PROJECT_TOOLS: EditorTool[] = ['select', 'boundary', 'request', 'text', 'rect', 'ellipse', 'line', 'image', 'insert', 'pointer', 'note', 'pan'];
const SHAPE_TOOLS = ['rect', 'ellipse', 'line'] as const;
type ShapeTool = typeof SHAPE_TOOLS[number];

// The shape group keeps the last shape used, so R, O and L stay one click away after a restart
// of the strip (the dock can be hidden and shown again).
let rememberedShape: ShapeTool = 'rect';

function toolTitle(tool: EditorTool) {
  return withShortcut(LABELS[tool], SHORTCUTS[tool]);
}

const REQUEST_SHAPES: { id: RequestToolShape; label: string; icon: string }[] = [
  { id: 'pick', label: 'Pick', icon: 'M3.5 2 12.5 9 8 10 6.5 14.5Z' },
  { id: 'rect', label: 'Rectangle', icon: 'M2.2 3.2h11.6v9.6H2.2Z' },
  { id: 'ellipse', label: 'Ellipse', icon: 'M2.2 8a5.8 4.8 0 1 0 11.6 0 5.8 4.8 0 1 0-11.6 0Z' },
  { id: 'polygon', label: 'Polygon', icon: 'M8 2.2 13.8 6.4 11.6 13 4.4 13 2.2 6.4Z' },
  { id: 'freehand', label: 'Freehand', icon: 'M2.5 10.5C3.5 4 6.5 13 9 7.5S13 3.5 13.5 6' },
];

export function ToolStrip({ tool, onTool, zoom, onZoomStep, onZoom, onFit, boundaryKind, onBoundaryKind, requestShape, onRequestShape, onInsert, insertSignal }: {
  tool: EditorTool; onTool: (tool: EditorTool) => void;
  zoom: number; onZoomStep: (direction: number) => void; onZoom: (zoom: number) => void; onFit: () => void;
  boundaryKind?: BoundaryToolKind; onBoundaryKind?: (kind: BoundaryToolKind) => void;
  requestShape?: RequestToolShape; onRequestShape?: (shape: RequestToolShape) => void;
  onInsert?: (kind: ElementKind) => void; insertSignal?: number;
}) {
  const tools = PROJECT_TOOLS;
  const isShape = (value: EditorTool): value is ShapeTool => (SHAPE_TOOLS as readonly EditorTool[]).includes(value);
  const activeShape: ShapeTool = isShape(tool) ? tool : rememberedShape;
  useEffect(() => { if (isShape(tool)) rememberedShape = tool; }, [tool]);
  return <div className="tool-strip" role="toolbar" aria-orientation="vertical" aria-label="Canvas tools">
    <div className="tool-strip-group">
      {tools.map(value => <div className={`tool-strip-slot${isShape(value) ? ' tool-shape-slot' : ''}${isShape(value) && value !== activeShape ? ' is-alternate' : ''}`} key={value}>
        {isShape(value) && value !== activeShape
          ? <button type="button" tabIndex={-1} aria-hidden="true" className="tool-strip-button tool-shape-proxy" aria-label={LABELS[value]} title={toolTitle(value)}
            onClick={() => onTool(value)} />
          : isShape(value)
          ? <>
            <button type="button" className={`tool-strip-button tool-${value}`} aria-pressed={tool === value} aria-label={LABELS[value]} title={toolTitle(value)}
              onPointerDown={event => { if (event.button === 0 && tool !== value) onTool(value); }} onClick={() => onTool(value)}>
              <svg viewBox="0 0 16 16" aria-hidden="true"><path d={ICONS[value]} /></svg>
            </button>
            <MenuButton className="tool-flyout tool-shape-flyout" showChevron={false}
              label={<svg viewBox="0 0 10 10" className="tool-shape-caret" aria-hidden="true"><path d="M9.6 0.4v9.2H0.4Z" /></svg>}
              ariaLabel="Shape tools" title={`Shape tools · ${LABELS[activeShape]}`}
              items={SHAPE_TOOLS.map(shape => ({ id: `shape-${shape}`, label: LABELS[shape], checked: activeShape === shape, shortcut: shortcutLabel(SHORTCUTS[shape]), action: () => onTool(shape) }))} />
          </>
          : value === 'insert' && onInsert
          ? <MenuButton className={`tool-insert${tool === "insert" ? " active" : ""}`} showChevron={false} openSignal={insertSignal} label={<Icon name="insert" />}
            ariaLabel="Insert" title={withShortcut('Insert any element', 'insert')}
            items={INSERT_GROUPS.map(group => ({
              id: `insert-${group.label}`, label: group.label,
              children: group.kinds.map(kind => ({ id: `insert-${kind}`, label: ELEMENT_KIND_LABELS[kind], action: () => onInsert(kind) })),
            }))} />
          : <button type="button" className={`tool-strip-button tool-${value}`} aria-pressed={tool === value} aria-label={LABELS[value]} title={toolTitle(value)}
            onPointerDown={event => { if (event.button === 0 && tool !== value) onTool(value); }} onClick={() => onTool(value)}>
            <svg viewBox="0 0 16 16" aria-hidden="true"><path d={ICONS[value]} /></svg>
          </button>}
        {value === 'boundary' && boundaryKind && onBoundaryKind && <MenuButton className="tool-flyout" showChevron={false}
          label={<Icon name={boundaryIcon(boundaryKind)} size={12} />} ariaLabel="Boundary kind" title={`Boundary kind · ${boundaryName(boundaryKind)}`}
          items={([['page', 'Page'], ['slide', 'Slide'], ['image', 'Image']] as const).map(([kind, label]) => ({ id: kind, label, checked: boundaryKind === kind, action: () => { onBoundaryKind(kind); onTool('boundary'); } }))} />}
        {value === 'request' && requestShape && onRequestShape && <MenuButton className="tool-flyout" showChevron={false}
          label={<svg viewBox="0 0 16 16" aria-hidden="true"><path d={REQUEST_SHAPES.find(shape => shape.id === requestShape)?.icon ?? ICONS.request} /></svg>}
          ariaLabel="Request area shape" title={`Request area shape · ${REQUEST_SHAPES.find(shape => shape.id === requestShape)?.label ?? 'Pick'}`}
          items={REQUEST_SHAPES.map(shape => ({ id: `request-${shape.id}`, label: shape.label, checked: requestShape === shape.id, action: () => { onRequestShape(shape.id); onTool('request'); } }))} />}
      </div>)}
    </div>
    <div className="tool-strip-foot">
      <button type="button" className="tool-strip-button" aria-label="Zoom out" title={withShortcut('Zoom out', 'zoomOut')} disabled={zoom <= MIN_ZOOM} onClick={() => onZoomStep(-1)}><Icon name="minus" /></button>
      <MenuButton className="tool-zoom" showChevron={false} label={zoomLabel(zoom)} ariaLabel="Zoom level" title="Zoom level"
        items={ZOOM_LEVELS.map(value => ({ id: `zoom-${value}`, label: zoomLabel(value), checked: Math.abs(zoom - value) < 1e-6, action: () => onZoom(value) }))} />
      <button type="button" className="tool-strip-button" aria-label="Zoom in" title={withShortcut('Zoom in', 'zoomIn')} disabled={zoom >= (ZOOM_LEVELS.at(-1) ?? 4)} onClick={() => onZoomStep(1)}><Icon name="plus" /></button>
      <button type="button" className="tool-strip-fit" title={withShortcut('Fit all', 'fitAll')} onClick={onFit}>Fit</button>
    </div>
  </div>;
}
