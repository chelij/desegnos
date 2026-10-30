import { useEffect, useRef, type PointerEvent as ReactPointerEvent } from 'react';
import { useDraftText } from '../DraftText';
import { Icon } from '../icons';
import { POINTER_ARROW_PX, POINTER_BREAK_PX, POINTER_COMMENT_ZOOM, POINTER_HIT_PX, pointerRoute, pointerVisible, screenPoint, type AutoLink, type Boundary, type CanvasCamera, type PointerRoute, type PointerWorld } from '../projectCanvasGeometry';
import type { Pointer, PointerStateView, Project, Rect } from '../types';
import { elementBoxes } from './requests';
import { isBehaviour, type BehaviourEntry } from './behaviours';

export type PointerEntry = { pointer: Pointer; route: PointerRoute };

// The layout a pointer is drawn against, including items and boundaries mid-drag.
export function pointerWorld(project: Project, boundaries: Boundary[], moving: { pageId: string; sizeId: string; rect: Rect } | null, _movingElements: Record<string, Rect>, movingItems: Record<string, Rect>): PointerWorld {
  return {
    requests: elementBoxes(project, boundaries, moving),
    boundaries: boundaries.map(item => ({ pageId: item.pageId, sizeId: item.sizeId, rect: moving && moving.sizeId === item.sizeId && moving.pageId === item.pageId ? moving.rect : item.rect })),
    reference: (project.reference ?? []).map(item => ({ itemId: item.id, rect: movingItems[item.id] ?? item.rect, url: item.kind.kind === 'link' ? item.kind.url : undefined })),
  };
}

// Routes every pointer once; hidden ones are dropped unless selected, so editing never hides.
// A behaviour points at its own layer, so it gets a badge instead of a line.
export function pointerEntries(pointers: Pointer[], world: PointerWorld, selected: Set<string>, viewport: Rect): PointerEntry[] {
  return pointers.flatMap(pointer => {
    if (isBehaviour(pointer)) return [];
    const route = pointerRoute(pointer, world);
    if (!route) return [];
    if (!selected.has(pointer.id) && !pointerVisible(route.source, route.target, viewport)) return [];
    return [{ pointer, route }];
  });
}

const midpoint = (route: { from: { x: number; y: number }; to: { x: number; y: number } }) => ({ x: (route.from.x + route.to.x) / 2, y: (route.from.y + route.to.y) / 2 });

function arrowPoints(from: { x: number; y: number }, to: { x: number; y: number }, size: number): string {
  const angle = Math.atan2(to.y - from.y, to.x - from.x);
  const back = { x: to.x - Math.cos(angle) * size, y: to.y - Math.sin(angle) * size };
  const side = { x: Math.sin(angle) * size * 0.5, y: -Math.cos(angle) * size * 0.5 };
  return `${to.x},${to.y} ${back.x + side.x},${back.y + side.y} ${back.x - side.x},${back.y - side.y}`;
}

// A small cross at the end that no longer has anything to point at.
function breakPath(from: { x: number; y: number }, to: { x: number; y: number }, size: number): string {
  const angle = Math.atan2(to.y - from.y, to.x - from.x), ux = Math.cos(angle), uy = Math.sin(angle), px = -uy, py = ux;
  const point = (back: number, side: number) => `${to.x - ux * size * back + px * size * side},${to.y - uy * size * back + py * size * side}`;
  return `M ${point(0.2, 0.8)} L ${point(1.1, -0.3)} M ${point(0.2, -0.8)} L ${point(1.1, 0.3)}`;
}

function pointerVisual(pointer: Pointer, state: PointerStateView | undefined): { state: string; clash: boolean; dashed: boolean; reference: boolean } {
  const value = state?.state ?? (pointer.wiring ? 'wired' : 'unwired');
  const clash = value === 'contradicts' || value === 'codeChanged';
  // A reference reads its target, so it stays thin and dashed; a change points at what it edits.
  const reference = pointer.kind !== 'change';
  return { state: value, clash, reference,
    dashed: reference || value === 'unwired' || value === 'needsGenerating' || value === 'needsExporting' || value === 'broken' };
}

