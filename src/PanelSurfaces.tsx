import { useState } from 'react';
import { RecoveryList } from './RecoveryList';
import { errorValue } from './errorMessage.js';
import { ImportContext } from './ImportContext';
import { Notes } from './Notes';
import { ProjectLocationSettings } from './ProjectLocation';
import { RecoveryChoice } from './RecoveryBanner';
import { StyleGuide } from './StyleGuide';
import { WorkspaceSurface } from './WorkspaceSurface';
import type { Editor } from './session/useEditor';
import { SHORTCUT_GROUPS } from './shortcuts';
import type { GridSettings } from './types';
import { GRID_PERCENT_MIN, GRID_PERCENT_MAX, GRID_PIXEL_MIN, GRID_PIXEL_MAX, MAX_NAME_CHARS } from './generated/limits';

/** The panels that open over the canvas, or over the start page when no canvas is open. */
export function PanelSurfaces({ editor }: { editor: Editor }) {
  const {
    project, view, onStartPage, panel, openPanel, closePanel, pending, recovery, projectLocation, changeProjectLocation, recover, dismissRecoveries,
    setError, edits, mutate, generation, openGeneration, changeBrief, aiSettings, setShowAiSettings, importContext, accept, setCanvasSelection,
    setCanvasFocus, setNotice, canvasRef } = editor;
  const place = onStartPage ? 'start' : 'panel';
  return <>
    {panel.kind === 'shortcuts' && <WorkspaceSurface place={place} title="Keyboard shortcuts" close={() => closePanel('shortcuts')}><div className="dialog-body shortcut-help">{SHORTCUT_GROUPS.map(group => <section key={group.group}><h3>{group.group}</h3><dl>{group.items.map(({ label, keys }) => <div key={label}><dt>{label}</dt><dd>{keys}</dd></div>)}</dl></section>)}<p className="field-help">Hold Space while dragging to pan. Alt+drag duplicates the selected layers. Escape stops Play, closes a preview, or cancels the current gesture.</p></div></WorkspaceSurface>}
    {recovery && <WorkspaceSurface place="banner" title="Unsaved changes" close={recovery.dismiss}><RecoveryChoice {...recovery} /></WorkspaceSurface>}
    {panel.kind === 'projectLocation' && <WorkspaceSurface place={place} title="Project location" close={() => closePanel('projectLocation')}><div className="dialog-body"><ProjectLocationSettings location={projectLocation} change={changeProjectLocation} /></div></WorkspaceSurface>}
    {panel.kind === 'recovery' && view && <WorkspaceSurface place={place} title={project ? 'Recover unsaved projects' : 'Unsaved work'} close={() => closePanel('recovery')}>
      <RecoveryList items={view.recoveries} busy={pending > 0} recover={id => { void recover(id).catch(error => setError(errorValue(error))); }} dismiss={ids => dismissRecoveries(ids).catch(error => setError(errorValue(error)))} /></WorkspaceSurface>}
    {panel.kind === 'import' && <WorkspaceSurface place={place} title="Import" close={() => closePanel('import')}><ImportContext key={`${project?.id ?? 'new'}:${panel.source?.id ?? ''}`} initialSource={panel.source} busy={pending > 0} settings={aiSettings} openSettings={() => setShowAiSettings(true)} importInto={importContext} onPagesImported={result => { accept(result.workspace); setCanvasSelection([]); setCanvasFocus(null); setNotice(`Imported ${result.pages} ${result.pages === 1 ? 'page' : 'pages'} with ${result.layers} ${result.layers === 1 ? 'layer' : 'layers'}${result.warnings.length ? `; ${result.warnings.length} item${result.warnings.length === 1 ? '' : 's'} left out` : ''}.`); canvasRef.current?.fit(); }} close={() => closePanel('import')} /></WorkspaceSurface>}
    {panel.kind === 'styleGuide' && project && <WorkspaceSurface place={place} title="Style guide" close={() => closePanel('styleGuide')}><StyleGuide project={project} edits={edits} openBrief={() => openPanel({ kind: 'notes', section: 'decisions' })} /></WorkspaceSurface>}
    {panel.kind === 'notes' && project && <WorkspaceSurface place={place} title="Project brief" close={() => closePanel('notes')}><Notes project={project} initialSection={panel.section} reviewGeneration={project.latestGeneration?.jobId === generation.job?.jobId ? openGeneration : undefined} edits={edits} changeBrief={changeBrief} /></WorkspaceSurface>}
    {panel.kind === 'gridSettings' && project && <WorkspaceSurface place={place} title="Grid settings" close={() => closePanel('gridSettings')}><GridForm settings={project.gridSettings} showGrid={project.showGrid} busy={pending > 0} submit={async (settings, visible) => {
      if (visible !== project.showGrid) { if (!await mutate({ type: 'set_grid', enabled: visible })) return; }
      if (await mutate({ type: 'set_grid_settings', settings })) closePanel('gridSettings');
    }} /></WorkspaceSurface>}
    {panel.kind === 'rename' && project && <WorkspaceSurface place={place} title="Rename project" close={() => closePanel('rename')}><RenameProject name={project.name} submit={async name => { if (await mutate({ type: 'rename_project', name })) closePanel('rename'); }} /></WorkspaceSurface>}
  </>;
}

function RenameProject({ name, submit }: { name: string; submit: (value: string) => Promise<void> }) {
  const [value, setValue] = useState(name);
  const [busy, setBusy] = useState(false);
  return <form className="dialog-body" onSubmit={event => { event.preventDefault(); setBusy(true); void submit(value.trim()).finally(() => setBusy(false)); }}><label>Project name<input autoFocus required maxLength={MAX_NAME_CHARS} value={value} onChange={event => setValue(event.target.value)} /></label><div className="form-actions"><button className="primary" disabled={busy || !value.trim()}>Rename project</button></div></form>;
}

function GridForm({ settings, showGrid, busy, submit }: { settings: GridSettings; showGrid: boolean; busy: boolean; submit: (settings: GridSettings, visible: boolean) => Promise<void> }) {
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
