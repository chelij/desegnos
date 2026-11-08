import { useProjectSession } from './useProjectSession';
import { type ProjectCanvasHandle } from '../ProjectCanvas';
import { viewNameForSlot } from '../shortcuts';
import { type CanvasCamera } from '../projectCanvas/camera';
import type { NamedView, Project } from '../types';

export function useNamedViews(project: Project | null, mutate: ReturnType<typeof useProjectSession>['mutate'], canvasRef: import('react').RefObject<ProjectCanvasHandle | null>, run: (operation: () => Promise<unknown>) => void) {
  const views = project?.views ?? [];
  // Named views live in the project file. Ctrl/Cmd+Alt+N saves into the view named "View N",
  // matching the names the '+' button creates, so numbered slots survive deletions and reordering.
  function viewForSlot(slot: number) {
    return views.find(view => view.name === viewNameForSlot(slot)) ?? null;
  }
  function nextViewName() {
    const names = new Set(views.map(view => view.name));
    let index = 1;
    while (names.has(viewNameForSlot(index))) index += 1;
    return viewNameForSlot(index);
  }
  function saveCurrentView(slot: number, camera: CanvasCamera) {
    if (!project) return;
    const existing = viewForSlot(slot);
    run(() => mutate(existing
      ? { type: 'update_named_view', view_id: existing.id, x: camera.x, y: camera.y, zoom: camera.zoom }
      : { type: 'add_named_view', name: viewNameForSlot(slot), x: camera.x, y: camera.y, zoom: camera.zoom }));
  }
  function addNamedViewAt(camera: CanvasCamera) {
    run(() => mutate({ type: 'add_named_view', name: nextViewName(), x: camera.x, y: camera.y, zoom: camera.zoom }));
  }
  function addNamedView() {
    const camera = canvasRef.current?.currentCamera();
    if (camera) addNamedViewAt(camera);
  }
  function renameNamedView(view: NamedView, name: string) {
    run(() => mutate({ type: 'update_named_view', view_id: view.id, name }));
  }
  function removeNamedView(view: NamedView) {
    run(() => mutate({ type: 'remove_named_view', view_id: view.id }));
  }

  return { views, saveCurrentView, addNamedViewAt, addNamedView, renameNamedView, removeNamedView };
}
