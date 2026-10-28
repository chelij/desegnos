import { canvasView } from './projectCanvas/canvasView';
import { useState } from 'react';
import { type MenuItem } from './Menus';
import { layoutBoundaries, type Boundary, type CanvasTarget } from './projectCanvasGeometry';
import { elementWorldRect, findElementIn, unionRects } from './projectCanvas/elementGeometry';
import { type PaletteCommand } from './CommandPalette';
import type { GridSettings, Page, Project, Rect, RecoveryInfo } from './types';
import { resolveForSize } from './projectCanvas/sizes';
import { elementBoundsHeight } from './projectCanvas/elementGeometry';

export type Theme = 'system' | 'light' | 'dark';

export type Replacement = { label: string; run: (discard: boolean) => Promise<void> };

export type UntitledSaveRequest = { name: string };

export type Confirmation = { title: string; message: string; action: () => Promise<void>; confirmLabel?: string };

export type ActionEvent = { preventDefault: () => void; key: string; code?: string; repeat?: boolean; target?: EventTarget | null };

export const NO_ACTION_EVENT: ActionEvent = { preventDefault: () => {}, key: '' };

export const PROJECT_COLOURS = ['#2f6fde', '#c2410c', '#267a36', '#ae3ec9', '#0e7490', '#b35c00'];

export function errorMessage(error: unknown) { if (error && typeof error === 'object' && 'message' in error) return String((error as { message: unknown }).message); return String(error ?? ''); }

export function sizeName(width: number): string {
  return width >= 1024 ? 'Desktop' : width >= 700 ? 'Tablet' : 'Phone';
}

export function sizeBoundaries(project: Project): Boundary[] {
  return canvasView(project).boundaries;
}

export function initialZoom(width: number, height = 900) { const fit = width > 600 ? (window.innerWidth - 96) / width : (window.innerHeight - 240) / height; return Math.max(0.1, Math.min(1, fit)); }

export function readRulerUnit(): 'pixels' | 'percent' { try { return localStorage.getItem('desegnos.ruler-unit') === 'percent' ? 'percent' : 'pixels'; } catch { return 'pixels'; } }

export function readTheme(): Theme {
  try {
    const saved = localStorage.getItem('desegnos.editor-theme');
    return saved === 'light' || saved === 'dark' ? saved : 'system';
  }
  catch { return 'system'; }
}

export function ownerPageId(project: Project, elementId: string): string | null {
  return project.pages.find(page => !!findElementIn(page.elements, elementId))?.id ?? null;
}

export function targetCanvasRect(project: Project, target: CanvasTarget): Rect | null {
  if (target.kind === 'reference') return project.reference.find(item => item.id === target.itemId)?.rect ?? null;
  if (target.kind !== 'element') return null;
  const page = project.pages.find(item => item.id === target.pageId);
  const size = page ? page.sizes.find(item => item.id === target.sizeId) ?? page.sizes[0] : null;
  if (!page || !size) return null;
  return elementWorldRect(project, page.id, target.elementId, { x: size.x, y: size.y });
}

export function selectionBounds(project: Project, selection: CanvasTarget[]): Rect | null {
  const rects = selection.flatMap(target => { const rect = targetCanvasRect(project, target); return rect ? [rect] : []; });
  return rects.length ? unionRects(rects) : null;
}

export function menuPaletteCommands(items: MenuItem[], group = 'Menu'): PaletteCommand[] {
  return items.flatMap(item => {
    if (item.separator) return [];
    if (item.children?.length) return menuPaletteCommands(item.children, item.label);
    if (item.shortcut || item.disabled || !item.action) return [];
    return [{ id: `menu:${item.id}`, label: item.label, group, run: item.action }];
  });
}

export function zoomFromSlider(value: number) {
  return Math.round(10 * Math.pow(40, Math.max(0, Math.min(1, value / 1000))) * 10) / 1000;
}

export function sliderFromZoom(zoom: number) {
  const percent = Math.max(10, Math.min(400, zoom * 100));
  return Math.round(1000 * Math.log(percent / 10) / Math.log(40));
}

