import { useBoundaryCommands } from './useBoundaryCommands';
import { propertySelection } from '../dock/selection';
import { useAiSettings } from './useAiSettings';
import { useEditorTheme } from './useEditorTheme';
import { useNamedViews } from './useNamedViews';
import { type BoundaryToolKind, projectKind } from '../projectCanvas/boundaryDefaults';
import { canvasView } from '../projectCanvas/canvasView';
import { useProjectSession } from './useProjectSession';
import { type SaveState, createProjectCommands } from './projectCommands';
import { useExportJobs } from '../exportJobs/useExportJobs';
import { createEditorShortcuts } from '../editorShortcuts';
import { Theme, Replacement, Confirmation, errorMessage, ownerPageId, selectionBounds, menuPaletteCommands } from '../editorShared';
import { readRequestAttachments } from '../requestAttachments';
import { elementTree, elementTreeIds } from '../projectCanvas/elementGeometry';
import { needsAiSettings } from '../ProblemBanner.js';
import { updateSummary, type UpdateCheck, type UpdateSummary } from '../updateCheck';
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { projectEdits } from './projectEdits';
import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { native } from '../api';
import { type EditorTool, type ProjectTool, type RequestToolShape } from '../EditorTools';
import { isTextField } from '../WorkspaceSurface';
import { type ProjectCanvasHandle } from '../ProjectCanvas';
import { type PendingRecovery, type RecoveryChoices } from '../RecoveryBanner';
import type { ProjectLocationView, RequestArea } from '../types';
import { type DossierSection } from '../Notes';
import { type MenuItem } from '../Menus';
import { KEYMAP, shortcutFor, shortcutLabel } from '../shortcuts';
import { toggleCanvasTarget, selectedReference, type CanvasTarget } from '../projectCanvasGeometry';

import { elementPath, findElementIn } from '../projectCanvas/elementGeometry';
import { usePanelState } from '../dock/panelState';
import { announcesStatus } from '../generationSession.js';
import { type PaletteCommand } from '../CommandPalette';
import { activeGeneration, useGeneration } from '../useGeneration';
import type { AppSettingsView, Brief, ElementKind, GenerationJobView, GenerationMode, GridSettings, PendingRequest, Pointer, PointerSource, PointerStateView, Project, Rect, RequestAttachment, WorkspaceView, RecentProjectsView, RecentProject } from '../types';
import { baseSizeId, resolveForSize } from '../projectCanvas/sizes';
import { coveredTargets } from '../projectCanvas/requests';
import { idlePlay, playClick, playIsActive, type PlayState } from '../projectCanvas/play';
import { behaviourSourceKey, playShouldPreview } from '../projectCanvas/behaviours';

export type EditorPanel =
  | { kind: 'closed' }
  | { kind: 'notes'; section: DossierSection }
  | { kind: 'import'; source: import('../types').IdentitySource | null }
  | { kind: 'log'; focus: number }
  | { kind: 'gridSettings' | 'styleGuide' | 'shortcuts' | 'recovery' | 'rename' | 'projectLocation' | 'history' | 'export' };

type CanvasToolState =
  | { kind: Exclude<ProjectTool, 'insert'> }
  | { kind: 'insert'; insertKind: ElementKind | null };

type UpdateState =
  | { kind: 'idle' | 'checking' }
  | { kind: 'done'; result: UpdateSummary }
  | { kind: 'failed'; message: string };