export function PointerLines({ entries, autoLinks, draft, camera, viewport, selected, pointerStates, enabled, onSelect }: {
  entries: PointerEntry[]; autoLinks: AutoLink[]; draft: { from: { x: number; y: number }; to: { x: number; y: number } } | null;
  camera: CanvasCamera; viewport: Rect; selected: Set<string>; pointerStates: Record<string, PointerStateView>; enabled: boolean; onSelect: (pointerId: string) => void;
}) {
  const inverse = 1 / camera.zoom;
  const visibleAuto = autoLinks.filter(line => pointerVisible(line.source, line.target, viewport));
  return <svg className="pointer-layer" width="1" height="1" aria-hidden="true">
    {visibleAuto.map(line => <path key={`${line.fromPageId}:${line.toPageId}`} className="pointer-auto" d={`M ${line.route.from.x} ${line.route.from.y} L ${line.route.to.x} ${line.route.to.y}`} strokeWidth={1.2 * inverse} />)}
    {entries.map(({ pointer, route }) => {
      const chosen = selected.has(pointer.id);
      const visual = pointerVisual(pointer, pointerStates[pointer.id]);
      const state = route.broken ? 'broken' : visual.state;
      const classes = `pointer-line ${state} ${visual.reference ? 'reference' : 'change'}${chosen ? ' selected' : ''}${visual.clash ? ' clash' : ''}`;
      const arrow = route.target && !visual.reference && route.brokenEnd !== 'to';
      const breakEnd = route.brokenEnd === 'to' ? route.to : route.brokenEnd === 'from' ? route.from : null;
      return <g key={pointer.id} className={classes}>
        <path className="pointer-stroke" d={`M ${route.from.x} ${route.from.y} L ${route.to.x} ${route.to.y}`} fill="none"
          strokeWidth={(visual.reference ? 1 : 1.4) * inverse} strokeDasharray={visual.dashed || route.broken ? `${5 * inverse} ${4 * inverse}` : undefined} />
        {arrow && <polygon className="pointer-arrow" points={arrowPoints(route.from, route.to, POINTER_ARROW_PX * inverse)} />}
        {breakEnd && <path className="pointer-break" d={breakPath(route.from, route.to, POINTER_BREAK_PX * inverse)} fill="none" strokeWidth={1.4 * inverse} />}
        <path className="pointer-hit" d={`M ${route.from.x} ${route.from.y} L ${route.to.x} ${route.to.y}`} fill="none" stroke="transparent"
          strokeWidth={POINTER_HIT_PX * 2 * inverse} pointerEvents={enabled ? 'stroke' : 'none'}
          onPointerDown={event => { if (!enabled) return; event.preventDefault(); event.stopPropagation(); onSelect(pointer.id); }} />
      </g>;
    })}
    {draft && <path className="pointer-draft" d={`M ${draft.from.x} ${draft.from.y} L ${draft.to.x} ${draft.to.y}`} fill="none" strokeWidth={1.4 * inverse} strokeDasharray={`${5 * inverse} ${4 * inverse}`} />}
  </svg>;
}

export function PointerComments({ entries, camera, selected, pointerStates, interactive, onSelect, change, changeKind, autoFocusId, close }: {
  entries: PointerEntry[]; camera: CanvasCamera; selected: Set<string>; pointerStates: Record<string, PointerStateView>; interactive: boolean; onSelect: (pointerId: string) => void;
  change: (pointerId: string, comment: string) => Promise<unknown>;
  changeKind: (pointerId: string, kind: Pointer['kind']) => Promise<unknown>; autoFocusId: string | null; close: () => void;
}) {
  const collapsed = camera.zoom < POINTER_COMMENT_ZOOM;
  return <>
    {entries.map(({ pointer, route }) => {
      const point = screenPoint(midpoint(route), camera);
      if (selected.has(pointer.id)) return <PointerCommentEditor key={pointer.id} pointer={pointer} screen={point} autoFocus={autoFocusId === pointer.id}
        change={comment => change(pointer.id, comment)} changeKind={kind => changeKind(pointer.id, kind)} close={close} />;
      const text = pointer.comment.trim();
      const visual = pointerVisual(pointer, pointerStates[pointer.id]);
      return <button key={pointer.id} type="button" className={`pointer-comment-chip${collapsed ? ' collapsed' : ''}${route.broken ? ' broken' : ''}${visual.clash ? ' clash' : ''}${(visual.state === 'needsGenerating' || visual.state === 'needsExporting') ? ' generating' : ''}${interactive ? '' : ' inert'}`}
        style={{ left: point.x, top: point.y }} title={text || 'Add a comment'} onPointerDown={event => event.stopPropagation()}
        onClick={() => onSelect(pointer.id)}>
        {collapsed ? <Icon name="link" size={12} /> : <span className="pointer-comment-text">{text || 'Add a comment…'}</span>}
        {collapsed && text && <span className="pointer-comment-popover">{text}</span>}
      </button>;
    })}
  </>;
}

