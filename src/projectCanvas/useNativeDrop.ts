import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useEffect, useRef, useState, type RefObject } from 'react';
import { IMAGE_DRAG_EVENT } from '../generated/commands';
import { MAX_NAME_CHARS } from '../generated/limits';
import type { Element, Project, Rect } from '../types';
import { boundaryAt, type Boundary } from './boundaryLayout';
import type { CanvasCamera, Point } from './camera';
import type { CanvasTarget } from './canvasTargets';
import { pastedImageRect, pastedNoteRect } from './clipboardPaste';
import { createElement, createTextElement, elementChildrenOf, elementLocalRect, elementParents, forBackground, withPickedColours } from './elementGeometry';
import { findElementIn } from './elementFacts';
import { droppedDataUri, droppedFile, droppedFileKind, droppedImageSize, droppedText } from './fileDrop';
import { NIL_UUID, elementAt, type CanvasHookProps } from './interactionTypes';
import { elementBoxesNow } from './pointerLayout';
import { requestAreaContains } from './requestGeometry';
import { pendingRequests } from './requests';
import { hasManySizes } from './sizes';
import type { NativeDrag } from './surface';

/** What a drop reads from the canvas at the moment it lands. */
type DropState = { camera: CanvasCamera; boundaries: Boundary[]; project: Project; selection: CanvasTarget[] }
  & Pick<CanvasHookProps, 'attachToRequest' | 'insertElements' | 'addReference' | 'clearSelection' | 'selectElement' | 'selectReference'>;

