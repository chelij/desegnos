import { boundaryVisible } from './projectCanvas/canvasView';
import { fitCamera } from './projectCanvasGeometry';
import { useProjectCanvas } from './projectCanvas/useProjectCanvas';
export type { ProjectCanvasHandle, ProjectCanvasProps, PreviewState } from './projectCanvas/interactionTypes';
import { CORNERS, ProjectCanvasHandle, ProjectCanvasProps } from './projectCanvas/interactionTypes';
import { forwardRef, useEffect, useState, type CSSProperties } from 'react';
import { isTextField } from './WorkspaceSurface';
import { BoundaryLabels } from './projectCanvas/boundaryLabels';
import { SelectionBar } from './projectCanvas/selectionBar';
import { BoundaryLayer } from './projectCanvas/boundaries';
import { RequestBoxEditor, RequestLayer } from './projectCanvas/requests';
import { ReferenceLayer, ReferenceNoteEditor } from './projectCanvas/reference';
import { PointerComments, PointerHandles, PointerLines } from './projectCanvas/pointers';
import { screenRect } from './projectCanvas/surface';
import { Minimap } from './projectCanvas/Minimap';
import { ElementSelectionOverlay, ElementTextEditor } from './projectCanvas/elements';
import { LinkChoiceRow, PasteChoiceRow, PlaceholderChoiceRow } from './projectCanvas/linkedChoices';
import { Notice } from './ProblemBanner';
import { playIsActive } from './projectCanvas/play';

// A click with the Boundary tool places its default boundary instead of doing nothing.

