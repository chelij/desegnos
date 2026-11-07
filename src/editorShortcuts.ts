import { type EditorTool } from './EditorTools';
import { type ProjectCanvasHandle } from './ProjectCanvas';
import { actionBinding, TOOL_ACTIONS, viewSlot, type ShortcutAction } from './shortcuts';
import type { CanvasTemplate, WorkspaceView } from './types';
import { ActionEvent, NO_ACTION_EVENT } from './editorShared';

type Context = {
  run: (action: () => Promise<unknown>) => void;
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
  travel: (command: 'undo_project' | 'redo_project') => Promise<void>;
  save: (saveAs?: boolean) => Promise<WorkspaceView | null>;
  closeProject: () => Promise<void>;
};

export function createEditorShortcuts(context: () => Context) {
  const handlers: Partial<Record<ShortcutAction, () => void | Promise<unknown>>> = {
    help: () => context().openPanel({ kind: 'shortcuts' }),
    commandPalette: () => context().setShowPalette(true),
    new: () => context().newUntitled(),
    open: () => context().open(),
    renameLayer: () => {
      const field = document.querySelector<HTMLInputElement>('[data-field="layer-name"]');
      field?.focus({ preventScroll: true }); field?.select();
    },
    insert: () => { context().clearInsertKind(); context().setInsertSignal(value => value + 1); },
    askAI: () => { context().canvasRef.current?.startRequestOnSelection(); },
    hidePanels: () => context().panelActions.toggleHidden(),
    zoomIn: () => context().canvasRef.current?.zoomStep(1),
    zoomOut: () => context().canvasRef.current?.zoomStep(-1),
    zoomReset: () => context().canvasRef.current?.zoomTo(1),
    fitAll: () => context().canvasRef.current?.fit(),
    fitSelection: () => context().canvasRef.current?.fitSelection(),
    play: () => context().togglePlay(),
    adaptLayout: () => context().adaptSelectedSize(),
    preview: () => context().previewSelectedBoundary(),
    export: () => context().openExportPanel(),
    undo: () => context().travel('undo_project'),
    redo: () => context().travel('redo_project'),
    save: () => context().save(),
    saveAs: () => context().save(true),
    closeProject: () => context().closeProject(),
  };
  for (const binding of TOOL_ACTIONS) {
    if (binding.action === 'requestTool' || binding.action === 'insert') continue;
    handlers[binding.action] = () => context().changeCanvasTool(binding.tool);
  }

  function canPerformAction(action: ShortcutAction): boolean {
    const { showStartPage, current, canvasRef } = context();
    if ((showStartPage || !current.current?.project) && !actionBinding(action).withoutProject) return false;
    return canvasRef.current?.canvasAction(action)?.enabled() ?? !!handlers[action];
  }

  function performAction(action: ShortcutAction, event: ActionEvent = NO_ACTION_EVENT) {
    if (!canPerformAction(action) || event.repeat && actionBinding(action).repeat === false) return;
    if (action === 'paste' && event !== NO_ACTION_EVENT) return;
    const { canvasRef, run } = context();
    const canvas = canvasRef.current?.canvasAction(action);
    if (canvas) {
      if (canvas.run({ key: event.key, slot: viewSlot(event) ?? undefined, repeat: event.repeat })) event.preventDefault();
      return;
    }
    const handler = handlers[action];
    if (!handler) return;
    event.preventDefault();
    run(async () => handler());
  }

  function actionMenuItem(action: ShortcutAction, id: string = action): import('./Menus').MenuItem {
    const binding = actionBinding(action);
    return { id, label: binding.label, shortcut: binding.display, disabled: !canPerformAction(action), action: () => performAction(action) };
  }

  return { performAction, canPerformAction, actionMenuItem };
}
