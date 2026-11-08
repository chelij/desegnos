import { canvasView } from './projectCanvas/canvasView';
import { MIN_ZOOM } from './projectCanvas/camera';
import { useState } from 'react';
import { type MenuItem } from './Menus';
import { MAX_ZOOM } from './projectCanvas/camera';
import { type Boundary } from './projectCanvas/boundaryLayout';
import { type CanvasTarget } from './projectCanvas/canvasTargets';
import { elementWorldRect } from './projectCanvas/elementGeometry';
import { findElementIn } from './projectCanvas/elementFacts';
import { unionBounds as unionBounds } from './projectCanvas/camera';
import { type PaletteCommand } from './CommandPalette';
import type { GridSettings, Project, Rect, RecoveryInfo } from './types';
import { GRID_PERCENT_MIN, GRID_PERCENT_MAX, GRID_PIXEL_MIN, GRID_PIXEL_MAX, RESERVED_ROUTE_NAMES, MAX_NAME_CHARS } from './generated/limits';

export type Theme = 'system' | 'light' | 'dark';

export type Replacement = { label: string; run: (discard: boolean) => Promise<void> };

export type Confirmation = { title: string; message: string; action: () => Promise<void>; confirmLabel?: string };

export type ActionEvent = { preventDefault: () => void; key: string; code?: string; repeat?: boolean; target?: EventTarget | null };

export const NO_ACTION_EVENT: ActionEvent = { preventDefault: () => {}, key: '' };

export const PROJECT_COLOURS = ['#2f6fde', '#c2410c', '#267a36', '#ae3ec9', '#0e7490', '#b35c00'];
export const PROJECT_NAME_MAX_LENGTH = MAX_NAME_CHARS;

export function sizeBoundaries(project: Project): Boundary[] {
  return canvasView(project).boundaries;
}

export function ownerPageId(project: Project, elementId: string): string | null {
  return project.pages.find(page => !!findElementIn(page.elements, elementId))?.id ?? null;
}

function targetCanvasRect(project: Project, target: CanvasTarget): Rect | null {
  if (target.kind === 'reference') return project.reference.find(item => item.id === target.itemId)?.rect ?? null;
  if (target.kind !== 'element') return null;
  const page = project.pages.find(item => item.id === target.pageId);
  const size = page ? page.sizes.find(item => item.id === target.sizeId) ?? page.sizes[0] : null;
  if (!page || !size) return null;
  return elementWorldRect(project, page.id, target.elementId, { x: size.x, y: size.y });
}

export function selectionBounds(project: Project, selection: CanvasTarget[]): Rect | null {
  const rects = selection.flatMap(target => { const rect = targetCanvasRect(project, target); return rect ? [rect] : []; });
  return rects.length ? unionBounds(rects) : null;
}

export function menuPaletteCommands(items: MenuItem[], group = 'Menu'): PaletteCommand[] {
  return items.flatMap(item => {
    if (item.separator) return [];
    if (item.children?.length) return menuPaletteCommands(item.children, item.label);
    if (item.shortcut || item.disabled || !item.action) return [];
    return [{ id: `menu:${item.id}`, label: item.label, group, run: item.action }];
  });
}

// The slider spans the canvas' own zoom range, so it can always reach the same zoom as the tool strip.
export function zoomFromSlider(value: number) {
  const fraction = Math.max(0, Math.min(1, value / 1000));
  return Math.round(MIN_ZOOM * Math.pow(MAX_ZOOM / MIN_ZOOM, fraction) * 1e6) / 1e6;
}

export function sliderFromZoom(zoom: number) {
  const bounded = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, zoom));
  return Math.round(1000 * Math.log(bounded / MIN_ZOOM) / Math.log(MAX_ZOOM / MIN_ZOOM));
}

export function editedAgo(editedAtMs: number) {
  const minutes = Math.max(0, Math.floor((Date.now() - editedAtMs) / 60_000));
  return !Number.isFinite(minutes) ? 'Unsaved draft' : minutes < 1 ? 'Edited just now' : minutes < 60 ? `Edited ${minutes} min ago` : minutes < 1440 ? `Edited ${Math.floor(minutes / 60)} hr ago` : `Edited ${Math.floor(minutes / 1440)} days ago`;
}