export const ProjectCanvas = forwardRef<ProjectCanvasHandle, ProjectCanvasProps>(function ProjectCanvas({
  editProject, project, openPage, createBoundary, savePlacements, saveView, selection, focused, selectBoundary, selectReference, selectPointer, selectElement, clearSelection,
  removeSelection, removeElements, deleteBoundary, addReference, updateReference, elementToReference, referenceToElement, attachToRequest,
  addRequest, updateRequest, selectRequest,
  insertElements, updateElements, moveElements, groupElements, ungroupElements, linkElements, unlinkElements, pasteLinked,
  addPointer, updatePointer, pointerStates, tool, insertKind, onTool, boundaryKind, requestShape, onRequestShape, focusedSize, widthPreview, onZoom, onLayout,
  play, onPlayBoundary, onPlayElement, onPlayExit, preview, previewClose, previewRefresh,
}, ref) {
  const [pointerFocus, setPointerFocus] = useState(false);
  const {
    runCanvasAction, surface, panning, spaceHeld, zoomHeld, painterArmed, dropHover, dotStyle, pointerDown, keyDown,
    doubleClick, paste, dropText, imagePicker, chooseImage, camera, scrubbed, moving, editing, movingElements,
    openPlaceholder, viewport, inverse, boundaries, requestDraft, draftArea, movingItems, draftNote, draft,
    draftElement, marquee, zoomRect, entries, autoLinks, pointerDraft, selectedPointerIds, choosePointer,
    createdPointer, selectionScreen, viewSize, selectionFill, selectionLinked,
    selectionLinkable, selectionHyperlink, focusElementLink, selectionScope, setSelectionFill, alignSelection, distributeSelection, setSelectionScope,
    startRequestOnSelection, linkSelection, unlinkSelection, duplicateElements, deleteSelectedLayers,
    minimapVisible, stopTransition, touched, setCamera, selectedPointerId, armedPointer, startReattach,
    selectedElementTargets, startElementResize, editingElement, editingScreen, commitEditingText,
    placeholderChoice, resolvePlaceholder, setPlaceholderChoice, submitRequestDraft,
    cancelRequestDraft, selectedItem, selectedItemScreen, createdItem, startReferenceResize, pasteChoice,
    setPasteChoice, confirmPaste, linkChoice, linkCopies, setLinkChoice, previewScreen, dropNotice,
    setDropNotice,
  } = useProjectCanvas({ editProject, project, openPage, createBoundary, savePlacements, saveView, selection, focused, selectBoundary, selectReference, selectPointer, selectElement, clearSelection, removeSelection, removeElements, deleteBoundary, addReference, updateReference, elementToReference, referenceToElement, attachToRequest, addRequest, updateRequest, selectRequest, insertElements, updateElements, moveElements, groupElements, ungroupElements, linkElements, unlinkElements, pasteLinked, addPointer, updatePointer, pointerStates, tool, insertKind, onTool, boundaryKind, requestShape, onRequestShape, focusedSize, widthPreview, onZoom, onLayout, play, onPlayBoundary, onPlayElement, onPlayExit, preview, previewClose, previewRefresh }, ref);
  const presenting = playIsActive(play) && project.pages.length > 0 && project.pages.every(page => page.output === 'slide');
  const slides = project.pages;
  const slideIndex = Math.max(0, slides.findIndex(page => page.id === play.boundary));
  const advance = (delta: number) => onPlayBoundary(slides[Math.max(0, Math.min(slides.length - 1, slideIndex + delta))].id);
  useEffect(() => {
    if (!presenting) return;
    const handle = (event: KeyboardEvent) => {
      if (event.ctrlKey || event.metaKey || event.altKey || event.shiftKey) return;
      const next = ['ArrowRight', 'ArrowDown', ' ', 'PageDown'].includes(event.key);
      const previous = ['ArrowLeft', 'ArrowUp', 'PageUp'].includes(event.key);
      if (!next && !previous && event.key !== 'Escape') return;
      event.preventDefault(); event.stopImmediatePropagation();
      if (event.key === 'Escape') onPlayExit(); else advance(next ? 1 : -1);
    };
    window.addEventListener('keydown', handle, true);
    return () => window.removeEventListener('keydown', handle, true);
  }, [presenting, slideIndex, project, onPlayBoundary, onPlayExit]);
  if (presenting) {
    const boundary = boundaries.find(boundary => boundary.pageId === slides[slideIndex].id)!;
    const size = slides[slideIndex].sizes[0];
    const slide = { ...boundary, rect: { ...boundary.rect, width: size.width, height: size.height } };
    const displayCamera = fitCamera(slide.rect, viewSize);
    return <div ref={surface} id="project-canvas" className="project-canvas playing presenting" tabIndex={0}>
      <div className="project-canvas-layer" style={{ transform: `translate(${displayCamera.x}px, ${displayCamera.y}px) scale(${displayCamera.zoom})` }}>
        <BoundaryLayer project={project} boundaries={[slide]} focused={null} selection={[]} moving={null} tool="select" spaceHeld={false} openPage={() => {}} editingId={null} movingElements={{}} onPlaceholder={() => {}} play={play} viewport={slide.rect} />
      </div>
      <div className="presentation-controls">
        <button type="button" aria-label="Previous slide" disabled={slideIndex === 0} onClick={() => advance(-1)}>←</button>
        <span>{slideIndex + 1} / {slides.length}</span>
        <button type="button" aria-label="Next slide" disabled={slideIndex === slides.length - 1} onClick={() => advance(1)}>→</button>
        <button type="button" onClick={onPlayExit}>Stop</button>
      </div>
    </div>;
  }
  return <div ref={surface} id="project-canvas" className={`project-canvas tool-${tool}${tool === 'request' ? ` request-shape-${requestShape}` : ''}${panning ? ' panning' : ''}${spaceHeld ? ' pan-ready' : ''}${zoomHeld ? ' zoom-ready' : ''}${painterArmed ? ' paint-format' : ''}${playIsActive(play) ? ' playing' : ''}${dropHover && !dropHover.rect ? ' drop-bed' : ''}${pointerFocus ? ' pointer-focus' : ''}`} style={dotStyle} tabIndex={0}
    onPointerDownCapture={() => setPointerFocus(true)} onKeyDownCapture={() => setPointerFocus(false)}
    onBlur={event => { if (!event.currentTarget.contains(event.relatedTarget)) setPointerFocus(false); }}
    onPointerDown={pointerDown} onKeyDown={keyDown} onDoubleClickCapture={doubleClick} onPaste={paste}
    onDragOver={event => { if (!isTextField(event.target)) event.preventDefault(); }} onDrop={dropText}>
    <input ref={imagePicker} type="file" hidden accept="image/png,image/jpeg,image/webp,image/gif" onChange={event => { chooseImage(event.target.files?.[0]); event.target.value = ''; }} />
    <div className="project-canvas-layer" style={{ transform: `translate(${camera.x}px, ${camera.y}px) scale(${camera.zoom})`, '--canvas-zoom': camera.zoom } as CSSProperties}>
      <BoundaryLayer project={scrubbed.project} boundaries={scrubbed.boundaries} focused={focused} selection={selection}
        moving={moving} tool={tool} spaceHeld={spaceHeld} openPage={openPage} editingId={editing?.elementId ?? null}
        movingElements={movingElements} onPlaceholder={openPlaceholder} play={play} viewport={viewport} margin={200 / camera.zoom} />
      {dropHover?.rect && <div className="canvas-drop-target" style={{ left: dropHover.rect.x, top: dropHover.rect.y, width: dropHover.rect.width, height: dropHover.rect.height, borderWidth: 2 * inverse }} />}
      <RequestLayer project={project} boundaries={boundaries} selection={selection} draft={requestDraft}
        draftArea={draftArea} inverse={inverse} selectRequest={selectRequest} />
      <ReferenceLayer project={project} movingItems={movingItems} selection={selection} draftNote={draftNote} />
      {draft && <div className={`boundary-draft${boundaryKind === 'screen' ? ' screen' : ''}`} style={{ left: draft.x, top: draft.y, width: draft.width, height: draft.height, borderWidth: 2 * inverse }} />}
      {draftElement && <div className={`element-draft${draftElement.line ? ' line' : ''}`} style={{ left: draftElement.rect.x, top: draftElement.rect.y, width: draftElement.rect.width, height: draftElement.rect.height, borderWidth: inverse, transform: draftElement.rotation ? `rotate(${draftElement.rotation}deg)` : undefined }} />}
      {marquee && <div className="element-marquee" style={{ left: marquee.x, top: marquee.y, width: marquee.width, height: marquee.height, borderWidth: inverse }} />}
      {zoomRect && <div className="zoom-marquee" style={{ left: zoomRect.x, top: zoomRect.y, width: zoomRect.width, height: zoomRect.height, borderWidth: inverse }} />}
      <PointerLines entries={entries} autoLinks={autoLinks} draft={pointerDraft} camera={camera} viewport={viewport}
        selected={selectedPointerIds} pointerStates={pointerStates} enabled={tool === 'select'} onSelect={choosePointer} />
    </div>
    <PointerComments entries={entries} camera={camera} selected={selectedPointerIds} pointerStates={pointerStates} interactive={tool === 'select'} onSelect={choosePointer}
      change={(pointerId, comment) => updatePointer(pointerId, { comment })}
      changeKind={(pointerId, kind) => updatePointer(pointerId, { kind })}
      autoFocusId={createdPointer.current} close={() => { createdPointer.current = null; surface.current?.focus({ preventScroll: true }); }} />
    <BoundaryLabels project={project} boundaries={boundaries} moving={moving} draft={draft} draftKind={boundaryKind} focused={focused} selection={selection} camera={camera} openPage={openPage} />
    {selectionScreen && <SelectionBar screen={selectionScreen} viewWidth={viewSize.width} fill={selectionFill} linked={selectionLinked} linkable={selectionLinkable} hyperlink={selectionHyperlink} scope={selectionScope}
      onFill={setSelectionFill} onAlign={alignSelection} onDistribute={distributeSelection} count={selectedElementTargets.length} onScope={setSelectionScope} onRequest={() => { startRequestOnSelection(); }}
      onLinkTo={focusElementLink} onLink={linkSelection} onUnlink={unlinkSelection} onDuplicate={duplicateElements} onDelete={deleteSelectedLayers} />}
    {minimapVisible && <Minimap boundaries={boundaries} reference={project.reference} camera={camera} viewSize={viewSize}
      move={next => { stopTransition(); touched.current = true; setCamera(next); }} />}
    <PointerHandles entries={entries} camera={camera} selectedId={selectedPointerId} armed={armedPointer} onStart={startReattach} />
    {!editing && <ElementSelectionOverlay surface={surface} project={project} selection={selectedElementTargets} camera={camera}
      version={scrubbed.boundaries.filter(boundary => boundaryVisible(moving?.sizeId === boundary.sizeId ? moving.rect : boundary.rect, viewport, 200 / camera.zoom)).map(boundary => boundary.sizeId).join(',') + '|'
        + Object.entries(movingElements).map(([id, rect]) => `${id}:${rect.x},${rect.y},${rect.width},${rect.height}`).join('|')} onResizeStart={startElementResize} />}
    {editing && editingElement && editingScreen && <ElementTextEditor key={`${editing.elementId}:${editing.created}`} element={editingElement} screen={editingScreen} zoom={camera.zoom}
      autoWidth={editing.autoWidth} autoFocus onAction={runCanvasAction} onCommit={commitEditingText} />}
    {placeholderChoice && <PlaceholderChoiceRow screen={placeholderChoice.screen}
      include={() => resolvePlaceholder(placeholderChoice, true)} dismiss={() => resolvePlaceholder(placeholderChoice, false)}
      close={() => { setPlaceholderChoice(null); surface.current?.focus({ preventScroll: true }); }} />}
    {requestDraft && <RequestBoxEditor screen={requestDraft.screen} text="" submit={submitRequestDraft} close={cancelRequestDraft} />}
    {selectedItem && selectedItemScreen && selectedItem.kind.kind === 'note' && <ReferenceNoteEditor key={selectedItem.id} item={selectedItem}
      kind={selectedItem.kind} screen={selectedItemScreen} autoFocus={createdItem.current === selectedItem.id}
      change={text => updateReference(selectedItem.id, { text })} close={() => { createdItem.current = null; surface.current?.focus({ preventScroll: true }); }} />}
    {selectedItem && selectedItemScreen && <>
      {CORNERS.map(corner => <button key={corner} type="button" data-reference-corner={corner} className={`canvas-grip reference-grip ${corner}`} aria-label={`Resize reference item ${corner}`} title="Drag to resize"
        style={{ left: selectedItemScreen.left + (corner === 'ne' || corner === 'se' ? selectedItemScreen.width : 0) - 5, top: selectedItemScreen.top + (corner === 'sw' || corner === 'se' ? selectedItemScreen.height : 0) - 5 }}
        onPointerDown={event => startReferenceResize(event, corner, selectedItem)} />)}
    </>}
    {pasteChoice && <PasteChoiceRow screen={pasteChoice.screen} value={pasteChoice.value} useDefault={pasteChoice.useDefault}
      change={value => setPasteChoice(current => current ? { ...current, value } : current)}
      toggleDefault={value => setPasteChoice(current => current ? { ...current, useDefault: value } : current)}
      confirm={confirmPaste} cancel={() => { setPasteChoice(null); surface.current?.focus({ preventScroll: true }); }} />}
    {linkChoice && <LinkChoiceRow screen={linkChoice.screen} copies={linkChoice.copies} error={linkChoice.error}
      pick={id => linkCopies(linkChoice.copies.map(copy => copy.id), id)}
      cancel={() => { setLinkChoice(null); surface.current?.focus({ preventScroll: true }); }} />}
    {preview && previewScreen && <div className="preview-overlay" style={{ left: previewScreen.left, top: previewScreen.top, width: previewScreen.width, height: previewScreen.height }} onPointerDown={event => event.stopPropagation()}>
      <div className="preview-overlay-bar">
        <span className="preview-overlay-title">Preview</span>
        {preview.stale ? <><span className="preview-outdated">Preview is out of date</span><button type="button" className="dock-action" onClick={previewRefresh}>Refresh</button></> : null}
        <button type="button" className="dock-action" onClick={previewClose}>Close (Esc)</button>
      </div>
      <iframe className="preview-overlay-frame" title="Preview export" src={preview.url} sandbox="allow-scripts allow-forms allow-popups allow-modals" />
    </div>}
    {dropNotice && <Notice key={dropNotice.id} variant={dropNotice.variant} message={dropNotice.message} dismiss={() => setDropNotice(null)} />}
  </div>;
});
