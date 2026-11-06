import { native } from '../api';
import { type ProjectCanvasHandle } from '../ProjectCanvas';
import { isPendingRecovery, type PendingRecovery } from '../RecoveryBanner';
import { type SaveConflict, type SaveOutcome, type SaveChoice } from '../SaveConflict';
import { type CanvasTarget } from '../projectCanvasGeometry';
import type { CanvasTemplate, CompactReport, WorkspaceView } from '../types';
import { Replacement, Confirmation } from '../editorShared';

type UntitledSave = {
  name: string;
  resolve: (workspace: WorkspaceView | null) => void;
};
type SaveFlight = { promise: Promise<WorkspaceView | null>; untitled: UntitledSave | null };
export type SaveState =
  | { kind: 'idle' }
  | (SaveFlight & { kind: 'naming'; untitled: UntitledSave })
  | (SaveFlight & { kind: 'saving' })
  | (SaveFlight & { kind: 'conflict'; conflict: SaveConflict; choose: (choice: SaveChoice) => void });

type Context = {
  setPendingRecovery: import("react").Dispatch<import("react").SetStateAction<PendingRecovery | null>>;
  setError: import("react").Dispatch<import("react").SetStateAction<string>>;
  saveState: import("react").RefObject<SaveState>;
  transitionSave: (state: SaveState) => void;
  current: import("react").RefObject<WorkspaceView | null>;
  panels: import("../EditorDock").PanelState;
  panelActions: import("../EditorDock").PanelActions;
  settle: () => Promise<void>;
  queue: <T>(operation: (workspace: WorkspaceView | null) => Promise<T>, reportErrors?: boolean, quiet?: boolean) => Promise<T>;
  accept: (next: WorkspaceView) => void;
  setNotice: (message: string, undo?: () => void) => void;
  refreshRecent: () => Promise<void>;
  setReplacement: import("react").Dispatch<import("react").SetStateAction<Replacement | null>>;
  setCanvasSelection: import("react").Dispatch<import("react").SetStateAction<CanvasTarget[]>>;
  setCanvasFocus: import("react").Dispatch<import("react").SetStateAction<string | null>>;
  setShowStartPage: import("react").Dispatch<import("react").SetStateAction<boolean>>;
  setConfirmation: import("react").Dispatch<import("react").SetStateAction<Confirmation | null>>;
  resolvedTheme: "dark" | "light";
  canvasRef: import("react").RefObject<ProjectCanvasHandle | null>;
  closePanel: (kind: import('./useEditor').EditorPanel['kind']) => void;
  setHistoryEpoch: import("react").Dispatch<import("react").SetStateAction<number>>;
};

