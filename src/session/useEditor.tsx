import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { native } from '../api';
import { AUTOSAVE_INTERVAL_MS } from '../generated/limits';
import { createEditorActions } from '../editorActions';
import type { Confirmation, Replacement } from './projectCommands';
import { selectionBounds } from '../projectCanvas/selectionGeometry';
import { errorValue, nativeErrorKind, type ErrorValue } from '../errorMessage.js';
import { type PendingRecovery, type RecoveryChoices } from '../RecoveryBanner';
import { needsAiSettings } from '../userErrors.js';
import { aiReady } from './aiReady';
import type { ProjectCanvasHandle } from '../ProjectCanvas';
import { propertySelection } from '../dock/selection';
import { usePanelState } from '../dock/panelState';
import { useExportJobs } from '../exportJobs/useExportJobs';
import { canvasView } from '../projectCanvas/canvasView';
import { playShouldPreview } from '../projectCanvas/behaviours';
import { findElementIn } from '../projectCanvas/elementFacts';
import { baseSizeId } from '../projectCanvas/sizes';
import { idlePlay, playClick, playIsActive, type PlayState } from '../projectCanvas/play';
import { dismissNewestOverlay } from '../escape';
import { isTextField } from '../textField';
import { shortcutFor } from '../shortcuts';
import type { AppSettingsView, GenerationJobView, GenerationMode, PointerStateView, WorkspaceView } from '../types';
import { editorMenu, paletteCommands } from './editorMenu';
import { useEditorPanel } from './editorPanel';
import { type SaveState, createProjectCommands } from './projectCommands';
import { projectEdits } from './projectEdits';
import { useAiSettings } from './useAiSettings';
import { useBoundaryCommands } from './useBoundaryCommands';
import { useCanvasEdits } from './useCanvasEdits';
import { useCanvasSelection } from './useCanvasSelection';
import { useCanvasTool } from './useCanvasTool';
import { useEditorTheme } from './useEditorTheme';
import { useGenerationFlow } from './useGenerationFlow';
import { useNamedViews } from './useNamedViews';
import { useNotices } from './useNotices';
import { useProjectSession } from './useProjectSession';
import { useRequestEdits } from './useRequestEdits';
import { useStartPageData } from './useStartPageData';
import { useUpdateCheck } from './useUpdateCheck';
import { useWindowClose } from './useWindowClose';

