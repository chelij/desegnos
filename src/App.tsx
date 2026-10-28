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
import { showsOpener } from './visibleActions';
import { StyleGuide } from './StyleGuide';
import { ImportContext } from './ImportContext';
import { WebsiteImport } from './WebsiteImport';
import { GenerationLog } from './GenerationLog';
import { HistoryPanel } from './HistoryPanel';
import { CommandPalette } from './CommandPalette';
import type { WorkspaceView } from './types';
import { ExportPanel } from './projectCanvas/exportPanel';
import { idlePlay, playIsActive } from './projectCanvas/play';

const ignoreCanvasLayout = () => {};

export default function App({ initialView }: { initialView?: WorkspaceView } = {}) {
  return <NoticeProvider><WorkspaceSurfaces><EditorCrashBoundary><Editor initialView={initialView} /></EditorCrashBoundary></WorkspaceSurfaces></NoticeProvider>;
}

function Editor({ initialView }: { initialView?: WorkspaceView } = {}) {
  const {
    showPalette, paletteCommands, setShowPalette, closeRequest, setCloseRequest, onStartPage, project, view,
    run, switchProjectTab, closeProjectTab, menuItems, resolvedTheme, setTheme, switchAppearance, showImport,
    setImportSource, setShowImport, travel, showHistory, setShowHistory, panels, panelActions, play,
    togglePlay, previewSelectedBoundary, showExport, openExportPanel, aiConnected, showAiSettings,
    setShowAiSettings, generation, showLog, setShowLog, requestedAreas, pendingImages, working, generate,
    error, setError, problemAction, openFailure, setNotice, setOpenFailure, updateResult, openRelease,
    setUpdateResult, updateError, setUpdateError, notices, setNotices, dismissedRecovery, setDismissedRecovery,
    loading, load, pending, newUntitled, open, appSettings, changeShowStartPage, recentProjects, openRecent,
    forgetRecent, pinRecent, recover, dismissRecoveries, canvasTool, chooseTool, canvasZoom, canvasRef,
    boundaryKind, setBoundaryKind, requestToolShape, setRequestToolShape, armInsert, insertSignal,
    selectCanvasBoundary, createCanvasBoundary, saveCanvasPlacements, addNamedViewAt, saveCurrentView,
    canvasSelection, canvasFocus, canvasFocusSize, selectCanvasReference, selectCanvasPointer,
    selectCanvasRequest, clearCanvasSelection, removeCanvasSelection, addCanvasReference,
    updateCanvasReference, elementToReference, referenceToElement, attachToRequest, addCanvasRequest,
    updateCanvasRequest, deleteBoundary, addCanvasPointer, updateCanvasPointer, pointerStateMap, insertKind,
    setCanvasTool, setInsertKind, reportCanvasZoom, selectCanvasElement, insertCanvasElements,
    updateCanvasElements, moveCanvasElements, groupCanvasElements, ungroupCanvasElements, linkCanvasElements,
    unlinkCanvasElements, pasteLinkedInstances, playElement, setPlay, preview, closePreview, refreshPreview,
    showGenerationStatus, openImportDialog, setShowNotes, editGenerationRequest, requestEditorIsOpen,
    openGeneration, draftCount, logFocus, aiSettings, manageChatgptUsage, replyToGeneration, exportDestination,
    exportJob, exportSending, exportError, chooseExportDestination, runExport, sendExport, cancelExport,
    openExportFolder, openInBrowser, setShowExport, historyEpoch, accept, untitledSave, projectLocation,
    closeUntitledSave, saveUntitledProject, selectedPointer, selectedPointerState, forgetExportPointer,
    keepPointerComment, keepPointerCode, selectedPointerOther, keepPointerThis, mergePointerClash,
    selectedReferenceItem, selectedRequest, attachReferenceContext, selectedElements, selectedElementPageId,
    selectedElementTargets, renameCanvasElement, selectedElementRequest, setCanvasElementRequest,
    updateCanvasElementContext, selectedBoundary, updateCanvasPage, mutate, updateCanvasPageSize,
    addCanvasPageSize, renameProject, showDotGrid, changeGridFlags, save, canvasBoundaries,
    moveCanvasElementTo, removeLinkedDefinition, views, addNamedView, renameNamedView, removeNamedView,
    selectedBounds, clashPointers, selectNextPointerClash, zoomPercent, showGridSettings, setShowGridSettings,
    showShortcuts, setShowShortcuts, recovery, showProjectLocation, setShowProjectLocation,
    changeProjectLocation, showRecovery, setShowRecovery, importSource, importContext, startWebsiteImport,
    showWebsiteImport, setShowWebsiteImport, setCanvasSelection, setCanvasFocus, showStyleGuide,
    setShowStyleGuide, showNotes, changeBrief, rulerUnit, setRulerUnit, showRename, setShowRename,
    saveConflict, chooseSaveConflict, replacement, setReplacement, confirmation, setConfirmation,
    refreshAiSettings, updateAiSettings, aiPollingError,
  } = useEditor({ initialView });
  return <div className="application editor-shell">
    {showPalette && <CommandPalette commands={paletteCommands} close={() => setShowPalette(false)} />}
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
        <button type="button" className="theme-toggle icon-button" aria-label={resolvedTheme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'} title={!onStartPage && project ? (resolvedTheme === 'dark' ? 'Light mode for the editor and canvas' : 'Dark mode for the editor and canvas') : (resolvedTheme === 'dark' ? 'Light mode' : 'Dark mode')} onClick={() => onStartPage || !project ? setTheme(resolvedTheme === 'dark' ? 'light' : 'dark') : run(() => switchAppearance(resolvedTheme === 'dark' ? 'light' : 'dark'))}>{resolvedTheme === 'dark' ? <Icon name="sun" /> : <Icon name="moon" />}</button>
        {project && showsOpener({ available: true, open: showImport }) && <button type="button" className="icon-button" aria-label="Import context" title="Import context" onClick={() => { setImportSource(null); setShowImport(true); }}><Icon name="folder" /></button>}
        {!onStartPage && project && <>{view?.canUndo && <button aria-label="Undo last change" type="button" className="icon-button" title={withShortcut('Undo', 'undo')} onClick={() => run(() => travel('undo_project'))}><Icon name="undo" /></button>}{view?.canRedo && <button aria-label="Redo last change" type="button" className="icon-button" title={withShortcut('Redo', 'redo')} onClick={() => run(() => travel('redo_project'))}><Icon name="redo" /></button>}
          <button type="button" className={showHistory ? 'icon-button active' : 'icon-button'} aria-pressed={showHistory} aria-label="History and alternatives" title="History and alternatives" onClick={() => { setShowHistory(value => !value); if (panels.hidden) panelActions.toggleHidden(); if (panels.collapsed) panelActions.setCollapsed(false); }}><Icon name="history" /></button></>}
        {!onStartPage && project && <button type="button" className={playIsActive(play) ? 'icon-button active' : 'icon-button'} aria-pressed={playIsActive(play)} aria-label="Play" title={withShortcut('Play', 'play')} onClick={() => togglePlay()}><Icon name="play" /></button>}
        {!onStartPage && project && <button type="button" className="icon-button" aria-label="Preview export" title={withShortcut('Preview export', 'preview')} onClick={previewSelectedBoundary}><Icon name="monitor" /></button>}
        {!onStartPage && project && <button type="button" className={showExport ? 'icon-button active' : 'icon-button'} aria-pressed={showExport} aria-label="Export" title={withShortcut('Export', 'export')} onClick={() => openExportPanel()}><Icon name="download" /></button>}
        {showsOpener({ available: !aiConnected, open: showAiSettings }) && <button className="ai-shortcut needs-connection" onClick={() => setShowAiSettings(true)} aria-label="AI settings" title="Connect AI to generate">Connect AI</button>}
        {showsOpener({ available: !onStartPage && !!project && !!(generation.job || generation.operation), open: showLog }) && <button type="button" className="icon-button" aria-label="AI log" title="AI conversation" onClick={() => setShowLog(true)}><Icon name="note" /></button>}
        {!onStartPage && project && <button className={generation.canStop ? undefined : 'primary'} title={generation.canStop ? 'Stop generation (Esc)' : requestedAreas === 0 && pendingImages === 0 ? 'Describe at least one request first.' : 'Generate the described changes'} disabled={!generation.canStop && ((requestedAreas === 0 && pendingImages === 0) || working)} onClick={generation.canStop ? () => { void generation.cancel(); } : () => generate()}>{generation.canStop ? 'Stop' : 'Generate'}</button>}
      </div>
    </header>
    <SurfaceHost place="banner" />
    {onStartPage && <NoticeHost />}
    {error && <ProblemBanner message={userError(error)} dismiss={() => setError('')} action={problemAction} />}
    {openFailure && <Notice variant="warning" title="Couldn't open your browser" message={openFailure.message} action={{ label: 'Copy link', onClick: () => { const url = openFailure.url; void native('copy_text', { text: url }).then(() => setNotice('Link copied.')).catch(error => setError(errorMessage(error))); } }} dismiss={() => setOpenFailure(null)} />}
    {updateResult && <Notice variant={updateResult.variant} message={updateResult.message} action={updateResult.download ? { label: 'Download', onClick: () => { const url = updateResult.download; if (url) void openRelease(url); } } : undefined} dismiss={() => setUpdateResult(null)} />}
    {updateError && <ProblemBanner message={userError(updateError)} dismiss={() => setUpdateError('')} />}
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
      {!panels.hidden && <ToolStrip tool={canvasTool} onTool={chooseTool}
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
      <SurfaceHost place="canvas" />
      <NoticeHost />
      <ProjectCanvas ref={canvasRef} project={project} openPage={id => selectCanvasBoundary(id)} createBoundary={createCanvasBoundary} savePlacements={saveCanvasPlacements} saveView={(slot, camera) => slot === null ? addNamedViewAt(camera) : saveCurrentView(slot, camera)} selection={canvasSelection} focused={canvasFocus} focusedSize={canvasFocusSize} selectBoundary={selectCanvasBoundary} selectReference={selectCanvasReference} selectPointer={selectCanvasPointer} selectRequest={id => selectCanvasRequest(id)} clearSelection={clearCanvasSelection} removeSelection={removeCanvasSelection} addReference={addCanvasReference} updateReference={updateCanvasReference} elementToReference={elementToReference} referenceToElement={referenceToElement} attachToRequest={(requestId, files, links) => run(() => attachToRequest(requestId, files, links))} addRequest={addCanvasRequest} updateRequest={updateCanvasRequest} deleteBoundary={deleteBoundary} addPointer={addCanvasPointer} updatePointer={updateCanvasPointer} pointerStates={pointerStateMap} tool={canvasTool} insertKind={insertKind} onTool={next => { setCanvasTool(next); if (next !== 'insert') setInsertKind(null); }} boundaryKind={boundaryKind} requestShape={requestToolShape} onRequestShape={setRequestToolShape} onZoom={reportCanvasZoom} onLayout={ignoreCanvasLayout}
        selectElement={selectCanvasElement} removeElements={items => removeCanvasSelection([], [], [], items)} insertElements={insertCanvasElements} updateElements={updateCanvasElements} moveElements={moveCanvasElements}
        groupElements={groupCanvasElements} ungroupElements={ungroupCanvasElements}
        linkElements={linkCanvasElements} unlinkElements={unlinkCanvasElements} pasteLinked={pasteLinkedInstances}
        play={play} onPlayElement={playElement} onPlayExit={() => setPlay(idlePlay)}
        preview={preview} previewClose={closePreview} previewRefresh={refreshPreview} />
      {showGenerationStatus ? <GenerationFeedback controller={generation} project={project} openSettings={() => setShowAiSettings(true)} openImport={openImportDialog} openBrief={() => setShowNotes('decisions')} changeRequest={editGenerationRequest} requestEditorOpen={requestEditorIsOpen} details={showsOpener({ available: true, open: showLog }) ? openGeneration : undefined} /> : draftCount > 0 && showsOpener({ available: true, open: showLog }) ? <Notice priority="result" message={`${draftCount} ${draftCount === 1 ? 'draft' : 'drafts'}`} action={{ label: 'Details', onClick: openGeneration }} /> : null}
      </div>
      {showLog && project && <GenerationLog key={`${project.id}:${logFocus}`} controller={generation} resultApplied={false} requestEditorOpen={requestEditorIsOpen} job={generation.job} operation={generation.operation} error={generation.error} project={project} settings={aiSettings} openSettings={() => setShowAiSettings(true)} openImport={openImportDialog} openBrief={() => setShowNotes('decisions')} manageUsage={manageChatgptUsage} editRequest={editGenerationRequest} reply={replyToGeneration} close={() => setShowLog(false)} />}
      <SurfaceHost place="panel" />
      </main>
      </div>
      <EditorDock state={panels} actions={panelActions}
        exportPanel={showExport && project
          ? <ExportPanel project={project} destination={exportDestination} busy={pending > 0} job={exportJob} sending={exportSending} error={exportError}
              chooseDestination={() => run(chooseExportDestination)} runExport={() => runExport()} sendExport={() => run(sendExport)}
              cancelExport={() => run(cancelExport)} retryExport={() => run(sendExport)}
              openFolder={openExportFolder} openInBrowser={() => run(openInBrowser)} previewExport={pageId => runExport(pageId)}
              close={() => setShowExport(false)} />
          : undefined}
        history={showHistory && project
          ? <HistoryPanel projectId={project.id} refreshKey={`${project.id}:${view?.dirty ? 'dirty' : 'saved'}:${view?.history?.version ?? 0}:${historyEpoch}`}
              apply={accept} notice={setNotice} error={setError} />
          : undefined}
        properties={<>
          {untitledSave && view?.untitled && <UntitledSaveForm key={project.id} project={project} location={projectLocation} busy={pending > 0}
            cancel={() => closeUntitledSave(null)}
            chooseLocation={name => native<string | null>('pick_project_save_path', { name })}
            save={saveUntitledProject} />}
          {selectedPointer
          ? <PointerProperties pointer={selectedPointer} project={project} state={selectedPointerState} busy={pending > 0} rewriting={false}
              aiConnected={aiConnected} connectAi={() => setShowAiSettings(true)}
              change={comment => updateCanvasPointer(selectedPointer.id, { comment })}
              keepComment={() => ["codeChanged", "needsExporting"].includes(selectedPointerState?.state ?? '') ? forgetExportPointer(selectedPointer.id) : keepPointerComment(selectedPointer.id)}
              keepCode={() => run(() => keepPointerCode(selectedPointer.id))}
              keepThis={() => selectedPointerOther && keepPointerThis(selectedPointer.id, selectedPointerOther.id)}
              merge={() => selectedPointerOther && mergePointerClash(selectedPointer, selectedPointerOther)}
              reattach={() => canvasRef.current?.armPointer(selectedPointer.id)} remove={() => removeCanvasSelection([], [selectedPointer.id])} />
          : selectedReferenceItem
          ? <ReferenceProperties item={selectedReferenceItem} busy={pending > 0} update={patch => updateCanvasReference(selectedReferenceItem.id, patch)} remove={() => removeCanvasSelection([selectedReferenceItem.id])} />
          : selectedRequest
          ? <RequestProperties request={selectedRequest} project={project} busy={pending > 0} update={updateCanvasRequest} remove={() => removeCanvasSelection([], [], [selectedRequest.id])} attach={attachReferenceContext} />
          : selectedElements.length && selectedElementPageId
            ? <ElementProperties styleCommand={mutate} project={project} page={project.pages.find(page => page.id === selectedElementPageId)!} pageId={selectedElementPageId} sizeId={selectedElementTargets[0].sizeId} elements={selectedElements} busy={pending > 0}
                update={patches => void updateCanvasElements(selectedElementPageId, patches)}
                rename={name => renameCanvasElement(selectedElementPageId, selectedElements[0].id, name)}
                link={() => canvasRef.current?.linkSelection()} unlink={() => canvasRef.current?.unlinkSelection()}
                canLink={selectedElementTargets.length >= 2}
                canUnlink={selectedElementTargets.length === 1 && (!!selectedElements[0]?.link || isNestedLayer(project, selectedElements[0]?.id ?? ''))}
                request={selectedElementRequest} setRequest={data => setCanvasElementRequest(selectedElements[0].id, data)} updateContext={updateCanvasElementContext}
                generate={() => generate(selectedElements.map(element => element.id))} error={setError} />
          : selectedBoundary
            ? <BoundaryProperties page={selectedBoundary} busy={pending > 0} commit={patch => updateCanvasPage(selectedBoundary.id, patch)}
                setOutput={output => void mutate({ type: 'set_page_output', page_id: selectedBoundary.id, output })}
                preview={() => previewSelectedBoundary()}
                updateSize={(sizeId, patch) => updateCanvasPageSize(selectedBoundary.id, sizeId, patch)}
                addSize={(preset, width, height) => addCanvasPageSize(selectedBoundary.id, preset, width, height)}
                removeSize={size => {
                  void mutate({ type: 'remove_page_size', page_id: selectedBoundary.id, size_id: size.id })
                    .then(() => setNotice(`Removed the ${size.label} size.`, () => { void travel('undo_project').catch(() => {}); }))
                    .catch(() => {});
                }}
                remove={() => deleteBoundary(selectedBoundary.id, canvasFocusSize)} />
            : <ProjectProperties project={project} busy={pending > 0} rename={renameProject} setGrid={showDotGrid} setGridSettings={changeGridFlags}
                save={view?.untitled ? () => run(() => save()) : undefined} />}
        </>}
        layers={<CanvasLayers styleCommand={mutate} project={project} boundaries={canvasBoundaries} selection={canvasSelection} focused={canvasFocus} selectBoundary={selectCanvasBoundary} selectReference={selectCanvasReference} selectPointer={selectCanvasPointer}
          selectRequest={id => selectCanvasRequest(id)} selectElement={selectCanvasElement} updateElements={(pageId, sizeId, patches) => void updateCanvasElements(pageId, patches.map(patch => ({ ...patch, size_id: sizeId })))}
          moveElement={moveCanvasElementTo} removeLinked={removeLinkedDefinition}
          removeElements={ids => {
            const elementIds = ids.flatMap(id => { const page = project.pages.find(item => !!findElementIn(item.elements, id)); return page ? [{ pageId: page.id, elementId: id }] : []; });
            removeCanvasSelection([], [], [], elementIds);
          }}
          deleteBoundary={deleteBoundary} removeRequest={id => removeCanvasSelection([], [], [id])}
          removeReference={id => removeCanvasSelection([id])} removePointer={id => removeCanvasSelection([], [id])}
          views={views} addView={addNamedView} jumpToView={view => canvasRef.current?.goToView(view)} renameView={renameNamedView} removeView={removeNamedView} />} />
      </div>
      <footer className="status-bar"><span className="status-selection" title={view.path ?? undefined}>{canvasSelection.length > 1 && <span className="status-count">{canvasSelection.length} selected</span>}{selectedBounds && <span className="status-bounds" aria-label="Selection bounds"><span>{Math.round(selectedBounds.width)} × {Math.round(selectedBounds.height)}</span><span className="status-coordinate">X {Math.round(selectedBounds.x)} · Y {Math.round(selectedBounds.y)}</span></span>}</span><div className="canvas-status-actions">{playIsActive(play) && <StatusChip tone="info" role="status" className="play-chip" onClick={() => setPlay(idlePlay)} ariaLabel="Stop Play">Playing · Esc to stop</StatusChip>}{clashPointers.length > 0 && <StatusChip tone="warning" className="clash-chip" onClick={selectNextPointerClash} role="status" ariaLabel="Select the next pointer clash">{clashPointers.length} clash{clashPointers.length === 1 ? '' : 'es'}</StatusChip>}<div className="status-zoom" role="group" aria-label="Canvas zoom controls"><button type="button" aria-label="Fit all boundaries" title={withShortcut('Fit all boundaries', 'fitAll')} onClick={() => canvasRef.current?.fit()}>Fit</button><input type="range" min={0} max={1000} step={1} value={sliderFromZoom(canvasZoom)} aria-label="Canvas zoom" title={`${zoomPercent}%`} onChange={event => canvasRef.current?.zoomTo(zoomFromSlider(Number(event.target.value)))} /><button type="button" className="status-zoom-value" aria-label="Zoom to 100%" title={withShortcut('Zoom to 100%', 'zoomReset')} onClick={() => canvasRef.current?.zoomTo(1)}>{zoomPercent}%</button></div>{showsOpener({ available: true, open: showGridSettings }) && <button onClick={() => setShowGridSettings(true)} aria-label="Grid and rulers">{project.gridSettings.spacing}{project.gridSettings.unit === 'percent' ? '%' : ' px'} grid{project.gridSettings.snap ? ' · Snap' : ''}</button>}<StatusChip tone={pending > 0 ? 'neutral' : view.dirty ? 'warning' : 'success'} role="status" className="save-state">{pending > 0 ? 'Saving locally…' : view.dirty ? 'Unsaved' : 'Saved'}</StatusChip></div></footer>
    </>}
    {showShortcuts && <WorkspaceSurface place={onStartPage ? 'start' : 'panel'} title="Keyboard shortcuts" close={() => setShowShortcuts(false)}><div className="dialog-body shortcut-help">{SHORTCUT_GROUPS.map(group => <section key={group.group}><h3>{group.group}</h3><dl>{group.items.map(({ label, keys }) => <div key={label}><dt>{label}</dt><dd>{keys}</dd></div>)}</dl></section>)}<p className="field-help">Hold Space while dragging to pan. Alt+drag duplicates the selected layers. Escape stops Play, closes a preview, or cancels the current gesture.</p></div></WorkspaceSurface>}
    {recovery && <WorkspaceSurface place="banner" title="Unsaved changes" close={recovery.dismiss}><RecoveryChoice {...recovery} /></WorkspaceSurface>}
    {showProjectLocation && <WorkspaceSurface place={onStartPage ? 'start' : 'panel'} title="Project location" close={() => setShowProjectLocation(false)}><div className="dialog-body"><ProjectLocationSettings location={projectLocation} change={changeProjectLocation} /></div></WorkspaceSurface>}
    {showRecovery && view && <WorkspaceSurface place={onStartPage ? 'start' : 'panel'} title={project ? 'Recover unsaved projects' : 'Unsaved work'} close={() => setShowRecovery(false)}>
      <RecoveryList items={view.recoveries} busy={pending > 0} recover={id => { void recover(id).catch(() => {}); }} dismiss={dismissRecoveries} /></WorkspaceSurface>}
    {showImport && <WorkspaceSurface place={onStartPage ? 'start' : 'panel'} title="Import" close={() => setShowImport(false)}><ImportContext key={`${project?.id ?? 'new'}:${importSource?.id ?? ''}`} initialSource={importSource} busy={pending > 0} settings={aiSettings} openSettings={() => setShowAiSettings(true)} importInto={importContext} importWebsite={startWebsiteImport} close={() => setShowImport(false)} /></WorkspaceSurface>}
    {showWebsiteImport && <WorkspaceSurface place={onStartPage ? 'start' : 'panel'} title="Import website" close={() => setShowWebsiteImport(false)}><WebsiteImport busy={pending > 0} onImported={result => { accept(result.workspace); setCanvasSelection([]); setCanvasFocus(null); setShowWebsiteImport(false); setNotice(`Imported ${result.pages} ${result.pages === 1 ? 'page' : 'pages'} with ${result.layers} ${result.layers === 1 ? 'layer' : 'layers'}${result.warnings.length ? `; ${result.warnings.length} item${result.warnings.length === 1 ? '' : 's'} left out` : ''}.`); requestAnimationFrame(() => canvasRef.current?.fit()); }} close={() => setShowWebsiteImport(false)} /></WorkspaceSurface>}
    {showStyleGuide && project && <WorkspaceSurface place={onStartPage ? 'start' : 'panel'} title="Style guide" close={() => setShowStyleGuide(false)}><StyleGuide project={project} apply={command => mutate(command).then(() => undefined)} openBrief={() => { setShowStyleGuide(false); setShowNotes('decisions'); }} /></WorkspaceSurface>}
    {showNotes && project && <WorkspaceSurface place={onStartPage ? 'start' : 'panel'} title="Project brief" close={() => setShowNotes(null)}><Notes project={project} initialSection={showNotes} reviewGeneration={project.latestGeneration?.jobId === generation.job?.jobId ? openGeneration : undefined} apply={command => mutate(command).then(() => undefined)} changeBrief={changeBrief} /></WorkspaceSurface>}
    {showGridSettings && project && <WorkspaceSurface place={onStartPage ? 'start' : 'panel'} title="Grid and rulers" close={() => setShowGridSettings(false)}><GridForm rulerUnit={rulerUnit} changeRulerUnit={setRulerUnit} settings={project.gridSettings} showGrid={project.showGrid} busy={pending > 0} submit={async (settings, visible) => {
      if (visible !== project.showGrid) await mutate({ type: 'set_grid', enabled: visible });
      await mutate({ type: 'set_grid_settings', settings }); setShowGridSettings(false);
    }} /></WorkspaceSurface>}
    {showRename && project && <WorkspaceSurface place={onStartPage ? 'start' : 'panel'} title="Rename project" close={() => setShowRename(false)}><RenameProject name={project.name} submit={async name => { await mutate({ type: 'rename_project', name }); setShowRename(false); }} /></WorkspaceSurface>}
    {saveConflict && <SaveConflictDialog conflict={saveConflict} choose={chooseSaveConflict} />}
    {replacement && <ModalDialog title="Save your changes?" close={() => setReplacement(null)}><p>Save “{project?.name}” before you {replacement.label}?</p><div className="form-actions">
      <button className="primary" disabled={pending > 0} onClick={() => { void (async () => { const saved = await save(); if (saved && !saved.dirty) { const run = replacement.run; setReplacement(null); await run(false); } })().catch(() => {}); }}>Save and continue</button>
      <button className="danger" disabled={pending > 0} onClick={() => { const run = replacement.run; setReplacement(null); void run(true).catch(() => {}); }}>Discard and continue</button><button onClick={() => setReplacement(null)}>Keep editing</button>
    </div></ModalDialog>}
    {confirmation && <WorkspaceSurface place="banner" title={confirmation.title} showHeader={false} close={() => setConfirmation(null)}><div className="inline-notice"><span>{confirmation.message}</span><button className={confirmation.confirmLabel ? undefined : 'danger'} disabled={pending > 0} onClick={() => { void confirmation.action().then(() => setConfirmation(null)).catch(() => {}); }}>{confirmation.confirmLabel ?? 'Delete'}</button><button onClick={() => setConfirmation(null)}>Cancel</button></div></WorkspaceSurface>}
    {showAiSettings && <WorkspaceSurface place={onStartPage ? 'start' : 'panel'} title="AI settings" close={() => setShowAiSettings(false)}><AiSettings current={aiSettings} refresh={refreshAiSettings} update={updateAiSettings} manageUsage={manageChatgptUsage} pollingError={aiPollingError} /></WorkspaceSurface>}
  </div>;
}
