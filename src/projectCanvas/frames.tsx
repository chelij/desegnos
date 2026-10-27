import { useEffect, useRef } from 'react';
import { useDraftText } from '../DraftText';
import { boundaryKey, type Boundary, type RequestBox } from '../projectCanvasGeometry';
import type { BoundaryKind, Element, Page, Project, Rect } from '../types';
import { elementOrigin, resolveForSize } from './sizes';
import { findElementIn } from './elementGeometry';

export type RequestPatch = { rect?: Rect; prompt?: string | (() => string) };
export type RequestChange = (patch: RequestPatch) => Promise<{ prompt: string }>;
export type BoundaryShift = { pageId: string; sizeId: string; rect: Rect } | null;
export type FrameDraft = { pageId: string; sizeId: string; rect: Rect } | null;

function walk(elements: Element[], origin: { x: number; y: number }, rows: RequestBox[], pageId: string, sizeId: string) {
  for (const element of elements) {
    rows.push({ pageId, elementId: element.id, sizeId, rect: { x: origin.x + element.rect.x, y: origin.y + element.rect.y, width: element.rect.width, height: element.rect.height } });
    walk(element.children, { x: origin.x + element.rect.x, y: origin.y + element.rect.y }, rows, pageId, sizeId);
  }
}

// Every layer of every size is a possible pointer endpoint, drawn at that size's canvas origin.
export function elementBoxes(project: Project, boundaries: Boundary[], moving: BoundaryShift): RequestBox[] {
  const boxes: RequestBox[] = [];
  for (const boundary of boundaries) {
    const page = project.pages.find(item => item.id === boundary.pageId);
    if (!page) continue;
    const origin = moving && boundaryKey(moving) === boundaryKey(boundary) ? moving.rect : boundary.rect;
    walk(resolveForSize(project, page, boundary.sizeId), origin, boxes, boundary.pageId, boundary.sizeId);
  }
  return boxes;
}

export const elementBoxesNow = (project: Project, boundaries: Boundary[]): RequestBox[] => elementBoxes(project, boundaries, null);

export function pendingRequests(project: Project): { pageId: string; elementId: string; element: Element; sizeId: string }[] {
  const rows: { pageId: string; elementId: string; element: Element; sizeId: string }[] = [];
  for (const page of project.pages) {
    const base = page.sizes[0];
    if (!base) continue;
    // A request label sits in the size it was written in, or in the base size.
    const walk = (elements: Element[]) => {
      for (const element of elements) {
        if (element.meta.pending) {
          const wanted = element.meta.pending.sizeId;
          const sizeId = wanted && page.sizes.some(size => size.id === wanted) ? wanted : base.id;
          const resolved = resolveForSize(project, page, sizeId);
          const shown = findElementIn(resolved, element.id);
          if (shown?.visible && elementOrigin(resolved, element.id)) {
            rows.push({ pageId: page.id, elementId: element.id, element: shown, sizeId });
          }
        }
        walk(element.children);
      }
    };
    walk(page.elements);
  }
  return rows;
}

// Request labels sit above their elements; the element renderer draws the box itself.
export function FrameLayer({ project, boundaries, moving, draftFrame }: {
  project: Project; boundaries: Boundary[]; moving: BoundaryShift; selection: unknown; draftFrame: FrameDraft; draft: Rect | null; boundaryKind: BoundaryKind; inverse: number;
}) {
  return <div className="frame-layer">
    {pendingRequests(project).map(({ pageId, element, sizeId }) => {
      const boundary = boundaries.find(item => item.pageId === pageId && item.sizeId === sizeId);
      if (!boundary) return null;
      const origin = moving && boundaryKey(moving) === boundaryKey(boundary) ? moving.rect : boundary.rect;
      const path = elementPathIn(project, pageId, sizeId, element.id);
      const local = path ?? element.rect;
      const shown = { x: origin.x + local.x, y: origin.y + local.y, width: element.rect.width, height: element.rect.height };
      const prompt = element.meta.pending?.prompt.trim() ?? '';
      return <div key={`${element.id}:${sizeId}`} className="frame-label-only" style={{ left: shown.x, top: shown.y, maxWidth: shown.width }} title={prompt || 'Empty frame'}>
        <span className="frame-label">{prompt || 'Description…'}</span>
      </div>;
    })}
    {draftFrame && <div className="frame-draft" style={{ left: draftFrame.rect.x, top: draftFrame.rect.y, width: draftFrame.rect.width, height: draftFrame.rect.height }} />}
  </div>;
}

/** The page-local top-left of a layer inside one size's resolved tree. */
function elementPathIn(project: Project, pageId: string, sizeId: string, elementId: string): { x: number; y: number } | null {
  const page = project.pages.find(item => item.id === pageId);
  return page ? elementOrigin(resolveForSize(project, page, sizeId), elementId) : null;
}

export function FrameRequestEditor({ page, element, screen, autoFocus, change, close }: {
  page: Page; element: Element; screen: { left: number; top: number; width: number; height: number }; autoFocus: boolean;
  change: RequestChange; close: () => void;
}) {
  const text = useRef<HTMLTextAreaElement>(null);
  const prompt = element.meta.pending?.prompt ?? '';
  const [draft, changeDraft, flushDraft] = useDraftText(prompt, (_value, latest) => change({ prompt: latest() }).then(updated => updated.prompt), 400);
  useEffect(() => { if (autoFocus) text.current?.focus({ preventScroll: true }); }, [autoFocus, element.id]);
  const width = Math.max(160, screen.width), height = 52;
  return <div className="frame-request-editor" style={{ left: screen.left, top: Math.max(0, screen.top - height - 4), width, height }} onPointerDown={event => event.stopPropagation()}>
    <textarea ref={text} aria-label={`Request for ${page.name}`} maxLength={16384} value={draft} placeholder="Describe what goes here…" spellCheck
      style={{ fontSize: 12 }}
      onChange={event => changeDraft(event.target.value)} onBlur={flushDraft}
      onKeyDown={event => {
        if (event.key !== 'Escape') return;
        event.preventDefault(); event.stopPropagation();
        flushDraft(); text.current?.blur(); close();
      }} />
  </div>;
}
