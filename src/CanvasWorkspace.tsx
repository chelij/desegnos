import { CanvasLayers } from './dock/layers';
import { ExportPanel } from './dock/exportPanel';
import { HistoryPanel } from './dock/historyPanel';
import { PropertiesPanel } from './dock/propertiesPanel';
import { EditorDock } from './EditorDock';
import { ToolStrip } from './EditorTools';
import { GenerationFeedback } from './Generation';
import { GenerationLog } from './GenerationLog';
import { NoticeHost } from './ProblemBanner.js';
import { ProjectCanvas } from './ProjectCanvas';
import { SurfaceHost } from './WorkspaceSurface';
import { findElementIn } from './projectCanvas/elementFacts';
import { idlePlay, isPresenting, playIsActive } from './projectCanvas/play';
import { sizeLabel } from './projectCanvas/sizes';
import type { Editor } from './session/useEditor';
import { withShortcut } from './shortcuts';
import { showsOpener } from './visibleActions';
import type { Project } from './types';

/** The tool strip, the canvas with its log, and the dock of panels beside it. */
export function CanvasWorkspace({ editor, project }: { editor: Editor; project: Project }) {
  const {
    actions, run, view, panel, closePanel, panels, panelActions, canvasRef, canvasTool, chooseTool, canvasZoom, boundaryKind, setBoundaryKind,
    requestToolShape, setRequestToolShape, armInsert, insertSignal, play, setPlay, playElement, exports, edits, mutate, pending, accept,
    setNotice, setError, setShowAiSettings, historyEpoch, compactHistory, canvasBoundaries, canvasSelection, canvasFocus, canvasFocusSize,
    widthPreview, selectCanvasBoundary, selectCanvasReference, selectCanvasPointer, selectCanvasRequest, selectCanvasElement, clearCanvasSelection,
    removeCanvasSelection, elementToReference, referenceToElement, attachToRequest, deleteBoundary, focusCanvasBehaviour, pointerStateMap,
    changeCanvasTool, reportCanvasZoom, createCanvasBoundary, addNamedViewAt, saveCurrentView, groupCanvasElements, ungroupCanvasElements,
    generation, generate, showGenerationStatus, openGeneration, editGenerationRequest, replyToGeneration, aiSettings, openPanel,
    updateCanvasPage, moveCanvasElementTo, removeLinkedDefinition, addProjectBoundary, views, addNamedView, renameNamedView, removeNamedView } = editor;
  const presenting = isPresenting(project, play);
  const playingPage = playIsActive(play) ? project.pages.find(page => page.id === play.boundary) ?? null : null;
  const playingSize = playingPage ? playingPage.sizes.find(size => size.id === canvasFocusSize) ?? playingPage.sizes[0] ?? null : null;
  const playingLabel = playingPage && playingSize ? `${playingPage.name} · ${sizeLabel(playingSize.width)}` : null;
  const openImport = () => actions.perform('importContext');
  const openBrief = () => openPanel({ kind: 'notes', section: 'decisions' });
  return <>
    <div className="editor-body">
      {!panels.hidden && <ToolStrip tool={canvasTool.kind} onTool={chooseTool}
        zoom={canvasZoom}
        perform={actions.perform}
        onZoom={value => canvasRef.current?.zoomTo(value)}
        boundaryKind={boundaryKind} onBoundaryKind={setBoundaryKind}
        requestShape={requestToolShape} onRequestShape={setRequestToolShape}
        onInsert={armInsert} insertSignal={insertSignal} />}
      <div className="editor-work">
        <main className="workspace" id="project-panel">
          <div className="design-pane">
            {panels.hidden && <button type="button" className="show-panels" title={withShortcut('Show panels', 'hidePanels')} onClick={() => actions.perform('hidePanels')}>Show panels</button>}
            <SurfaceHost place="canvas" />
            <NoticeHost />
            {playIsActive(play) && !presenting && playingLabel && <div className="play-pill" role="status"><span className="play-pill-label">Playing {playingLabel}</span><button type="button" onClick={() => setPlay(idlePlay)}>Stop</button></div>}
            <ProjectCanvas ref={canvasRef} edits={edits} editProject={mutate} project={project} openPage={id => selectCanvasBoundary(id)} createBoundary={createCanvasBoundary} saveView={(slot, camera) => slot === null ? addNamedViewAt(camera) : saveCurrentView(slot, camera)} selection={canvasSelection} focused={canvasFocus} focusedSize={canvasFocusSize} widthPreview={widthPreview} selectBoundary={selectCanvasBoundary} selectReference={selectCanvasReference} selectPointer={selectCanvasPointer} selectRequest={id => selectCanvasRequest(id)} clearSelection={clearCanvasSelection} removeSelection={removeCanvasSelection} elementToReference={elementToReference} referenceToElement={referenceToElement} attachToRequest={(requestId, files, links) => run(() => attachToRequest(requestId, files, links))} deleteBoundary={deleteBoundary} focusBehaviour={focusCanvasBehaviour} pointerStates={pointerStateMap} tool={canvasTool.kind} insertKind={canvasTool.kind === 'insert' ? canvasTool.insertKind : null} onTool={changeCanvasTool} boundaryKind={boundaryKind} requestShape={requestToolShape} onZoom={reportCanvasZoom}
              selectElement={selectCanvasElement} removeElements={items => removeCanvasSelection([], [], [], items)}
              groupElements={groupCanvasElements} ungroupElements={ungroupCanvasElements}
              play={play} onPlayBoundary={id => setPlay(current => ({ ...current, boundary: id }))} onPlayElement={playElement} onPlayExit={() => setPlay(idlePlay)}
              preview={exports.preview} previewClose={exports.closePreview} previewRefresh={exports.refreshPreview} generate={ids => generate(ids)} />
            {showGenerationStatus ? <GenerationFeedback controller={generation} project={project} openSettings={() => setShowAiSettings(true)} openImport={openImport} openBrief={openBrief} changeRequest={editGenerationRequest} details={showsOpener({ available: true, open: panel.kind === 'log' }) ? openGeneration : undefined} /> : null}
          </div>
          {panel.kind === 'log' && <GenerationLog key={`${project.id}:${panel.focus}`} controller={generation} job={generation.job} operation={generation.operation} error={generation.error} project={project} settings={aiSettings} openSettings={() => setShowAiSettings(true)} openImport={openImport} openBrief={openBrief} editRequest={editGenerationRequest} reply={replyToGeneration} close={() => closePanel('log')} />}
          <SurfaceHost place="panel" />
        </main>
      </div>
      <EditorDock state={panels} actions={panelActions}
        exportPanel={panel.kind === 'export'
          ? <ExportPanel openSettings={() => setShowAiSettings(true)} project={project} exports={exports} busy={pending > 0}
              selectBoundary={selectCanvasBoundary} setIncluded={(pageId, included) => updateCanvasPage(pageId, { included })}
              setOutput={(pageId, output) => void mutate({ type: 'set_page_output', page_id: pageId, output })}
              close={() => closePanel('export')} />
          : undefined}
        history={panel.kind === 'history'
          ? <HistoryPanel projectId={project.id} refreshKey={`${project.id}:${view?.dirty ? 'dirty' : 'saved'}:${view?.history?.version ?? 0}:${historyEpoch}`}
              apply={accept} notice={setNotice} error={setError} compact={() => run(compactHistory)} />
          : undefined}
        properties={<PropertiesPanel editor={editor} />}
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
  </>;
}
