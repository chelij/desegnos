import { type BoundaryToolKind, boundaryDefaults, layoutKind, outputKind, pageKind, projectKind, nextBoundaryName } from '../projectCanvas/boundaryDefaults';
import { canvasView } from '../projectCanvas/canvasView';
import { useProjectSession } from './useProjectSession';
import { createProjectCommands } from './projectCommands';
import { useExportJobs } from '../exportJobs/useExportJobs';
import { createEditorShortcuts } from '../editorShortcuts';
import {
  Theme, Replacement, UntitledSaveRequest, Confirmation, errorMessage, sizeName, sizeBoundaries, initialZoom,
  readTheme, ownerPageId, selectionBounds, menuPaletteCommands,
  suggestedPagePath,
} from '../editorShared';
import { readRequestAttachments } from '../requestAttachments';
import { elementTree, elementTreeIds } from '../projectCanvas/elementGeometry';
import { needsAiSettings } from '../ProblemBanner.js';
import { updateSummary, type UpdateCheck, type UpdateSummary } from '../updateCheck';
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { native, edit } from '../api';
import { type EditorTool, type ProjectTool, type RequestToolShape } from '../EditorTools';
import { isTextField } from '../WorkspaceSurface';
import { type ProjectCanvasHandle } from '../ProjectCanvas';
import { type PendingRecovery, type RecoveryChoices } from '../RecoveryBanner';
import { type SaveConflict, type SaveChoice } from '../SaveConflict';
import type { ProjectLocationView } from '../types';
import { type DossierSection } from '../Notes';
import { type MenuItem } from '../Menus';
import { KEYMAP, shortcutFor, shortcutLabel, viewNameForSlot } from '../shortcuts';
import {
  BOUNDARY_GAP, unionBounds, referenceMoves, pushApartReference, boundaryMoves, pushApartBoundaries, toggleCanvasTarget, selectedReference, type Boundary, type BoundaryMove,
  type CanvasCamera, type CanvasTarget, type ReferenceMove,
} from '../projectCanvasGeometry';
import { findPageElement } from '../projectCanvas/elementGeometry';
import { elementPath, findElementIn } from '../projectCanvas/elementGeometry';
import { usePanelState } from '../EditorDock';
import { announcedDraftCount, announcesStatus } from '../generationSession.js';
import { type PaletteCommand } from '../CommandPalette';
import { activeGeneration, useGeneration } from '../useGeneration';
import type { AiSettingsView, AppSettingsView, BoundaryKind, Brief, Element, ElementKind, ElementMove, ElementPatch, GenerationJobView, GenerationMode, GridSettings, LinkedPasteInstance, NamedView, PageSize, PendingRequest, Pointer, PointerSource, PointerStateView, PointerTarget, Project, Rect, ReferenceItem, RequestAttachment, SizePreset, WorkspaceView, RecentProjectsView, RecentProject } from '../types';
import { baseSizeId, resolveForSize } from '../projectCanvas/sizes';
import { idlePlay, playClick, playIsActive, type PlayState } from '../projectCanvas/play';
import { behaviourSourceKey, playShouldPreview } from '../projectCanvas/behaviours';