/** Assembles the editor from its feature hooks; the hooks own the state, this owns how they connect. */
export function useEditor({ initialView }: { initialView?: WorkspaceView } = {}) {
  const [loading, setLoading] = useState(!initialView);
  const [error, setError] = useState<ErrorValue>('');
  const { view, current, pending, accept, queue, mutate, settle } = useProjectSession(initialView, setError);
  const project = view?.project ?? null;
  const edits = useMemo(() => projectEdits(mutate), [mutate]);
  const run = useCallback((operation: () => Promise<unknown>) => { void operation().catch(error => setError(errorValue(error))); }, []);
  const [openFailure, setOpenFailure] = useState<{ message: string; url: string } | null>(null);
  const { notices, setNotice, dismissNotice } = useNotices(current, view);
  const { updateState, dismissUpdate, checkForUpdates, openRelease } = useUpdateCheck(setOpenFailure);
  const [showPalette, setShowPalette] = useState(false);
  const [dismissedRecovery, setDismissedRecovery] = useState<string[]>([]);
  const canvasRef = useRef<ProjectCanvasHandle>(null);
  const tool = useCanvasTool(canvasRef, project);
  const selection = useCanvasSelection(project);
  const { canvasSelection, canvasFocus, canvasFocusSize, setCanvasSelection, setCanvasFocus, setCanvasFocusSize, selectCanvasBoundary } = selection;
  const [canvasZoom, setCanvasZoom] = useState(1);
  const [panels, panelActions] = usePanelState();
  const { panel, openPanel, closePanel, togglePanel } = useEditorPanel();
  const startData = useStartPageData({ queue, setError });
  const { recentProjects, refreshRecent, forgetRecent, appSettings, setAppSettings } = startData;
  const [showStartPage, setShowStartPage] = useState(!initialView);
  const [play, setPlay] = useState<PlayState>(idlePlay);
  const [pendingRecovery, setPendingRecovery] = useState<PendingRecovery | null>(null);
  const { theme, setTheme, resolvedTheme } = useEditorTheme();
  const { settleAiSettings, showAiSettings, setShowAiSettings, aiSettings, aiCurrent, aiPollingError, refreshAiSettings, updateAiSettings, manageChatgptUsage } = useAiSettings();
  const [historyEpoch, setHistoryEpoch] = useState(0);
  const [replacement, setReplacement] = useState<Replacement | null>(null);
  const [confirmation, setConfirmation] = useState<Confirmation | null>(null);
  const [saving, setSaving] = useState<SaveState>({ kind: 'idle' });
  const saveState = useRef<SaveState>(saving);
  function transitionSave(next: SaveState) { saveState.current = next; setSaving(next); }
  const generationJob = useRef<GenerationJobView | null>(null);
  const { closeRequest, setCloseRequest } = useWindowClose({ settle, current, generation: generationJob, aiAuthentication: aiCurrent, setError });

  const selectedElementPageId = propertySelection(project, canvasSelection, canvasFocus, {}).selectedElementPageId;
  const exports = useExportJobs({ current, panels, panelActions, accept, setNotice, settle, setError, setOpenFailure, openPanel, closePanel, mutate, canvasFocus, selectedElementPageId, play, setPlay, view, panel, aiConnected: aiReady(aiSettings, null), spendingLimitUsd: aiSettings?.settings.spendingLimitUsd ?? null });
  const { preview, closePreview, checkedStates, checkedProject } = exports;

  const {
    chooseSaveConflict, closeUntitledSave, saveUntitledProject, save,
    closeProject, switchProjectTab, closeProjectTab, open, openRecent, newUntitled,
    recover, dismissRecoveries, travel, saveCopy, compactHistory } = createProjectCommands({
    setPendingRecovery, setError, saveState, transitionSave, current, panels, panelActions, settle, queue, accept, setNotice, refreshRecent,
    setReplacement, setCanvasSelection, setCanvasFocus, setShowStartPage,
    setConfirmation, resolvedTheme, closePanel, setHistoryEpoch });

  const { createCanvasBoundary, addProjectBoundary, updateCanvasPage, updateCanvasPageSize, addCanvasPageSize } = useBoundaryCommands(current, mutate, canvasRef, selectCanvasBoundary);
  const canvasEdits = useCanvasEdits({ current, mutate, edits, run, setNotice, travel, canvasRef, setCanvasSelection, setCanvasFocus, setCanvasFocusSize });
  const requestEdits = useRequestEdits({ current, mutate, edits, setError, setNotice, setCanvasSelection });
  const { views, saveCurrentView, addNamedViewAt, addNamedView, renameNamedView, removeNamedView } = useNamedViews(project, mutate, canvasRef, run);
  const changeBrief = useCallback(async (patch: Partial<NonNullable<typeof project>['brief']>) => {
    const before = current.current!.project!.brief;
    const workspace = await mutate(project => ({ type: 'set_brief', brief: { ...project.brief, ...patch } }));
    return workspace?.project?.brief ?? before;
  }, [mutate]);

  const load = useCallback(async () => {
    setLoading(true); setError('');
    try {
      const settings = await native<AppSettingsView>('get_app_settings');
      setAppSettings(settings);
      let loaded = await native<WorkspaceView>('get_workspace');
      if (!loaded.project && !settings.showStartPage) loaded = await native<WorkspaceView>('new_untitled_project', {});
      accept(loaded);
      setShowStartPage(settings.showStartPage && !loaded.project);
      if (!settings.showStartPage && loaded.recoveries.length) openPanel({ kind: 'recovery' });
    }
    catch (error) { setError(errorValue(error)); }
    finally { setLoading(false); }
  }, [accept]);
  useEffect(() => { void load(); }, [load]);
  useEffect(() => {
    if (!view?.dirty || !view.project) return;
    const timer = window.setInterval(() => { void native('autosave_project').catch(error => setError(errorValue(error))); }, AUTOSAVE_INTERVAL_MS);
    return () => window.clearInterval(timer);
  }, [view?.dirty, view?.project?.id]);
  useEffect(() => { if (showStartPage) closePanel('export'); }, [showStartPage]);
  useEffect(() => { if (!view?.recoveries.length) closePanel('recovery'); }, [view?.recoveries.length]);
  // The inline preview covers its page, so a tool that draws or types there would act on nothing.
  useEffect(() => { if (preview && tool.canvasTool.kind !== 'select' && tool.canvasTool.kind !== 'pan') closePreview(); }, [tool.canvasTool.kind]);

  async function importContext(analysisId: string, findings: number[], assets: string[], photos: string[], items: number[], pages: string[], attachMatching: boolean) {
    await settle();
    await queue(async workspace => {
      if (!workspace?.project) throw new Error('Create or open a project first.');
      const next = await native<WorkspaceView>('import_context', { revision: workspace.project.revision, identityAnalysisId: analysisId, identityFindingIndices: findings, identityAssetIds: assets, identityPhotoIds: photos, identityItemIndices: items, identityPagePaths: pages, attachMatchingPages: attachMatching });
      accept(next);
      setNotice('Context added to the project brief.');
    });
  }

  function togglePlay(pageId?: string | null) {
    const projectNow = current.current?.project;
    const target = pageId ?? canvasFocus ?? selectedElementPageId ?? projectNow?.pages[0]?.id ?? null;
    if (!target) { setNotice('Select a boundary to play.'); return; }
    // Behaviours only run as exported code, so starting Play hands this boundary to Preview.
    if (!playIsActive(play) && projectNow && playShouldPreview(projectNow, target)) { setCanvasFocus(target); exports.previewBoundary(target); return; }
    if (preview) closePreview();
    setPlay(current => current.boundary === target ? idlePlay : { ...idlePlay, boundary: target });
    setCanvasFocus(target);
    setCanvasSelection([]);
  }
  function playElement(pageId: string, elementId: string) {
    const projectNow = current.current?.project;
    if (!projectNow) return;
    setPlay(current => {
      if (current.boundary !== pageId) return current;
      const step = playClick(projectNow, current, pageId, elementId);
      if (step.goTo) {
        setCanvasFocus(step.goTo);
        canvasRef.current?.focusBoundary(step.goTo);
      }
      return step.play;
    });
  }

  const onStartPage = !project || showStartPage;
  const canvasBoundaries = project ? canvasView(project).boundaries : [];
  const pointerStates = view?.pointerStates ?? [];
  const effectivePointerStates = checkedStates && checkedProject === project?.id ? checkedStates : pointerStates;
  const pointerStateMap = Object.fromEntries(effectivePointerStates.map(item => [item.id, item])) as Record<string, PointerStateView>;
  const clashPointers = effectivePointerStates.filter(item => item.state === 'contradicts' || item.state === 'broken' || item.state === 'codeChanged');
  function selectNextPointerClash() {
    const ids = clashPointers.map(item => item.id);
    if (!ids.length) return;
    const selected = canvasSelection.length === 1 && canvasSelection[0].kind === 'pointer' ? canvasSelection[0].pointerId : null;
    const index = selected ? ids.indexOf(selected) : -1;
    setCanvasSelection([{ kind: 'pointer', pointerId: ids[(index + 1) % ids.length] }]);
  }

  const requireAiReady = useCallback(async () => {
    await settleAiSettings();
    const { readiness } = await refreshAiSettings();
    if (!readiness.ready) { setShowAiSettings(true); throw new Error(readiness.blockedReason ?? 'Review your AI settings, then try again.'); }
  }, [refreshAiSettings, settleAiSettings]);
  const flow = useGenerationFlow({ project, canGenerateFromBrief: !!view?.canGenerateFromBrief, canvasSelection, canvasBoundaries, queue, accept, mutate, requireAiReady, setNotice, setError, addFeedback: requestEdits.addFeedback });
  const { generation } = flow;
  generationJob.current = generation.job;
  useEffect(() => { closePanel('log'); }, [project?.id]);
  useEffect(() => { if (generation.visible) { openPanel({ kind: 'log', focus: 0 }); generation.close(); } }, [generation.visible]);
  const composeJob = generation.job && ['prepared', 'generating', 'ready', 'blocked', 'applying'].includes(generation.job.status) ? generation.job : null;
  const aiConnected = aiReady(aiSettings, composeJob);
  function openGeneration() {
    openPanel({ kind: 'log', focus: panel.kind === 'log' ? panel.focus + 1 : 1 });
    void generation.refresh();
    void refreshAiSettings().catch(error => setError(errorValue(error)));
  }
  function editGenerationRequest(id: string) {
    const owner = project?.pages.find(item => !!findElementIn(item.elements, id));
    if (owner) { setCanvasFocus(owner.id); setCanvasSelection([{ kind: 'element', pageId: owner.id, elementId: id, sizeId: baseSizeId(owner) }]); }
  }

  const actions = createEditorActions(() => ({
    run, showStartPage, current, pending, play, setPlay, openPanel, setShowPalette, setShowStartPage, setShowAiSettings,
    newUntitled, open, clearInsertKind: tool.clearInsertKind, bumpInsertSignal: tool.bumpInsertSignal, chooseTool: tool.chooseTool,
    panelActions, canvasRef, togglePlay, previewSelectedBoundary: exports.previewBoundary,
    openExportPanel: exports.openPanel, runExport: exports.run, openInBrowser: exports.openInBrowser,
    travel, save, saveCopy, closeProject, checkForUpdates, checkingForUpdates: updateState.kind === 'checking',
    hasRecoveries: !!view?.recoveries.length,
    adaptSelectedSize: () => {
      const page = current.current?.project?.pages.find(page => page.id === canvasFocus);
      const size = page?.sizes.find(size => size.id === canvasFocusSize);
      if (page && size && size.width < page.sizes[0].width) void mutate({ type: 'adapt_size', page_id: page.id, size_id: size.id });
    },
  }));
  const menuItems = editorMenu({
    actions, onStartPage, project, recentProjects, appSettings, theme, run, setTheme,
    working: flow.working, openRecent, forgetRecent, changeShowStartPage: startData.changeShowStartPage,
    setGenerationMode: (mode: GenerationMode) => mutate(current => current.generationMode === mode ? null : { type: 'set_generation_mode', mode }),
  });

  useEffect(() => {
    const listener = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      const editingText = isTextField(event.target);
      // Text fields keep their own editing history; WebKitGTK needs the explicit mapping.
      if (editingText && navigator.userAgent.includes('Linux') && (event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'z') {
        event.preventDefault(); document.execCommand(event.shiftKey ? 'redo' : 'undo'); return;
      }
      // One Escape path: the newest overlay closes first, then the canvas ends its own gesture, Play or preview.
      if (event.key === 'Escape' && !editingText && dismissNewestOverlay()) { event.preventDefault(); return; }
      if (!onStartPage && canvasRef.current?.handleKeyDown(event)) return;
      const canvasText = event.target instanceof HTMLElement && event.target.classList.contains('element-text-editor');
      const action = shortcutFor(event, editingText, canvasText, undefined, canvasRef.current?.containsTarget(event.target) ?? false);
      if (action) actions.perform(action, event);
    };
    const release = (event: KeyboardEvent) => canvasRef.current?.handleKeyUp(event);
    window.addEventListener('keydown', listener);
    window.addEventListener('keyup', release);
    return () => { window.removeEventListener('keydown', listener); window.removeEventListener('keyup', release); };
  });

  const problemAction = needsAiSettings(error) ? { label: 'AI settings', onClick: () => setShowAiSettings(true) }
    : project && nativeErrorKind(error) === 'saveLocation' ? { label: 'Choose another location', disabled: pending > 0, onClick: () => run(() => save(true)) } : undefined;
  const recovery: RecoveryChoices | null = pendingRecovery ? {
    path: pendingRecovery.path, busy: pending > 0,
    recover: () => run(() => recover(pendingRecovery.projectId)),
    openSaved: () => run(() => open(false, pendingRecovery.path, true)),
    dismiss: () => setPendingRecovery(null) } : null;

  const focusedBoundary = project && canvasFocus
    ? canvasBoundaries.find(item => item.pageId === canvasFocus && (!canvasFocusSize || item.sizeId === canvasFocusSize))
      ?? canvasBoundaries.find(item => item.pageId === canvasFocus) ?? null
    : null;
  const selectedBounds = project ? selectionBounds(project, canvasSelection) ?? (canvasSelection.length ? null : focusedBoundary?.rect ?? null) : null;

  return {
    ...tool, ...selection, ...canvasEdits, ...startData, ...flow,
    actions, edits, exports, canvasRef, run, mutate, accept, save,
    view, project, pending, onStartPage, loading, load, error, setError, problemAction,
    panel, openPanel, closePanel, togglePanel, panels, panelActions, historyEpoch,
    saving, replacement, setReplacement, confirmation, setConfirmation, chooseSaveConflict, closeUntitledSave, saveUntitledProject, compactHistory,
    notices, setNotice, dismissNotice, updateState, dismissUpdate, openRelease, openFailure, setOpenFailure, dismissedRecovery, setDismissedRecovery,
    closeRequest, setCloseRequest, showPalette, setShowPalette, paletteCommands: paletteCommands(actions, menuItems), menuItems,
    switchProjectTab, closeProjectTab, newUntitled, open, openRecent, recover, dismissRecoveries, recovery,
    resolvedTheme, setTheme, play, setPlay, togglePlay, playElement,
    canvasZoom, reportCanvasZoom: setCanvasZoom, canvasBoundaries, selectedBounds, zoomPercent: Math.round(canvasZoom * 100),
    pointerStateMap, clashPointers, selectNextPointerClash,
    aiConnected, aiSettings, showAiSettings, setShowAiSettings, refreshAiSettings, updateAiSettings, manageChatgptUsage, aiPollingError,
    openGeneration, editGenerationRequest, importContext, changeBrief,
    createCanvasBoundary, addProjectBoundary, updateCanvasPage, updateCanvasPageSize, addCanvasPageSize,
    views, saveCurrentView, addNamedViewAt, addNamedView, renameNamedView, removeNamedView,
    setCanvasElementRequest: requestEdits.setCanvasElementRequest, attachToRequest: requestEdits.attachToRequest,
    travel,
  };
}

/** Everything the editor's components read. Each component destructures only what it needs. */
export type Editor = ReturnType<typeof useEditor>;