// Native screen pixels: the comment never lives inside the transformed layer.
function PointerCommentEditor({ pointer, screen, autoFocus, change, changeKind, close }: {
  pointer: Pointer; screen: { x: number; y: number }; autoFocus: boolean;
  change: (comment: string) => Promise<unknown>; changeKind: (kind: Pointer['kind']) => Promise<unknown>; close: () => void;
}) {
  const text = useRef<HTMLTextAreaElement>(null);
  const [draft, changeDraft, flushDraft] = useDraftText(pointer.comment, (_value, latest) => change(latest()).then(() => latest()), 400);
  const kind = pointer.kind ?? 'reference';
  useEffect(() => { if (autoFocus) text.current?.focus({ preventScroll: true }); }, [autoFocus, pointer.id]);
  return <div className="pointer-comment-editor" style={{ left: screen.x, top: screen.y, height: 'auto', flexDirection: 'column' }} onPointerDown={event => event.stopPropagation()}>
    <div className="segmented" role="group" aria-label="Pointer kind" style={{ flex: 'none', alignSelf: 'flex-start', margin: 1 }}>
      <button type="button" aria-pressed={kind === 'reference'} onClick={() => void changeKind('reference')}>Reference</button>
      <button type="button" aria-pressed={kind === 'change'} onClick={() => void changeKind('change')}>Change</button>
    </div>
    <textarea ref={text} aria-label="Pointer comment" maxLength={16384} value={draft} placeholder="What should this pointer do?" spellCheck style={{ flex: 'none', width: '100%', height: 56 }}
      onChange={event => changeDraft(event.target.value)} onBlur={flushDraft}
      onKeyDown={event => {
        if (event.key !== 'Escape') return;
        event.preventDefault(); event.stopPropagation();
        flushDraft(); text.current?.blur(); close();
      }} />
  </div>;
}

// Screen pixels at the top-right corner of each layer or boundary that has a behaviour.
export function BehaviourBadges({ entries, camera, viewport, pointerStates, enabled, onSelect }: {
  entries: BehaviourEntry[]; camera: CanvasCamera; viewport: Rect;
  pointerStates: Record<string, PointerStateView>; enabled: boolean; onSelect: (pointer: Pointer) => void;
}) {
  return <>
    {entries.map(({ pointer, rect }) => {
      if (!pointerVisible(rect, rect, viewport)) return null;
      const point = screenPoint({ x: rect.x + rect.width, y: rect.y }, camera);
      const visual = pointerVisual(pointer, pointerStates[pointer.id]);
      const clash = visual.clash || visual.state === 'broken';
      const text = pointer.comment.trim();
      return <button key={pointer.id} type="button" data-behaviour-badge={pointer.id}
        className={`behaviour-badge${visual.state === 'wired' ? ' built' : ''}${clash ? ' clash' : ''}`}
        style={{ left: point.x - 4, top: point.y + 4 }} title={text || 'Behaviour'} aria-label={text || 'Behaviour'}
        onPointerDown={event => event.stopPropagation()}
        onClick={event => { event.stopPropagation(); if (enabled) onSelect(pointer); }}>
        <Icon name="code" size={12} />
      </button>;
    })}
  </>;
}

export function PointerHandles({ entries, camera, selectedId, armed, onStart }: {
  entries: PointerEntry[]; camera: CanvasCamera; selectedId: string | null;
  armed: { pointerId: string; end: 'from' | 'to' } | null;
  onStart: (event: ReactPointerEvent<HTMLButtonElement>, pointerId: string, end: 'from' | 'to') => void;
}) {
  const entry = entries.find(item => item.pointer.id === selectedId);
  if (!entry) return null;
  return <>
    {(['from', 'to'] as const).map(end => {
      const point = screenPoint(end === 'from' ? entry.route.from : entry.route.to, camera);
      const active = armed?.pointerId === entry.pointer.id && armed.end === end;
      const broken = entry.route.brokenEnd === end;
      return <button key={end} type="button" data-pointer-end={end} data-pointer-id={entry.pointer.id}
        className={`pointer-end-handle ${end}${active ? ' armed' : ''}${broken ? ' broken' : ''}`} style={{ left: point.x - 5, top: point.y - 5 }}
        aria-label={end === 'from' ? 'Move pointer start' : 'Move pointer end'} title="Drag to reattach"
        onPointerDown={event => { event.preventDefault(); event.stopPropagation(); onStart(event, entry.pointer.id, end); }} />;
    })}
  </>;
}
