import type { Project, PointerStateView } from '../types';
import type { CanvasTarget } from '../projectCanvas/canvasTargets';
import { findPageElement } from '../projectCanvas/elementGeometry';

export function propertySelection(project: Project | null, canvasSelection: CanvasTarget[], canvasFocus: string | null, pointerStateMap: Record<string, PointerStateView>) {
  const selectedBoundary = canvasFocus ? project?.pages.find(item => item.id === canvasFocus) ?? null : null;
  const selectedReferenceItem = canvasSelection.length === 1 && canvasSelection[0].kind === 'reference'
    ? project?.reference.find(item => item.id === (canvasSelection[0] as { kind: 'reference'; itemId: string }).itemId) ?? null
    : null;
  const selectedPointer = canvasSelection.length === 1 && canvasSelection[0].kind === 'pointer'
    ? (project?.pointers ?? []).find(item => item.id === (canvasSelection[0] as { kind: 'pointer'; pointerId: string }).pointerId) ?? null
    : null;
  const selectedRequest = canvasSelection.length === 1 && canvasSelection[0].kind === 'request'
    ? (project?.requests ?? []).find(item => item.id === (canvasSelection[0] as { kind: 'request'; requestId: string }).requestId) ?? null
    : null;
  const selectedElementTargets = canvasSelection.flatMap(target => target.kind === 'element' ? [target] : []);
  const selectedElementPageId = selectedElementTargets.length && selectedElementTargets.every(target => target.pageId === selectedElementTargets[0].pageId) ? selectedElementTargets[0].pageId : null;
  const selectedElements = selectedElementPageId && project
    ? selectedElementTargets.flatMap(target => {
      const element = findPageElement(project, target.pageId, target.elementId);
      return element ? [element] : [];
    })
    : [];
  const selectedElementRequest = selectedElements.length === 1 && project
    ? (project.requests ?? []).find(request => request.targets.length === 1 && request.targets[0].kind === 'element' && request.targets[0].id === selectedElements[0].id) ?? null
    : null;
  const selectedPointerState = selectedPointer ? pointerStateMap[selectedPointer.id] ?? null : null;
  return { selectedBoundary, selectedReferenceItem, selectedPointer, selectedRequest, selectedElementTargets, selectedElementPageId, selectedElements, selectedElementRequest, selectedPointerState };
}
