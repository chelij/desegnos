import { useMemo, useRef, type PointerEvent as ReactPointerEvent } from 'react';
import { viewportWorldRect, type Boundary, type CanvasCamera } from '../projectCanvasGeometry';
import type { ReferenceItem } from '../types';
import {
  MINIMAP_HEIGHT, MINIMAP_WIDTH, cameraCentredOn, minimapPoint, minimapTransform, minimapWorld, minimapWorldPoint, overviewRects,
} from './minimap';

/** A small overview of every boundary and reference item, with the visible viewport on top.
 * The content layer depends only on the canvas layout; panning redraws just the viewport box. */
export function Minimap({ boundaries, reference, camera, viewSize, move }: {
  boundaries: Boundary[]; reference: ReferenceItem[]; camera: CanvasCamera;
  viewSize: { width: number; height: number }; move: (camera: CanvasCamera) => void;
}) {
  const dragging = useRef(false);
  const rects = overviewRects(boundaries, reference);
  const transform = minimapTransform(minimapWorld(rects, camera, viewSize));
  const viewportWorld = viewportWorldRect(camera, viewSize);
  const viewport = minimapPoint(transform, viewportWorld);
  const content = useMemo(() => <>
    {boundaries.map(boundary => {
      const point = minimapPoint(transform, boundary.rect);
      return <rect key={boundary.sizeId} className="minimap-boundary" x={point.x} y={point.y}
        width={Math.max(1.5, boundary.rect.width * transform.scale)} height={Math.max(1.5, boundary.rect.height * transform.scale)} />;
    })}
    {reference.map((item, index) => {
      const point = minimapPoint(transform, item.rect);
      return <rect key={`reference-${index}`} className="minimap-reference" x={point.x} y={point.y}
        width={Math.max(1, item.rect.width * transform.scale)} height={Math.max(1, item.rect.height * transform.scale)} />;
    })}
  </>, [boundaries, reference, transform.scale, transform.x, transform.y]);

  function pointOn(event: ReactPointerEvent<SVGSVGElement>) {
    const box = event.currentTarget.getBoundingClientRect();
    return {
      x: (event.clientX - box.left) * MINIMAP_WIDTH / Math.max(1, box.width),
      y: (event.clientY - box.top) * MINIMAP_HEIGHT / Math.max(1, box.height),
    };
  }
  function jumpTo(event: ReactPointerEvent<SVGSVGElement>) {
    move(cameraCentredOn(minimapWorldPoint(transform, pointOn(event)), camera, viewSize));
  }
  return <svg className="project-minimap" width={MINIMAP_WIDTH} height={MINIMAP_HEIGHT} viewBox={`0 0 ${MINIMAP_WIDTH} ${MINIMAP_HEIGHT}`}
    role="group" aria-label="Minimap. Click or drag to move the view." data-minimap
    onPointerDown={event => {
      if (event.button !== 0) return;
      event.preventDefault(); event.stopPropagation();
      event.currentTarget.setPointerCapture(event.pointerId);
      dragging.current = true;
      jumpTo(event);
    }}
    onPointerMove={event => {
      if (!dragging.current) return;
      event.preventDefault(); event.stopPropagation();
      jumpTo(event);
    }}
    onPointerUp={event => {
      dragging.current = false;
      if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
    }}
    onPointerCancel={() => { dragging.current = false; }}
    onDoubleClick={event => event.stopPropagation()}
    onWheel={event => event.stopPropagation()}>
    <title>Minimap</title>
    <rect className="minimap-bed" x={0} y={0} width={MINIMAP_WIDTH} height={MINIMAP_HEIGHT} rx={6} />
    {content}
    <rect className="minimap-viewport" x={viewport.x} y={viewport.y} width={Math.max(2, viewportWorld.width * transform.scale)} height={Math.max(2, viewportWorld.height * transform.scale)} rx={1} />
  </svg>;
}
