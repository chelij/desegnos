import type { Dispatch, RefObject, SetStateAction } from 'react';
import type { EditorTool } from './EditorTools';
import type { MenuItem } from './Menus';
import type { ProjectCanvasHandle } from './ProjectCanvas';
import { ActionEvent, NO_ACTION_EVENT } from './editorShared';
import type { PanelActions } from './dock/panelState';
import { playIsActive, type PlayState, idlePlay } from './projectCanvas/play';
import type { OpenPanel } from './session/editorPanel';
import { actionKeys, actionLabel, bindingOf, TOOL_ACTIONS, viewSlot, type ActionId, type ShortcutAction } from './shortcuts';
import type { CanvasTemplate, WorkspaceView } from './types';

type Context = {
  run: (action: () => Promise<unknown>) => void;
  showStartPage: boolean;
  current: RefObject<WorkspaceView | null>;
  pending: number;
  play: PlayState;
  setPlay: Dispatch<SetStateAction<PlayState>>;
  openPanel: (panel: OpenPanel) => void;
  setShowPalette: Dispatch<SetStateAction<boolean>>;
  setShowStartPage: Dispatch<SetStateAction<boolean>>;
  setShowAiSettings: Dispatch<SetStateAction<boolean>>;
  newUntitled: (template?: CanvasTemplate) => Promise<void>;
  open: (folder?: boolean, path?: string, discardRecovery?: boolean) => Promise<void>;
  clearInsertKind: () => void;
  bumpInsertSignal: () => void;
  chooseTool: (tool: EditorTool) => void;
  panelActions: PanelActions;
  canvasRef: RefObject<ProjectCanvasHandle | null>;
  togglePlay: (pageId?: string | null) => void;
  previewSelectedBoundary: () => void;
  adaptSelectedSize: () => void;
  openExportPanel: (restore?: boolean) => void;
  runExport: (previewPageId?: string | null) => Promise<void>;
  openInBrowser: () => Promise<void>;
  travel: (command: 'undo_project' | 'redo_project') => Promise<void>;
  save: (saveAs?: boolean) => Promise<WorkspaceView | null>;
  saveCopy: () => Promise<void>;
  closeProject: () => Promise<void>;
  checkForUpdates: () => Promise<void>;
  checkingForUpdates: boolean;
  hasRecoveries: boolean;
};

/** What an action does and when it may. The canvas supplies its own entries and wins when it has one. */
type Entry = { enabled?: () => boolean; run: () => void | Promise<unknown>; withoutProject?: boolean };

/** One registry for every command the app offers: keys, menu, palette and buttons all ask it whether an action is enabled and then run it. */
export function createEditorActions(context: () => Context) {
  const editorProject = () => !context().showStartPage && !!context().current.current?.project;
  const canvas = () => context().canvasRef.current;
  const entries: Partial<Record<ActionId, Entry>> = {
    help: { run: () => context().openPanel({ kind: 'shortcuts' }) },
    commandPalette: { run: () => context().setShowPalette(true) },
    new: { run: () => context().newUntitled() },
    open: { run: () => context().open() },
    openFolder: { run: () => context().open(true), withoutProject: true },
    renameLayer: { run: () => {
      const field = document.querySelector<HTMLInputElement>('[data-field="layer-name"]');
      field?.focus({ preventScroll: true }); field?.select();
    } },
    insert: { run: () => { context().clearInsertKind(); context().bumpInsertSignal(); } },
    askAI: { run: () => { canvas()?.startRequestOnSelection(); } },
    hidePanels: { run: () => context().panelActions.toggleHidden() },
    zoomIn: { run: () => canvas()?.zoomStep(1) },
    zoomOut: { run: () => canvas()?.zoomStep(-1) },
    zoomReset: { run: () => canvas()?.zoomTo(1) },
    fitAll: { run: () => canvas()?.fit() },
    fitSelection: { run: () => canvas()?.fitSelection() },
    play: { run: () => { const { play, setPlay, togglePlay } = context(); if (playIsActive(play)) setPlay(idlePlay); else togglePlay(); } },
    adaptLayout: { run: () => context().adaptSelectedSize() },
    preview: { run: () => context().previewSelectedBoundary() },
    export: { run: () => { context().openExportPanel(); return context().runExport(); } },
    openInBrowser: { run: () => context().openInBrowser() },
    undo: { enabled: () => !!context().current.current?.canUndo, run: () => context().travel('undo_project') },
    redo: { enabled: () => !!context().current.current?.canRedo, run: () => context().travel('redo_project') },
    save: { run: () => context().save() },
    saveAs: { run: () => context().save(true) },
    saveCopy: { enabled: () => context().pending === 0, run: () => context().saveCopy() },
    closeProject: { run: () => context().closeProject() },
    renameProject: { run: () => context().openPanel({ kind: 'rename' }) },
    recoverUnsaved: { enabled: () => context().hasRecoveries, run: () => context().openPanel({ kind: 'recovery' }), withoutProject: true },
    aiSettings: { run: () => context().setShowAiSettings(true), withoutProject: true },
    styleGuide: { run: () => context().openPanel({ kind: 'styleGuide' }) },
    importContext: { run: () => context().openPanel({ kind: 'import', source: null }) },
    projectBrief: { run: () => context().openPanel({ kind: 'notes', section: 'overview' }) },
    projectLocation: { run: () => context().openPanel({ kind: 'projectLocation' }), withoutProject: true },
    checkUpdates: { enabled: () => !context().checkingForUpdates, run: () => context().checkForUpdates(), withoutProject: true },
    home: { run: () => context().setShowStartPage(true), withoutProject: true },
    backToCanvas: { run: () => context().setShowStartPage(false), withoutProject: true },
  };
  for (const binding of TOOL_ACTIONS) {
    if (binding.action === 'requestTool' || binding.action === 'insert') continue;
    entries[binding.action] = { run: () => context().chooseTool(binding.tool) };
  }

  /** Whether the action exists and may run now. */
  function can(action: ActionId): boolean {
    const entry = entries[action];
    if (!editorProject() && !(entry?.withoutProject ?? bindingOf(action)?.withoutProject)) return false;
    const fromCanvas = canvas()?.canvasAction(action as ShortcutAction);
    if (fromCanvas) return fromCanvas.enabled();
    return !!entry && (entry.enabled?.() ?? true);
  }

  function perform(action: ActionId, event: ActionEvent = NO_ACTION_EVENT) {
    if (!can(action) || event.repeat && bindingOf(action)?.repeat === false) return;
    if (action === 'paste' && event !== NO_ACTION_EVENT) return;
    const fromCanvas = canvas()?.canvasAction(action as ShortcutAction);
    if (fromCanvas) {
      if (fromCanvas.run({ key: event.key, slot: viewSlot(event) ?? undefined, repeat: event.repeat })) event.preventDefault();
      return;
    }
    event.preventDefault();
    context().run(async () => entries[action]!.run());
  }

  function menuItem(action: ActionId, id: string = action): MenuItem {
    return { id, label: actionLabel(action), shortcut: actionKeys(action), disabled: !can(action), action: () => perform(action) };
  }

  return { perform, can, menuItem };
}

/** The editor-level actions, for components that render buttons from them. */
export type EditorActions = ReturnType<typeof createEditorActions>;