export function RecoveryList({ items, busy, recover, dismiss }: { items: RecoveryInfo[]; busy: boolean; recover: (id: string) => void; dismiss: (ids: string[]) => Promise<void> }) {
  const [confirmIds, setConfirmIds] = useState<string[] | null>(null);
  const submit = (ids: string[]) => { void dismiss(ids).then(() => setConfirmIds(null)); };
  const requestDismiss = (selected: RecoveryInfo[]) => {
    const ids = selected.map(item => item.projectId);
    if (selected.some(item => !item.savedPath)) setConfirmIds(ids);
    else submit(ids);
  };
  const confirmation = <div className="recovery-confirmation"><p>Never saved — dismiss anyway?</p><div className="form-actions">
    <button className="danger" disabled={busy} onClick={() => confirmIds && submit(confirmIds)}>Dismiss</button>
    <button disabled={busy} onClick={() => setConfirmIds(null)}>Cancel</button>
  </div></div>;
  return <div className="recovery-list">{items.map(item => <section className="recovery-row" key={item.projectId} aria-label={`Recover ${item.name}`}>
    <div className="recovery-details"><strong>{item.name}</strong>
    <p className="muted" title={item.savedPath ?? undefined}>{editedAgo(item.editedAtMs)}{item.savedPath ? ` · ${item.savedPath.split(/[\\/]/).at(-1)}` : ' · Never saved'}</p></div>
    {confirmIds?.length === 1 && confirmIds[0] === item.projectId ? confirmation : <div className="form-actions">
      <button className="primary" disabled={busy} onClick={() => recover(item.projectId)}>Recover</button>
      <button disabled={busy} onClick={() => requestDismiss([item])}>Dismiss</button>
    </div>}
  </section>)}
    {items.length >= 2 && (confirmIds && confirmIds.length > 1 ? confirmation : <div className="form-actions"><button disabled={busy} onClick={() => requestDismiss(items)}>Dismiss all</button></div>)}
  </div>;
}

export function RenameProject({ name, submit }: { name: string; submit: (value: string) => Promise<void> }) {
  const [value, setValue] = useState(name);
  const [busy, setBusy] = useState(false);
  return <form className="dialog-body" onSubmit={event => { event.preventDefault(); setBusy(true); void submit(value.trim()).finally(() => setBusy(false)); }}><label>Project name<input autoFocus required maxLength={PROJECT_NAME_MAX_LENGTH} value={value} onChange={event => setValue(event.target.value)} /></label><div className="form-actions"><button className="primary" disabled={busy || !value.trim()}>Rename project</button></div></form>;
}

export function suggestedPagePath(name: string, paths: string[]) {
  let slug = name.trim().toLowerCase().replace(/[^\p{L}\p{N}]+/gu, '-').replace(/^-|-$/g, '') || 'page';
  if (RESERVED_ROUTE_NAMES.includes(slug)) slug += '-page';
  const base = `/${slug}`;
  let path = base; let suffix = 2;
  while (paths.some(existing => existing.toLowerCase() === path.toLowerCase())) path = `${base}-${suffix++}`;
  return path;
}

export function GridForm({ settings, showGrid, busy, submit }: { settings: GridSettings; showGrid: boolean; busy: boolean; submit: (settings: GridSettings, visible: boolean) => Promise<void> }) {
  const [draft, setDraft] = useState(settings);
  const [visible, setVisible] = useState(showGrid);
  const [saving, setSaving] = useState(false);
  const min = draft.unit === 'percent' ? GRID_PERCENT_MIN : GRID_PIXEL_MIN;
  const max = draft.unit === 'percent' ? GRID_PERCENT_MAX : GRID_PIXEL_MAX;
  const valid = Number.isFinite(draft.spacing) && draft.spacing >= min && draft.spacing <= max;
  return <form className="dialog-body grid-form" onSubmit={event => {
    event.preventDefault(); if (!valid) return; setSaving(true); void submit(draft, visible).finally(() => setSaving(false));
  }}>
    <label className="check-label"><input type="checkbox" checked={visible} onChange={event => setVisible(event.target.checked)} />Show grid</label>
    <div className="field-pair"><label>Grid spacing<input autoFocus type="number" required min={min} max={max} step="any" value={draft.spacing} onChange={event => setDraft({ ...draft, spacing: Number(event.target.value) })} /></label>
      <label>Unit<select value={draft.unit} onChange={event => {
        const unit = event.target.value as GridSettings['unit'];
        setDraft({ ...draft, unit, spacing: unit === 'percent' ? Math.max(GRID_PERCENT_MIN, Math.min(GRID_PERCENT_MAX, draft.spacing)) : Math.max(GRID_PIXEL_MIN, Math.min(GRID_PIXEL_MAX, draft.spacing)) });
      }}><option value="pixels">Pixels</option><option value="percent">Percent</option></select></label></div>
    <label className="check-label"><input type="checkbox" checked={draft.snap} onChange={event => setDraft({ ...draft, snap: event.target.checked })} />Snap requests to the grid</label>
    <div className="form-actions"><button className="primary" disabled={busy || saving || !valid}>Save grid settings</button></div>
  </form>;
}