export function useNativeDrop(surface: RefObject<HTMLDivElement | null>, latest: RefObject<DropState>) {
  const nativeDrop = useRef<{ id: string; x: number; y: number } | null>(null);
  const nativeOver = useRef<{ id: string; x: number; y: number } | null>(null);
  const dropNoticeId = useRef(0);
  const [dropHover, setDropHover] = useState<{ key: string; rect: Rect | null } | null>(null);
  const [dropNotice, setDropNotice] = useState<{ id: number; message: string; variant: 'info' | 'warning' } | null>(null);

  // Files dropped on the project canvas: pictures and text land inside the boundary under
  // the pointer or become reference outside every boundary, and other accepted files attach
  // to a request. File access still starts only at the operating-system drop.
  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false; let stop: (() => void) | undefined;

    /** Native drag positions are physical pixels; the canvas draws in CSS pixels. */
    function clientPoint(position: { x: number; y: number }): Point {
      const ratio = window.devicePixelRatio || 1;
      return { x: position.x / ratio, y: position.y / ratio };
    }
    function worldFromClient(x: number, y: number): Point | null {
      const node = surface.current;
      if (!node) return null;
      const box = node.getBoundingClientRect();
      if (x < box.left || x > box.right || y < box.top || y > box.bottom) return null;
      const current = latest.current.camera;
      return { x: (x - box.left - current.x) / current.zoom, y: (y - box.top - current.y) / current.zoom };
    }
    function highlight(point: Point | null) {
      if (!point) { setDropHover(null); return; }
      const boundary = boundaryAt(latest.current.boundaries, point);
      const key = boundary ? `${boundary.pageId}:${boundary.sizeId}` : 'bed';
      setDropHover(current => current?.key === key ? current : { key, rect: boundary?.rect ?? null });
    }
    function requestAt(point: Point): string | null {
      const rows = pendingRequests(latest.current.project);
      for (let index = rows.length - 1; index >= 0; index--) {
        const request = rows[index].request;
        if (request.area && requestAreaContains(request.area.shape, point)) return request.id;
      }
      return null;
    }
    function requestForElement(elementId: string): string | null {
      const project = latest.current.project;
      const page = project.pages.find(item => !!findElementIn(item.elements, elementId));
      const ids = page ? [elementId, ...elementParents(project, page.id, elementId)] : [elementId];
      return project.requests.find(request => request.targets.some(target => target.kind === 'element' && ids.includes(target.id)))?.id ?? null;
    }
    function showNotice(message: string, variant: 'info' | 'warning') {
      if (!message) return;
      setDropNotice({ id: ++dropNoticeId.current, message, variant });
    }

    async function dropFiles(point: Point, client: Point, value: NativeDrag) {
      const state = latest.current;
      const files = value.files.length ? value.files : value.picture ? [value.picture] : [];
      const prepared = files.map(file => ({ file, kind: droppedFileKind(file) }));
      const usable = prepared.filter(item => item.kind !== null);
      const messages = [...value.fileErrors];
      // The native fallback message is only useful when nothing else was readable; a text or
      // zip drop is valid even though it is not a picture.
      if (value.message && !value.fileErrors.length && !usable.length) messages.push(value.message);

      // A request marker, a request area, or a layer a request targets all take attachments.
      const hit = document.elementFromPoint(client.x, client.y);
      const markerRequest = hit instanceof Element ? hit.closest<HTMLElement>('[data-request]')?.dataset.request ?? null : null;
      const domElementId = hit instanceof Element ? hit.closest<HTMLElement>('[data-element]')?.dataset.element ?? null : null;
      // Geometry covers overlays and the brief moment after a drag when hit testing is busy.
      const elementId = domElementId ?? elementAt(elementBoxesNow(state.project, state.boundaries), point)?.elementId ?? null;
      const targetRequest = requestAt(point) ?? markerRequest ?? (elementId ? requestForElement(elementId) : null);

      const boundary = boundaryAt(state.boundaries, point);
      const placeable = usable.filter(item => item.kind === 'image' || item.kind === 'text');
      const other = usable.filter(item => item.kind === 'other');
      const unplaceable = prepared.filter(item => item.kind === null);
      const info: string[] = [];

      if (other.length) {
        if (targetRequest) state.attachToRequest(targetRequest, other.map(item => droppedFile(item.file)), []);
        else info.push(`${other.map(item => item.file.name).join(', ')}: this file type can't be placed on the canvas.`);
      }
      if (unplaceable.length) info.push(`${unplaceable.map(item => item.file.name).join(', ')}: this file type can't be placed on the canvas.`);

      const dropped: { pageId: string; elementId: string; sizeId: string | null }[] = [];
      const references: string[] = [];

      if (boundary && placeable.length) {
        const page = state.project.pages.find(item => item.id === boundary.pageId);
        if (page) {
          const allSizes = state.selection.some(target => target.kind === 'element' && target.pageId === boundary.pageId && target.sizeId === null);
          const sizeId = hasManySizes(page) && !allSizes ? boundary.sizeId : null;
          const elements: { element: Element; picture?: string }[] = [];
          for (const [index, item] of placeable.entries()) {
            const offset = index * 24;
            if (item.kind === 'image') {
              const picture = droppedDataUri(item.file);
              const size = await droppedImageSize(picture);
              const worldRect = pastedImageRect({ x: point.x + offset, y: point.y + offset }, size.width, size.height);
              const element = withPickedColours(forBackground(createElement('image', worldRect, { image: null }), page.background));
              element.rect = { ...elementLocalRect(worldRect, boundary.rect, { x: 0, y: 0 }), width: worldRect.width, height: worldRect.height };
              elements.push({ element, picture });
            } else {
              const worldRect = { x: Math.round(point.x + offset), y: Math.round(point.y + offset), width: 0, height: 0 };
              const element = withPickedColours(forBackground(createTextElement(worldRect, droppedText(item.file), page.output), page.background));
              element.rect = { ...elementLocalRect(worldRect, boundary.rect, { x: 0, y: 0 }), width: 0, height: 0 };
              elements.push({ element });
            }
          }
          const images = elements.filter(item => item.picture);
          let index = elementChildrenOf(state.project, boundary.pageId, null).length;

          if (images.length <= 1) {
            // One command keeps the drop to one undo step; it carries one picture for the batch.
            await state.insertElements(boundary.pageId, null, index, elements.map(item => item.element), images[0]?.picture, boundary.sizeId);
            dropped.push(...elements.map(item => ({ pageId: boundary.pageId, elementId: item.element.id, sizeId })));
          } else {
            const text = elements.filter(item => !item.picture);
            if (text.length) {
              await state.insertElements(boundary.pageId, null, index, text.map(item => item.element), undefined, boundary.sizeId);
              index += text.length;
              dropped.push(...text.map(item => ({ pageId: boundary.pageId, elementId: item.element.id, sizeId })));
            }
            for (const item of images) {
              await state.insertElements(boundary.pageId, null, index, [item.element], item.picture, boundary.sizeId);
              index += 1;
              dropped.push({ pageId: boundary.pageId, elementId: item.element.id, sizeId });
            }
          }

        }
      } else if (placeable.length) {
        for (const [index, item] of placeable.entries()) {
          const offset = index * 24;
          const id = crypto.randomUUID();

          if (item.kind === 'image') {
            const picture = droppedDataUri(item.file);
            const size = await droppedImageSize(picture);
            const rect = pastedImageRect({ x: point.x + offset, y: point.y + offset }, size.width, size.height);
            await state.addReference({ id, rect, kind: { kind: 'image', picture: NIL_UUID, name: item.file.name.slice(0, MAX_NAME_CHARS) } }, picture);
          } else {
            const rect = { ...pastedNoteRect(point), x: Math.round(point.x + offset), y: Math.round(point.y + offset) };
            await state.addReference({ id, rect, kind: { kind: 'note', text: droppedText(item.file) } });
          }
          references.push(id);

        }
      }

      if (dropped.length || references.length) {
        state.clearSelection();
        for (const item of dropped) state.selectElement(item.pageId, item.elementId, item.sizeId, true);
        for (const id of references) state.selectReference(id, true);
      }
      const unique = [...new Set(messages.filter(Boolean))];
      const notices = info.filter(Boolean);
      if (unique.length) showNotice([...unique, ...notices].join(' '), 'warning');
      else if (notices.length) showNotice(notices.join(' '), 'info');
    }

    function handle(value: NativeDrag) {
      if (value.phase === 'leave') { nativeDrop.current = null; nativeOver.current = null; setDropHover(null); return; }
      const at = value.position ? clientPoint(value.position) : null;
      if (value.phase === 'over') {
        if (at) nativeOver.current = { id: value.id, x: at.x, y: at.y };
        highlight(at ? worldFromClient(at.x, at.y) : null);
        return;
      }
      if (value.phase === 'pending') {
        if (at) nativeDrop.current = { id: value.id, x: at.x, y: at.y };
        return;
      }
      if (value.phase !== 'drop') return;
      const pendingDrop = nativeDrop.current;
      // The drop names the pending event; fall back to the last over position if it is missing.
      const last = pendingDrop && pendingDrop.id === value.id ? pendingDrop : nativeOver.current;
      nativeDrop.current = null; nativeOver.current = null; setDropHover(null);
      if (!last) return;
      const point = worldFromClient(last.x, last.y);
      if (!point) return;
      void dropFiles(point, { x: last.x, y: last.y }, value);
    }

    void getCurrentWindow().listen<NativeDrag>(IMAGE_DRAG_EVENT, event => handle(event.payload)).then(unlisten => { if (disposed) unlisten(); else stop = unlisten; });
    return () => { disposed = true; stop?.(); };
  }, []);

  return { dropHover, dropNotice, setDropNotice };
}
