import type { Editor } from '../session/useEditor';
import { isNestedLayer } from '../projectCanvas/links';
import { ElementProperties } from './elementProperties';
import { BoundaryProperties, PointerProperties, ProjectProperties, ReferenceProperties, RequestProperties } from './properties';
import type { Pointer, PointerSource, PointerStateView } from '../types';
import { behaviourCommand, behaviourFor, behaviourSourceKey } from '../projectCanvas/behaviours';
import { propertySelection } from './selection';
type PropertiesEditor = Pick<Editor, 'project' | 'canvasSelection' | 'canvasFocus' | 'pointerStateMap' | 'exports' | 'keepPointerComment' | 'keepPointerThis' | 'mergePointerClash' | 'run' | 'canvasRef' | 'removeCanvasSelection' | 'mutate' | 'behaviourFocus' | 'pending' | 'aiConnected' | 'edits' | 'attachReferenceContext' | 'renameCanvasElement' | 'setCanvasElementRequest' | 'setError' | 'canvasFocusSize' | 'widthPreview' | 'setWidthPreview' | 'selectCanvasBoundary' | 'updateCanvasPage' | 'updateCanvasPageSize' | 'addCanvasPageSize' | 'deleteBoundary' | 'renameProject' | 'showDotGrid' | 'changeGridFlags' | 'view' | 'save' | 'removedWithUndo'>;

export function PropertiesPanel({ editor }: { editor: PropertiesEditor }) {
  const { project, canvasSelection, canvasFocus, pointerStateMap, exports, keepPointerComment, keepPointerThis, mergePointerClash, run, canvasRef, removeCanvasSelection, mutate, behaviourFocus, pending, aiConnected, edits, attachReferenceContext, renameCanvasElement, setCanvasElementRequest, setError, canvasFocusSize, widthPreview, setWidthPreview, selectCanvasBoundary, updateCanvasPage, updateCanvasPageSize, addCanvasPageSize, deleteBoundary, renameProject, showDotGrid, changeGridFlags, view, save, removedWithUndo } = editor;
  if (!project) return null;
  const { selectedBoundary, selectedReferenceItem, selectedPointer, selectedRequest, selectedElementTargets, selectedElementPageId, selectedElements, selectedElementRequest, selectedPointerState } = propertySelection(project, canvasSelection, canvasFocus, pointerStateMap);
  function clashActions(pointer: Pointer, state: PointerStateView | null) {
    const other = state?.clashWith.length ? project?.pointers.find(item => item.id === state.clashWith[0]) ?? null : null;
    return {
      keepComment: () => ["codeChanged", "needsExporting"].includes(state?.state ?? '') ? exports.forgetPointer(pointer.id) : keepPointerComment(pointer.id),
      keepCode: () => run(() => exports.keepPointerCode(pointer.id)),
      keepThis: () => { if (other) keepPointerThis(pointer.id, other.id); },
      merge: () => { if (other) mergePointerClash(pointer, other); },
      reattach: () => canvasRef.current?.armPointer(pointer.id),
      remove: () => removeCanvasSelection([], [pointer.id]) };
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
        if (command) void mutate(command);
      },
      focusToken: behaviourFocus?.source === key ? behaviourFocus.token : null,
      busy: pending > 0, aiConnected,
      actions: pointer ? clashActions(pointer, state) : undefined };
  }
  const elementBehaviourSource = project && selectedElementPageId && selectedElements.length === 1
    ? { kind: 'element' as const, pageId: selectedElementPageId, elementId: selectedElements[0].id }
    : null;
  const boundaryBehaviourSource = project && selectedBoundary ? { kind: 'boundary' as const, pageId: selectedBoundary.id } : null;
  return <>
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
                adaptSize={sizeId => { void mutate({ type: 'adapt_size', page_id: selectedBoundary.id, size_id: sizeId }); }}
                updateSize={(sizeId, patch) => updateCanvasPageSize(selectedBoundary.id, sizeId, patch)}
                addSize={(preset, width, height) => addCanvasPageSize(selectedBoundary.id, preset, width, height)}
                removeSize={size => {
                  void mutate({ type: 'remove_page_size', page_id: selectedBoundary.id, size_id: size.id })
                    .then(next => { if (next) removedWithUndo(`Removed the ${size.label} size.`); });
                }}
                remove={() => deleteBoundary(selectedBoundary.id, canvasFocusSize)}
                behaviour={boundaryBehaviourSource ? behaviourProps(boundaryBehaviourSource) : undefined} />
            : <ProjectProperties project={project} busy={pending > 0} rename={renameProject} setGrid={showDotGrid} setGridSettings={changeGridFlags}
                save={view?.untitled ? () => run(() => save()) : undefined} />}
        </>;
}
