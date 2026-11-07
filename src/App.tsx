import { useEditor } from './session/useEditor';
import { PROJECT_COLOURS, errorMessage, zoomFromSlider, sliderFromZoom, RecoveryList, RenameProject, GridForm } from './editorShared';
import { CrashReportNotice, EditorCrashBoundary } from './CrashReport';
import { ProblemBanner, Notice, NoticeProvider, NoticeHost } from './ProblemBanner.js';
import { userError } from './userErrors.js';
import { native } from './api';
import { ToolStrip } from './EditorTools';
import { WorkspaceSurface, WorkspaceSurfaces, SurfaceHost } from './WorkspaceSurface';
import { ModalDialog } from './ModalDialog';
import { ProjectCanvas } from './ProjectCanvas';
import { RecoveryChoice } from './RecoveryBanner';
import { SaveConflictDialog } from './SaveConflict';
import brandLight from './assets/brand/desegnos-wordmark-light.svg';
import brandDark from './assets/brand/desegnos-wordmark-dark.svg';
import { ProjectLocationSettings } from './ProjectLocation';
import { StartPage } from './StartPage';
import { Notes } from './Notes';
import { MenuButton } from './Menus';
import './shell.css';
import { SHORTCUT_GROUPS, withShortcut } from './shortcuts';
import { Icon } from './icons';
import { StatusChip } from './StatusChip';
import { isNestedLayer } from './projectCanvas/links';
import { ElementProperties } from './projectCanvas/elementProperties';
import { findElementIn } from './projectCanvas/elementGeometry';
import {
  BoundaryProperties, CanvasLayers, EditorDock, PointerProperties, ProjectProperties, ReferenceProperties,
  RequestProperties, UntitledSaveForm,
} from './EditorDock';
import { AiSettings } from './AiSettings';
import { GenerationFeedback } from './Generation';
import { projectHasPreviewPages, showsOpener } from './visibleActions';
import { StyleGuide } from './StyleGuide';
import { ImportContext } from './ImportContext';
import { GenerationLog } from './GenerationLog';
import { HistoryPanel } from './HistoryPanel';
import { CommandPalette } from './CommandPalette';
import type { Pointer, PointerSource, PointerStateView, WorkspaceView } from './types';
import { ExportPanel } from './projectCanvas/exportPanel';
import { idlePlay, playIsActive } from './projectCanvas/play';
import { sizeLabel } from './projectCanvas/sizes';
import { behaviourCommand, behaviourFor, behaviourSourceKey } from './projectCanvas/behaviours';

const ignoreCanvasLayout = () => {};

export default function App({ initialView }: { initialView?: WorkspaceView } = {}) {
  return <NoticeProvider><WorkspaceSurfaces><EditorCrashBoundary><Editor initialView={initialView} /></EditorCrashBoundary></WorkspaceSurfaces></NoticeProvider>;
}

