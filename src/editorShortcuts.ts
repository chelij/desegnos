import { type EditorTool } from './EditorTools';
import { type ProjectCanvasHandle } from './ProjectCanvas';
import { viewSlot, type ShortcutAction } from './shortcuts';
import type { CanvasTemplate, WorkspaceView } from './types';
import { ActionEvent, NO_ACTION_EVENT } from './editorShared';

type Context = {
  showStartPage: boolean;
  current: import("react").RefObject<WorkspaceView | null>;
  openPanel: (panel: Exclude<import('./session/useEditor').EditorPanel, { kind: 'closed' }>) => void;
  setShowPalette: import("react").Dispatch<import("react").SetStateAction<boolean>>;
  newUntitled: (template?: CanvasTemplate) => Promise<void>;
  open: (folder?: boolean, path?: string, discardRecovery?: boolean) => Promise<void>;
  clearInsertKind: () => void;
  setInsertSignal: import("react").Dispatch<import("react").SetStateAction<number>>;
  changeCanvasTool: (tool: EditorTool) => void;
  panelActions: import("./EditorDock").PanelActions;
  canvasRef: import("react").RefObject<ProjectCanvasHandle | null>;
  togglePlay: (pageId?: string | null) => void;
  previewSelectedBoundary: () => void;
  adaptSelectedSize: () => void;
  openExportPanel: (restore?: boolean) => void;
  runExport: (previewPageId?: string | null) => Promise<void>;
  travel: (command: 'undo_project' | 'redo_project') => Promise<void>;
  save: (saveAs?: boolean) => Promise<WorkspaceView | null>;
  closeProject: () => Promise<void>;
};

export function createEditorShortcuts(context: () => Context) {
  function performAction(action: ShortcutAction, event: ActionEvent = NO_ACTION_EVENT) {
    const { showStartPage, current, openPanel, setShowPalette, newUntitled, open, clearInsertKind, setInsertSignal, changeCanvasTool, panelActions, canvasRef, togglePlay, previewSelectedBoundary, adaptSelectedSize, openExportPanel, runExport, travel, save, closeProject } = context();
    // The start page has no canvas: project commands wait until a canvas is open again.
    if ((showStartPage || !current.current?.project) && !['new', 'open', 'help', 'commandPalette'].includes(action)) return;
    const project = current.current?.project;
    if (event.repeat && ['new', 'open', 'saveAs', 'help', 'hidePanels'].includes(action)) return;
    switch (action) {
      // These always act on the canvas selection, wherever focus sits in the editor.
      // The canvas' own listener may have run first; its preventDefault keeps this a single run.
      case 'copy': case 'cut': case 'pasteChoice': case 'duplicate':
      case 'link': case 'unlink': case 'group': case 'ungroup': case 'forward': case 'backward': case 'front': case 'back':
      case 'selectAll': case 'selectChild': case 'selectParent': case 'nudge': case 'nudgeFar': case 'delete':
      case 'copyStyle': case 'pasteStyle': case 'paintFormat': case 'minimap': case 'saveView': case 'goToView': {
        if (canvasRef.current?.runCanvasAction(action, { key: event.key, slot: viewSlot(event) ?? undefined, repeat: event.repeat })) event.preventDefault();
        return;
      }
      // Plain paste stays with the paste event, which also carries content from other apps.
      case 'paste':
        return;
      case 'help': event.preventDefault(); openPanel({ kind: 'shortcuts' }); return;
      case 'commandPalette': event.preventDefault(); setShowPalette(true); return;
      case 'new': event.preventDefault(); void newUntitled().catch(() => {}); return;
      case 'open': event.preventDefault(); void open().catch(() => {}); return;
      case 'renameLayer': {
        const field = document.querySelector<HTMLInputElement>('[data-field="layer-name"]');
        if (!field) return;
        event.preventDefault(); field.focus({ preventScroll: true }); field.select(); return;
      }
      case 'insert': event.preventDefault(); clearInsertKind(); setInsertSignal(value => value + 1); return;
      case 'imageTool': event.preventDefault(); changeCanvasTool('image'); return;
      case 'selectTool': case 'boundaryTool': case 'pointerTool': case 'textTool': case 'panTool':
      case 'rectTool': case 'ellipseTool': case 'lineTool': case 'noteTool': {
        event.preventDefault();
        changeCanvasTool(action === 'selectTool' ? 'select' : action === 'boundaryTool' ? 'boundary'
          : action === 'pointerTool' ? 'pointer' : action === 'textTool' ? 'text' : action === 'panTool' ? 'pan'
            : action === 'rectTool' ? 'rect' : action === 'ellipseTool' ? 'ellipse' : action === 'lineTool' ? 'line' : 'note');
        return;
      }
      case 'requestTool': case 'requestFromShape':
        // The canvas starts a request on the selection; with nothing selected F picks the tool.
        return;
      case 'askAI': event.preventDefault(); canvasRef.current?.startRequestOnSelection(); return;
      case 'hidePanels':
        if (!project) return;
        if (event.target instanceof Element && event.target.closest('.workspace-surface, .editor-menu, .layers-popover')) return;
        event.preventDefault(); panelActions.toggleHidden(); return;
      case 'zoomIn': event.preventDefault(); canvasRef.current?.zoomStep(1); return;
      case 'zoomOut': event.preventDefault(); canvasRef.current?.zoomStep(-1); return;
      case 'zoomReset': event.preventDefault(); canvasRef.current?.zoomTo(1); return;
      case 'fitAll': event.preventDefault(); canvasRef.current?.fit(); return;
      case 'fitSelection': event.preventDefault(); canvasRef.current?.fitSelection(); return;
      case 'play': if (!project) return; event.preventDefault(); togglePlay(); return;
      case 'adaptLayout': event.preventDefault(); adaptSelectedSize(); return;
      case 'preview': if (!project) return; event.preventDefault(); previewSelectedBoundary(); return;
      case 'export':
        if (!project) return;
        event.preventDefault(); openExportPanel();
        // runExport resolves the remembered or default folder, so this is one click.
        void runExport();
        return;
      case 'undo': case 'redo':
        if (!project) return;
        event.preventDefault();
        void travel(action === 'undo' ? 'undo_project' : 'redo_project').catch(() => {});
        return;
      case 'save': if (!project) return; event.preventDefault(); void save().catch(() => {}); return;
      case 'saveAs': if (!project) return; event.preventDefault(); void save(true).catch(() => {}); return;
      case 'closeProject': if (!project) return; event.preventDefault(); void closeProject().catch(() => {}); return;
    }
  }

  return { performAction };
}
