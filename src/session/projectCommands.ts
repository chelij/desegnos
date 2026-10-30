import { native } from '../api';
import { type ProjectCanvasHandle } from '../ProjectCanvas';
import { isPendingRecovery, type PendingRecovery } from '../RecoveryBanner';
import { type SaveConflict, type SaveOutcome, type SaveChoice } from '../SaveConflict';
import { type CanvasTarget } from '../projectCanvasGeometry';
import type { CanvasTemplate, CompactReport, WorkspaceView } from '../types';
import { Replacement, UntitledSaveRequest, Confirmation, initialZoom } from '../editorShared';

type Context = {
  setPendingRecovery: import("react").Dispatch<import("react").SetStateAction<PendingRecovery | null>>;
  setError: import("react").Dispatch<import("react").SetStateAction<string>>;
  saveChoice: import("react").RefObject<((choice: SaveChoice) => void) | null>;
  setSaveConflict: import("react").Dispatch<import("react").SetStateAction<SaveConflict | null>>;
  untitledSaveResolve: import("react").RefObject<((workspace: WorkspaceView | null) => void) | null>;
  setUntitledSave: import("react").Dispatch<import("react").SetStateAction<UntitledSaveRequest | null>>;
  untitledSaveFlight: import("react").RefObject<Promise<WorkspaceView | null> | null>;
  current: import("react").RefObject<WorkspaceView | null>;
  panels: import("../EditorDock").PanelState;
  panelActions: import("../EditorDock").PanelActions;
  settle: () => Promise<void>;
  queue: <T>(operation: (workspace: WorkspaceView | null) => Promise<T>, reportErrors?: boolean, quiet?: boolean) => Promise<T>;
  accept: (next: WorkspaceView) => void;
  setNotice: (message: string, undo?: () => void) => void;
  refreshRecent: () => Promise<void>;
  saveFlight: import("react").RefObject<Promise<WorkspaceView | null> | null>;
  setReplacement: import("react").Dispatch<import("react").SetStateAction<Replacement | null>>;
  setSelected: import("react").Dispatch<import("react").SetStateAction<string | null>>;
  setCanvasSelection: import("react").Dispatch<import("react").SetStateAction<CanvasTarget[]>>;
  setCanvasFocus: import("react").Dispatch<import("react").SetStateAction<string | null>>;
  setShowStartPage: import("react").Dispatch<import("react").SetStateAction<boolean>>;
  setConfirmation: import("react").Dispatch<import("react").SetStateAction<Confirmation | null>>;
  setZoom: import("react").Dispatch<import("react").SetStateAction<number>>;
  resolvedTheme: "dark" | "light";
  canvasRef: import("react").RefObject<ProjectCanvasHandle | null>;
  setShowRecovery: import("react").Dispatch<import("react").SetStateAction<boolean>>;
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
    const { saveChoice, setSaveConflict } = context(); const resolve = saveChoice.current; saveChoice.current = null; setSaveConflict(null); resolve?.(choice); }

  function closeUntitledSave(workspace: WorkspaceView | null) {
    const { untitledSaveResolve, setUntitledSave } = context();
    untitledSaveResolve.current?.(workspace);
    untitledSaveResolve.current = null;
    setUntitledSave(null);
  }

  function requestUntitledSave(): Promise<WorkspaceView | null> {
    const { untitledSaveFlight, current, panels, panelActions, setUntitledSave, untitledSaveResolve } = context();
    if (untitledSaveFlight.current) return untitledSaveFlight.current;
    const project = current.current?.project;
    if (!project) return Promise.resolve(null);
    if (panels.hidden) panelActions.toggleHidden();
    if (panels.collapsed) panelActions.setCollapsed(false);
    setUntitledSave({ name: project.name === 'Untitled project' ? '' : project.name });
    const promise = new Promise<WorkspaceView | null>(resolve => { untitledSaveResolve.current = resolve; });
    untitledSaveFlight.current = promise.finally(() => { untitledSaveFlight.current = null; });
    return untitledSaveFlight.current;
  }

  async function saveUntitledProject(name: string, path: string | null) {
    const { settle, queue, saveChoice, setSaveConflict, accept, setNotice, refreshRecent } = context();
    await settle();
    let outcome = await queue(() => native<SaveOutcome>('save_untitled_project', { name, path, label: null }));
    while (outcome.status === 'conflict') {
      const conflict = outcome.conflict;
      const choice = await new Promise<SaveChoice>(resolve => { saveChoice.current = resolve; setSaveConflict(conflict); });
      outcome = await queue(() => native<SaveOutcome>('resolve_save_conflict', { token: conflict.token, choice }));
    }
    if (outcome.status === 'cancelled') throw new Error('Save cancelled.');
    accept(outcome.workspace); setNotice('Project saved.'); void refreshRecent().catch(() => {});
    closeUntitledSave(outcome.workspace);
  }

  function save(saveAs = false): Promise<WorkspaceView | null> {
    const { current, saveFlight, settle, queue, saveChoice, setSaveConflict, accept, setNotice, refreshRecent } = context();
    if (current.current?.untitled) return requestUntitledSave();
    if (saveFlight.current) return saveFlight.current;
    const operation = async () => {
      await settle();
      const label = null;
      let outcome = await queue(() => native<SaveOutcome>('save_project_checked', { saveAs, label }));
      while (outcome.status === 'conflict') {
        const conflict = outcome.conflict;
        const choice = await new Promise<SaveChoice>(resolve => { saveChoice.current = resolve; setSaveConflict(conflict); });
        outcome = await queue(() => native<SaveOutcome>('resolve_save_conflict', { token: conflict.token, choice }));
      }
      if (outcome.status === 'cancelled') return null;
      accept(outcome.workspace); setNotice('Project saved.'); void refreshRecent().catch(() => {});
      return outcome.workspace;
    };
    saveFlight.current = operation().finally(() => { saveFlight.current = null; });
    return saveFlight.current;
  }

  async function replace(label: string, run: Replacement['run']) {
    const { settle, current, setReplacement } = context();
    await settle();
    if (current.current?.dirty) setReplacement({ label, run });
    else await run(false);
  }

  async function closeProject() {
    const { queue, accept, setSelected, setCanvasSelection, setCanvasFocus, setNotice, setShowStartPage } = context();
    await replace('close this project', async discard => {
      await queue(async () => {
        const next = await native<WorkspaceView>('close_project', { discard });
        accept(next);
        setSelected(null); setCanvasSelection([]); setCanvasFocus(null); setNotice('Project closed.');
        if (!next.project) setShowStartPage(true);
      });
    });
  }

  async function switchProjectTab(projectId: string) {
    const { current, settle, queue, accept, setShowStartPage, setSelected, setCanvasSelection, setCanvasFocus, setNotice } = context();
    if (projectId === current.current?.project?.id) return;
    await settle();
    await queue(async () => {
      accept(await native<WorkspaceView>('switch_project', { projectId }));
      setShowStartPage(false);
      setSelected(null); setCanvasSelection([]); setCanvasFocus(null); setNotice('Switched project.');
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
    const { queue, accept, setShowStartPage, setSelected, setNotice, refreshRecent, setZoom } = context();
    await openAlongside(async discard => {
      await queue(async () => {
        const opened = await requestProjectOpen(() => native<WorkspaceView | null>('open_project', { discard, folder, path: path ?? null, discardRecovery }));
        if (opened) { accept(opened); setShowStartPage(false); setSelected(null); setNotice('Project opened.'); void refreshRecent().catch(() => {}); if (opened.project) setZoom(initialZoom(opened.project.viewport.width, opened.project.viewport.height)); }
      });
    });
  }

  async function openRecent(path: string) {
    const { queue, accept, setShowStartPage, setSelected, setNotice, setZoom, refreshRecent } = context();
    await openAlongside(async discard => {
      await queue(async () => {
        const opened = await requestProjectOpen(() => native<WorkspaceView>('open_recent_project', { path, discard }));
        if (!opened) return;
        accept(opened); setShowStartPage(false); setSelected(null); setNotice('Project opened.');
        if (opened.project) setZoom(initialZoom(opened.project.viewport.width, opened.project.viewport.height));
        void refreshRecent().catch(() => {});
      }, false);
    });
  }

  async function newUntitled(template: CanvasTemplate = 'blank') {
    const { settle, queue, resolvedTheme, accept, setShowStartPage, setSelected, setCanvasSelection, setCanvasFocus, setZoom, canvasRef } = context();
    await settle();
    await queue(async () => {
      const next = await native<WorkspaceView>('new_untitled_project', { appearance: resolvedTheme, template });
      accept(next);
      setShowStartPage(false);
      setSelected(null); setCanvasSelection([]); setCanvasFocus(null);
      if (next.project) setZoom(initialZoom(next.project.viewport.width, next.project.viewport.height));
      requestAnimationFrame(() => canvasRef.current?.fit());
    });
  }

  async function recover(projectId: string) {
    const { queue, accept, setShowStartPage, setZoom, setSelected, setShowRecovery, setPendingRecovery, setNotice } = context();
    await openAlongside(async discard => {
      await queue(async () => {
        const recovered = await native<WorkspaceView>('recover_project', { projectId, discard });
        accept(recovered);
        setShowStartPage(false);
        if (recovered.project) setZoom(initialZoom(recovered.project.viewport.width, recovered.project.viewport.height));
        setSelected(null); setShowRecovery(false); setPendingRecovery(null);
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
    requestProjectOpen, chooseSaveConflict, closeUntitledSave, requestUntitledSave, saveUntitledProject, save,
    replace, closeProject, switchProjectTab, closeProjectTab, openAlongside, open, openRecent, newUntitled,
    recover, dismissRecoveries, travel, saveCopy, compactHistory,
  };
}