export function useEditor({ initialView }: { initialView?: WorkspaceView } = {}) {

  const {
    performAction,
  } = createEditorShortcuts(() => ({
    showStartPage, current, openPanel, setShowPalette, newUntitled, open, clearInsertKind,
    setInsertSignal, changeCanvasTool, panelActions, canvasRef, togglePlay, previewSelectedBoundary,
    openExportPanel, runExport, travel, save, closeProject,
    adaptSelectedSize: () => {
      const page = current.current?.project?.pages.find(page => page.id === canvasFocus);
      const size = page?.sizes.find(size => size.id === canvasFocusSize);
      if (page && size && size.width < page.sizes[0].width) void mutate({ type: 'adapt_size', page_id: page.id, size_id: size.id }).catch(() => {});
    },
  }));

  const [closeRequest, setCloseRequest] = useState<{ message: string; resolve: (answer: boolean) => void } | null>(null);
  const [loading, setLoading] = useState(!initialView);
  const [error, setError] = useState('');
  const { view, current, pending, accept, queue, mutate, settle } = useProjectSession(initialView, setError);
  const edits = useMemo(() => projectEdits(mutate), [mutate]);
  const [openFailure, setOpenFailure] = useState<{ message: string; url: string } | null>(null);
  const [updateState, setUpdateState] = useState<UpdateState>({ kind: 'idle' });
  function dismissUpdate() { setUpdateState({ kind: 'idle' }); }
  const [notices, setNotices] = useState<{ id: number; message: string; undo?: () => void; history: string }[]>([]);
  const noticeId = useRef(0);
  // A notice may carry one reversible follow-up, which the toast offers as Undo; repeating a
  // message restarts that toast instead of stacking an identical row.
  const setNotice = useCallback((message: string, undo?: () => void) => {
    const id = ++noticeId.current;
    const project = current.current?.project;
    setNotices(items => [...items.filter(item => item.message !== message), { id, message, undo, history: project ? `${project.id}:${project.revision}` : '' }]);
  }, []);
  // Undo acts on the change it announced, so it leaves as soon as the history moves on.
  const historyKey = view?.project ? `${view.project.id}:${view.project.revision}` : '';
  useLayoutEffect(() => {
    setNotices(items => {
      const kept = items.filter(item => !item.undo || item.history === historyKey);
      return kept.length === items.length ? items : kept;
    });
  }, [historyKey]);
  const [showPalette, setShowPalette] = useState(false);
  const [dismissedRecovery, setDismissedRecovery] = useState<string[]>([]);
  const [canvasTool, setCanvasTool] = useState<CanvasToolState>({ kind: 'select' });
  function changeCanvasTool(kind: ProjectTool, insertKind?: ElementKind | null) {
    setCanvasTool(previous => kind === 'insert'
      ? { kind, insertKind: insertKind === undefined ? (previous.kind === 'insert' ? previous.insertKind : null) : insertKind }
      : { kind });
  }
  function clearInsertKind() {
    setCanvasTool(previous => previous.kind === 'insert' ? { kind: 'insert', insertKind: null } : previous);
  }
  const [insertSignal, setInsertSignal] = useState(0);
  const [boundaryKind, setBoundaryKind] = useState<BoundaryToolKind>('page');
  const [requestToolShape, setRequestToolShape] = useState<RequestToolShape>('pick');
  const [canvasSelection, setCanvasSelection] = useState<CanvasTarget[]>([]);
  const [canvasFocus, setCanvasFocus] = useState<string | null>(null);
  const [widthPreview, setWidthPreview] = useState<{ sizeId: string; width: number } | null>(null);
  const [canvasFocusSize, setCanvasFocusSize] = useState<string | null>(null);
  const [behaviourFocus, setBehaviourFocus] = useState<{ source: string; token: number } | null>(null);
  const behaviourToken = useRef(0);

  const [canvasZoom, setCanvasZoom] = useState(1);
  const canvasRef = useRef<ProjectCanvasHandle>(null);
  const [panels, panelActions] = usePanelState();
  const [recentProjects, setRecentProjects] = useState<RecentProject[]>([]);
  const [appSettings, setAppSettings] = useState<AppSettingsView>({ showStartPage: true });
  const [showStartPage, setShowStartPage] = useState(!initialView);
  const [projectLocation, setProjectLocation] = useState<ProjectLocationView | null>(null);
  const [play, setPlay] = useState<PlayState>(idlePlay);
  const [pendingRecovery, setPendingRecovery] = useState<PendingRecovery | null>(null);

  const [panel, setPanel] = useState<EditorPanel>({ kind: 'closed' });
  function openPanel(next: Exclude<EditorPanel, { kind: 'closed' }>) {
    setPanel(previous => next.kind === 'log' && next.focus === 0 && previous.kind === 'log' ? previous : next);
  }
  const closePanel = useCallback((kind: EditorPanel['kind']) => {
    setPanel(previous => previous.kind === kind ? { kind: 'closed' } : previous);
  }, []);
  function togglePanel(next: Exclude<EditorPanel, { kind: 'closed' }>) {
    setPanel(previous => previous.kind === next.kind ? { kind: 'closed' } : next);
  }
  const { theme, setTheme, resolvedTheme } = useEditorTheme();
  const { settleAiSettings, showAiSettings, setShowAiSettings, aiSettings, aiCurrent, aiPollingError, refreshAiSettings, updateAiSettings, manageChatgptUsage } = useAiSettings();
  const [historyEpoch, setHistoryEpoch] = useState(0);
  const [replacement, setReplacement] = useState<Replacement | null>(null);
  const [confirmation, setConfirmation] = useState<Confirmation | null>(null);
  const closing = useRef(false);
  const [saving, setSaving] = useState<SaveState>({ kind: 'idle' });
  const saveState = useRef<SaveState>(saving);
  function transitionSave(next: SaveState) { saveState.current = next; setSaving(next); }
  const generationRef = useRef<GenerationJobView | null>(null);
  const generationOwns = useRef(false);
  const briefGenerateFlight = useRef(false);
  const project = view?.project ?? null;
  const selectedElementPageId = propertySelection(project, canvasSelection, canvasFocus, {}).selectedElementPageId;
  const exportAccount = aiSettings?.chatgpt.accounts.find(account => account.id === aiSettings.chatgpt.activeAccountId);
  const exportConnected = !!aiSettings && !aiSettings.configurationError && (aiSettings.authentication === 'chatgpt'
    ? !!exportAccount?.connected && exportAccount.planEnabled && !exportAccount.storageRepairRequired : aiSettings.credential.configured);
  const exports = useExportJobs({ current, panels, panelActions, accept, setNotice, settle, setError, setOpenFailure, openPanel, closePanel, mutate, canvasFocus, selectedElementPageId, play, setPlay, view, panel, aiConnected: exportConnected, spendingLimitUsd: aiSettings?.settings.spendingLimitUsd ?? null });
  const { openPanel: openExportPanel, run: runExport, previewBoundary: previewSelectedBoundary, closePreview, checkedStates, checkedProject, preview } = exports;

  const refreshRecent = useCallback(async () => {
    try {
      const value = await native<RecentProjectsView>('get_recent_projects');
      setRecentProjects(value.projects);
      if (value.error) setError(`Recent projects: ${value.error}`);
    } catch (error) { setError(errorMessage(error)); }
  }, []);
  useEffect(() => { void refreshRecent().catch(error => setError(errorMessage(error))); }, [refreshRecent]);
  useEffect(() => { void native<ProjectLocationView>('get_project_location').then(setProjectLocation).catch(error => setError(errorMessage(error))); }, []);
  // The export panel belongs to an open canvas; Home and project switches close it.
  useEffect(() => { if (showStartPage) closePanel('export'); }, [showStartPage]);
  async function changeProjectLocation(reset: boolean) {
    const path = reset ? null : await native<string | null>('pick_project_location');
    if (!reset && !path) return;
    setProjectLocation(await native<ProjectLocationView>('set_project_location', { path }));
  }

  const {
    chooseSaveConflict, closeUntitledSave, saveUntitledProject, save,
    closeProject, switchProjectTab, closeProjectTab, open, openRecent, newUntitled,
    recover, dismissRecoveries, travel, saveCopy, compactHistory } = createProjectCommands({
    setPendingRecovery, setError, saveState, transitionSave, current, panels, panelActions, settle, queue, accept, setNotice, refreshRecent,
    setReplacement, setCanvasSelection, setCanvasFocus, setShowStartPage,
    setConfirmation, resolvedTheme, canvasRef, closePanel, setHistoryEpoch });
  useEffect(() => {
    if (view?.project) setBoundaryKind(projectKind(view.project));
  }, [view?.project?.id]);

  const selectCanvasBoundary = useCallback((pageId: string | null, sizeId?: string | null, additive = false) => {
    setCanvasFocus(pageId);
    setCanvasFocusSize(sizeId ?? null);
    if (!additive || !pageId) setCanvasSelection([]);
  }, []);
  const { createCanvasBoundary, addProjectBoundary, updateCanvasPage, updateCanvasPageSize, addCanvasPageSize } = useBoundaryCommands(current, mutate, canvasRef, selectCanvasBoundary);
  const selectCanvasReference = useCallback((itemId: string, additive = false) => {
    setCanvasSelection(current => toggleCanvasTarget(current, { kind: 'reference', itemId }, additive));
  }, []);
  const selectCanvasElement = useCallback((pageId: string, elementId: string, sizeId: string | null, additive = false) => {
    setCanvasFocus(pageId);
    setCanvasSelection(current => toggleCanvasTarget(current, { kind: 'element', pageId, elementId, sizeId }, additive));
  }, []);
  // A behaviour badge or a pointer dropped on its own layer lands here: select it and open its field.
  const focusCanvasBehaviour = useCallback((source: PointerSource) => {
    if (source.kind === 'element') selectCanvasElement(source.pageId, source.elementId, source.sizeId ?? null);
    else selectCanvasBoundary(source.pageId, source.sizeId ?? null);
    setBehaviourFocus({ source: behaviourSourceKey(source), token: ++behaviourToken.current });
  }, [selectCanvasBoundary, selectCanvasElement]);
  const selectCanvasPointer = useCallback((pointerId: string, additive = false) => {
    setCanvasSelection(current => toggleCanvasTarget(current, { kind: 'pointer', pointerId }, additive));
  }, []);
  const selectCanvasRequest = useCallback((requestId: string, additive = false) => {
    setCanvasSelection(current => toggleCanvasTarget(current, { kind: 'request', requestId }, additive));
  }, []);
  const clearCanvasSelection = useCallback(() => setCanvasSelection([]), []);
  const groupCanvasElements = useCallback(async (pageId: string, ids: string[]) => {
    const before = new Set(elementTreeIds(current.current?.project?.pages.find(page => page.id === pageId)?.elements ?? []));
    const next = await mutate({ type: 'group_elements', page_id: pageId, ids });
    const page = next?.project?.pages.find(page => page.id === pageId);
    return page ? elementTree(page.elements).find(element => element.kind === 'group' && !before.has(element.id))?.id ?? null : null;
  }, [mutate]);
  const ungroupCanvasElements = useCallback(async (pageId: string, id: string) => {
    const group = findElementIn(current.current?.project?.pages.find(page => page.id === pageId)?.elements ?? [], id);
    const children = group?.kind === 'group' ? group.children.map(child => child.id) : [];
    const next = await mutate({ type: 'ungroup_elements', page_id: pageId, id });
    return next ? children : [];
  }, [mutate]);
  const removedWithUndo = useCallback((message: string) => {
    requestAnimationFrame(() => { if (!document.querySelector('.workspace-surface:focus-within')) canvasRef.current?.focusCanvas(); });
    setNotice(message, () => { run(() => travel('undo_project')); });
  }, [setNotice, travel]);
  const removeLinkedDefinition = useCallback((id: string) => {
    const name = current.current?.project?.linked?.[id]?.name ?? 'this linked element';
    void mutate({ type: 'delete_items', linked_ids: [id] })
      .then(next => { if (next) removedWithUndo(`Removed ${name}.`); })
      ;
  }, [mutate, setNotice]);
  const renameCanvasElement = useCallback((pageId: string, elementId: string, name: string | null) => mutate({ type: 'update_elements', page_id: pageId, patches: [{ id: elementId, name }] }).then(() => undefined), [mutate]);
  const removeCanvasRequests = useCallback((ids: string[]) => {
    if (!ids.length) return;
    setCanvasSelection(current => current.filter(target => target.kind !== 'request' || !ids.includes(target.requestId)));
    void mutate({ type: 'remove_requests', ids });
  }, [mutate]);
  // The layer's request box in Properties edits the one request that targets only that layer.
  const setCanvasElementRequest = useCallback((elementId: string, data: { text: string; attachments: RequestAttachment[] } | null) => {
    const project = current.current?.project;
    if (!project) return;
    const existing = (project.requests ?? []).find(request => request.targets.length === 1
      && request.targets[0].kind === 'element' && request.targets[0].id === elementId) ?? null;
    if (data === null || (!data.text.trim() && !data.attachments.length)) {
      if (existing) removeCanvasRequests([existing.id]);
      return;
    }
    const page = project.pages.find(item => item.id === ownerPageId(project, elementId));
    const request: PendingRequest = existing
      ? { ...existing, text: data.text, attachments: data.attachments }
      : { id: crypto.randomUUID(), text: data.text, attachments: data.attachments, requestedAt: new Date().toISOString(),
        targets: [{ kind: 'element', page: page?.id ?? '', size: page && page.sizes.length > 1 ? baseSizeId(page) : null, id: elementId }], area: null };
    if (existing) void edits.updateRequest(request);
    else void edits.addRequest(request);
  }, [edits, removeCanvasRequests]);
  const keepPointerComment = useCallback((pointerId: string) => {
    void mutate({ type: 'set_pointer_wiring', pointer_id: pointerId, wiring: null });
  }, [mutate]);
  const keepPointerThis = useCallback((pointerId: string, otherId: string) => {
    void mutate({ type: 'resolve_pointer_clash', keep: pointerId, drop: [otherId] });
  }, [mutate]);
  const mergePointerClash = useCallback((pointer: Pointer, other: Pointer) => {
    const comment = [pointer.comment.trim(), other.comment.trim()].filter(Boolean).join('\n\n');
    void mutate({ type: 'resolve_pointer_clash', keep: pointer.id, drop: [other.id], merged_comment: comment });
  }, [mutate]);
  const elementToReference = useCallback(async (pageId: string, elementId: string, rect: Rect, sizeId?: string | null) => {
    if (await mutate({ type: 'element_to_reference', page_id: pageId, element_id: elementId, rect, size_id: sizeId })) setCanvasSelection([]);
  }, [mutate]);
  const referenceToElement = useCallback(async (itemId: string, pageId: string, rect: Rect, sizeId?: string | null) => {
    const next = await mutate({ type: 'reference_to_element', item_id: itemId, page_id: pageId, rect, size_id: sizeId });
    if (!next) return;
    const added = next.project?.pages.find(page => page.id === pageId)?.elements.at(-1);
    setCanvasFocus(pageId);
    setCanvasSelection(added ? [{ kind: 'element', pageId, elementId: added.id, sizeId: sizeId ?? null }] : []);
  }, [mutate]);
  const attachReferenceContext = useCallback(async (requestIds: string[], itemIds: string[]) => {
    if (!requestIds.length || !itemIds.length) return;
    await mutate({ type: 'attach_reference_context', request_ids: requestIds, item_ids: itemIds });
  }, [mutate]);
  const reportCanvasZoom = useCallback((zoom: number) => setCanvasZoom(zoom), []);
  const renameProject = useCallback((name: string) => { void mutate({ type: 'rename_project', name }); }, [mutate]);
  const showDotGrid = useCallback((enabled: boolean) => { void mutate({ type: 'set_grid', enabled }); }, [mutate]);
  const changeGridFlags = useCallback((patch: Partial<Pick<GridSettings, 'snap'>>) => {
    void mutate(project => ({ type: 'set_grid_settings', settings: { ...project.gridSettings, ...patch } }));
  }, [mutate]);
  const load = useCallback(async () => {
    setLoading(true); setError('');
    try {
      // The start-page choice decides whether launch opens a canvas or waits on the start page.
      const settings = await native<AppSettingsView>('get_app_settings');
      setAppSettings(settings);
      let loaded = await native<WorkspaceView>('get_workspace');
      if (!loaded.project && !settings.showStartPage) loaded = await native<WorkspaceView>('new_untitled_project', {});
      accept(loaded);
      setShowStartPage(settings.showStartPage && !loaded.project);
      if (!settings.showStartPage && loaded.recoveries.length) openPanel({ kind: 'recovery' });
    }
    catch (error) { setError(errorMessage(error)); }
    finally { setLoading(false); }
  }, [accept]);
  useEffect(() => { void load(); }, [load]);
  useEffect(() => {
    const active = view?.project;
    if (!view?.dirty || !active) return;
    const timer = window.setInterval(() => { void native('autosave_project').catch(error => setError(errorMessage(error))); }, 3000);
    return () => window.clearInterval(timer);
  }, [view?.dirty, view?.project?.id]);
  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void getCurrentWindow().onCloseRequested(async event => {
      event.preventDefault();
      if (closing.current) return;
      closing.current = true;
      try {
        await settle();
        const identity = await native<import('../types').IdentityAnalysis | null>('get_identity_analysis');
        const generating = generationRef.current?.status === 'generating' || identity?.status === 'analyzing';
        const plan = identity?.status === 'analyzing' ? (identity.authentication ?? aiCurrent.current?.authentication) === 'chatgpt' : generationRef.current?.authentication === 'chatgpt';
        if ((current.current?.anyDirty || current.current?.dirty || generating) && !await new Promise<boolean>(resolve => setCloseRequest({ resolve, message: `${generating ? `Generation is still running. Closing may interrupt the request, and ${plan ? 'submitted work may still use your ChatGPT plan or credits' : 'OpenAI may still charge for submitted work'}. Its outcome will be shown as uncertain when you return. ` : ''}${current.current?.dirty ? 'This project has changes that have not been saved to its file. ' : ''}Close Desegnos?` }))) return;
        await getCurrentWindow().destroy();
      } catch (error) { setError(errorMessage(error)); }
      finally { closing.current = false; }
    }).then(stop => { if (disposed) stop(); else unlisten = stop; }, error => setError(errorMessage(error)));
    return () => { disposed = true; unlisten?.(); };
  }, [settle]);

  async function importContext(analysisId: string, findings: number[], assets: string[], photos: string[], items: number[], pages: string[], attachMatching: boolean) {
    await settle();
    await queue(async workspace => {
      if (!workspace?.project) throw new Error('Create or open a project first.');
      const next = await native<WorkspaceView>('import_context', { revision: workspace.project.revision, identityAnalysisId: analysisId, identityFindingIndices: findings, identityAssetIds: assets, identityPhotoIds: photos, identityItemIndices: items, identityPagePaths: pages, attachMatchingPages: attachMatching });
      accept(next);
      setNotice('Context added to the project brief.');
    });
  }

  async function forgetRecent(path: string) {
    await queue(async () => {
      setRecentProjects(await native<RecentProject[]>('forget_recent_project', { path }));
    }, false);
  }
  async function pinRecent(path: string, pinned: boolean) {
    await queue(async () => {
      setRecentProjects(await native<RecentProject[]>('set_recent_project_pin', { path, pinned }));
    }, false);
  }
  async function changeShowStartPage(value: boolean) {
    try { setAppSettings(await native<AppSettingsView>('set_show_start_page', { value })); }
    catch (error) { setError(errorMessage(error)); }
  }
  // The only update request: a manual read of the newest release, with no install step.
  async function checkForUpdates() {
    setUpdateState({ kind: 'checking' });
    try { setUpdateState({ kind: 'done', result: updateSummary(await native<UpdateCheck>('check_for_updates')) }); }
    catch (error) { setUpdateState({ kind: 'failed', message: errorMessage(error) }); }
  }
  async function openRelease(url: string) {
    try { await native('open_preview_link', { url }); }
    catch { setOpenFailure({ message: url, url }); }
  }

  function togglePlay(pageId?: string | null) {
    const projectNow = current.current?.project;
    const target = pageId ?? canvasFocus ?? selectedElementPageId ?? projectNow?.pages[0]?.id ?? null;
    if (!target) { setNotice('Select a boundary to play.'); return; }
    // Behaviours only run as exported code, so starting Play hands this boundary to Preview.
    if (!playIsActive(play) && projectNow && playShouldPreview(projectNow, target)) { setCanvasFocus(target); previewSelectedBoundary(target); return; }
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
        requestAnimationFrame(() => canvasRef.current?.focusBoundary(step.goTo!));
      }
      return step.play;
    });
  }

  useEffect(() => { if (!view?.recoveries.length) closePanel('recovery'); }, [view?.recoveries.length]);
  const changeBrief = useCallback(async (patch: Partial<Brief>) => {
    const before = current.current?.project?.brief ?? { purpose: '', audience: '', brandName: '', tone: '' };
    const workspace = await mutate(project => ({ type: 'set_brief', brief: { ...project.brief, ...patch } }));
    return workspace?.project?.brief ?? before;
  }, [mutate]);

  useEffect(() => {
    const listener = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      const editingText = isTextField(event.target);
      // Text fields keep their own editing history; WebKitGTK needs the explicit mapping.
      if (editingText && navigator.userAgent.includes('Linux') && (event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'z') {
        event.preventDefault(); document.execCommand(event.shiftKey ? 'redo' : 'undo'); return;
      }
      const action = shortcutFor(event, editingText);
      if (action) performAction(action, event);
    };
    window.addEventListener('keydown', listener);
    return () => window.removeEventListener('keydown', listener);
  });

  const canvasBoundaries = project ? canvasView(project).boundaries : [];
  const onStartPage = !project || showStartPage;
  const pointerStates = view?.pointerStates ?? [];
  const pointerStateMap = Object.fromEntries((checkedStates && checkedProject === project?.id ? checkedStates : pointerStates).map(item => [item.id, item])) as Record<string, PointerStateView>;
  const requestedAreas = project ? (project.requests ?? []).filter(request => request.text.trim()).length : 0;
  const effectivePointerStates = checkedStates && checkedProject === project?.id ? checkedStates : pointerStates;
  const clashPointers = effectivePointerStates.filter(item => item.state === 'contradicts' || item.state === 'broken' || item.state === 'codeChanged');
  const activeProjectId = project?.id ?? null;
  const generationCall = useCallback(<T,>(command: string, args?: Record<string, unknown>, signal?: AbortSignal) => Promise.resolve().then(() => queue(async workspace => {
    signal?.throwIfAborted();
    if (!activeProjectId || workspace?.project?.id !== activeProjectId) throw new Error('Return to the project that owns this generation request.');
    if (command === 'prepare_generation') {
      await settleAiSettings();
      const settings = await refreshAiSettings();
      if (settings.authentication === 'chatgpt') {
        const account = settings.chatgpt.accounts.find(account => account.id === settings.chatgpt.activeAccountId);
        if (!account?.connected || !account.planEnabled) { setShowAiSettings(true); throw new Error('Connect a ChatGPT account with plan usage enabled in AI settings, then prepare the request again.'); }
        if (settings.chatgpt.models.length > 0 && !settings.catalogue.some(model => model.id === settings.settings.model)) { setShowAiSettings(true); throw new Error('Choose and save a supported model for this ChatGPT account, then prepare the request again.'); }
      } else if (!settings.credential.configured) { setShowAiSettings(true); throw new Error(`Add your ${settings.providers?.find(item => item.id === settings.provider)?.name ?? 'AI'} key in AI settings, then try again.`); }
      if (settings.configurationError) { setShowAiSettings(true); throw new Error('Review and save your AI settings, then prepare the request again.'); }
    }
    signal?.throwIfAborted();
    const result = await native<T>(command, args);
    if (command === 'apply_generation' && !signal?.aborted) accept(result as WorkspaceView);
    return result;
  }, false)), [queue, activeProjectId, accept, refreshAiSettings, settleAiSettings]);
  const generationApplied = useCallback((workspace: WorkspaceView) => { if (workspace.project?.latestGeneration) setNotice('Generated result applied.'); }, []);
  const generationRecorded = useCallback(() => queue(async () => {
    const refreshed = await native<WorkspaceView>('get_workspace');
    if (refreshed.project?.id === activeProjectId) accept(refreshed);
  }, false), [queue, accept, activeProjectId]);
  const generation = useGeneration(activeProjectId, generationCall, generationApplied, generationRecorded);
  generationRef.current = generation.job;
  generationOwns.current = generation.owns;
  const working = generation.canStop || activeGeneration(generation.job) || !!generation.operation;
  useEffect(() => { closePanel('log'); }, [activeProjectId]);
  useEffect(() => { setCanvasSelection([]); setCanvasFocus(null); setCanvasFocusSize(null); }, [view?.project?.id]);
  useEffect(() => {
    const pages = view?.project?.pages, reference = view?.project?.reference ?? [], pointers = view?.project?.pointers ?? [];
    setCanvasSelection(current => {
      const requests = view?.project?.requests ?? [];
      const next = current.filter(target => target.kind === 'request'
        ? requests.some(request => request.id === target.requestId)
        : target.kind === 'element'
        ? !!pages?.some(page => page.id === target.pageId && findElementIn(page.elements, target.elementId)
          && (target.sizeId === null || page.sizes.some(size => size.id === target.sizeId)))
        : target.kind === 'pointer' ? pointers.some(pointer => pointer.id === target.pointerId) : reference.some(item => item.id === target.itemId));
      return next.length === current.length ? current : next;
    });
    if (canvasFocus && !pages?.some(page => page.id === canvasFocus)) setCanvasFocus(null);
  }, [view?.project?.pages, view?.project?.reference, view?.project?.pointers, view?.project?.requests, canvasFocus]);
  useEffect(() => { if (generation.visible) { openPanel({ kind: 'log', focus: 0 }); generation.close(); } }, [generation.visible]);
  const showGenerationStatus = !!generation.error || announcesStatus(generation.job, { owns: generation.owns, stopped: generation.stopped, working });
  const composeJob = generation.job && ['prepared', 'generating', 'ready', 'blocked', 'applying'].includes(generation.job.status) ? generation.job : null;
  const composeUsesChatgpt = (composeJob?.authentication ?? aiSettings?.authentication) === 'chatgpt';
  const composeAccountId = composeJob ? composeJob.chatgptAccountId : aiSettings?.chatgpt.activeAccountId;
  const composeAccount = aiSettings?.chatgpt.accounts.find(account => account.id === composeAccountId);
  const aiConnected = !!aiSettings && !aiSettings.configurationError && (composeUsesChatgpt ? !!composeAccount?.connected && composeAccount.planEnabled && !composeAccount.storageRepairRequired : aiSettings.credential.configured);
  function openGeneration() {
    openPanel({ kind: 'log', focus: panel.kind === 'log' ? panel.focus + 1 : 1 });
    void generation.refresh();
    void refreshAiSettings().catch(error => setError(errorMessage(error)));
  }
  function editGenerationRequest(id: string) {
    const owner = project?.pages.find(item => !!findElementIn(item.elements, id));
    if (owner) { setCanvasFocus(owner.id); setCanvasSelection([{ kind: 'element', pageId: owner.id, elementId: id, sizeId: baseSizeId(owner) }]); }
  }
  async function replyToGeneration(targets: { pageId: string; elementId: string }[], text: string) {
    const elementIds: string[] = [];
    for (const target of targets) {
      const element = project?.pages.find(page => page.id === target.pageId) && findElementIn(project!.pages.find(page => page.id === target.pageId)!.elements, target.elementId);
      if (!element) continue;
      const prompt = (current.current?.project?.requests ?? []).find(request => request.targets.some(target => target.kind === 'element' && target.id === element.id))?.text.trim() ?? '';
      await changeRequest(target.pageId, target.elementId, { prompt: prompt ? `${prompt}\n\nFeedback: ${text}` : `Feedback: ${text}` });
      elementIds.push(target.elementId);
    }
    if (!elementIds.length) throw new Error('Those requests are no longer in the project.');
    await generation.prepare(elementIds);
  }
  function openImportDialog() { openPanel({ kind: 'import', source: null }); }
  async function generate(elementIds?: string[]) {
    const contextItems = selectedReference(canvasSelection);
    setError('');
    // Progress shows in the generation notice; its Details opens the AI conversation.
    // With nothing described, a filled brief captures the first empty page as one request area.
    const page = project?.pages[0];
    const boundary = page && canvasBoundaries.find(item => item.pageId === page.id && item.sizeId === baseSizeId(page));
    const fromBrief = !!project && !!page && !!boundary && !page.elements.length && !!project.brief.purpose.trim()
      && !(project.requests ?? []).some(request => request.text.trim());
    if (!fromBrief) { await generation.prepare(elementIds, contextItems); return; }
    // The request is added before the job is prepared, so a second click must not add another.
    if (briefGenerateFlight.current) return;
    briefGenerateFlight.current = true;
    try {
      const area: RequestArea = { page: page.id, size: page.sizes.length > 1 ? baseSizeId(page) : null, shape: { kind: 'rect', rect: boundary.rect } };
      const request: PendingRequest = {
        id: crypto.randomUUID(), text: 'Build this page from the project brief.', attachments: [],
        targets: coveredTargets(project, canvasBoundaries, area), area, requestedAt: new Date().toISOString() };
      if (!await mutate({ type: 'add_request', request })) return;
      await generation.prepare(elementIds, contextItems);
    } finally { briefGenerateFlight.current = false; }
  }
  function requestTargetingElement(project: Project, elementId: string): PendingRequest | null {
    return (project.requests ?? []).find(request => request.targets.some(target => target.kind === 'element' && target.id === elementId)) ?? null;
  }
  async function changeRequest(pageId: string, elementId: string, patch: { prompt?: string | (() => string); rect?: Rect }): Promise<{ prompt: string }> {
    const project = current.current?.project;
    const page = project?.pages.find(item => item.id === pageId);
    if (!project || !page || !findElementIn(page.elements, elementId)) throw new Error('This layer is no longer in the project.');
    const existing = requestTargetingElement(project, elementId);
    const prompt = (typeof patch.prompt === 'function' ? patch.prompt() : patch.prompt) ?? existing?.text ?? '';
    const request: PendingRequest = existing
      ? { ...existing, text: prompt }
      : { id: crypto.randomUUID(), text: prompt, attachments: [], requestedAt: new Date().toISOString(),
        targets: [{ kind: 'element', page: pageId, size: page.sizes.length > 1 ? baseSizeId(page) : null, id: elementId }], area: null };
    await mutate(existing ? { type: 'update_request', request } : { type: 'add_request', request });
    return { prompt };
  }
  const attachToRequest = useCallback(async (requestId: string, files: File[], links: string[]) => {
    const request = (current.current?.project?.requests ?? []).find(item => item.id === requestId);
    if (!request) return;
    const { attachments, errors: problems } = await readRequestAttachments(request.attachments, files, links);
    if (!await mutate({ type: 'update_request', request: { ...request, attachments } })) return;
    if (problems.length) setError(problems.join(' ')); else setNotice('Attached to the request.');
  }, [mutate]);
  // One undoable step covers a whole canvas selection, so removing never interrupts the work.
  function removeCanvasSelection(referenceIds: string[], pointerIds: string[] = [], requestIds: string[] = [], elementIds: { pageId: string; elementId: string }[] = []) {
    const count = referenceIds.length + pointerIds.length + requestIds.length + elementIds.length;
    if (!count) return;
    run(async () => {
      if (!await mutate({
        type: 'delete_items',
        elements: elementIds.map(item => [item.pageId, item.elementId]) as [string, string][],
        reference_ids: referenceIds,
        pointer_ids: pointerIds,
        request_ids: requestIds })) return;
      setCanvasSelection([]);
      removedWithUndo(`${count} ${count === 1 ? 'item' : 'items'} deleted.`);
    });
  }
  // A boundary is deleted as a whole page, or as the selected linked size. The base size
  // carries the values the other sizes inherit, so deleting it deletes the page.
  function deleteBoundary(pageId: string, sizeId: string | null) {
    const page = project?.pages.find(item => item.id === pageId);
    if (!page) return;
    const size = sizeId ? page.sizes.find(item => item.id === sizeId) : null;
    const removeSize = !!size && size.id !== page.sizes[0].id;
    const name = removeSize ? `${page.name} · ${size.label}` : page.name;
    run(async () => {
      if (!await mutate(removeSize ? { type: 'delete_items', size_ids: [[pageId, size!.id]] } : { type: 'delete_items', page_ids: [pageId] })) return;
      setCanvasFocus(null); setCanvasFocusSize(null);
      setCanvasSelection([]);
      removedWithUndo(`Removed ${name}.`);
    });
  }
  const run = (operation: () => Promise<unknown>) => { void operation().catch(error => setError(errorMessage(error))); };
  const canEditProject = !!project && !onStartPage;

  const { views, saveCurrentView, addNamedViewAt, addNamedView, renameNamedView, removeNamedView } = useNamedViews(project, mutate, canvasRef, run);

  const menuItems: MenuItem[] = [
    ...(!onStartPage ? [{ id: 'home', label: 'Home', action: () => setShowStartPage(true) }] : []),
    ...(onStartPage && project ? [{ id: 'back-to-canvas', label: 'Back to canvas', action: () => setShowStartPage(false) }] : []),
    { id: 'new', label: 'New project', shortcut: shortcutLabel('new'), action: () => run(newUntitled) },
    { id: 'open', label: 'Open project…', shortcut: shortcutLabel('open'), action: () => run(open) },
    ...(recentProjects.length ? [{ id: 'open-recent', label: 'Open recent', children: [
      ...recentProjects.map((entry, index) => ({ id: `open-recent-${index}`, label: entry.name, action: () => run(() => openRecent(entry.path)) })),
      { id: 'open-recent-separator', label: '', separator: true },
      { id: 'forget-recent', label: 'Forget recent project', children: recentProjects.map((entry, index) => ({ id: `forget-recent-${index}`, label: entry.name, action: () => run(() => forgetRecent(entry.path)) })) },
    ] }] : []),
    { id: 'open-folder', label: 'Open project folder…', action: () => run(() => open(true)) },
    { id: 'save', label: 'Save', shortcut: shortcutLabel('save'), disabled: !canEditProject, action: () => run(() => save()) },
    { id: 'close-project', label: 'Close project', shortcut: shortcutLabel('closeProject'), disabled: !canEditProject, action: () => run(closeProject) },
    { id: 'save-as', label: 'Save as', shortcut: shortcutLabel('saveAs'), disabled: !canEditProject, action: () => run(() => save(true)) },
    { id: 'save-copy', label: 'Save a copy without history…', disabled: !canEditProject || pending > 0, action: () => run(saveCopy) },
    { id: 'rename-project', label: 'Rename project…', disabled: !canEditProject, action: () => openPanel({ kind: 'rename' }) },
    ...(view?.recoveries.length ? [{ id: 'recover', label: 'Recover unsaved projects', action: () => openPanel({ kind: 'recovery' }) }] : []),
    { id: 'file-divider', label: '', separator: true },
    { id: 'export', label: 'Export…', shortcut: shortcutLabel('export'), disabled: !canEditProject, action: openExportPanel },
    ...(canEditProject && project.lastExport ? [{ id: 'open-browser', label: 'Open in browser', action: () => run(exports.openInBrowser) }] : []),
    { id: 'ai-divider', label: '', separator: true },
    { id: 'settings', label: 'AI settings', action: () => setShowAiSettings(true) },
    { id: 'mode', label: 'AI style', disabled: !canEditProject || working, children: (['fuzzy', 'strict'] as GenerationMode[]).map(mode => ({ id: mode, label: mode === 'fuzzy' ? 'Flexible' : 'Precise', checked: project?.generationMode === mode, action: () => run(() => mutate(current => current.generationMode === mode ? null : { type: 'set_generation_mode', mode })) })) },
    { id: 'style-guide', label: 'Style guide', disabled: !canEditProject, action: () => openPanel({ kind: 'styleGuide' }) },
    { id: 'import', label: 'Import…', disabled: !canEditProject, action: () => { openPanel({ kind: 'import', source: null }); } },
    { id: 'dossier', label: 'Project brief', disabled: !canEditProject, action: () => openPanel({ kind: 'notes', section: 'overview' }) },
    { id: 'help-divider', label: '', separator: true },
    { id: 'preferences', label: 'Preferences', children: [
      { id: 'show-start-page', label: 'Show start page on launch', checked: appSettings.showStartPage, action: () => run(() => changeShowStartPage(!appSettings.showStartPage)) },
      { id: 'preferences-divider', label: '', separator: true },
      { id: 'appearance', label: 'Editor theme', children: (['system', 'light', 'dark'] as Theme[]).map(value => ({ id: value, label: value === 'system' ? 'System' : value === 'light' ? 'Light' : 'Dark', checked: theme === value, action: () => setTheme(value) })) },
      { id: 'project-location', label: 'Default project location', action: () => openPanel({ kind: 'projectLocation' }) },
    ] },
    { id: 'check-updates', label: 'Check for updates…', disabled: updateState.kind === 'checking', action: () => { void checkForUpdates(); } },
    { id: 'help', label: 'Keyboard shortcuts', shortcut: shortcutLabel('help'), action: () => openPanel({ kind: 'shortcuts' }) },
  ];

  const problemAction = needsAiSettings(error) ? { label: 'AI settings', onClick: () => setShowAiSettings(true) }
    : project && /permission denied|not writable|read.only|access denied|can't save here/i.test(error) ? { label: 'Choose another location', disabled: pending > 0, onClick: () => run(() => save(true)) } : undefined;
  const recovery: RecoveryChoices | null = pendingRecovery ? {
    path: pendingRecovery.path, busy: pending > 0,
    recover: () => run(() => recover(pendingRecovery.projectId)),
    openSaved: () => run(() => open(false, pendingRecovery.path, true)),
    dismiss: () => setPendingRecovery(null) } : null;
  useEffect(() => setWidthPreview(null), [project?.id, project?.revision, canvasFocus, canvasFocusSize, canvasSelection]);
  function selectNextPointerClash() {
    const ids = clashPointers.map(item => item.id);
    if (!ids.length) return;
    const selected = canvasSelection.length === 1 && canvasSelection[0].kind === 'pointer' ? canvasSelection[0].pointerId : null;
    const index = selected ? ids.indexOf(selected) : -1;
    setCanvasSelection([{ kind: 'pointer', pointerId: ids[(index + 1) % ids.length] }]);
  }
  function chooseTool(next: EditorTool) {
    changeCanvasTool(next, null);
    // The next press belongs to the canvas: leave the tool strip or its flyout behind, so no
    // button keeps focus or the pointer.
    canvasRef.current?.focusCanvas();
  }
  // The inline preview covers its page, so a tool that draws or types there would act on nothing.
  useEffect(() => { if (preview && canvasTool.kind !== 'select' && canvasTool.kind !== 'pan') closePreview(); }, [canvasTool.kind]);
  function moveCanvasElementTo(pageId: string, sizeId: string, elementId: string, parentId: string | null, index: number) {
    const projectNow = current.current?.project;
    const page = projectNow?.pages.find(page => page.id === pageId);
    if (!projectNow || !page) return;
    const element = findElementIn(resolveForSize(projectNow, page, sizeId), elementId);
    const path = elementPath(projectNow, pageId, elementId, sizeId);
    const origin = path?.[path.length - 1]?.origin;
    const parentOrigin = parentId ? elementPath(projectNow, pageId, parentId, sizeId)?.at(-1)?.origin ?? { x: 0, y: 0 } : { x: 0, y: 0 };
    if (!element || !path || !origin) return;
    const over = element.overrides?.[sizeId];
    const shown = over?.rect ?? element.rect;
    const rect = { x: origin.x - parentOrigin.x, y: origin.y - parentOrigin.y, width: shown.width, height: shown.height };
    void edits.moveElements([{ id: elementId, to_page_id: pageId, to_parent_id: parentId, index, rect, size_id: sizeId }]);
  }
  function armInsert(kind: ElementKind) {
    changeCanvasTool(kind === 'image' || kind === 'text' ? kind : 'insert', kind);
    canvasRef.current?.focusCanvas();
  }
  const focusedBoundary = project && canvasFocus
    ? canvasBoundaries.find(item => item.pageId === canvasFocus && (!canvasFocusSize || item.sizeId === canvasFocusSize))
      ?? canvasBoundaries.find(item => item.pageId === canvasFocus) ?? null
    : null;
  const selectedBounds = project ? selectionBounds(project, canvasSelection) ?? (canvasSelection.length ? null : focusedBoundary?.rect ?? null) : null;
  const zoomPercent = Math.round(canvasZoom * 100);
  const paletteCommands: PaletteCommand[] = [
    ...KEYMAP.filter(binding => binding.action !== 'commandPalette').map(binding => {
      const canvasAnswer = canvasRef.current?.canRunCanvasAction(binding.action) ?? null;
      const missingProject = (onStartPage || !project) && !['new', 'open', 'help'].includes(binding.action);
      return {
        id: `action:${binding.action}`, label: binding.label, keys: binding.display, group: binding.group,
        disabled: canvasAnswer === null ? missingProject : !canvasAnswer,
        run: () => { if (!canvasRef.current?.runCanvasAction(binding.action)) performAction(binding.action); } };
    }),
    ...menuPaletteCommands(menuItems),
  ];

  return {
    edits, exports, removedWithUndo,
    panel, openPanel, closePanel, togglePanel, saving, updateState, dismissUpdate, changeCanvasTool,
    showPalette, paletteCommands, setShowPalette, closeRequest, setCloseRequest, onStartPage, project, view,
    run, switchProjectTab, closeProjectTab, menuItems, resolvedTheme, setTheme, compactHistory,
    travel, panels, panelActions, play,
    togglePlay, aiConnected, showAiSettings,
    setShowAiSettings, generation, requestedAreas, working, generate,
    error, setError, problemAction, openFailure, setNotice, setOpenFailure, openRelease,
    notices, setNotices, dismissedRecovery, setDismissedRecovery,
    loading, load, pending, newUntitled, open, appSettings, changeShowStartPage, recentProjects, openRecent,
    forgetRecent, pinRecent, recover, dismissRecoveries, canvasTool, chooseTool, canvasZoom, canvasRef,
    boundaryKind, setBoundaryKind, requestToolShape, setRequestToolShape, armInsert, insertSignal,
    selectCanvasBoundary, createCanvasBoundary, addProjectBoundary, addNamedViewAt, saveCurrentView,
    canvasSelection, canvasFocus, canvasFocusSize, widthPreview, setWidthPreview, selectCanvasReference, selectCanvasPointer,
    selectCanvasRequest, clearCanvasSelection, removeCanvasSelection, elementToReference, referenceToElement, attachToRequest, deleteBoundary, pointerStateMap,
    reportCanvasZoom, selectCanvasElement, groupCanvasElements, ungroupCanvasElements, playElement, setPlay, showGenerationStatus, openImportDialog, editGenerationRequest,
    openGeneration, aiSettings, manageChatgptUsage, replyToGeneration, historyEpoch, accept, projectLocation,
    closeUntitledSave, saveUntitledProject, keepPointerComment, keepPointerThis, mergePointerClash,
    attachReferenceContext, renameCanvasElement, setCanvasElementRequest,
    updateCanvasPage, mutate, updateCanvasPageSize,
    addCanvasPageSize, renameProject, showDotGrid, changeGridFlags, save, canvasBoundaries,
    moveCanvasElementTo, removeLinkedDefinition, views, addNamedView, renameNamedView, removeNamedView,
    selectedBounds, clashPointers, selectNextPointerClash, zoomPercent,
    recovery, changeProjectLocation, importContext,
    setCanvasSelection, setCanvasFocus, focusCanvasBehaviour, behaviourFocus, changeBrief,
    chooseSaveConflict, replacement, setReplacement, confirmation, setConfirmation,
    refreshAiSettings, updateAiSettings, aiPollingError };
}