export function editedAgo(editedAtMs: number) {
  const minutes = Math.max(0, Math.floor((Date.now() - editedAtMs) / 60_000));
  return !Number.isFinite(minutes) ? 'Unsaved draft' : minutes < 1 ? 'Edited just now' : minutes < 60 ? `Edited ${minutes} min ago` : minutes < 1440 ? `Edited ${Math.floor(minutes / 60)} hr ago` : `Edited ${Math.floor(minutes / 1440)} days ago`;
}

export function RecoveryList({ items, busy, recover, dismiss }: { items: RecoveryInfo[]; busy: boolean; recover: (id: string) => void; dismiss: (ids: string[]) => Promise<void> }) {
  const [confirmIds, setConfirmIds] = useState<string[] | null>(null);
  const submit = (ids: string[]) => { void dismiss(ids).then(() => setConfirmIds(null)).catch(() => {}); };
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
  return <form className="dialog-body" onSubmit={event => { event.preventDefault(); setBusy(true); void submit(value.trim()).catch(() => {}).finally(() => setBusy(false)); }}><label>Project name<input autoFocus required maxLength={120} value={value} onChange={event => setValue(event.target.value)} /></label><div className="form-actions"><button className="primary" disabled={busy || !value.trim()}>Rename project</button></div></form>;
}

export function canvasPageName(pages: Page[]) {
  const names = new Set(pages.map(page => page.name.trim().toLowerCase()));
  let number = 1;
  while (names.has(`page ${number}`)) number++;
  return `Page ${number}`;
}

export function suggestedPagePath(name: string, paths: string[]) {
  let slug = name.trim().toLowerCase().replace(/[^\p{L}\p{N}]+/gu, '-').replace(/^-|-$/g, '') || 'page';
  if (/^(assets|src|node_modules|con|prn|aux|nul|com[1-9]|lpt[1-9])$/.test(slug)) slug += '-page';
  const base = `/${slug}`;
  let path = base; let suffix = 2;
  while (paths.some(existing => existing.toLowerCase() === path.toLowerCase())) path = `${base}-${suffix++}`;
  return path;
}

export function GridForm({ settings, showGrid, busy, submit, rulerUnit, changeRulerUnit }: { rulerUnit: 'pixels' | 'percent'; changeRulerUnit: (value: 'pixels' | 'percent') => void; settings: GridSettings; showGrid: boolean; busy: boolean; submit: (settings: GridSettings, visible: boolean) => Promise<void> }) {
  const [draft, setDraft] = useState(settings);
  const [visible, setVisible] = useState(showGrid);
  const [saving, setSaving] = useState(false);
  const min = draft.unit === 'percent' ? 0.25 : 1;
  const max = draft.unit === 'percent' ? 100 : 512;
  const valid = Number.isFinite(draft.spacing) && draft.spacing >= min && draft.spacing <= max;
  return <form className="dialog-body grid-form" onSubmit={event => {
    event.preventDefault(); if (!valid) return; setSaving(true); void submit(draft, visible).catch(() => {}).finally(() => setSaving(false));
  }}>
    <label className="check-label"><input type="checkbox" checked={visible} onChange={event => setVisible(event.target.checked)} />Show grid</label>
    <div className="field-pair"><label>Grid spacing<input autoFocus type="number" required min={min} max={max} step="any" value={draft.spacing} onChange={event => setDraft({ ...draft, spacing: Number(event.target.value) })} /></label>
      <label>Unit<select value={draft.unit} onChange={event => {
        const unit = event.target.value as GridSettings['unit'];
        setDraft({ ...draft, unit, spacing: unit === 'percent' ? Math.max(0.25, Math.min(100, draft.spacing)) : Math.max(1, Math.min(512, draft.spacing)) });
      }}><option value="pixels">Pixels</option><option value="percent">Percent</option></select></label></div>
    <label className="check-label"><input type="checkbox" checked={draft.snap} onChange={event => setDraft({ ...draft, snap: event.target.checked })} />Snap requests to the grid</label>
    <label className="check-label"><input type="checkbox" checked={draft.rulers} onChange={event => setDraft({ ...draft, rulers: event.target.checked })} />Show rulers</label>
    <label>Ruler units<select value={rulerUnit} onChange={event => changeRulerUnit(event.target.value as 'pixels' | 'percent')}><option value="pixels">Pixels</option><option value="percent">Percent</option></select></label>
    <div className="form-actions"><button className="primary" disabled={busy || saving || !valid}>Save grid settings</button></div>
  </form>;
}