export function createProjectCommands(context: () => Context) {
  async function requestProjectOpen<T>(operation: () => Promise<T>): Promise<T | null> {
    const { setPendingRecovery, setError } = context();
    setPendingRecovery(null);
    try { return await operation(); }
    catch (error) {
      if (!isPendingRecovery(error)) throw error;
      setError(''); setPendingRecovery(error);
      return null;
    }
  }

  function chooseSaveConflict(choice: SaveChoice) {
    const { saveState, transitionSave } = context();
    const state = saveState.current;
    if (state.kind !== 'conflict') return;
    transitionSave({ kind: 'saving', promise: state.promise, untitled: state.untitled });
    state.choose(choice);
  }

  function closeUntitledSave(workspace: WorkspaceView | null) {
    const { saveState, transitionSave } = context();
    const state = saveState.current;
    if (state.kind === 'idle' || !state.untitled) return;
    // Closing the name dialog must not discard a pending conflict choice.
    transitionSave(state.kind === 'conflict' ? { ...state, untitled: null } : { kind: 'idle' });
    state.untitled.resolve(workspace);
  }

  function requestUntitledSave(): Promise<WorkspaceView | null> {
    const { saveState, current, panels, panelActions, transitionSave } = context();
    if (saveState.current.kind !== 'idle') return saveState.current.promise;
    const project = current.current?.project;
    if (!project) return Promise.resolve(null);
    if (panels.hidden) panelActions.toggleHidden();
    if (panels.collapsed) panelActions.setCollapsed(false);
    let resolve!: UntitledSave['resolve'];
    const promise = new Promise<WorkspaceView | null>(done => { resolve = done; });
    transitionSave({ kind: 'naming', promise, untitled: { name: project.name === 'Untitled project' ? '' : project.name, resolve } });
    return promise;
  }

  async function resolveSaveOutcome(outcome: SaveOutcome, flight: SaveFlight) {
    const { queue, saveState, transitionSave } = context();
    while (outcome.status === 'conflict') {
      const conflict = outcome.conflict;
      const choice = await new Promise<SaveChoice>(choose => {
        const current = saveState.current;
        const untitled = current.kind !== 'idle' && current.promise === flight.promise ? current.untitled : null;
        transitionSave({ kind: 'conflict', promise: flight.promise, untitled, conflict, choose });
      });
      outcome = await queue(() => native<SaveOutcome>('resolve_save_conflict', { token: conflict.token, choice }));
    }
    return outcome;
  }

  async function saveUntitledProject(name: string, path: string | null) {
    const { saveState, transitionSave, settle, queue, accept, setNotice, refreshRecent } = context();
    const state = saveState.current;
    if (state.kind !== 'naming') return;
    transitionSave({ kind: 'saving', promise: state.promise, untitled: state.untitled });
    try {
      await settle();
      const outcome = await resolveSaveOutcome(await queue(() => native<SaveOutcome>('save_untitled_project', { name, path, label: null })), state);
      if (outcome.status === 'cancelled') throw new Error('Save cancelled.');
      accept(outcome.workspace); setNotice('Project saved.'); void refreshRecent().catch(() => {});
      if (saveState.current.kind !== 'idle' && saveState.current.promise === state.promise) {
        transitionSave({ kind: 'idle' });
      }
      state.untitled.resolve(outcome.workspace);
    } catch (error) {
      // A failed attempt leaves the name dialog and its original save promise available for retry.
      if (saveState.current.kind !== 'idle' && saveState.current.promise === state.promise) {
        transitionSave(saveState.current.untitled ? state : { kind: 'idle' });
      }
      throw error;
    }
  }

  function save(saveAs = false): Promise<WorkspaceView | null> {
    const { current, saveState, transitionSave, settle, queue, accept, setNotice, refreshRecent } = context();
    if (saveState.current.kind !== 'idle') return saveState.current.promise;
    if (current.current?.untitled) return requestUntitledSave();
    // Install the flight before work starts, so every concurrent caller receives the same promise.
    const promise = Promise.resolve().then(async () => {
      await settle();
      const outcome = await resolveSaveOutcome(await queue(() => native<SaveOutcome>('save_project_checked', { saveAs, label: null })), { promise, untitled: null });
      if (outcome.status === 'cancelled') return null;
      accept(outcome.workspace); setNotice('Project saved.'); void refreshRecent().catch(() => {});
      return outcome.workspace;
    }).finally(() => {
      if (saveState.current.kind !== 'idle' && saveState.current.promise === promise) transitionSave({ kind: 'idle' });
    });
    transitionSave({ kind: 'saving', promise, untitled: null });
    return promise;
  }

  async function replace(label: string, run: Replacement['run']) {
    const { settle, current, setReplacement } = context();
    await settle();
    if (current.current?.dirty) setReplacement({ label, run });
    else await run(false);
  }

  async function closeProject() {
    const { queue, accept, setCanvasSelection, setCanvasFocus, setNotice, setShowStartPage } = context();
    await replace('close this project', async discard => {
      await queue(async () => {
        const next = await native<WorkspaceView>('close_project', { discard });
        accept(next); setCanvasSelection([]); setCanvasFocus(null); setNotice('Project closed.');
        if (!next.project) setShowStartPage(true);
      });
    });
  }

  async function switchProjectTab(projectId: string) {
    const { current, settle, queue, accept, setShowStartPage, setCanvasSelection, setCanvasFocus, setNotice } = context();
    if (projectId === current.current?.project?.id) return;
    await settle();
    await queue(async () => {
      accept(await native<WorkspaceView>('switch_project', { projectId }));
      setShowStartPage(false); setCanvasSelection([]); setCanvasFocus(null); setNotice('Switched project.');
    }).catch(() => {});
  }

  async function closeProjectTab(projectId: string) {
    const { current, setConfirmation, queue, accept, setNotice } = context();
    const entry = current.current?.openProjects?.find(item => item.id === projectId);
    if (!entry) return;
    if (projectId === current.current?.project?.id) {
      await closeProject();
      return;
    }
    if (entry.dirty) {
      setConfirmation({ title: `Close “${entry.name}”?`, confirmLabel: 'Close project', message: 'This project has changes that are not saved to its file. Closing it will discard them.', action: async () => {
        await queue(async () => {
          accept(await native<WorkspaceView>('close_project', { projectId, discard: true }));
          setNotice('Project closed.');
        });
      } });
      return;
    }
    await queue(async () => {
      accept(await native<WorkspaceView>('close_project', { projectId, discard: false }));
      setNotice('Project closed.');
    }).catch(() => {});
  }

  async function openAlongside(run: Replacement['run']) {
    const { settle } = context(); await settle(); await run(false); }

  async function open(folder = false, path?: string, discardRecovery = false) {
    const { queue, accept, setShowStartPage, refreshRecent } = context();
    await openAlongside(async discard => {
      await queue(async () => {
        const opened = await requestProjectOpen(() => native<WorkspaceView | null>('open_project', { discard, folder, path: path ?? null, discardRecovery }));
        if (opened) { accept(opened); setShowStartPage(false); void refreshRecent().catch(() => {}); }
      });
    });
  }

  async function openRecent(path: string) {
    const { queue, accept, setShowStartPage, refreshRecent } = context();
    await openAlongside(async discard => {
      await queue(async () => {
        const opened = await requestProjectOpen(() => native<WorkspaceView>('open_recent_project', { path, discard }));
        if (!opened) return;
        accept(opened); setShowStartPage(false);
        void refreshRecent().catch(() => {});
      }, false);
    });
  }

  async function newUntitled(template: CanvasTemplate = 'blank') {
    const { settle, queue, resolvedTheme, accept, setShowStartPage, setCanvasSelection, setCanvasFocus, canvasRef } = context();
    await settle();
    await queue(async () => {
      const next = await native<WorkspaceView>('new_untitled_project', { appearance: resolvedTheme, template });
      accept(next);
      setShowStartPage(false); setCanvasSelection([]); setCanvasFocus(null);
      requestAnimationFrame(() => canvasRef.current?.fit());
    });
  }

  async function recover(projectId: string) {
    const { queue, accept, setShowStartPage, closePanel, setPendingRecovery, setNotice } = context();
    await openAlongside(async discard => {
      await queue(async () => {
        const recovered = await native<WorkspaceView>('recover_project', { projectId, discard });
        accept(recovered);
        setShowStartPage(false); closePanel('recovery'); setPendingRecovery(null);
        setNotice('Project recovered.');
      });
    });
  }

  async function dismissRecoveries(projectIds: string[]) {
    const { queue, current, accept } = context();
    await queue(async () => {
      for (const projectId of projectIds) {
        if (!current.current?.recoveries.some(item => item.projectId === projectId)) continue;
        accept(await native<WorkspaceView>('dismiss_recovery', { projectId }));
      }
    });
  }

  async function travel(command: 'undo_project' | 'redo_project') {
    const { queue, accept, setNotice, current } = context();
    const projectId = current.current?.project?.id;
    await queue(async workspace => {
      if (workspace?.project?.id !== projectId || !(command === 'undo_project' ? workspace?.canUndo : workspace?.canRedo)) return;
      accept(await native<WorkspaceView>(command));
      setNotice(command === 'undo_project' ? 'Change undone.' : 'Change redone.');
    });
  }

  async function saveCopy() {
    const { queue, setNotice } = context();
    const result = await queue(() => native<WorkspaceView | null>('save_project_copy'));
    if (result) setNotice('Saved a copy without history.');
  }

  async function compactHistory() {
    const { queue, setHistoryEpoch, setNotice } = context();
    const report = await queue(() => native<CompactReport>('compact_history'));
    setHistoryEpoch(value => value + 1);
    setNotice(report.removedVersions === 0 && report.removedBlobs === 0
      ? 'History is already compact.'
      : `History compacted: removed ${report.removedVersions} ${report.removedVersions === 1 ? 'version' : 'versions'}.`);
  }

  return {
    chooseSaveConflict, closeUntitledSave, saveUntitledProject, save,
    closeProject, switchProjectTab, closeProjectTab, open, openRecent, newUntitled,
    recover, dismissRecoveries, travel, saveCopy, compactHistory,
  };
}
