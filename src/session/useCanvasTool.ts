import { useEffect, useState, type RefObject } from 'react';
import { type EditorTool, type ProjectTool, type RequestToolShape } from '../EditorTools';
import { type BoundaryToolKind, projectKind } from '../projectCanvas/boundaryDefaults';
import type { ProjectCanvasHandle } from '../ProjectCanvas';
import type { ElementKind, Project } from '../types';

type CanvasToolState =
  | { kind: Exclude<ProjectTool, 'insert'> }
  | { kind: 'insert'; insertKind: ElementKind | null };

/** The armed tool and the options its flyouts remember. */
export function useCanvasTool(canvasRef: RefObject<ProjectCanvasHandle | null>, project: Project | null) {
  const [canvasTool, setCanvasTool] = useState<CanvasToolState>({ kind: 'select' });
  const [insertSignal, setInsertSignal] = useState(0);
  const [boundaryKind, setBoundaryKind] = useState<BoundaryToolKind>('page');
  const [requestToolShape, setRequestToolShape] = useState<RequestToolShape>('pick');
  useEffect(() => { if (project) setBoundaryKind(projectKind(project)); }, [project?.id]);

  function changeCanvasTool(kind: ProjectTool, insertKind?: ElementKind | null) {
    setCanvasTool(previous => kind === 'insert'
      ? { kind, insertKind: insertKind === undefined ? (previous.kind === 'insert' ? previous.insertKind : null) : insertKind }
      : { kind });
  }
  function clearInsertKind() {
    setCanvasTool(previous => previous.kind === 'insert' ? { kind: 'insert', insertKind: null } : previous);
  }
  // The next press belongs to the canvas: leave the tool strip or its flyout behind, so no
  // button keeps focus or the pointer.
  function chooseTool(next: EditorTool) {
    changeCanvasTool(next, null);
    canvasRef.current?.focusCanvas();
  }
  function armInsert(kind: ElementKind) {
    changeCanvasTool(kind === 'image' || kind === 'text' ? kind : 'insert', kind);
    canvasRef.current?.focusCanvas();
  }
  return {
    canvasTool, changeCanvasTool, clearInsertKind, chooseTool, armInsert,
    insertSignal, bumpInsertSignal: () => setInsertSignal(value => value + 1),
    boundaryKind, setBoundaryKind, requestToolShape, setRequestToolShape };
}
