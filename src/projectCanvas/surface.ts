import { useEffect, useRef, useState } from 'react';
import { isTextField } from '../WorkspaceSurface';
import { steppedZoom, wheelZoom, clamp, MIN_ZOOM } from '../canvasGeometry';
import { MAX_ZOOM, fitCamera, panCamera, unionBounds, zoomCameraAt, type Boundary, type CanvasCamera, type Point } from '../projectCanvasGeometry';
import type { Rect } from '../types';

export type ScreenRect = { left: number; top: number; width: number; height: number };
export type NativeFile = { name: string; mime: string; data: string };
export type NativeDrag = { phase: string; id: string; position: { x: number; y: number } | null; picture: NativeFile | null; files: NativeFile[]; fileErrors: string[]; message: string | null };

// A canvas target's identity, shared by selection, drags and Layers.
export const centre = (rect: Rect): Point => ({ x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 });
export const screenRect = (rect: Rect, camera: CanvasCamera): ScreenRect => ({
  left: rect.x * camera.zoom + camera.x, top: rect.y * camera.zoom + camera.y,
  width: rect.width * camera.zoom, height: rect.height * camera.zoom,
});
export const hostname = (url: string) => { try { return new URL(url).hostname; } catch { return url; } };

// Camera, wheel, Space-pan and Fit: the surface's view of the world, separate from its content.
export function useCanvasCamera(surface: { current: HTMLDivElement | null }, boundaries: Boundary[], projectId: string) {
  const [camera, setCamera] = useState<CanvasCamera>({ x: 0, y: 0, zoom: 1 });
  const [panning, setPanning] = useState(false);
  const [spaceHeld, setSpaceHeld] = useState(false);
  const [zoomHeld, setZoomHeld] = useState(false);
  const space = useRef(false);
  const zoom = useRef(false);
  const touched = useRef(false);
  // The latest camera, for commands that need it without re-rendering the surface.
  const cameraRef = useRef(camera);
  cameraRef.current = camera;
  const transition = useRef<number | null>(null);

  function stopTransition() {
    if (transition.current !== null) { cancelAnimationFrame(transition.current); transition.current = null; }
  }

  // Jumping to a saved view is short and eased, so the eye can follow what moved.
  function animateTo(target: CanvasCamera) {
    stopTransition();
    const from = cameraRef.current;
    const zoomValue = clamp(Number.isFinite(target.zoom) ? target.zoom : from.zoom, MIN_ZOOM, MAX_ZOOM);
    if (!Number.isFinite(target.x) || !Number.isFinite(target.y)) return;
    const to = { x: target.x, y: target.y, zoom: zoomValue };
    touched.current = true;
    const start = performance.now();
    const duration = 240;
    const step = (now: number) => {
      const progress = Math.min(1, Math.max(0, (now - start) / duration));
      const eased = 1 - (1 - progress) ** 3;
      const next = {
        x: from.x + (to.x - from.x) * eased,
        y: from.y + (to.y - from.y) * eased,
        zoom: from.zoom + (to.zoom - from.zoom) * eased,
      };
      cameraRef.current = next;
      setCamera(next);
      transition.current = progress < 1 ? requestAnimationFrame(step) : null;
    };
    transition.current = requestAnimationFrame(step);
  }

  useEffect(() => () => stopTransition(), []);

  // Fit once when a project opens; after that the view only moves when the user moves it,
  // so drawing never shifts what is on screen.
  const fittedProject = useRef<string | null>(null);
  useEffect(() => {
    const node = surface.current;
    if (fittedProject.current === projectId || !node) return;
    const box = node.getBoundingClientRect();
    if (box.width < 1 || box.height < 1) return;
    fittedProject.current = projectId;
    if (boundaries.length) setCamera(fitCamera(unionBounds(boundaries.map(item => item.rect)), { width: box.width, height: box.height }));
  }, [boundaries, projectId]);

  useEffect(() => {
    const node = surface.current;
    if (!node) return;
    const wheel = (event: WheelEvent) => {
      const box = node.getBoundingClientRect();
      const point = { x: event.clientX - box.left, y: event.clientY - box.top };
      event.preventDefault();
      touched.current = true;
      stopTransition();
      setCamera(current => event.ctrlKey || event.metaKey ? zoomCameraAt(current, point, wheelZoom(current.zoom, event.deltaY)) : panCamera(current, { x: -event.deltaX, y: -event.deltaY }));
    };
    node.addEventListener('wheel', wheel, { passive: false });
    return () => node.removeEventListener('wheel', wheel);
  }, []);

  useEffect(() => {
    const down = (event: KeyboardEvent) => {
      if (isTextField(event.target)) return;
      if (event.code === 'Space' && !event.repeat && !event.shiftKey) {
        event.preventDefault();
        space.current = true; setSpaceHeld(true);
        return;
      }
      // Z arms the zoom tool: drag a rectangle, or click to zoom in (Alt+click zooms out).
      // Ctrl/Cmd+Z stays Undo, so a command chord never arms it.
      if (event.code !== 'KeyZ' || event.repeat || event.ctrlKey || event.metaKey) return;
      zoom.current = true; setZoomHeld(true);
    };
    const up = (event: KeyboardEvent) => {
      if (event.code === 'Space') { space.current = false; setSpaceHeld(false); }
      if (event.code === 'KeyZ') { zoom.current = false; setZoomHeld(false); }
    };
    const blur = () => { space.current = false; setSpaceHeld(false); zoom.current = false; setZoomHeld(false); };
    window.addEventListener('keydown', down);
    window.addEventListener('keyup', up);
    window.addEventListener('blur', blur);
    return () => { window.removeEventListener('keydown', down); window.removeEventListener('keyup', up); window.removeEventListener('blur', blur); };
  }, []);

  function fit() {
    const node = surface.current;
    if (!node || !boundaries.length) return;
    const box = node.getBoundingClientRect();
    stopTransition();
    touched.current = false;
    setCamera(fitCamera(unionBounds(boundaries.map(item => item.rect)), { width: box.width, height: box.height }));
  }
  function zoomStep(direction: number) {
    const node = surface.current;
    if (!node) return;
    const box = node.getBoundingClientRect();
    stopTransition();
    touched.current = true;
    setCamera(current => zoomCameraAt(current, { x: box.width / 2, y: box.height / 2 }, steppedZoom(current.zoom, direction)));
  }
  function zoomTo(zoom: number) {
    const node = surface.current;
    if (!node) return;
    const box = node.getBoundingClientRect();
    stopTransition();
    touched.current = true;
    setCamera(current => zoomCameraAt(current, { x: box.width / 2, y: box.height / 2 }, zoom));
  }
  function worldPoint(event: { clientX: number; clientY: number }): Point {
    const box = surface.current!.getBoundingClientRect();
    return { x: (event.clientX - box.left - camera.x) / camera.zoom, y: (event.clientY - box.top - camera.y) / camera.zoom };
  }
  return { camera, setCamera, panning, setPanning, spaceHeld, space, zoomHeld, zoom, touched, fit, zoomStep, zoomTo, worldPoint,
    currentCamera: () => cameraRef.current, animateTo, stopTransition };
}
