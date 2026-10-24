import type { PointerEvent, MouseEvent } from 'react';
import { clipToWorkspace, type WorkspaceBounds } from './canvasGeometry';
import type { Rect, RegionShape } from './types';

export function RequestShapeIcon({ kind }: { kind: RegionShape['kind'] }) {
  return <svg className="icon" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth={1.5} strokeLinejoin="round" aria-hidden="true">{kind === 'ellipse' ? <ellipse cx="8" cy="8" rx="6" ry="5" /> : kind === 'freehand' ? <path d="M2.5 5Q0 1 5 2.5Q11 0 13 5Q16.5 10 10.5 12Q5 15.5 2.5 10.5Q1 8 2.5 5Z" /> : <rect x="2" y="2.5" width="12" height="11" />}</svg>;
}

export function RegionOutline({ shape, rect, bounds, zoom, layer, selected = false, draft = false, regionId, onPointerDown, onContextMenu }: {
  shape: RegionShape; rect: Rect; bounds: WorkspaceBounds; zoom: number; layer: number; selected?: boolean; draft?: boolean; regionId?: string;
  onPointerDown?: (event: PointerEvent<SVGSVGElement>) => void; onContextMenu?: (event: MouseEvent<SVGSVGElement>) => void;
}) {
  const clipped = clipToWorkspace(rect, bounds);
  if (!clipped || clipped.width <= 0 || clipped.height <= 0) return null;
  const interactive = onPointerDown ? 'all' : 'none';
  return <svg className={`region-outline ${selected ? 'selected' : ''} ${draft ? 'draft' : ''}`} data-testid={draft ? 'area-draft' : 'area-outline'} data-region-id={regionId} data-shape={shape.kind} aria-hidden="true"
    style={{ position: 'absolute', left: (clipped.x - bounds.left) * zoom, top: (clipped.y - bounds.top) * zoom, width: clipped.width * zoom, height: clipped.height * zoom, zIndex: layer }}
    viewBox={`${clipped.x - rect.x} ${clipped.y - rect.y} ${clipped.width} ${clipped.height}`} preserveAspectRatio="none" onPointerDown={onPointerDown} onContextMenu={onContextMenu}>
    {shape.kind === 'ellipse' ? <ellipse cx={rect.width / 2} cy={rect.height / 2} rx={rect.width / 2} ry={rect.height / 2} vectorEffect="non-scaling-stroke" pointerEvents={interactive} />
      : shape.kind === 'freehand' ? <path d={`${shape.points.map(([x, y], i) => `${i ? 'L' : 'M'}${x * rect.width} ${y * rect.height}`).join(' ')}Z`} fillRule="evenodd" vectorEffect="non-scaling-stroke" pointerEvents={interactive} />
      : <rect width={rect.width} height={rect.height} vectorEffect="non-scaling-stroke" pointerEvents={interactive} />}
  </svg>;
}
