import { useCallback, useEffect, useRef, useState } from 'react';
import { behaviourSourceKey } from '../projectCanvas/behaviours';
import { toggleCanvasTarget, type CanvasTarget } from '../projectCanvas/canvasTargets';
import { findElementIn } from '../projectCanvas/elementFacts';
import type { PointerSource, Project } from '../types';

/** What is selected or focused on the canvas, kept consistent with the project it points into. */
export function useCanvasSelection(project: Project | null) {
  const [canvasSelection, setCanvasSelection] = useState<CanvasTarget[]>([]);
  const [canvasFocus, setCanvasFocus] = useState<string | null>(null);
  const [canvasFocusSize, setCanvasFocusSize] = useState<string | null>(null);
  const [widthPreview, setWidthPreview] = useState<{ sizeId: string; width: number } | null>(null);
  const [behaviourFocus, setBehaviourFocus] = useState<{ source: string; token: number } | null>(null);
  const behaviourToken = useRef(0);

  const selectCanvasBoundary = useCallback((pageId: string | null, sizeId?: string | null, additive = false) => {
    setCanvasFocus(pageId);
    setCanvasFocusSize(sizeId ?? null);
    if (!additive || !pageId) setCanvasSelection([]);
  }, []);
  const selectCanvasReference = useCallback((itemId: string, additive = false) => {
    setCanvasSelection(current => toggleCanvasTarget(current, { kind: 'reference', itemId }, additive));
  }, []);
  const selectCanvasElement = useCallback((pageId: string, elementId: string, sizeId: string | null, additive = false) => {
    setCanvasFocus(pageId);
    setCanvasSelection(current => toggleCanvasTarget(current, { kind: 'element', pageId, elementId, sizeId }, additive));
  }, []);
  const selectCanvasPointer = useCallback((pointerId: string, additive = false) => {
    setCanvasSelection(current => toggleCanvasTarget(current, { kind: 'pointer', pointerId }, additive));
  }, []);
  const selectCanvasRequest = useCallback((requestId: string, additive = false) => {
    setCanvasSelection(current => toggleCanvasTarget(current, { kind: 'request', requestId }, additive));
  }, []);
  const clearCanvasSelection = useCallback(() => setCanvasSelection([]), []);
  // A behaviour badge or a pointer dropped on its own layer lands here: select it and open its field.
  const focusCanvasBehaviour = useCallback((source: PointerSource) => {
    if (source.kind === 'element') selectCanvasElement(source.pageId, source.elementId, source.sizeId ?? null);
    else selectCanvasBoundary(source.pageId, source.sizeId ?? null);
    setBehaviourFocus({ source: behaviourSourceKey(source), token: ++behaviourToken.current });
  }, [selectCanvasBoundary, selectCanvasElement]);

  useEffect(() => { setCanvasSelection([]); setCanvasFocus(null); setCanvasFocusSize(null); }, [project?.id]);
  useEffect(() => {
    const pages = project?.pages, reference = project?.reference ?? [], pointers = project?.pointers ?? [];
    setCanvasSelection(current => {
      const requests = project?.requests ?? [];
      const next = current.filter(target => target.kind === 'request'
        ? requests.some(request => request.id === target.requestId)
        : target.kind === 'element'
        ? !!pages?.some(page => page.id === target.pageId && findElementIn(page.elements, target.elementId)
          && (target.sizeId === null || page.sizes.some(size => size.id === target.sizeId)))
        : target.kind === 'pointer' ? pointers.some(pointer => pointer.id === target.pointerId) : reference.some(item => item.id === target.itemId));
      return next.length === current.length ? current : next;
    });
    if (canvasFocus && !pages?.some(page => page.id === canvasFocus)) setCanvasFocus(null);
  }, [project?.pages, project?.reference, project?.pointers, project?.requests, canvasFocus]);
  useEffect(() => setWidthPreview(null), [project?.id, project?.revision, canvasFocus, canvasFocusSize, canvasSelection]);

  return {
    canvasSelection, setCanvasSelection, canvasFocus, setCanvasFocus, canvasFocusSize, setCanvasFocusSize,
    widthPreview, setWidthPreview, behaviourFocus,
    selectCanvasBoundary, selectCanvasReference, selectCanvasElement, selectCanvasPointer, selectCanvasRequest,
    clearCanvasSelection, focusCanvasBehaviour };
}
