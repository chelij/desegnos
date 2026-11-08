import { GridForm, RecoveryList, RenameProject, errorMessage } from './editorShared';
import { ImportContext } from './ImportContext';
import { Notes } from './Notes';
import { ProjectLocationSettings } from './ProjectLocation';
import { RecoveryChoice } from './RecoveryBanner';
import { StyleGuide } from './StyleGuide';
import { WorkspaceSurface } from './WorkspaceSurface';
import type { Editor } from './session/useEditor';
import { SHORTCUT_GROUPS } from './shortcuts';

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
      <RecoveryList items={view.recoveries} busy={pending > 0} recover={id => { void recover(id).catch(error => setError(errorMessage(error))); }} dismiss={ids => dismissRecoveries(ids).catch(error => setError(errorMessage(error)))} /></WorkspaceSurface>}
    {panel.kind === 'import' && <WorkspaceSurface place={place} title="Import" close={() => closePanel('import')}><ImportContext key={`${project?.id ?? 'new'}:${panel.source?.id ?? ''}`} initialSource={panel.source} busy={pending > 0} settings={aiSettings} openSettings={() => setShowAiSettings(true)} importInto={importContext} onPagesImported={result => { accept(result.workspace); setCanvasSelection([]); setCanvasFocus(null); setNotice(`Imported ${result.pages} ${result.pages === 1 ? 'page' : 'pages'} with ${result.layers} ${result.layers === 1 ? 'layer' : 'layers'}${result.warnings.length ? `; ${result.warnings.length} item${result.warnings.length === 1 ? '' : 's'} left out` : ''}.`); requestAnimationFrame(() => canvasRef.current?.fit()); }} close={() => closePanel('import')} /></WorkspaceSurface>}
    {panel.kind === 'styleGuide' && project && <WorkspaceSurface place={place} title="Style guide" close={() => closePanel('styleGuide')}><StyleGuide project={project} edits={edits} openBrief={() => openPanel({ kind: 'notes', section: 'decisions' })} /></WorkspaceSurface>}
    {panel.kind === 'notes' && project && <WorkspaceSurface place={place} title="Project brief" close={() => closePanel('notes')}><Notes project={project} initialSection={panel.section} reviewGeneration={project.latestGeneration?.jobId === generation.job?.jobId ? openGeneration : undefined} edits={edits} changeBrief={changeBrief} /></WorkspaceSurface>}
    {panel.kind === 'gridSettings' && project && <WorkspaceSurface place={place} title="Grid settings" close={() => closePanel('gridSettings')}><GridForm settings={project.gridSettings} showGrid={project.showGrid} busy={pending > 0} submit={async (settings, visible) => {
      if (visible !== project.showGrid) { if (!await mutate({ type: 'set_grid', enabled: visible })) return; }
      if (await mutate({ type: 'set_grid_settings', settings })) closePanel('gridSettings');
    }} /></WorkspaceSurface>}
    {panel.kind === 'rename' && project && <WorkspaceSurface place={place} title="Rename project" close={() => closePanel('rename')}><RenameProject name={project.name} submit={async name => { if (await mutate({ type: 'rename_project', name })) closePanel('rename'); }} /></WorkspaceSurface>}
  </>;
}
