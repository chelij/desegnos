import { MenuButton } from './Menus';
import { zoomLabel, ZOOM_LEVELS, MIN_ZOOM } from './canvasGeometry';
import { Icon } from './icons';
import { INSERT_GROUPS, ELEMENT_KIND_LABELS } from './projectCanvas/elementGeometry';
import { withShortcut, type ShortcutAction } from './shortcuts';
import type { BoundaryKind, ElementKind } from './types';

export type CanvasTool = 'select' | 'request' | 'pan';
export type EditorTool = 'select' | 'frame' | 'boundary' | 'pointer' | 'text' | 'pan' | 'rect' | 'ellipse' | 'line' | 'image' | 'note' | 'insert';
export type ProjectTool = EditorTool;

const ICONS: Record<EditorTool, string> = {
  select: 'M3.5 2 12.5 9 8 10 6.5 14.5Z',
  frame: 'M2.5 2.5h11v11h-11Z',
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
  select: 'Select', frame: 'Frame', boundary: 'Boundary', pointer: 'Pointer', text: 'Text', rect: 'Rectangle',
  ellipse: 'Ellipse', line: 'Line', image: 'Image', note: 'Note', insert: 'Insert', pan: 'Pan',
};
const SHORTCUTS: Record<EditorTool, ShortcutAction> = {
  select: 'selectTool', frame: 'frameTool', boundary: 'boundaryTool', pointer: 'pointerTool', text: 'textTool',
  rect: 'rectTool', ellipse: 'ellipseTool', line: 'lineTool', image: 'imageTool', note: 'noteTool', insert: 'insert', pan: 'panTool',
};

const PROJECT_TOOLS: EditorTool[] = ['select', 'frame', 'boundary', 'pointer', 'text', 'rect', 'ellipse', 'line', 'image', 'note', 'insert', 'pan'];

function toolTitle(tool: EditorTool) {
  return withShortcut(LABELS[tool], SHORTCUTS[tool]);
}

export function ToolStrip({ tool, onTool, zoom, onZoomStep, onZoom, onFit, boundaryKind, onBoundaryKind, onInsert, insertSignal }: {
  tool: EditorTool; onTool: (tool: EditorTool) => void;
  zoom: number; onZoomStep: (direction: number) => void; onZoom: (zoom: number) => void; onFit: () => void;
  boundaryKind?: BoundaryKind; onBoundaryKind?: (kind: BoundaryKind) => void;
  onInsert?: (kind: ElementKind) => void; insertSignal?: number;
}) {
  const tools = PROJECT_TOOLS;
  return <div className="tool-strip" role="toolbar" aria-orientation="vertical" aria-label="Canvas tools">
    <div className="tool-strip-group">
      {tools.map(value => <div className="tool-strip-slot" key={value}>
        {value === 'insert' && onInsert
          ? <MenuButton className={`tool-insert${tool === "insert" ? " active" : ""}`} showChevron={false} openSignal={insertSignal} label={<Icon name="insert" />}
            ariaLabel="Insert" title={withShortcut('Insert any element', 'insert')}
            items={INSERT_GROUPS.map(group => ({
              id: `insert-${group.label}`, label: group.label,
              children: group.kinds.map(kind => ({ id: `insert-${kind}`, label: ELEMENT_KIND_LABELS[kind], action: () => onInsert(kind) })),
            }))} />
          : <button type="button" className={`tool-strip-button tool-${value}`} aria-pressed={tool === value} aria-label={LABELS[value]} title={toolTitle(value)}
            onClick={() => onTool(value)}>
            <svg viewBox="0 0 16 16" aria-hidden="true"><path d={ICONS[value]} /></svg>
          </button>}
        {value === 'boundary' && boundaryKind && onBoundaryKind && <MenuButton className="tool-flyout" showChevron={false}
          label={<Icon name={boundaryKind === 'screen' ? 'monitor' : 'canvas'} size={12} />} ariaLabel="Boundary kind" title={`Boundary kind · ${boundaryKind === 'screen' ? 'Screen' : 'Page'}`}
          items={([['page', 'Page'], ['screen', 'Screen']] as const).map(([kind, label]) => ({ id: kind, label, checked: boundaryKind === kind, action: () => { onBoundaryKind(kind); onTool('boundary'); } }))} />}
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