export function useEditor({ initialView }: { initialView?: WorkspaceView } = {}) {

  const {
    requestProjectOpen, chooseSaveConflict, closeUntitledSave, requestUntitledSave, saveUntitledProject, save,
    replace, closeProject, switchProjectTab, closeProjectTab, openAlongside, open, openRecent, newUntitled,
    recover, dismissRecoveries, travel, saveCopy, compactHistory,
  } = createProjectCommands(() => ({
    setPendingRecovery, setError, saveChoice, setSaveConflict, untitledSaveResolve, setUntitledSave,
    untitledSaveFlight, current, panels, panelActions, settle, queue, accept, setNotice, refreshRecent,
    saveFlight, setReplacement, setSelected, setCanvasSelection, setCanvasFocus, setShowStartPage,
    setConfirmation, setZoom, resolvedTheme, canvasRef, setShowRecovery, setHistoryEpoch,
  }));
  const {
    performAction,
  } = createEditorShortcuts(() => ({
    showStartPage, current, setShowShortcuts, setShowPalette, newUntitled, open, setInsertKind,
    setInsertSignal, setCanvasTool, panelActions, canvasRef, togglePlay, previewSelectedBoundary,
    openExportPanel, runExport, travel, save, closeProject,
  }));

  const [closeRequest, setCloseRequest] = useState<{ message: string; resolve: (answer: boolean) => void } | null>(null);
  const [loading, setLoading] = useState(!initialView);
  const [error, setError] = useState('');
  const { view, current, pending, accept, queue, mutate, settle, synchronize } = useProjectSession(initialView, setError);
  const [openFailure, setOpenFailure] = useState<{ message: string; url: string } | null>(null);
  const [checkingUpdates, setCheckingUpdates] = useState(false);
  const [updateResult, setUpdateResult] = useState<UpdateSummary | null>(null);
  const [updateError, setUpdateError] = useState('');
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
  const [theme, setTheme] = useState<Theme>(readTheme);
  const [selected, setSelected] = useState<string | null>(null);
  const [focusPrompt, setFocusPrompt] = useState(false);
  const [zoom, setZoom] = useState(0.75);
  const [tool, setTool] = useState<EditorTool>('select');
  const [canvasTool, setCanvasTool] = useState<ProjectTool>('select');
  const [insertKind, setInsertKind] = useState<ElementKind | null>(null);
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
  const [showProjectLocation, setShowProjectLocation] = useState(false);
  const [play, setPlay] = useState<PlayState>(idlePlay);
  const [pendingImages, setPendingImages] = useState(0);
  const [untitledSave, setUntitledSave] = useState<UntitledSaveRequest | null>(null);
  const [pendingRecovery, setPendingRecovery] = useState<PendingRecovery | null>(null);

  const [showNotes, setShowNotes] = useState<DossierSection | null>(null);
  const [showGridSettings, setShowGridSettings] = useState(false);
  const [showStyleGuide, setShowStyleGuide] = useState(false);
  const [showImport, setShowImport] = useState(false);
  const [importSource, setImportSource] = useState<import('../types').IdentitySource | null>(null);
  const [showLog, setShowLog] = useState(false);
  const [logFocus, setLogFocus] = useState(0);
  const [resolvedTheme, setResolvedTheme] = useState<'light' | 'dark'>('light');
  const [showHistory, setShowHistory] = useState(false);
  const [historyEpoch, setHistoryEpoch] = useState(0);
  const [showShortcuts, setShowShortcuts] = useState(false);
  const [showRecovery, setShowRecovery] = useState(false);
  const [showRename, setShowRename] = useState(false);
  const [showAiSettings, setShowAiSettings] = useState(false);
  const [aiSettings, setAiSettings] = useState<AiSettingsView | null>(null);
  const aiCurrent = useRef<AiSettingsView | null>(null);
  const aiEpoch = useRef(0);
  const aiRead = useRef(0);
  const aiTail = useRef<Promise<void>>(Promise.resolve());
  const [aiPollingError, setAiPollingError] = useState('');
  const [replacement, setReplacement] = useState<Replacement | null>(null);
  const [confirmation, setConfirmation] = useState<Confirmation | null>(null);
  const closing = useRef(false);
  const [saveConflict, setSaveConflict] = useState<SaveConflict | null>(null);
  const saveChoice = useRef<((choice: SaveChoice) => void) | null>(null);
  const saveFlight = useRef<Promise<WorkspaceView | null> | null>(null);
  const untitledSaveFlight = useRef<Promise<WorkspaceView | null> | null>(null);
  const untitledSaveResolve = useRef<((workspace: WorkspaceView | null) => void) | null>(null);
  const generationRef = useRef<GenerationJobView | null>(null);
  const generationOwns = useRef(false);
  const { openExportPanel, chooseExportDestination, refreshWorkspace, showJobResult, runExport, sendExport, cancelExport, openExportFolder, openInBrowser, keepPointerCode, forgetExportPointer, previewSelectedBoundary, closePreview, refreshPreview, showExport, setShowExport, exportDestination, exportJob, exportError, exportSending, checkedStates, checkedProject, preview } = useExportJobs({ current, view, showHistory, play, setPlay }, () => ({ current, panels, panelActions, accept, setNotice, settle, setError, setOpenFailure, setShowHistory, mutate, canvasFocus, selectedElementPageId, play, setPlay, view, showHistory }));

  const storeAiSettings = useCallback((value: AiSettingsView) => { aiCurrent.current = value; setAiSettings(value); setAiPollingError(''); }, []);
  const refreshAiSettings = useCallback(async () => {
    const epoch = aiEpoch.current; const read = ++aiRead.current;
    const value = await native<AiSettingsView>('get_ai_settings');
    if (epoch === aiEpoch.current && read === aiRead.current) { storeAiSettings(value); return value; }
    return aiCurrent.current ?? value;
  }, [storeAiSettings]);
  const updateAiSettings = useCallback((command: string, args?: Record<string, unknown>) => {
    const result = aiTail.current.then(async () => {
      aiEpoch.current += 1; aiRead.current += 1;
      const value = await native<AiSettingsView>(command, args);
      storeAiSettings(value); return value;
    });
    aiTail.current = result.then(() => undefined, () => undefined);
    return result;
  }, [storeAiSettings]);
  const manageChatgptUsage = useCallback(() => native<void>('open_chatgpt_usage'), []);
  useEffect(() => { void refreshAiSettings().catch(() => {}); }, [refreshAiSettings]);
  useEffect(() => {
    if (aiSettings?.chatgpt.signIn.status !== 'awaitingBrowser') return;
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      let next = 900;
      try { await refreshAiSettings(); }
      catch (error) { if (active) setAiPollingError(`ChatGPT sign-in status could not be refreshed: ${errorMessage(error)}`); next = 3000; }
      if (active) timer = setTimeout(() => { void poll(); }, next);
    }
    timer = setTimeout(() => { void poll(); }, 900);
    return () => { active = false; clearTimeout(timer); };
  }, [aiSettings?.chatgpt.signIn.status, refreshAiSettings]);

  const refreshRecent = useCallback(async () => {
    const value = await native<RecentProjectsView>('get_recent_projects');
    setRecentProjects(value.projects);
    if (value.error) setError(`Recent projects: ${value.error}`);
  }, []);
  useEffect(() => { void refreshRecent().catch(error => setError(errorMessage(error))); }, [refreshRecent]);
  useEffect(() => { void native<ProjectLocationView>('get_project_location').then(setProjectLocation).catch(error => setError(errorMessage(error))); }, []);
  // The export panel belongs to an open canvas; Home and project switches close it.
  useEffect(() => { if (showStartPage) setShowExport(false); }, [showStartPage]);
  async function changeProjectLocation(reset: boolean) {
    const path = reset ? null : await native<string | null>('pick_project_location');
    if (!reset && !path) return;
    setProjectLocation(await native<ProjectLocationView>('set_project_location', { path }));
  }

  useEffect(() => {
    if (view?.project) setBoundaryKind(projectKind(view.project));
  }, [view?.project?.id]);

  const createCanvasBoundary = useCallback(async (bounds: Rect, kind: BoundaryToolKind, sourceId?: string) => {
    let createdId: string | null = null;
    await mutate(project => {
      const source = project.pages.find(page => page.id === sourceId);
      const first = project.pages.find(page => pageKind(page) === kind);
      const template = source ?? (kind === 'page' && (first?.sizes.length ?? 0) > 1 ? first : null);
      const name = nextBoundaryName(project, kind);
      const size: PageSize = { id: crypto.randomUUID(), label: sizeName(bounds.width), width: Math.round(bounds.width), height: Math.round(bounds.height), kind: layoutKind(kind), x: bounds.x, y: bounds.y };
      let nextX = bounds.x;
      const sizes = template ? template.sizes.map((old, index) => {
        const next = { ...old, id: index === 0 ? size.id : crypto.randomUUID(),
          x: source ? bounds.x + old.x - template.sizes[0].x : nextX,
          y: source ? bounds.y + old.y - template.sizes[0].y : bounds.y,
          ...(index === 0 ? { width: size.width, height: size.height } : {}) };
        nextX = next.x + next.width + BOUNDARY_GAP;
        return next;
      }) : [size];
      const sizeIds = new Map(template?.sizes.map((old, index) => [old.id, sizes[index].id]));
      const copy = (element: Element): Element => ({ ...structuredClone(element), id: crypto.randomUUID(), children: element.children.map(copy), overrides: Object.fromEntries(Object.entries(element.overrides ?? {}).flatMap(([id, value]) => sizeIds.has(id) ? [[sizeIds.get(id)!, value]] : [])) });
      createdId = crypto.randomUUID();
      const page = { id: createdId, name, path: project.pages.length ? suggestedPagePath(name, project.pages.map(page => page.path)) : '/', included: true, output: outputKind(kind), background: source?.background ?? '#ffffff', sizes, elements: source?.elements.map(copy) ?? [] };
      const before = sizeBoundaries(project);
      const additions = sizes.map(item => ({ pageId: page.id, sizeId: item.id, label: item.label, rect: { x: item.x, y: item.y, width: item.width, height: item.height }, kind: item.kind }));
      // Keep the whole linked-size row fixed while making room around it.
      const anchor = { ...additions[0], rect: unionBounds(additions.map(item => item.rect)) };
      const rowWidth = anchor.rect.width + BOUNDARY_GAP;
      const prepared = source ? before.map(item => item.rect.x >= anchor.rect.x
        && item.rect.y < anchor.rect.y + anchor.rect.height && item.rect.y + item.rect.height > anchor.rect.y
        ? { ...item, rect: { ...item.rect, x: item.rect.x + rowWidth } } : item) : before;
      const pushed = pushApartBoundaries([...prepared, anchor], anchor.sizeId, BOUNDARY_GAP).filter(item => item.sizeId !== anchor.sizeId);
      const placed = [...pushed, ...additions];
      const boxes = project.reference.map(item => ({ itemId: item.id, rect: item.rect }));
      const reference = referenceMoves(pushApartReference(boxes, placed), boxes);
      return { type: 'create_boundary' as const, page, after_page_id: sourceId,
        moves: boundaryMoves(placed, [...before, ...additions]).map(({ pageId, sizeId, x, y, kind }) => ({ page_id: pageId, size_id: sizeId, x, y, kind })),
        reference: reference.map(({ itemId, x, y }) => ({ item_id: itemId, x, y })),
      };
    });
    return createdId;
  }, [mutate]);
  async function addProjectBoundary() {
    const project = current.current?.project;
    if (!project) return;
    const kind = projectKind(project);
    const id = await createCanvasBoundary(boundaryDefaults(project, kind), kind);
    selectCanvasBoundary(id);
    if (id) requestAnimationFrame(() => canvasRef.current?.focusBoundary(id));
  }
  const saveCanvasPlacements = useCallback(async (placements: BoundaryMove[], reference: ReferenceMove[]) => {
    if (!placements.length && !reference.length) return;
    await mutate({
      type: 'set_canvas_layout',
      placements: placements.map(({ pageId, sizeId, x, y, kind }) => ({ page_id: pageId, size_id: sizeId, x, y, kind })),
      reference: reference.map(({ itemId, x, y }) => ({ item_id: itemId, x, y })),
    });
  }, [mutate]);
  const selectCanvasBoundary = useCallback((pageId: string | null, sizeId?: string | null, additive = false) => {
    setCanvasFocus(pageId);
    setCanvasFocusSize(sizeId ?? null);
    if (!additive || !pageId) setCanvasSelection([]);
  }, []);
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
  const addCanvasReference = useCallback(async (item: ReferenceItem, pictureDataUri?: string) => {
    await mutate(pictureDataUri === undefined
      ? { type: 'add_reference_item', item }
      : { type: 'add_reference_item', item, picture_data_uri: pictureDataUri });
  }, [mutate]);
  const updateCanvasReference = useCallback((itemId: string, patch: { rect?: Rect; text?: string; url?: string; title?: string; color?: string }) => {
    void mutate({ type: 'update_reference_item', item_id: itemId, ...patch }).catch(() => {});
  }, [mutate]);
  const addCanvasPointer = useCallback(async (pointer: Pointer) => {
    await mutate({ type: 'add_pointer', pointer });
  }, [mutate]);
  const updateCanvasPointer = useCallback((pointerId: string, patch: { from?: PointerSource; to?: PointerTarget; comment?: string }) => {
    return mutate({ type: 'update_pointer', pointer_id: pointerId, ...patch });
  }, [mutate]);
  const insertCanvasElements = useCallback(async (pageId: string, parentId: string | null, index: number, elements: Element[], pictureDataUri?: string, sizeId?: string | null) => {
    await mutate({ type: 'insert_elements', page_id: pageId, parent_id: parentId, index, elements,
      ...(pictureDataUri === undefined ? {} : { picture_data_uri: pictureDataUri }),
      ...(sizeId ? { size_id: sizeId } : {}) });
  }, [mutate]);
  const updateCanvasElements = useCallback(async (pageId: string, patches: ElementPatch[]) => {
    if (!patches.length) return;
    try { await mutate({ type: 'update_elements', page_id: pageId, patches }); } catch { /* App surfaces the failed command. */ }
  }, [mutate]);
  const moveCanvasElements = useCallback(async (moves: ElementMove[]) => {
    if (!moves.length) return;
    try { await mutate({ type: 'move_elements', moves }); } catch { /* App surfaces the failed command. */ }
  }, [mutate]);
  const groupCanvasElements = useCallback(async (pageId: string, ids: string[]) => {
    const before = new Set(elementTreeIds(current.current?.project?.pages.find(page => page.id === pageId)?.elements ?? []));
    const next = await mutate({ type: 'group_elements', page_id: pageId, ids });
    const page = next.project?.pages.find(page => page.id === pageId);
    return page ? elementTree(page.elements).find(element => element.kind === 'group' && !before.has(element.id))?.id ?? null : null;
  }, [mutate]);
  const ungroupCanvasElements = useCallback(async (pageId: string, id: string) => {
    const group = findElementIn(current.current?.project?.pages.find(page => page.id === pageId)?.elements ?? [], id);
    const children = group?.kind === 'group' ? group.children.map(child => child.id) : [];
    await mutate({ type: 'ungroup_elements', page_id: pageId, id });
    return children;
  }, [mutate]);
  const linkCanvasElements = useCallback((ids: string[], sourceId: string | null) => {
    return mutate({ type: 'link_elements', ids, source_id: sourceId }).then(() => undefined);
  }, [mutate]);
  const unlinkCanvasElements = useCallback((ids: string[]) => {
    return mutate({ type: 'unlink_elements', ids }).then(() => undefined);
  }, [mutate]);
  const pasteLinkedInstances = useCallback((items: LinkedPasteInstance[]) => {
    return mutate({ type: 'paste_linked_instances', items }).then(() => undefined);
  }, [mutate]);
  const removeLinkedDefinition = useCallback((id: string) => {
    const name = current.current?.project?.linked?.[id]?.name ?? 'this linked element';
    void mutate({ type: 'delete_items', linked_ids: [id] })
      .then(() => setNotice(`Removed ${name}.`, () => { void travel('undo_project').catch(() => {}); }))
      .catch(() => {});
  }, [mutate, setNotice]);
  // An inline field reports its own error, so a rejected name never leaves the panel.
  const renameCanvasElement = useCallback((pageId: string, elementId: string, name: string | null) => {
    return queue(async workspace => {
      const project = workspace?.project;
      if (!project) throw new Error('Create or open a project first.');
      const next = await edit(project.revision, { type: 'update_elements', page_id: pageId, patches: [{ id: elementId, name }] });
      accept(next); return next;
    }, false, true).then(() => undefined);
  }, [queue, accept]);
  const addCanvasRequest = useCallback(async (request: PendingRequest) => {
    await mutate({ type: 'add_request', request });
  }, [mutate]);
  const updateCanvasRequest = useCallback(async (request: PendingRequest) => {
    await mutate({ type: 'update_request', request });
  }, [mutate]);
  const removeCanvasRequests = useCallback((ids: string[]) => {
    if (!ids.length) return;
    setCanvasSelection(current => current.filter(target => target.kind !== 'request' || !ids.includes(target.requestId)));
    void mutate({ type: 'remove_requests', ids }).catch(() => {});
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
    if (existing) void updateCanvasRequest(request).catch(() => {});
    else void addCanvasRequest(request).catch(() => {});
  }, [addCanvasRequest, updateCanvasRequest, removeCanvasRequests]);
  const updateCanvasElementContext = useCallback((elementId: string, context: RequestAttachment[]) => {
    void mutate({ type: 'update_element_context', element_id: elementId, context }).catch(() => {});
  }, [mutate]);
  const keepPointerComment = useCallback((pointerId: string) => {
    void mutate({ type: 'set_pointer_wiring', pointer_id: pointerId, wiring: null }).catch(() => {});
  }, [mutate]);
  const keepPointerThis = useCallback((pointerId: string, otherId: string) => {
    void mutate({ type: 'resolve_pointer_clash', keep: pointerId, drop: [otherId] }).catch(() => {});
  }, [mutate]);
  const mergePointerClash = useCallback((pointer: Pointer, other: Pointer) => {
    const comment = [pointer.comment.trim(), other.comment.trim()].filter(Boolean).join('\n\n');
    void mutate({ type: 'resolve_pointer_clash', keep: pointer.id, drop: [other.id], merged_comment: comment }).catch(() => {});
  }, [mutate]);
  const elementToReference = useCallback(async (pageId: string, elementId: string, rect: Rect, sizeId?: string | null) => {
    setCanvasSelection([]);
    await mutate({ type: 'element_to_reference', page_id: pageId, element_id: elementId, rect, size_id: sizeId });
  }, [mutate]);
  const referenceToElement = useCallback(async (itemId: string, pageId: string, rect: Rect, sizeId?: string | null) => {
    const next = await mutate({ type: 'reference_to_element', item_id: itemId, page_id: pageId, rect, size_id: sizeId });
    const added = next.project?.pages.find(page => page.id === pageId)?.elements.at(-1);
    setCanvasFocus(pageId);
    setCanvasSelection(added ? [{ kind: 'element', pageId, elementId: added.id, sizeId: sizeId ?? null }] : []);
  }, [mutate]);
  const attachReferenceContext = useCallback(async (requestIds: string[], itemIds: string[]) => {
    if (!requestIds.length || !itemIds.length) return;
    await mutate({ type: 'attach_reference_context', request_ids: requestIds, item_ids: itemIds });
  }, [mutate]);
  const reportCanvasZoom = useCallback((zoom: number) => setCanvasZoom(zoom), []);
  const updateCanvasPage = useCallback((pageId: string, patch: { name?: string; path?: string; background?: string; included?: boolean }) => {
    void mutate(project => {
      const page = project.pages.find(item => item.id === pageId);
      if (!page) return null;
      const name = patch.name ?? page.name, path = patch.path ?? page.path, background = patch.background ?? page.background;
      const included = patch.included ?? page.included;
      if (name === page.name && path === page.path && background === page.background && included === page.included) return null;
      return { type: 'update_page', page_id: pageId, name, path, background, included };
    }).catch(() => {});
  }, [mutate]);
  const updateCanvasPageSize = useCallback((pageId: string, sizeId: string, patch: { label?: string; width?: number; height?: number; kind?: BoundaryKind }) => {
    void mutate(project => {
      const page = project.pages.find(item => item.id === pageId);
      const size = page?.sizes.find(item => item.id === sizeId);
      if (!page || !size) return null;
      const before = sizeBoundaries(project);
      const resized = before.map(item => item.sizeId === sizeId
        ? { ...item, rect: { ...item.rect, width: patch.width ?? item.rect.width, height: patch.height ?? item.rect.height } }
        : item);
      const pushed = pushApartBoundaries(resized, sizeId);
      return {
        type: 'update_page_size' as const, page_id: pageId, size_id: sizeId, ...patch,
        moves: boundaryMoves(pushed, before).map(({ pageId: movePageId, sizeId: movedId, x, y, kind }) => ({ page_id: movePageId, size_id: movedId, x, y, kind })),
        reference: [],
      };
    }).catch(() => {});
  }, [mutate]);
  /** Adding a linked size is one undo step, including the boundaries it pushes aside. */
  const addCanvasPageSize = useCallback((pageId: string, preset: SizePreset, width = 0, height = 0) => {
    void mutate(project => {
      const page = project.pages.find(item => item.id === pageId);
      if (!page) return null;
      const last = page.sizes.at(-1)!;
      const dimensions = preset === 'phone' ? { label: 'Phone', width: 390, height: 844 }
        : preset === 'tablet' ? { label: 'Tablet', width: 820, height: 1180 }
          : preset === 'desktop' ? { label: 'Desktop', width: 1440, height: 900 }
            : { label: sizeName(Math.round(width)), width: Math.round(width), height: Math.round(height) };
      const size: PageSize = { id: crypto.randomUUID(), ...dimensions, kind: last.kind, x: last.x + last.width + 160, y: last.y };
      const anchor = sizeBoundaries(project);
      const pushed = pushApartBoundaries([...anchor, { pageId, sizeId: size.id, label: size.label, rect: { x: size.x, y: size.y, width: size.width, height: size.height }, kind: size.kind }], size.id);
      const placed = pushed.find(item => item.sizeId === size.id)!;
      return {
        type: 'add_page_size' as const, page_id: pageId,
        size: { ...size, x: placed.rect.x, y: placed.rect.y },
        moves: boundaryMoves(pushed.filter(item => item.sizeId !== size.id), anchor)
          .map(({ pageId: movePageId, sizeId, x, y, kind }) => ({ page_id: movePageId, size_id: sizeId, x, y, kind })),
        reference: [],
      };
    }).catch(() => {});
  }, [mutate]);
  const removeCanvasPageSize = useCallback((pageId: string, sizeId: string) => {
    void mutate({ type: 'remove_page_size', page_id: pageId, size_id: sizeId }).catch(() => {});
  }, [mutate]);
  const renameProject = useCallback((name: string) => { void mutate({ type: 'rename_project', name }).catch(() => {}); }, [mutate]);
  const showDotGrid = useCallback((enabled: boolean) => { void mutate({ type: 'set_grid', enabled }).catch(() => {}); }, [mutate]);
  const changeGridFlags = useCallback((patch: Partial<Pick<GridSettings, 'snap'>>) => {
    void mutate(project => ({ type: 'set_grid_settings', settings: { ...project.gridSettings, ...patch } })).catch(() => {});
  }, [mutate]);
  const load = useCallback(async () => {
    setLoading(true); setError('');
    try {
      // The start-page choice decides whether launch opens a canvas or waits on the start page.
      const settings = await native<AppSettingsView>('get_app_settings').catch(() => ({ showStartPage: true }));
      setAppSettings(settings);
      let loaded = await native<WorkspaceView>('get_workspace');
      if (!loaded.project && !settings.showStartPage) loaded = await native<WorkspaceView>('new_untitled_project', {});
      accept(loaded);
      setShowStartPage(settings.showStartPage && !loaded.project);
      if (!settings.showStartPage && loaded.recoveries.length) setShowRecovery(true);
      if (loaded.project) setZoom(initialZoom(loaded.project.viewport.width, loaded.project.viewport.height));
    }
    catch (error) { setError(errorMessage(error)); }
    finally { setLoading(false); }
  }, [accept]);
  useEffect(() => { void load(); }, [load]);
  useEffect(() => {
    // Keep unsaved edits in the project file while the project stays dirty and still.
    const active = view?.project;
    if (!view?.dirty || !active) return;
    const timer = window.setInterval(() => { void native('autosave_project').catch(() => {}); }, 3000);
    return () => window.clearInterval(timer);
  }, [view?.dirty, view?.project?.id]);
  useEffect(() => {
    const media = window.matchMedia('(prefers-color-scheme: dark)');
    const update = () => { const value = theme === 'system' ? (media.matches ? 'dark' : 'light') : theme; document.documentElement.dataset.theme = value; setResolvedTheme(value); };
    update(); media.addEventListener('change', update);
    try { localStorage.setItem('desegnos.editor-theme', theme); } catch {}
    return () => media.removeEventListener('change', update);
  }, [theme]);
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
        const identity = await native<import('../types').IdentityAnalysis | null>('get_identity_analysis').catch(() => null);
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
    setCheckingUpdates(true); setUpdateError(''); setUpdateResult(null);
    try { setUpdateResult(updateSummary(await native<UpdateCheck>('check_for_updates'))); }
    catch (error) { setUpdateError(errorMessage(error)); }
    finally { setCheckingUpdates(false); }
  }
  async function openRelease(url: string) {
    try { await native('open_preview_link', { url }); }
    catch { setOpenFailure({ message: url, url }); }
  }
  // Export: destination, included boundaries and the paid-send decision live in the dock.

  // Play and Preview export.
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

  useEffect(() => { if (!view?.recoveries.length) setShowRecovery(false); }, [view?.recoveries.length]);
  const changeBrief = useCallback((patch: Partial<Brief>) => mutate(project => ({ type: 'set_brief', brief: { ...project.brief, ...patch } }))
    .then(workspace => workspace.project!.brief), [mutate]);

  // One callable path per keymap action the canvas does not own; its keyboard listener and the
  // command palette both use it.

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

  const project = view?.project ?? null;
  const canvasBoundaries = project ? canvasView(project).boundaries : [];
  const onStartPage = !project || showStartPage;
  const pointerStates = view?.pointerStates ?? [];
  const pointerStateMap = Object.fromEntries((checkedStates && checkedProject === project?.id ? checkedStates : pointerStates).map(item => [item.id, item])) as Record<string, PointerStateView>;
  const requestedAreas = project ? (project.requests ?? []).filter(request => request.text.trim()).length : 0;
  const needsGeneratingPointers = pointerStates.filter(item => item.state === 'needsGenerating').length;
  const effectivePointerStates = checkedStates && checkedProject === project?.id ? checkedStates : pointerStates;
  const clashPointers = effectivePointerStates.filter(item => item.state === 'contradicts' || item.state === 'broken' || item.state === 'codeChanged');
  const activeProjectId = project?.id ?? null;
  const generationCall = useCallback(<T,>(command: string, args?: Record<string, unknown>, signal?: AbortSignal) => Promise.resolve().then(() => queue(async workspace => {
    signal?.throwIfAborted();
    if (!activeProjectId || workspace?.project?.id !== activeProjectId) throw new Error('Return to the project that owns this generation request.');
    if (command === 'prepare_generation') {
      await aiTail.current;
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
  })), [queue, activeProjectId, accept, refreshAiSettings]);
  const generationApplied = useCallback((workspace: WorkspaceView) => { setSelected(null); setFocusPrompt(false); if (workspace.project?.latestGeneration) setNotice('Generated result applied.'); }, []);
  const generationRecorded = useCallback(() => queue(async () => {
    const refreshed = await native<WorkspaceView>('get_workspace');
    if (refreshed.project?.id === activeProjectId) accept(refreshed);
  }), [queue, accept, activeProjectId]);
  const generation = useGeneration(activeProjectId, generationCall, generationApplied, generationRecorded);
  generationRef.current = generation.job;
  generationOwns.current = generation.owns;
  const working = generation.canStop || activeGeneration(generation.job) || !!generation.operation;
  useEffect(() => { setShowLog(false); setSelected(null); }, [activeProjectId]);
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
  useEffect(() => { if (generation.visible) { setShowLog(true); generation.close(); } }, [generation.visible]);
  const draftCount = announcedDraftCount(generation.job, generation.owns);
  const showGenerationStatus = !!generation.error || announcesStatus(generation.job, { owns: generation.owns, stopped: generation.stopped, working });
  const composeJob = generation.job && ['prepared', 'generating', 'ready', 'blocked', 'applying'].includes(generation.job.status) ? generation.job : null;
  const composeUsesChatgpt = (composeJob?.authentication ?? aiSettings?.authentication) === 'chatgpt';
  const composeAccountId = composeJob ? composeJob.chatgptAccountId : aiSettings?.chatgpt.activeAccountId;
  const composeAccount = aiSettings?.chatgpt.accounts.find(account => account.id === composeAccountId);
  const aiConnected = !!aiSettings && !aiSettings.configurationError && (composeUsesChatgpt ? !!composeAccount?.connected && composeAccount.planEnabled && !composeAccount.storageRepairRequired : aiSettings.credential.configured);
  function openGeneration() {
    setLogFocus(value => value + 1);
    setShowLog(true);
    void generation.refresh();
    void refreshAiSettings().catch(() => {});
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
  async function allowWholePageAndRetry(elementId: string) {
    await generation.prepare([elementId]);
  }
  const requestEditorIsOpen = () => false;
  function openImportDialog() { setImportSource(null); setShowImport(true); }
  function resolveGenerationFix(fix: 'aiSettings' | 'import' | 'brief') {
    if (fix === 'aiSettings') setShowAiSettings(true);
    else if (fix === 'import') openImportDialog();
    else setShowNotes('decisions');
  }
  function generate(elementIds?: string[]) {
    const contextItems = selectedReference(canvasSelection);
    if (!requestedAreas && pendingImages) { setNotice('Pictures resized and updated.'); return; }
    setError('');
    setShowLog(true);
    void generation.prepare(elementIds, contextItems).catch(() => {});
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
    await mutate({ type: 'update_request', request: { ...request, attachments } });
    if (problems.length) setError(problems.join(' ')); else setNotice('Attached to the request.');
  }, [mutate]);
  // One undoable step covers a whole canvas selection, so removing never interrupts the work.
  function removeCanvasSelection(referenceIds: string[], pointerIds: string[] = [], requestIds: string[] = [], elementIds: { pageId: string; elementId: string }[] = []) {
    const count = referenceIds.length + pointerIds.length + requestIds.length + elementIds.length;
    if (!count) return;
    run(async () => {
      await mutate({
        type: 'delete_items',
        elements: elementIds.map(item => [item.pageId, item.elementId]) as [string, string][],
        reference_ids: referenceIds,
        pointer_ids: pointerIds,
        request_ids: requestIds,
      });
      setSelected(null); setCanvasSelection([]);
      requestAnimationFrame(() => { if (!document.querySelector('.workspace-surface:focus-within')) document.querySelector<HTMLElement>('.project-canvas')?.focus(); });
      setNotice(`${count} ${count === 1 ? 'item' : 'items'} deleted.`, () => { void travel('undo_project').catch(() => {}); });
    });
  }
  // A boundary is deleted as a whole page, or as the selected linked size.
  function deleteBoundary(pageId: string, sizeId: string | null) {
    const page = project?.pages.find(item => item.id === pageId);
    if (!page) return;
    const size = sizeId ? page.sizes.find(item => item.id === sizeId) : null;
    const removeSize = !!size && page.sizes.length > 1;
    const name = removeSize ? `${page.name} · ${size.label}` : page.name;
    run(async () => {
      await mutate(removeSize ? { type: 'delete_items', size_ids: [[pageId, size!.id]] } : { type: 'delete_items', page_ids: [pageId] });
      setCanvasFocus(null); setCanvasFocusSize(null); setCanvasSelection([]);
      requestAnimationFrame(() => { if (!document.querySelector('.workspace-surface:focus-within')) document.querySelector<HTMLElement>('.project-canvas')?.focus(); });
      setNotice(`Removed ${name}.`, () => { void travel('undo_project').catch(() => {}); });
    });
  }
  const run = (operation: () => Promise<unknown>) => { void operation().catch(() => {}); };
  const canEditProject = !!project && !onStartPage;
  const views = project?.views ?? [];

  // Named views live in the project file. Ctrl/Cmd+Alt+N saves into the view named "View N",
  // matching the names the '+' button creates, so numbered slots survive deletions and reordering.
  function viewForSlot(slot: number) {
    return views.find(view => view.name === viewNameForSlot(slot)) ?? null;
  }
  function nextViewName() {
    const names = new Set(views.map(view => view.name));
    let index = 1;
    while (names.has(viewNameForSlot(index))) index += 1;
    return viewNameForSlot(index);
  }
  function saveCurrentView(slot: number, camera: CanvasCamera) {
    if (!project) return;
    const existing = viewForSlot(slot);
    run(() => mutate(existing
      ? { type: 'update_named_view', view_id: existing.id, x: camera.x, y: camera.y, zoom: camera.zoom }
      : { type: 'add_named_view', name: viewNameForSlot(slot), x: camera.x, y: camera.y, zoom: camera.zoom }));
  }
  function addNamedViewAt(camera: CanvasCamera) {
    run(() => mutate({ type: 'add_named_view', name: nextViewName(), x: camera.x, y: camera.y, zoom: camera.zoom }));
  }
  function addNamedView() {
    const camera = canvasRef.current?.currentCamera();
    if (camera) addNamedViewAt(camera);
  }
  function renameNamedView(view: NamedView, name: string) {
    run(() => mutate({ type: 'update_named_view', view_id: view.id, name }));
  }
  function removeNamedView(view: NamedView) {
    run(() => mutate({ type: 'remove_named_view', view_id: view.id }));
  }

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
    { id: 'rename-project', label: 'Rename project…', disabled: !canEditProject, action: () => setShowRename(true) },
    ...(view?.recoveries.length ? [{ id: 'recover', label: 'Recover unsaved projects', action: () => setShowRecovery(true) }] : []),
    { id: 'file-divider', label: '', separator: true },
    { id: 'export', label: 'Export…', shortcut: shortcutLabel('export'), disabled: !canEditProject, action: openExportPanel },
    ...(canEditProject && project.lastExport ? [{ id: 'open-browser', label: 'Open in browser', action: () => run(openInBrowser) }] : []),
    { id: 'ai-divider', label: '', separator: true },
    { id: 'settings', label: 'AI settings', action: () => setShowAiSettings(true) },
    { id: 'mode', label: 'AI style', disabled: !canEditProject || working, children: (['fuzzy', 'strict'] as GenerationMode[]).map(mode => ({ id: mode, label: mode === 'fuzzy' ? 'Flexible' : 'Precise', checked: project?.generationMode === mode, action: () => run(() => mutate(current => current.generationMode === mode ? null : { type: 'set_generation_mode', mode })) })) },
    { id: 'style-guide', label: 'Style guide', disabled: !canEditProject, action: () => setShowStyleGuide(true) },
    { id: 'import', label: 'Import…', disabled: !canEditProject, action: () => { setImportSource(null); setShowImport(true); } },
    { id: 'dossier', label: 'Project brief', disabled: !canEditProject, action: () => setShowNotes('overview') },
    { id: 'help-divider', label: '', separator: true },
    { id: 'preferences', label: 'Preferences', children: [
      { id: 'show-start-page', label: 'Show start page on launch', checked: appSettings.showStartPage, action: () => run(() => changeShowStartPage(!appSettings.showStartPage)) },
      { id: 'preferences-divider', label: '', separator: true },
      { id: 'appearance', label: 'Editor theme', children: (['system', 'light', 'dark'] as Theme[]).map(value => ({ id: value, label: value === 'system' ? 'System' : value === 'light' ? 'Light' : 'Dark', checked: theme === value, action: () => setTheme(value) })) },
      { id: 'project-location', label: 'Default project location', action: () => setShowProjectLocation(true) },
    ] },
    { id: 'check-updates', label: 'Check for updates…', disabled: checkingUpdates, action: () => { void checkForUpdates(); } },
    { id: 'help', label: 'Keyboard shortcuts', shortcut: shortcutLabel('help'), action: () => setShowShortcuts(true) },
  ];

  const problemAction = needsAiSettings(error) ? { label: 'AI settings', onClick: () => setShowAiSettings(true) }
    : project && /permission denied|not writable|read.only|access denied|can't save here/i.test(error) ? { label: 'Choose another location', disabled: pending > 0, onClick: () => run(() => save(true)) } : undefined;
  const recovery: RecoveryChoices | null = pendingRecovery ? {
    path: pendingRecovery.path, busy: pending > 0,
    recover: () => run(() => recover(pendingRecovery.projectId)),
    openSaved: () => run(() => open(false, pendingRecovery.path, true)),
    dismiss: () => setPendingRecovery(null),
  } : null;
  const selectedBoundary = canvasFocus ? project?.pages.find(item => item.id === canvasFocus) ?? null : null;
  useEffect(() => setWidthPreview(null), [project?.id, project?.revision, canvasFocus, canvasFocusSize, canvasSelection]);
  const selectedReferenceItem = canvasSelection.length === 1 && canvasSelection[0].kind === 'reference'
    ? project?.reference.find(item => item.id === (canvasSelection[0] as { kind: 'reference'; itemId: string }).itemId) ?? null
    : null;
  const selectedPointer = canvasSelection.length === 1 && canvasSelection[0].kind === 'pointer'
    ? (project?.pointers ?? []).find(item => item.id === (canvasSelection[0] as { kind: 'pointer'; pointerId: string }).pointerId) ?? null
    : null;
  const selectedRequest = canvasSelection.length === 1 && canvasSelection[0].kind === 'request'
    ? (project?.requests ?? []).find(item => item.id === (canvasSelection[0] as { kind: 'request'; requestId: string }).requestId) ?? null
    : null;
  const selectedElementTargets = canvasSelection.flatMap(target => target.kind === 'element' ? [target] : []);
  const selectedElementPageId = selectedElementTargets.length && selectedElementTargets.every(target => target.pageId === selectedElementTargets[0].pageId) ? selectedElementTargets[0].pageId : null;
  const selectedElements = selectedElementPageId && project
    ? selectedElementTargets.flatMap(target => {
      const element = findPageElement(project, target.pageId, target.elementId);
      return element ? [element] : [];
    })
    : [];
  const selectedElementRequest = selectedElements.length === 1 && project
    ? (project.requests ?? []).find(request => request.targets.length === 1 && request.targets[0].kind === 'element' && request.targets[0].id === selectedElements[0].id) ?? null
    : null;
  const selectedPointerState = selectedPointer ? pointerStateMap[selectedPointer.id] ?? null : null;
  const selectedPointerOther = selectedPointerState?.clashWith.length
    ? (project?.pointers ?? []).find(item => item.id === selectedPointerState.clashWith[0]) ?? null
    : null;
  function selectNextPointerClash() {
    const ids = clashPointers.map(item => item.id);
    if (!ids.length) return;
    const index = selectedPointer ? ids.indexOf(selectedPointer.id) : -1;
    setCanvasSelection([{ kind: 'pointer', pointerId: ids[(index + 1) % ids.length] }]);
  }
  function chooseTool(next: EditorTool) {
    setInsertKind(null); setCanvasTool(next);
    // The next press belongs to the canvas: leave the tool strip or its flyout behind, so no
    // button keeps focus or the pointer.
    canvasRef.current?.focusCanvas();
  }
  // The inline preview covers its page, so a tool that draws or types there would act on nothing.
  useEffect(() => { if (preview && canvasTool !== 'select' && canvasTool !== 'pan') closePreview(); }, [canvasTool]);
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
    void moveCanvasElements([{ id: elementId, to_page_id: pageId, to_parent_id: parentId, index, rect, size_id: sizeId }]);
  }
  function armInsert(kind: ElementKind) {
    if (kind === 'image') { setInsertKind(null); setCanvasTool('image'); canvasRef.current?.focusCanvas(); return; }
    if (kind === 'text') { setInsertKind(null); setCanvasTool('text'); canvasRef.current?.focusCanvas(); return; }
    setInsertKind(kind);
    setCanvasTool('insert');
    canvasRef.current?.focusCanvas();
  }
  // Status readouts: what is selected and where it sits, then the canvas zoom. A boundary the
  // user is working inside answers when no layer is selected.
  const focusedBoundary = project && canvasFocus
    ? canvasBoundaries.find(item => item.pageId === canvasFocus && (!canvasFocusSize || item.sizeId === canvasFocusSize))
      ?? canvasBoundaries.find(item => item.pageId === canvasFocus) ?? null
    : null;
  const selectedBounds = project ? selectionBounds(project, canvasSelection) ?? (canvasSelection.length ? null : focusedBoundary?.rect ?? null) : null;
  const zoomPercent = Math.round(canvasZoom * 100);
  // The palette reaches canvas actions through the canvas's own dispatch, so a row runs exactly
  // what its key runs. A row stands down only while the action has nothing to change.
  const paletteCommands: PaletteCommand[] = [
    ...KEYMAP.filter(binding => binding.action !== 'commandPalette').map(binding => {
      const canvasAnswer = canvasRef.current?.canRunCanvasAction(binding.action) ?? null;
      const missingProject = (onStartPage || !project) && !['new', 'open', 'help'].includes(binding.action);
      return {
        id: `action:${binding.action}`, label: binding.label, keys: binding.display, group: binding.group,
        disabled: canvasAnswer === null ? missingProject : !canvasAnswer,
        run: () => { if (!canvasRef.current?.runCanvasAction(binding.action)) performAction(binding.action); },
      };
    }),
    ...menuPaletteCommands(menuItems),
  ];

  return {
    showPalette, paletteCommands, setShowPalette, closeRequest, setCloseRequest, onStartPage, project, view,
    run, switchProjectTab, closeProjectTab, menuItems, resolvedTheme, setTheme, compactHistory, showImport,
    setImportSource, setShowImport, travel, showHistory, setShowHistory, panels, panelActions, play,
    togglePlay, previewSelectedBoundary, showExport, openExportPanel, aiConnected, showAiSettings,
    setShowAiSettings, generation, showLog, setShowLog, requestedAreas, pendingImages, working, generate,
    error, setError, problemAction, openFailure, setNotice, setOpenFailure, updateResult, openRelease,
    setUpdateResult, updateError, setUpdateError, notices, setNotices, dismissedRecovery, setDismissedRecovery,
    loading, load, pending, newUntitled, open, appSettings, changeShowStartPage, recentProjects, openRecent,
    forgetRecent, pinRecent, recover, dismissRecoveries, canvasTool, chooseTool, canvasZoom, canvasRef,
    boundaryKind, setBoundaryKind, requestToolShape, setRequestToolShape, armInsert, insertSignal,
    selectCanvasBoundary, createCanvasBoundary, addProjectBoundary, saveCanvasPlacements, addNamedViewAt, saveCurrentView,
    canvasSelection, canvasFocus, canvasFocusSize, widthPreview, setWidthPreview, selectCanvasReference, selectCanvasPointer,
    selectCanvasRequest, clearCanvasSelection, removeCanvasSelection, addCanvasReference,
    updateCanvasReference, elementToReference, referenceToElement, attachToRequest, addCanvasRequest,
    updateCanvasRequest, deleteBoundary, addCanvasPointer, updateCanvasPointer, pointerStateMap, insertKind,
    setCanvasTool, setInsertKind, reportCanvasZoom, selectCanvasElement, insertCanvasElements,
    updateCanvasElements, moveCanvasElements, groupCanvasElements, ungroupCanvasElements, linkCanvasElements,
    unlinkCanvasElements, pasteLinkedInstances, playElement, setPlay, preview, closePreview, refreshPreview,
    showGenerationStatus, openImportDialog, setShowNotes, editGenerationRequest, requestEditorIsOpen,
    openGeneration, draftCount, logFocus, aiSettings, manageChatgptUsage, replyToGeneration, exportDestination,
    exportJob, exportSending, exportError, chooseExportDestination, runExport, sendExport, cancelExport,
    openExportFolder, openInBrowser, setShowExport, historyEpoch, accept, untitledSave, projectLocation,
    closeUntitledSave, saveUntitledProject, selectedPointer, selectedPointerState, forgetExportPointer,
    keepPointerComment, keepPointerCode, selectedPointerOther, keepPointerThis, mergePointerClash,
    selectedReferenceItem, selectedRequest, attachReferenceContext, selectedElements, selectedElementPageId,
    selectedElementTargets, renameCanvasElement, selectedElementRequest, setCanvasElementRequest,
    updateCanvasElementContext, selectedBoundary, updateCanvasPage, mutate, updateCanvasPageSize,
    addCanvasPageSize, renameProject, showDotGrid, changeGridFlags, save, canvasBoundaries,
    moveCanvasElementTo, removeLinkedDefinition, views, addNamedView, renameNamedView, removeNamedView,
    selectedBounds, clashPointers, selectNextPointerClash, zoomPercent, showGridSettings, setShowGridSettings,
    showShortcuts, setShowShortcuts, recovery, showProjectLocation, setShowProjectLocation,
    changeProjectLocation, showRecovery, setShowRecovery, importSource, importContext,
    setCanvasSelection, setCanvasFocus, focusCanvasBehaviour, behaviourFocus, showStyleGuide,
    setShowStyleGuide, showNotes, changeBrief, showRename, setShowRename,
    saveConflict, chooseSaveConflict, replacement, setReplacement, confirmation, setConfirmation,
    refreshAiSettings, updateAiSettings, aiPollingError,
  };
}