function Editor({ initialView }: { initialView?: WorkspaceView } = {}) {
  const {
    edits, exports,
    panel, openPanel, closePanel, togglePanel, saving, updateState, dismissUpdate, changeCanvasTool,
    showPalette, paletteCommands, setShowPalette, closeRequest, setCloseRequest, onStartPage, project, view,
    run, switchProjectTab, closeProjectTab, menuItems, resolvedTheme, setTheme,
    travel, panels, panelActions, play,
    togglePlay, aiConnected, showAiSettings,
    setShowAiSettings, generation, requestedAreas, working, generate,
    error, setError, problemAction, openFailure, setNotice, setOpenFailure, openRelease,
    notices, setNotices, dismissedRecovery, setDismissedRecovery,
    loading, load, pending, newUntitled, open, appSettings, changeShowStartPage, recentProjects, openRecent,
    forgetRecent, pinRecent, recover, dismissRecoveries, canvasTool, chooseTool, canvasZoom, canvasRef,
    boundaryKind, setBoundaryKind, requestToolShape, setRequestToolShape, armInsert, insertSignal,
    selectCanvasBoundary, createCanvasBoundary, addProjectBoundary, addNamedViewAt, saveCurrentView,
    canvasSelection, canvasFocus, canvasFocusSize, widthPreview, setWidthPreview, selectCanvasReference, selectCanvasPointer,
    selectCanvasRequest, clearCanvasSelection, removeCanvasSelection, elementToReference, referenceToElement, attachToRequest, deleteBoundary, pointerStateMap,
    reportCanvasZoom, selectCanvasElement, groupCanvasElements, ungroupCanvasElements, playElement, setPlay, showGenerationStatus, openImportDialog, editGenerationRequest,
    openGeneration, aiSettings, manageChatgptUsage, replyToGeneration, historyEpoch, accept, projectLocation,
    closeUntitledSave, saveUntitledProject, selectedPointer, selectedPointerState, keepPointerComment, keepPointerThis, mergePointerClash,
    selectedReferenceItem, selectedRequest, attachReferenceContext, selectedElements, selectedElementPageId,
    selectedElementTargets, renameCanvasElement, selectedElementRequest, setCanvasElementRequest,
    selectedBoundary, updateCanvasPage, mutate, updateCanvasPageSize,
    addCanvasPageSize, renameProject, showDotGrid, changeGridFlags, save, canvasBoundaries,
    moveCanvasElementTo, removeLinkedDefinition, views, addNamedView, renameNamedView, removeNamedView,
    selectedBounds, clashPointers, selectNextPointerClash, zoomPercent,
    recovery, changeProjectLocation, importContext,
    setCanvasSelection, setCanvasFocus, focusCanvasBehaviour, behaviourFocus, changeBrief,
    chooseSaveConflict, replacement, setReplacement, confirmation, setConfirmation,
    refreshAiSettings, updateAiSettings, aiPollingError, compactHistory,
  } = useEditor({ initialView });
  // Behaviours are ordinary pointers on one layer or boundary; the dock edits their comment.
  function clashActions(pointer: Pointer, state: PointerStateView | null) {
    const other = state?.clashWith.length ? (project?.pointers ?? []).find(item => item.id === state.clashWith[0]) ?? null : null;
    return {
      keepComment: () => ["codeChanged", "needsExporting"].includes(state?.state ?? '') ? exports.forgetPointer(pointer.id) : keepPointerComment(pointer.id),
      keepCode: () => run(() => exports.keepPointerCode(pointer.id)),
      keepThis: () => { if (other) keepPointerThis(pointer.id, other.id); },
      merge: () => { if (other) mergePointerClash(pointer, other); },
      reattach: () => canvasRef.current?.armPointer(pointer.id),
      remove: () => removeCanvasSelection([], [pointer.id]),
    };
  }
  function behaviourProps(source: PointerSource) {
    const pointer = project ? behaviourFor(project, source) : null;
    const state = pointer ? pointerStateMap[pointer.id] ?? null : null;
    const key = behaviourSourceKey(source);
    return {
      project: project!,
      pointer, state,
      commit: (comment: string) => {
        const command = behaviourCommand(pointer, source, comment);
        if (command) void mutate(command).catch(() => {});
      },
      focusToken: behaviourFocus?.source === key ? behaviourFocus.token : null,
      busy: pending > 0, aiConnected,
      actions: pointer ? clashActions(pointer, state) : undefined,
    };
  }
  const elementBehaviourSource = project && selectedElementPageId && selectedElements.length === 1
    ? { kind: 'element' as const, pageId: selectedElementPageId, elementId: selectedElements[0].id }
    : null;
  const boundaryBehaviourSource = project && selectedBoundary ? { kind: 'boundary' as const, pageId: selectedBoundary.id } : null;
  // Presenting a deck gives the slide the whole window; Esc or Stop brings the editor back.
  const presenting = !onStartPage && !!project && playIsActive(play) && project.pages.length > 0 && project.pages.every(page => page.output === 'slide');
  const playingPage = playIsActive(play) && project ? project.pages.find(page => page.id === play.boundary) ?? null : null;
  const playingSize = playingPage ? playingPage.sizes.find(size => size.id === canvasFocusSize) ?? playingPage.sizes[0] ?? null : null;
  const playingLabel = playingPage && playingSize ? `${playingPage.name} · ${sizeLabel(playingSize.width)}` : null;
  const briefOnly = requestedAreas === 0 && !!project && !!project.brief.purpose.trim() && project.pages[0]?.elements.length === 0;
  const nothingToGenerate = requestedAreas === 0 && !briefOnly;
  const previewAvailable = !onStartPage && !!project && projectHasPreviewPages(project);
  return <div className={presenting ? 'application editor-shell presenting' : 'application editor-shell'}>
    {showPalette && <CommandPalette commands={paletteCommands} close={() => setShowPalette(false)} />}
    {saving.kind !== 'idle' && saving.untitled && view?.untitled && project && <ModalDialog title="Save project" close={() => closeUntitledSave(null)}>
      <UntitledSaveForm key={project.id} project={project} location={projectLocation} busy={pending > 0}
        cancel={() => closeUntitledSave(null)}
        chooseLocation={name => native<string | null>('pick_project_save_path', { name })}
        save={saveUntitledProject} />
    </ModalDialog>}
    {closeRequest && <ModalDialog title="Close Desegnos?" close={() => { closeRequest.resolve(false); setCloseRequest(null); }}><p>{closeRequest.message}</p><div className="form-actions"><button className="primary" onClick={() => { closeRequest.resolve(false); setCloseRequest(null); }}>Keep editing</button><button className="danger" onClick={() => { closeRequest.resolve(true); setCloseRequest(null); }}>Close Desegnos</button></div></ModalDialog>}
    <header className="app-bar"><div className="wordmark" aria-label="Desegnos"><img className="brand-light" src={brandLight} alt="" /><img className="brand-dark" src={brandDark} alt="" /><span className="sr-only">Desegnos</span></div>
      {!onStartPage && project && !!view?.openProjects?.length && <nav className="project-tabs" aria-label="Open projects">
        {view.openProjects.map((entry, index) => <div className={entry.active ? 'project-tab active' : 'project-tab'} key={entry.id}>
          <button type="button" className="project-tab-name" title={entry.name} aria-current={entry.active ? 'page' : undefined} onClick={() => run(() => switchProjectTab(entry.id))}>
            <span className="project-dot" style={{ background: PROJECT_COLOURS[index % PROJECT_COLOURS.length] }} aria-hidden="true" />
            <span className="project-tab-label">{entry.name}</span>
            {entry.dirty && <span className="dirty-dot" aria-label="Unsaved changes" />}
          </button>
          <button type="button" className="project-tab-close icon-button sm" aria-label={`Close ${entry.name}`} title={`Close ${entry.name}`} onClick={() => run(() => closeProjectTab(entry.id))}><Icon name="close" size={12} /></button>
        </div>)}
      </nav>}
      <MenuButton label="Menu" ariaLabel="Editor menu" items={menuItems} />
      <div className="shell-primary-actions">
        {(onStartPage || !project) && <button type="button" className="theme-toggle icon-button" aria-label={resolvedTheme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'} title={resolvedTheme === 'dark' ? 'Light mode' : 'Dark mode'} onClick={() => setTheme(resolvedTheme === 'dark' ? 'light' : 'dark')}>{resolvedTheme === 'dark' ? <Icon name="sun" /> : <Icon name="moon" />}</button>}
        {project && <button type="button" className={`icon-button labeled${panel.kind === 'import' ? ' active' : ''}`} aria-pressed={panel.kind === 'import'} aria-label="Import" title="Import" onClick={() => togglePanel({ kind: 'import', source: null })}><Icon name="folder" /><span className="button-label">Import</span></button>}
        {!onStartPage && project && <><button aria-label="Undo last change" type="button" className="icon-button" disabled={!view?.canUndo} title={withShortcut('Undo', 'undo')} onClick={() => run(() => travel('undo_project'))}><Icon name="undo" /></button><button aria-label="Redo last change" type="button" className="icon-button" disabled={!view?.canRedo} title={withShortcut('Redo', 'redo')} onClick={() => run(() => travel('redo_project'))}><Icon name="redo" /></button>
          <button type="button" className={panel.kind === 'history' ? 'icon-button active' : 'icon-button'} aria-pressed={panel.kind === 'history'} aria-label="History and alternatives" title="History and alternatives" onClick={() => { togglePanel({ kind: 'history' }); if (panels.hidden) panelActions.toggleHidden(); if (panels.collapsed) panelActions.setCollapsed(false); }}><Icon name="history" /></button></>}
        {!onStartPage && project && <button type="button" className={`icon-button labeled${playIsActive(play) ? ' active' : ''}`} aria-pressed={playIsActive(play)} aria-label={playIsActive(play) ? 'Stop Play' : 'Play'} title={withShortcut(playIsActive(play) ? 'Stop Play' : 'Play', 'play')} onClick={() => playIsActive(play) ? setPlay(idlePlay) : togglePlay()}><Icon name={playIsActive(play) ? 'stop' : 'play'} /><span className="button-label">{playIsActive(play) ? 'Stop' : 'Play'}</span></button>}
        {previewAvailable && <button type="button" className="icon-button labeled" aria-label="Preview export" title={withShortcut('Preview export', 'preview')} onClick={() => exports.previewBoundary()}><Icon name="monitor" /><span className="button-label">Preview</span></button>}
        {!onStartPage && project && <button type="button" className={`icon-button labeled${panel.kind === 'export' ? ' active' : ''}`} aria-pressed={panel.kind === 'export'} aria-label="Export" title={withShortcut('Export', 'export')} onClick={() => panel.kind === 'export' ? closePanel('export') : exports.openPanel()}><Icon name="download" /><span className="button-label">Export</span></button>}
        {showsOpener({ available: !aiConnected, open: showAiSettings }) && <button className="ai-shortcut needs-connection" onClick={() => setShowAiSettings(true)} aria-label="AI settings" title="Connect AI to generate">Connect AI</button>}
        {showsOpener({ available: !onStartPage && !!project && !!(generation.job || generation.operation), open: panel.kind === 'log' }) && <button type="button" className="icon-button" aria-label="AI log" title="AI conversation" onClick={() => openPanel({ kind: 'log', focus: 0 })}><Icon name="note" /></button>}
        {!onStartPage && project && <button className={generation.canStop ? undefined : 'primary'} title={generation.canStop ? 'Stop generation (Esc)' : nothingToGenerate ? 'Describe at least one request first.' : working ? 'Wait for the current generation to finish.' : !aiConnected ? 'Connect AI to generate' : 'Generate the described changes'} disabled={!generation.canStop && (nothingToGenerate || working)} onClick={generation.canStop ? () => { void generation.cancel(); } : !aiConnected && !nothingToGenerate ? () => setShowAiSettings(true) : () => generate()}>{generation.canStop ? 'Stop' : 'Generate'}</button>}
      </div>
    </header>
    <SurfaceHost place="banner" />
    {onStartPage && <NoticeHost />}
    {error && <ProblemBanner message={userError(error)} dismiss={() => setError('')} action={problemAction} />}
    {openFailure && <Notice variant="warning" title="Couldn't open your browser" message={openFailure.message} action={{ label: 'Copy link', onClick: () => { const url = openFailure.url; void native('copy_text', { text: url }).then(() => setNotice('Link copied.')).catch(error => setError(errorMessage(error))); } }} dismiss={() => setOpenFailure(null)} />}
    {updateState.kind === 'done' && <Notice variant={updateState.result.variant} message={updateState.result.message} action={updateState.result.download ? { label: 'Download', onClick: () => { const url = updateState.result.download; if (url) void openRelease(url); } } : undefined} dismiss={dismissUpdate} />}
    {updateState.kind === 'failed' && <ProblemBanner message={userError(updateState.message)} dismiss={dismissUpdate} />}
    {notices.map(item => <Notice key={item.id} variant="info" message={item.message} undo={item.undo ? { label: 'Undo', onClick: item.undo } : undefined} dismiss={() => setNotices(items => items.filter(queued => queued.id !== item.id))} />)}
    <CrashReportNotice />
    {view?.recoveryError && !dismissedRecovery.includes(`error:${view.recoveryError}`) && <ProblemBanner message={userError(view.recoveryError)} dismiss={() => setDismissedRecovery(items => [...items, `error:${view.recoveryError}`])} />}
    {loading ? <main className="startup"><p role="status">Opening your workspace…</p></main> : !view ? <main className="startup"><h1>Open the Desegnos desktop application</h1><button onClick={() => { void load(); }}>Retry workspace connection</button></main> : !project || onStartPage ? <>
      <SurfaceHost place="start" />
      <StartPage busy={pending > 0} create={template => run(() => newUntitled(template))}
        openProject={() => run(open)} showStartPageOnLaunch={appSettings.showStartPage} changeShowStartPage={value => run(() => changeShowStartPage(value))}
        recentProjects={recentProjects} openRecent={path => run(() => openRecent(path))}
        forgetRecent={path => run(() => forgetRecent(path))} pinRecent={(path, pinned) => run(() => pinRecent(path, pinned))}
        recovery={view.recoveries.length ? <RecoveryList items={view.recoveries} busy={pending > 0} recover={id => { void recover(id).catch(() => {}); }} dismiss={dismissRecoveries} /> : null} />
    </> : <>
      <div className="editor-body">
      {!panels.hidden && <ToolStrip tool={canvasTool.kind} onTool={chooseTool}
        zoom={canvasZoom}
        onZoomStep={direction => canvasRef.current?.zoomStep(direction)}
        onZoom={value => canvasRef.current?.zoomTo(value)}
        onFit={() => canvasRef.current?.fit()}
        boundaryKind={boundaryKind} onBoundaryKind={setBoundaryKind}
        requestShape={requestToolShape} onRequestShape={setRequestToolShape}
        onInsert={armInsert} insertSignal={insertSignal} />}
      <div className="editor-work">
      <main className="workspace" id="project-panel">
      <div className="design-pane">
      {panels.hidden && <button type="button" className="show-panels" title={withShortcut('Show panels', 'hidePanels')} onClick={() => panelActions.toggleHidden()}>Show panels</button>}
      <SurfaceHost place="canvas" />
      <NoticeHost />
      {playIsActive(play) && !presenting && playingLabel && <div className="play-pill" role="status"><span className="play-pill-label">Playing {playingLabel}</span><button type="button" onClick={() => setPlay(idlePlay)}>Stop</button></div>}
      <ProjectCanvas ref={canvasRef} edits={edits} editProject={mutate} project={project} openPage={id => selectCanvasBoundary(id)} createBoundary={createCanvasBoundary} saveView={(slot, camera) => slot === null ? addNamedViewAt(camera) : saveCurrentView(slot, camera)} selection={canvasSelection} focused={canvasFocus} focusedSize={canvasFocusSize} widthPreview={widthPreview} selectBoundary={selectCanvasBoundary} selectReference={selectCanvasReference} selectPointer={selectCanvasPointer} selectRequest={id => selectCanvasRequest(id)} clearSelection={clearCanvasSelection} removeSelection={removeCanvasSelection} elementToReference={elementToReference} referenceToElement={referenceToElement} attachToRequest={(requestId, files, links) => run(() => attachToRequest(requestId, files, links))} deleteBoundary={deleteBoundary} focusBehaviour={focusCanvasBehaviour} pointerStates={pointerStateMap} tool={canvasTool.kind} insertKind={canvasTool.kind === 'insert' ? canvasTool.insertKind : null} onTool={changeCanvasTool} boundaryKind={boundaryKind} requestShape={requestToolShape} onZoom={reportCanvasZoom} onLayout={ignoreCanvasLayout}
        selectElement={selectCanvasElement} removeElements={items => removeCanvasSelection([], [], [], items)}
        groupElements={groupCanvasElements} ungroupElements={ungroupCanvasElements}
       
        play={play} onPlayBoundary={id => setPlay(current => ({ ...current, boundary: id }))} onPlayElement={playElement} onPlayExit={() => setPlay(idlePlay)}
        preview={exports.preview} previewClose={exports.closePreview} previewRefresh={exports.refreshPreview} generate={ids => generate(ids)} />
      {showGenerationStatus ? <GenerationFeedback controller={generation} project={project} openSettings={() => setShowAiSettings(true)} openImport={openImportDialog} openBrief={() => openPanel({ kind: 'notes', section: 'decisions' })} changeRequest={editGenerationRequest} details={showsOpener({ available: true, open: panel.kind === 'log' }) ? openGeneration : undefined} /> : null}
      </div>
      {panel.kind === 'log' && project && <GenerationLog key={`${project.id}:${panel.focus}`} controller={generation} job={generation.job} operation={generation.operation} error={generation.error} project={project} settings={aiSettings} openSettings={() => setShowAiSettings(true)} openImport={openImportDialog} openBrief={() => openPanel({ kind: 'notes', section: 'decisions' })} editRequest={editGenerationRequest} reply={replyToGeneration} close={() => closePanel('log')} />}
      <SurfaceHost place="panel" />
      </main>
      </div>
      <EditorDock state={panels} actions={panelActions}
        exportPanel={panel.kind === 'export' && project
          ? <ExportPanel openSettings={() => setShowAiSettings(true)} project={project} exports={exports} busy={pending > 0}
              selectBoundary={selectCanvasBoundary} setIncluded={(pageId, included) => updateCanvasPage(pageId, { included })}
              setOutput={(pageId, output) => void mutate({ type: 'set_page_output', page_id: pageId, output })}
              close={() => closePanel('export')} />
          : undefined}
        history={panel.kind === 'history' && project
          ? <HistoryPanel projectId={project.id} refreshKey={`${project.id}:${view?.dirty ? 'dirty' : 'saved'}:${view?.history?.version ?? 0}:${historyEpoch}`}
              apply={accept} notice={setNotice} error={setError} compact={() => run(compactHistory)} />
          : undefined}
        properties={<>
          {selectedPointer
          ? <PointerProperties pointer={selectedPointer} project={project} state={selectedPointerState} busy={pending > 0}
              aiConnected={aiConnected}
              change={comment => edits.updatePointer(selectedPointer.id, { comment })}
              actions={clashActions(selectedPointer, selectedPointerState)} />
          : selectedReferenceItem
          ? <ReferenceProperties item={selectedReferenceItem} busy={pending > 0} update={patch => edits.updateReference(selectedReferenceItem.id, patch)} remove={() => removeCanvasSelection([selectedReferenceItem.id])} />
          : selectedRequest
          ? <RequestProperties request={selectedRequest} project={project} busy={pending > 0} update={edits.updateRequest} remove={() => removeCanvasSelection([], [], [selectedRequest.id])} attach={attachReferenceContext} />
          : selectedElements.length && selectedElementPageId
            ? <ElementProperties styleCommand={mutate} project={project} page={project.pages.find(page => page.id === selectedElementPageId)!} sizeId={selectedElementTargets[0].sizeId} elements={selectedElements} busy={pending > 0}
                update={patches => void edits.updateElements(selectedElementPageId, patches)}
                rename={name => renameCanvasElement(selectedElementPageId, selectedElements[0].id, name)}
                link={() => canvasRef.current?.linkSelection()} unlink={() => canvasRef.current?.unlinkSelection()}
                canLink={selectedElementTargets.length >= 2}
                canUnlink={selectedElementTargets.length === 1 && (!!selectedElements[0]?.link || isNestedLayer(project, selectedElements[0]?.id ?? ''))}
                request={selectedElementRequest} setRequest={data => setCanvasElementRequest(selectedElements[0].id, data)}
                error={setError}
                behaviour={elementBehaviourSource ? behaviourProps(elementBehaviourSource) : undefined} />
          : selectedBoundary
            ? <BoundaryProperties selectSize={sizeId => selectCanvasBoundary(selectedBoundary.id, sizeId)} page={selectedBoundary} selectedSizeId={canvasFocusSize} widthPreview={widthPreview} previewWidth={setWidthPreview} busy={pending > 0} commit={patch => updateCanvasPage(selectedBoundary.id, patch)}
                adaptSize={sizeId => { void mutate({ type: 'adapt_size', page_id: selectedBoundary.id, size_id: sizeId }).catch(() => {}); }}
                updateSize={(sizeId, patch) => updateCanvasPageSize(selectedBoundary.id, sizeId, patch)}
                addSize={(preset, width, height) => addCanvasPageSize(selectedBoundary.id, preset, width, height)}
                removeSize={size => {
                  void mutate({ type: 'remove_page_size', page_id: selectedBoundary.id, size_id: size.id })
                    .then(() => setNotice(`Removed the ${size.label} size.`, () => { void travel('undo_project').catch(() => {}); }))
                    .catch(() => {});
                }}
                remove={() => deleteBoundary(selectedBoundary.id, canvasFocusSize)}
                behaviour={boundaryBehaviourSource ? behaviourProps(boundaryBehaviourSource) : undefined} />
            : <ProjectProperties project={project} busy={pending > 0} rename={renameProject} setGrid={showDotGrid} setGridSettings={changeGridFlags}
                save={view?.untitled ? () => run(() => save()) : undefined} />}
        </>}
        layers={<CanvasLayers styleCommand={mutate} project={project} boundaries={canvasBoundaries} selection={canvasSelection} focused={canvasFocus} selectBoundary={selectCanvasBoundary} selectReference={selectCanvasReference} selectPointer={selectCanvasPointer}
          selectRequest={id => selectCanvasRequest(id)} selectElement={selectCanvasElement} updateElements={(pageId, sizeId, patches) => void edits.updateElements(pageId, patches.map(patch => ({ ...patch, size_id: sizeId })))}
          moveElement={moveCanvasElementTo} removeLinked={removeLinkedDefinition}
          removeElements={ids => {
            const elementIds = ids.flatMap(id => { const page = project.pages.find(item => !!findElementIn(item.elements, id)); return page ? [{ pageId: page.id, elementId: id }] : []; });
            removeCanvasSelection([], [], [], elementIds);
          }}
          deleteBoundary={deleteBoundary} removeRequest={id => removeCanvasSelection([], [], [id])}
          removeReference={id => removeCanvasSelection([id])} removePointer={id => removeCanvasSelection([], [id])}
          addBoundary={() => run(addProjectBoundary)} views={views} addView={addNamedView} jumpToView={view => canvasRef.current?.goToView(view)} renameView={renameNamedView} removeView={removeNamedView} />} />
      </div>
      <footer className="status-bar"><span className="status-selection" title={view.path ?? undefined}>{canvasSelection.length > 1 && <span className="status-count">{canvasSelection.length} selected</span>}{selectedBounds && <span className="status-bounds" aria-label="Selection bounds"><span>{Math.round(selectedBounds.width)} × {Math.round(selectedBounds.height)}</span><span className="status-coordinate">X {Math.round(selectedBounds.x)} · Y {Math.round(selectedBounds.y)}</span></span>}</span><div className="canvas-status-actions">{clashPointers.length > 0 && <StatusChip tone="warning" className="clash-chip" onClick={selectNextPointerClash} role="status" ariaLabel="Select the next pointer clash">{clashPointers.length} clash{clashPointers.length === 1 ? '' : 'es'}</StatusChip>}<div className="status-zoom" role="group" aria-label="Canvas zoom controls"><button type="button" aria-label="Fit all boundaries" title={withShortcut('Fit all boundaries', 'fitAll')} onClick={() => canvasRef.current?.fit()}>Fit</button><input type="range" min={0} max={1000} step={1} value={sliderFromZoom(canvasZoom)} aria-label="Canvas zoom" title={`${zoomPercent}%`} onChange={event => canvasRef.current?.zoomTo(zoomFromSlider(Number(event.target.value)))} /><button type="button" className="status-zoom-value" aria-label="Zoom to 100%" title={withShortcut('Zoom to 100%', 'zoomReset')} onClick={() => canvasRef.current?.zoomTo(1)}>{zoomPercent}%</button></div>{showsOpener({ available: true, open: panel.kind === 'gridSettings' }) && <button onClick={() => openPanel({ kind: 'gridSettings' })} aria-label="Grid settings">{project.gridSettings.spacing}{project.gridSettings.unit === 'percent' ? '%' : ' px'} grid{project.gridSettings.snap ? ' · Snap' : ''}</button>}<StatusChip tone={pending > 0 ? 'neutral' : view.dirty ? 'warning' : 'success'} role="status" className="save-state">{pending > 0 ? 'Saving locally…' : view.dirty ? 'Unsaved' : 'Saved'}</StatusChip></div></footer>
    </>}
    {panel.kind === 'shortcuts' && <WorkspaceSurface place={onStartPage ? 'start' : 'panel'} title="Keyboard shortcuts" close={() => closePanel('shortcuts')}><div className="dialog-body shortcut-help">{SHORTCUT_GROUPS.map(group => <section key={group.group}><h3>{group.group}</h3><dl>{group.items.map(({ label, keys }) => <div key={label}><dt>{label}</dt><dd>{keys}</dd></div>)}</dl></section>)}<p className="field-help">Hold Space while dragging to pan. Alt+drag duplicates the selected layers. Escape stops Play, closes a preview, or cancels the current gesture.</p></div></WorkspaceSurface>}
    {recovery && <WorkspaceSurface place="banner" title="Unsaved changes" close={recovery.dismiss}><RecoveryChoice {...recovery} /></WorkspaceSurface>}
    {panel.kind === 'projectLocation' && <WorkspaceSurface place={onStartPage ? 'start' : 'panel'} title="Project location" close={() => closePanel('projectLocation')}><div className="dialog-body"><ProjectLocationSettings location={projectLocation} change={changeProjectLocation} /></div></WorkspaceSurface>}
    {panel.kind === 'recovery' && view && <WorkspaceSurface place={onStartPage ? 'start' : 'panel'} title={project ? 'Recover unsaved projects' : 'Unsaved work'} close={() => closePanel('recovery')}>
      <RecoveryList items={view.recoveries} busy={pending > 0} recover={id => { void recover(id).catch(() => {}); }} dismiss={dismissRecoveries} /></WorkspaceSurface>}
    {panel.kind === 'import' && <WorkspaceSurface place={onStartPage ? 'start' : 'panel'} title="Import" close={() => closePanel('import')}><ImportContext key={`${project?.id ?? 'new'}:${panel.source?.id ?? ''}`} initialSource={panel.source} busy={pending > 0} settings={aiSettings} openSettings={() => setShowAiSettings(true)} importInto={importContext} onPagesImported={result => { accept(result.workspace); setCanvasSelection([]); setCanvasFocus(null); setNotice(`Imported ${result.pages} ${result.pages === 1 ? 'page' : 'pages'} with ${result.layers} ${result.layers === 1 ? 'layer' : 'layers'}${result.warnings.length ? `; ${result.warnings.length} item${result.warnings.length === 1 ? '' : 's'} left out` : ''}.`); requestAnimationFrame(() => canvasRef.current?.fit()); }} close={() => closePanel('import')} /></WorkspaceSurface>}
    {panel.kind === 'styleGuide' && project && <WorkspaceSurface place={onStartPage ? 'start' : 'panel'} title="Style guide" close={() => closePanel('styleGuide')}><StyleGuide project={project} apply={command => mutate(command).then(() => undefined)} openBrief={() => openPanel({ kind: 'notes', section: 'decisions' })} /></WorkspaceSurface>}
    {panel.kind === 'notes' && project && <WorkspaceSurface place={onStartPage ? 'start' : 'panel'} title="Project brief" close={() => closePanel('notes')}><Notes project={project} initialSection={panel.section} reviewGeneration={project.latestGeneration?.jobId === generation.job?.jobId ? openGeneration : undefined} apply={command => mutate(command).then(() => undefined)} changeBrief={changeBrief} /></WorkspaceSurface>}
    {panel.kind === 'gridSettings' && project && <WorkspaceSurface place={onStartPage ? 'start' : 'panel'} title="Grid settings" close={() => closePanel('gridSettings')}><GridForm settings={project.gridSettings} showGrid={project.showGrid} busy={pending > 0} submit={async (settings, visible) => {
      if (visible !== project.showGrid) await mutate({ type: 'set_grid', enabled: visible });
      await mutate({ type: 'set_grid_settings', settings }); closePanel('gridSettings');
    }} /></WorkspaceSurface>}
    {panel.kind === 'rename' && project && <WorkspaceSurface place={onStartPage ? 'start' : 'panel'} title="Rename project" close={() => closePanel('rename')}><RenameProject name={project.name} submit={async name => { await mutate({ type: 'rename_project', name }); closePanel('rename'); }} /></WorkspaceSurface>}
    {saving.kind === 'conflict' && <SaveConflictDialog conflict={saving.conflict} choose={chooseSaveConflict} />}
    {replacement && <ModalDialog title="Save your changes?" close={() => setReplacement(null)}><p>Save “{project?.name}” before you {replacement.label}?</p><div className="form-actions">
      <button className="primary" disabled={pending > 0} onClick={() => { void (async () => { const saved = await save(); if (saved && !saved.dirty) { const run = replacement.run; setReplacement(null); await run(false); } })().catch(() => {}); }}>Save</button>
      <button className="danger" disabled={pending > 0} onClick={() => { const run = replacement.run; setReplacement(null); void run(true).catch(() => {}); }}>Discard</button><button onClick={() => setReplacement(null)}>Keep editing</button>
    </div></ModalDialog>}
    {confirmation && <WorkspaceSurface place="banner" title={confirmation.title} showHeader={false} close={() => setConfirmation(null)}><div className="inline-notice"><span>{confirmation.message}</span><button className={confirmation.confirmLabel ? undefined : 'danger'} disabled={pending > 0} onClick={() => { void confirmation.action().then(() => setConfirmation(null)).catch(() => {}); }}>{confirmation.confirmLabel ?? 'Delete'}</button><button onClick={() => setConfirmation(null)}>Cancel</button></div></WorkspaceSurface>}
    {showAiSettings && <ModalDialog title="AI settings" className="ai-settings-dialog" showClose close={() => setShowAiSettings(false)}><AiSettings current={aiSettings} refresh={refreshAiSettings} update={updateAiSettings} manageUsage={manageChatgptUsage} pollingError={aiPollingError} /></ModalDialog>}
  </div>;
}
