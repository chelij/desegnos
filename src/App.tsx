import { CrashReportNotice, EditorCrashBoundary } from './CrashReport';
import { needsAiSettings, ProblemBanner, Notice, NoticeProvider, NoticeHost } from './ProblemBanner.js';
import { browserOpenUrl, userError } from './userErrors.js';
import { updateSummary, type UpdateCheck, type UpdateSummary } from './updateCheck';
import { useCallback, useEffect, useRef, useState, type FormEvent } from 'react';
import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { native, edit } from './api';
import { ToolStrip, type EditorTool, type ProjectTool, type RequestToolShape } from './EditorTools';
import { WorkspaceSurface, WorkspaceSurfaces, SurfaceHost, isTextField } from './WorkspaceSurface';
import { ProjectCanvas, type ProjectCanvasHandle } from './ProjectCanvas';
import { RecoveryChoice, isPendingRecovery, type PendingRecovery, type RecoveryChoices } from './RecoveryBanner';
import { SaveConflictDialog, type SaveConflict, type SaveOutcome, type SaveChoice } from './SaveConflict';
import brandLight from './assets/brand/desegnos-wordmark-light.svg';
import brandDark from './assets/brand/desegnos-wordmark-dark.svg';
import { ProjectLocationSettings } from './ProjectLocation';
import { StartPage } from './StartPage';
import type { ProjectLocationView } from './types';
import { Notes, type DossierSection } from './Notes';
import { MenuButton, type MenuItem } from './Menus';
import './shell.css';
import { shortcutFor, shortcutLabel, SHORTCUT_GROUPS, withShortcut } from './shortcuts';
import { Icon } from './icons';
import { StatusChip } from './StatusChip';
import { steppedZoom } from './canvasGeometry';
import { boundaryMoves, layoutBoundaries, pushApartBoundaries, toggleCanvasTarget, selectedReference, selectedElementIds, type Boundary, type BoundaryMove, type CanvasTarget, type ReferenceMove } from './projectCanvasGeometry';
import { findPageElement } from './projectCanvas/elementGeometry';
import { isNestedLayer } from './projectCanvas/links';
import { setPasteDefault, usePasteDefault } from './pastePreference';
import { ElementProperties } from './projectCanvas/elementProperties';
import { elementPath, findElementIn } from './projectCanvas/elementGeometry';
import { BoundaryProperties, CanvasLayers, EditorDock, PointerProperties, ProjectProperties, ReferenceProperties, RequestProperties, UntitledSaveForm, usePanelState } from './EditorDock';
import { AiSettings } from './AiSettings';
import { GenerationFeedback } from './Generation';
import { showsOpener } from './visibleActions';
import { StyleGuide } from './StyleGuide';
import { ImportContext } from './ImportContext';
import { WebsiteImport } from './WebsiteImport';
import { announcedDraftCount, announcesStatus } from './generationSession.js';
import { GenerationLog } from './GenerationLog';
import { askBeforeDelete, setAskBeforeDelete, useAskBeforeDelete } from './deletePreference';
import { useHistory } from './useHistory';
import { HistoryPanel } from './HistoryPanel';
import { activeGeneration, useGeneration } from './useGeneration';
import type { AiSettingsView, AppSettingsView, Appearance, BoundaryKind, Brief, CanvasTemplate, Command, CompactReport, Element, ElementKind, ElementMove, ElementPatch, GenerationJobView, GenerationMode, GridSettings, LinkedPasteInstance, Page, PageSize, PendingRequest, Pointer, PointerSource, PointerStateView, PointerTarget, Project, Rect, RecoveryInfo, ReferenceItem, RequestAttachment, RequestRecord, SizePreset, WorkspaceView, RecentProjectsView, RecentProject, ExportCheck, ExportJobView, OutputType } from './types';
import { baseSizeId, resolveForSize } from './projectCanvas/sizes';
import { elementBoundsHeight } from './projectCanvas/elementGeometry';
import { ExportPanel } from './projectCanvas/exportPanel';
import { idlePlay, playClick, playPointerAt, playIsActive, type PlayState } from './projectCanvas/play';
import type { PreviewState } from './ProjectCanvas';

type Theme = 'system' | 'light' | 'dark';
type Replacement = { label: string; run: (discard: boolean) => Promise<void> };
type UntitledSaveRequest = { name: string };
type Confirmation = { title: string; message: string; action: () => Promise<void>; skippable?: boolean; confirmLabel?: string };

const PROJECT_COLOURS = ['#2f6fde', '#c2410c', '#267a36', '#ae3ec9', '#0e7490', '#b35c00'];
function errorMessage(error: unknown) { if (error && typeof error === 'object' && 'message' in error) return String((error as { message: unknown }).message); return String(error ?? ''); }
function sizeName(width: number): string {
  return width >= 1024 ? 'Desktop' : width >= 700 ? 'Tablet' : 'Phone';
}

/** The boundaries a project draws, one per linked size, with Page heights from its content. */
function sizeBoundaries(project: Project): Boundary[] {
  return layoutBoundaries(project.pages.map(page => ({ id: page.id, sizes: page.sizes.map(size => ({
    ...size, height: Math.max(size.height, elementBoundsHeight(resolveForSize(project, page, size.id), size.height)),
  })) })), project.homePageId);
}

function initialZoom(width: number, height = 900) { const fit = width > 600 ? (window.innerWidth - 96) / width : (window.innerHeight - 240) / height; return Math.max(0.1, Math.min(1, fit)); }
function readRulerUnit(): 'pixels' | 'percent' { try { return localStorage.getItem('desegnos.ruler-unit') === 'percent' ? 'percent' : 'pixels'; } catch { return 'pixels'; } }
function readTheme(): Theme {
  try {
    const saved = localStorage.getItem('desegnos.editor-theme');
    return saved === 'light' || saved === 'dark' ? saved : 'system';
  }
  catch { return 'system'; }
}

function elementTree(elements: Element[]): Element[] {
  return elements.flatMap(element => [element, ...elementTree(element.children)]);
}

function elementTreeIds(elements: Element[]): string[] {
  return elementTree(elements).map(element => element.id);
}

function ownerPageId(project: Project, elementId: string): string | null {
  return project.pages.find(page => !!findElementIn(page.elements, elementId))?.id ?? null;
}


export default function App({ initialView }: { initialView?: WorkspaceView } = {}) {
  return <NoticeProvider><WorkspaceSurfaces><EditorCrashBoundary><Editor initialView={initialView} /></EditorCrashBoundary></WorkspaceSurfaces></NoticeProvider>;
}

function Editor({ initialView }: { initialView?: WorkspaceView } = {}) {
  const [closeRequest, setCloseRequest] = useState<{ message: string; resolve: (answer: boolean) => void } | null>(null);
  const [view, setView] = useState<WorkspaceView | null>(initialView ?? null);
  const current = useRef<WorkspaceView | null>(initialView ?? null);
  const tail = useRef<Promise<void>>(Promise.resolve());
  const [pending, setPending] = useState(0);
  const [loading, setLoading] = useState(!initialView);
  const [error, setError] = useState('');
  const [openFailure, setOpenFailure] = useState<{ message: string; url: string } | null>(null);
  const [checkingUpdates, setCheckingUpdates] = useState(false);
  const [updateResult, setUpdateResult] = useState<UpdateSummary | null>(null);
  const [updateError, setUpdateError] = useState('');
  const [notices, setNotices] = useState<{ id: number; message: string }[]>([]);
  const noticeId = useRef(0);
  const setNotice = useCallback((message: string) => { const id = ++noticeId.current; setNotices(items => [...items, { id, message }]); }, []);
  const [dismissedRecovery, setDismissedRecovery] = useState<string[]>([]);
  const [theme, setTheme] = useState<Theme>(readTheme);
  const [selected, setSelected] = useState<string | null>(null);
  const [focusPrompt, setFocusPrompt] = useState(false);
  const [zoom, setZoom] = useState(0.75);
  const [tool, setTool] = useState<EditorTool>('select');
  const [canvasTool, setCanvasTool] = useState<ProjectTool>('select');
  const [insertKind, setInsertKind] = useState<ElementKind | null>(null);
  const [insertSignal, setInsertSignal] = useState(0);
  const [boundaryKind, setBoundaryKind] = useState<BoundaryKind>('page');
  const [requestToolShape, setRequestToolShape] = useState<RequestToolShape>('pick');
  const [canvasSelection, setCanvasSelection] = useState<CanvasTarget[]>([]);
  const [canvasFocus, setCanvasFocus] = useState<string | null>(null);
  const [canvasFocusSize, setCanvasFocusSize] = useState<string | null>(null);
  const [canvasBoundaries, setCanvasBoundaries] = useState<Boundary[]>([]);
  const [canvasZoom, setCanvasZoom] = useState(1);
  const canvasRef = useRef<ProjectCanvasHandle>(null);
  const [panels, panelActions] = usePanelState();
  const [rulerUnit, setRulerUnit] = useState<'pixels' | 'percent'>(readRulerUnit);
  const [recentProjects, setRecentProjects] = useState<RecentProject[]>([]);
  const [appSettings, setAppSettings] = useState<AppSettingsView>({ showStartPage: true });
  const [showStartPage, setShowStartPage] = useState(!initialView);
  const [projectLocation, setProjectLocation] = useState<ProjectLocationView | null>(null);
  const [showProjectLocation, setShowProjectLocation] = useState(false);
  const [showExport, setShowExport] = useState(false);
  const [exportDestination, setExportDestination] = useState<string | null>(() => { try { return localStorage.getItem('desegnos.export-folder'); } catch { return null; } });
  const [exportJob, setExportJob] = useState<ExportJobView | null>(null);
  const [exportError, setExportError] = useState('');
  const [exportSending, setExportSending] = useState(false);
  const [checkedStates, setCheckedStates] = useState<PointerStateView[] | null>(null);
  const [checkedProject, setCheckedProject] = useState<string | null>(null);
  const [play, setPlay] = useState<PlayState>(idlePlay);
  const [preview, setPreview] = useState<PreviewState | null>(null);
  const [previewRevision, setPreviewRevision] = useState<number | null>(null);
  const checkTimer = useRef<number | null>(null);
  const [pendingImages, setPendingImages] = useState(0);
  const [untitledSave, setUntitledSave] = useState<UntitledSaveRequest | null>(null);
  const [pendingRecovery, setPendingRecovery] = useState<PendingRecovery | null>(null);
  async function requestProjectOpen<T>(operation: () => Promise<T>): Promise<T | null> {
    setPendingRecovery(null);
    try { return await operation(); }
    catch (error) {
      if (!isPendingRecovery(error)) throw error;
      setError(''); setPendingRecovery(error);
      return null;
    }
  }
  const [showNotes, setShowNotes] = useState<DossierSection | null>(null);
  const [showGridSettings, setShowGridSettings] = useState(false);
  const [showStyleGuide, setShowStyleGuide] = useState(false);
  const [showImport, setShowImport] = useState(false);
  const [showWebsiteImport, setShowWebsiteImport] = useState(false);
  const [importSource, setImportSource] = useState<import('./types').IdentitySource | null>(null);
  function startWebsiteImport() { setShowImport(false); setShowWebsiteImport(true); }
  const [showLog, setShowLog] = useState(false);
  const [logFocus, setLogFocus] = useState(0);
  const [resolvedTheme, setResolvedTheme] = useState<'light' | 'dark'>('light');
  const askDelete = useAskBeforeDelete();
  const pasteMode = usePasteDefault();
  const [skipNextAsk, setSkipNextAsk] = useState(false);
  const history = useHistory();
  const [showHistory, setShowHistory] = useState(false);
  const [historyEpoch, setHistoryEpoch] = useState(0);
  const savedPosition = useRef({ sessionId: null as string | null, position: 0 });
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
  async function changeProjectLocation(reset: boolean) {
    const path = reset ? null : await native<string | null>('pick_project_location');
    if (!reset && !path) return;
    setProjectLocation(await native<ProjectLocationView>('set_project_location', { path }));
  }
  useEffect(() => { try { localStorage.setItem('desegnos.ruler-unit', rulerUnit); } catch {} }, [rulerUnit]);

  const accept = useCallback((next: WorkspaceView) => { current.current = next; setView(next); }, []);
  const queue = useCallback(<T,>(operation: (workspace: WorkspaceView | null) => Promise<T>, reportErrors = true, quiet = false): Promise<T> => {
    if (!quiet) setPending(count => count + 1);
    const result = tail.current.then(() => operation(current.current));
    tail.current = result.then(() => undefined, () => undefined);
    return result.catch(error => { if (reportErrors) setError(errorMessage(error)); throw error; }).finally(() => { if (!quiet) setPending(count => count - 1); });
  }, []);
  const mutate = useCallback((command: Command | ((project: Project) => Command | null), quiet = false) => queue(async workspace => {
    const project = workspace?.project;
    if (!project) throw new Error('Create or open a project first.');
    const resolved = typeof command === 'function' ? command(project) : command;
    if (!resolved) return workspace!;
    const next = await edit(project.revision, resolved);
    accept(next); return next;
  }, true, quiet), [queue, accept]);
  const createCanvasBoundary = useCallback(async (bounds: Rect, kind: BoundaryKind, moves: BoundaryMove[], reference: ReferenceMove[]) => {
    const added = await mutate(project => {
      const first = project.pages.length === 0;
      const name = canvasPageName(project.pages);
      const width = Math.round(bounds.width), height = Math.round(bounds.height);
      const size: PageSize = { id: crypto.randomUUID(), label: sizeName(width), width, height, kind, x: bounds.x, y: bounds.y };
      return {
        type: 'add_canvas_page' as const, name, path: first ? '/' : suggestedPagePath(name, project.pages.map(page => page.path)),
        size,
        moves: moves.map(({ pageId, sizeId, x, y, kind }) => ({ page_id: pageId, size_id: sizeId, x, y, kind })),
        reference: reference.map(({ itemId, x, y }) => ({ item_id: itemId, x, y })),
      };
    });
    return added.project!.pages.at(-1)!.id;
  }, [mutate]);
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
    const action = async () => { await mutate({ type: 'delete_items', linked_ids: [id] }); };
    if (!askBeforeDelete()) { run(action); return; }
    setSkipNextAsk(false);
    setConfirmation({ title: 'Remove this linked element?', message: `Remove ${name}? Every copy becomes a plain layer.`, action, skippable: true });
  }, [mutate]);
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
  const elementToReference = useCallback(async (pageId: string, elementId: string, rect: Rect) => {
    setCanvasSelection([]);
    await mutate({ type: 'element_to_reference', page_id: pageId, element_id: elementId, rect });
  }, [mutate]);
  const referenceToElement = useCallback(async (itemId: string, pageId: string, rect: Rect, sizeId?: string | null) => {
    const next = await mutate({ type: 'reference_to_element', item_id: itemId, page_id: pageId, rect });
    const added = next.project?.pages.find(page => page.id === pageId)?.elements.at(-1);
    setCanvasFocus(pageId);
    setCanvasSelection(added ? [{ kind: 'element', pageId, elementId: added.id, sizeId: sizeId ?? null }] : []);
  }, [mutate]);
  const attachReferenceContext = useCallback(async (requestIds: string[], itemIds: string[]) => {
    if (!requestIds.length || !itemIds.length) return;
    await mutate({ type: 'attach_reference_context', request_ids: requestIds, item_ids: itemIds });
  }, [mutate]);
  const reportCanvasZoom = useCallback((zoom: number) => setCanvasZoom(zoom), []);
  const updateCanvasPage = useCallback((pageId: string, patch: { name?: string; path?: string; appearance?: Appearance; included?: boolean }) => {
    void mutate(project => {
      const page = project.pages.find(item => item.id === pageId);
      if (!page) return null;
      const name = patch.name ?? page.name, path = patch.path ?? page.path, appearance = patch.appearance ?? page.appearance;
      const included = patch.included ?? page.included;
      if (name === page.name && path === page.path && appearance === page.appearance && included === page.included) return null;
      return { type: 'update_page', page_id: pageId, name, path, appearance, included };
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
  const changeGridFlags = useCallback((patch: Partial<Pick<GridSettings, 'snap' | 'rulers'>>) => {
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
  useEffect(() => { history.observe(view); }, [view, history.observe]);
  useEffect(() => {
    // Keep unsaved edits in the project file while the project stays dirty and still.
    const active = view?.project;
    if (!view?.dirty || !active) return;
    const timer = window.setInterval(() => { void native('autosave_project').catch(() => {}); }, 3000);
    return () => window.clearInterval(timer);
  }, [view?.dirty, view?.project?.id]);
  useEffect(() => {
    // A different session starts a fresh count of the changes since the last saved version.
    savedPosition.current = { sessionId: view?.history?.sessionId ?? null, position: 0 };
  }, [view?.history?.sessionId]);
  useEffect(() => {
    // Once the project matches its file again (a save, or a restored version), counting restarts.
    if (view && !view.dirty && view.history) {
      savedPosition.current = { sessionId: view.history.sessionId, position: view.history.position };
    }
  }, [view]);
  function versionLabel() {
    return history.entries.filter(entry => entry.position > savedPosition.current.position).map(entry => entry.label).slice(-4).join(' · ');
  }
  useEffect(() => {
    const media = window.matchMedia('(prefers-color-scheme: dark)');
    const update = () => { const value = theme === 'system' ? (media.matches ? 'dark' : 'light') : theme; document.documentElement.dataset.theme = value; setResolvedTheme(value); };
    update(); media.addEventListener('change', update);
    try { localStorage.setItem('desegnos.editor-theme', theme); } catch {}
    return () => media.removeEventListener('change', update);
  }, [theme]);
  const settle = useCallback(async () => { await tail.current; }, []);
  const synchronize = useCallback(async () => { await settle(); }, [settle]);
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
        const identity = await native<import('./types').IdentityAnalysis | null>('get_identity_analysis').catch(() => null);
        const generating = generationRef.current?.status === 'generating' || identity?.status === 'analyzing';
        const plan = identity?.status === 'analyzing' ? (identity.authentication ?? aiCurrent.current?.authentication) === 'chatgpt' : generationRef.current?.authentication === 'chatgpt';
        if ((current.current?.anyDirty || current.current?.dirty || generating) && !await new Promise<boolean>(resolve => setCloseRequest({ resolve, message: `${generating ? `Generation is still running. Closing may interrupt the request, and ${plan ? 'submitted work may still use your ChatGPT plan or credits' : 'OpenAI may still charge for submitted work'}. Its outcome will be shown as uncertain when you return. ` : ''}${current.current?.dirty ? 'This project has changes that have not been saved to its file. ' : ''}Close Desegnos?` }))) return;
        await getCurrentWindow().destroy();
      } catch (error) { setError(errorMessage(error)); }
      finally { closing.current = false; }
    }).then(stop => { if (disposed) stop(); else unlisten = stop; }, error => setError(errorMessage(error)));
    return () => { disposed = true; unlisten?.(); };
  }, [settle]);

  function chooseSaveConflict(choice: SaveChoice) { const resolve = saveChoice.current; saveChoice.current = null; setSaveConflict(null); resolve?.(choice); }
  function closeUntitledSave(workspace: WorkspaceView | null) {
    untitledSaveResolve.current?.(workspace);
    untitledSaveResolve.current = null;
    setUntitledSave(null);
  }
  function requestUntitledSave(): Promise<WorkspaceView | null> {
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
  async function saveUntitledProject(name: string, path: string) {
    await settle();
    let outcome = await queue(() => native<SaveOutcome>('save_untitled_project', { name, path, label: versionLabel() || null }));
    while (outcome.status === 'conflict') {
      const conflict = outcome.conflict;
      const choice = await new Promise<SaveChoice>(resolve => { saveChoice.current = resolve; setSaveConflict(conflict); });
      outcome = await queue(() => native<SaveOutcome>('resolve_save_conflict', { token: conflict.token, choice }));
    }
    if (outcome.status === 'cancelled') throw new Error('Save cancelled.');
    accept(outcome.workspace); setNotice('Project saved.'); void refreshRecent().catch(() => {});
    savedPosition.current = { sessionId: outcome.workspace.history?.sessionId ?? null, position: outcome.workspace.history?.position ?? 0 };
    closeUntitledSave(outcome.workspace);
  }
  function save(saveAs = false): Promise<WorkspaceView | null> {
    if (current.current?.untitled) return requestUntitledSave();
    if (saveFlight.current) return saveFlight.current;
    const operation = async () => {
      await settle();
      const label = versionLabel() || null;
      let outcome = await queue(() => native<SaveOutcome>('save_project_checked', { saveAs, label }));
      while (outcome.status === 'conflict') {
        const conflict = outcome.conflict;
        const choice = await new Promise<SaveChoice>(resolve => { saveChoice.current = resolve; setSaveConflict(conflict); });
        outcome = await queue(() => native<SaveOutcome>('resolve_save_conflict', { token: conflict.token, choice }));
      }
      if (outcome.status === 'cancelled') return null;
      accept(outcome.workspace); setNotice('Project saved.'); void refreshRecent().catch(() => {});
      savedPosition.current = { sessionId: outcome.workspace.history?.sessionId ?? null, position: outcome.workspace.history?.position ?? 0 };
      return outcome.workspace;
    };
    saveFlight.current = operation().finally(() => { saveFlight.current = null; });
    return saveFlight.current;
  }
  async function replace(label: string, run: Replacement['run']) {
    await settle();
    if (current.current?.dirty) setReplacement({ label, run });
    else await run(false);
  }
  async function closeProject() {
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
    if (projectId === current.current?.project?.id) return;
    await settle();
    await queue(async () => {
      accept(await native<WorkspaceView>('switch_project', { projectId }));
      setShowStartPage(false);
      setSelected(null); setCanvasSelection([]); setCanvasFocus(null); setNotice('Switched project.');
    }).catch(() => {});
  }
  async function closeProjectTab(projectId: string) {
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
  async function importContext(analysisId: string, findings: number[], assets: string[], photos: string[], items: number[], pages: string[], attachMatching: boolean) {
    await settle();
    await queue(async workspace => {
      if (!workspace?.project) throw new Error('Create or open a project first.');
      const next = await native<WorkspaceView>('import_context', { revision: workspace.project.revision, identityAnalysisId: analysisId, identityFindingIndices: findings, identityAssetIds: assets, identityPhotoIds: photos, identityItemIndices: items, identityPagePaths: pages, attachMatchingPages: attachMatching });
      accept(next);
      setNotice('Context added to the project brief.');
    });
  }
  async function openAlongside(run: Replacement['run']) { await settle(); await run(false); }
  async function open(folder = false, path?: string, discardRecovery = false) {
    await openAlongside(async discard => {
      await queue(async () => {
        const opened = await requestProjectOpen(() => native<WorkspaceView | null>('open_project', { discard, folder, path: path ?? null, discardRecovery }));
        if (opened) { accept(opened); setShowStartPage(false); setSelected(null); setNotice('Project opened.'); void refreshRecent().catch(() => {}); if (opened.project) setZoom(initialZoom(opened.project.viewport.width, opened.project.viewport.height)); }
      });
    });
  }
  async function openRecent(path: string) {
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
  function openExportPanel(restore = true) {
    const project = current.current?.project;
    if (!project) return;
    setShowExport(true);
    if (restore && !exportJob) void native<ExportJobView | null>('get_export_job', { projectId: project.id }).then(job => { if (job) setExportJob(job); }).catch(() => {});
    if (panels.hidden) panelActions.toggleHidden();
    if (panels.collapsed) panelActions.setCollapsed(false);
  }
  async function chooseExportDestination() {
    try {
      const picked = await native<{ path: string } | null>('pick_export_destination');
      if (!picked) return;
      setExportDestination(picked.path);
      try { localStorage.setItem('desegnos.export-folder', picked.path); } catch { /* Preferences can be unavailable. */ }
    } catch (error) { setExportError(errorMessage(error)); }
  }
  async function refreshWorkspace() {
    try { accept(await native<WorkspaceView>('get_workspace')); } catch { /* The next edit reports its own error. */ }
  }
  function showJobResult(job: ExportJobView, previewPageId?: string | null) {
    if (job.status === 'ready') void refreshWorkspace();
    if (job.result?.previewUrl) {
      const pageId = previewPageId ?? job.pages[0]?.pageId ?? null;
      if (pageId) { setPreview({ pageId, url: job.result.previewUrl, stale: false }); setPreviewRevision(current.current?.project?.revision ?? null); }
    }
    if (job.status === 'ready' && !job.result?.previewUrl) setNotice(job.message);
  }
  async function runExport(previewPageId: string | null = null) {
    await settle();
    const project = current.current?.project;
    if (!project) return;
    if (!previewPageId && !exportDestination) { await chooseExportDestination(); return; }
    setExportError(''); setShowExport(true); setExportSending(false); setExportJob(null);
    try {
      const job = await native<ExportJobView>('prepare_export', {
        projectId: project.id, revision: project.revision,
        destination: previewPageId ? null : exportDestination,
        pageIds: previewPageId ? [previewPageId] : null,
        preview: !!previewPageId,
      });
      setExportJob(job);
      showJobResult(job, previewPageId);
    } catch (error) { setExportError(errorMessage(error)); }
  }
  async function sendExport() {
    if (!exportJob) return;
    setExportSending(true); setExportError('');
    try {
      const job = await native<ExportJobView>('start_export', { jobId: exportJob.jobId });
      setExportJob(job);
      showJobResult(job);
      if (job.status === 'failed' || job.status === 'cancelled') setExportError(job.message);
    } catch (error) { setExportError(errorMessage(error)); }
    finally { setExportSending(false); }
  }
  async function cancelExport() {
    if (!exportJob) return;
    try { setExportJob(await native<ExportJobView>('cancel_export', { jobId: exportJob.jobId })); }
    catch (error) { setExportError(errorMessage(error)); }
  }
  function openExportFolder(path: string) {
    const project = current.current?.project;
    if (!project) return;
    void native('open_export_folder', { projectId: project.id, path }).catch(error => setError(errorMessage(error)));
  }
  async function openInBrowser() {
    const project = current.current?.project;
    if (!project) return;
    try { await native<string>('open_in_browser', { projectId: project.id }); }
    catch (error) {
      const message = errorMessage(error);
      const url = message.match(/https?:\/\/\S+/)?.[0];
      setError(message);
      if (url) setOpenFailure({ message, url });
    }
  }
  async function keepPointerCode(pointerId: string) {
    const project = current.current?.project;
    if (!project) return;
    try {
      const job = await native<ExportJobView>('keep_pointer_code', { projectId: project.id, pointerId });
      setExportJob(job); openExportPanel(false); setShowHistory(false);
    } catch (error) { setError(errorMessage(error)); }
  }
  function forgetExportPointer(pointerId: string) {
    void mutate({ type: 'forget_export_pointer', pointer_id: pointerId }).catch(() => {});
  }
  // Clash check: fingerprints are recomputed from the last export folder on open and on focus.
  const recheckExport = useCallback(async () => {
    const project = current.current?.project;
    if (!project) { setCheckedStates(null); setCheckedProject(null); return; }
    try {
      const check = await native<ExportCheck>('recheck_export', { projectId: project.id });
      setCheckedStates(check.states); setCheckedProject(project.id);
    } catch { /* A missing folder simply reports the project's own states. */ }
  }, []);
  const scheduleExportCheck = useCallback(() => {
    if (checkTimer.current !== null) window.clearTimeout(checkTimer.current);
    checkTimer.current = window.setTimeout(() => { checkTimer.current = null; void recheckExport(); }, 250);
  }, [recheckExport]);
  useEffect(() => { const currentProject = current.current?.project; if (!currentProject) { setCheckedStates(null); setCheckedProject(null); return; } scheduleExportCheck(); }, [view?.project?.id, view?.project?.revision, view?.project?.lastExport?.id, scheduleExportCheck]);
  useEffect(() => {
    const onFocus = () => scheduleExportCheck();
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, [scheduleExportCheck]);
  useEffect(() => { if (showHistory) scheduleExportCheck(); }, [showHistory, scheduleExportCheck]);
  // Play and Preview export.
  function togglePlay(pageId?: string | null) {
    const target = pageId ?? canvasFocus ?? (selectedElementPageId ?? null);
    if (!target) { setNotice('Select a boundary to play.'); return; }
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
  function previewSelectedBoundary() {
    const target = canvasFocus ?? selectedElementPageId ?? current.current?.project?.homePageId ?? null;
    if (!target) return;
    const page = current.current?.project?.pages.find(item => item.id === target);
    if (page && page.output !== 'webpage' && page.output !== 'appScreen') {
      setNotice('Slides and images export directly; preview shows AI-built pages.');
      return;
    }
    if (playIsActive(play)) setPlay(idlePlay);
    void runExport(target);
  }
  function closePreview() {
    setPreview(null);
    void native('close_preview').catch(() => {});
  }
  function refreshPreview() {
    const pageId = preview?.pageId;
    if (!pageId) return;
    closePreview();
    void runExport(pageId);
  }
  useEffect(() => { const revision = view?.project?.revision; if (preview && revision !== undefined && previewRevision !== null && revision !== previewRevision) setPreview(value => value ? { ...value, stale: true } : value); }, [view?.project?.revision, preview, previewRevision]);
  useEffect(() => { if (!view?.project) setShowExport(false); setExportJob(null); setExportError(''); setPreview(null); setPlay(idlePlay); }, [view?.project?.id]);
  useEffect(() => {
    if (!preview && !playIsActive(play)) return;
    const escape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || event.defaultPrevented) return;
      if (isTextField(event.target)) return;
      event.preventDefault();
      if (preview) { closePreview(); return; }
      setPlay(idlePlay);
    };
    window.addEventListener('keydown', escape);
    return () => window.removeEventListener('keydown', escape);
  }, [preview, play]);
  async function newUntitled(template: CanvasTemplate = 'blank') {
    await settle();
    await queue(async () => {
      const next = await native<WorkspaceView>('new_untitled_project', { appearance: resolvedTheme, template });
      accept(next);
      setShowStartPage(false);
      setSelected(null); setCanvasSelection([]); setCanvasFocus(null); setNotice('New untitled project.');
      if (next.project) setZoom(initialZoom(next.project.viewport.width, next.project.viewport.height));
      requestAnimationFrame(() => canvasRef.current?.fit());
    });
  }
  async function recover(projectId: string) {
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
    await queue(async () => {
      for (const projectId of projectIds) {
        if (!current.current?.recoveries.some(item => item.projectId === projectId)) continue;
        accept(await native<WorkspaceView>('dismiss_recovery', { projectId }));
      }
    });
  }
  useEffect(() => { if (!view?.recoveries.length) setShowRecovery(false); }, [view?.recoveries.length]);
  async function travel(command: 'undo_project' | 'redo_project') {
    await settle();
    await queue(async () => { accept(await native<WorkspaceView>(command)); setNotice(command === 'undo_project' ? 'Change undone.' : 'Change redone.'); });
  }
  async function saveCopy() {
    const result = await queue(() => native<WorkspaceView | null>('save_project_copy'));
    if (result) setNotice('Saved a copy without history.');
  }
  async function compactHistory() {
    const report = await queue(() => native<CompactReport>('compact_history'));
    setHistoryEpoch(value => value + 1);
    setNotice(report.removedVersions === 0 && report.removedBlobs === 0
      ? 'History is already compact.'
      : `History compacted: removed ${report.removedVersions} ${report.removedVersions === 1 ? 'version' : 'versions'}.`);
  }
  const changeBrief = useCallback((patch: Partial<Brief>) => mutate(project => ({ type: 'set_brief', brief: { ...project.brief, ...patch } }))
    .then(workspace => workspace.project!.brief), [mutate]);

  useEffect(() => {
    const listener = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      const editingText = isTextField(event.target);
      // Text fields keep their own editing history; WebKitGTK needs the explicit mapping.
      if (editingText && navigator.userAgent.includes('Linux') && (event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'z') {
        event.preventDefault(); document.execCommand(event.shiftKey ? 'redo' : 'undo'); return;
      }
      const action = shortcutFor(event, editingText);
      if (!action) return;
      // The start page has no canvas: project commands wait until a canvas is open again.
      if ((showStartPage || !current.current?.project) && !['new', 'open', 'help'].includes(action)) return;
      const project = current.current?.project;
      if (event.repeat && ['new', 'open', 'saveAs', 'help', 'hidePanels'].includes(action)) return;
      switch (action) {
        // The canvas owns these; its listener runs first and prevents the default.
        case 'copy': case 'cut': case 'paste': case 'pasteChoice': case 'duplicate':
        case 'link': case 'unlink': case 'group': case 'ungroup': case 'forward': case 'backward': case 'front': case 'back':
        case 'selectAll': case 'selectChild': case 'selectParent': case 'nudge': case 'nudgeFar': case 'delete':
          return;
        case 'help': event.preventDefault(); setShowShortcuts(true); return;
        case 'new': event.preventDefault(); void newUntitled().catch(() => {}); return;
        case 'open': event.preventDefault(); void open().catch(() => {}); return;
        case 'renameLayer': {
          const field = document.querySelector<HTMLInputElement>('[data-field="layer-name"]');
          if (!field) return;
          event.preventDefault(); field.focus({ preventScroll: true }); field.select(); return;
        }
        case 'insert': event.preventDefault(); setInsertKind(null); setInsertSignal(value => value + 1); return;
        case 'imageTool': event.preventDefault(); setInsertKind(null); setCanvasTool('image'); return;
        case 'selectTool': case 'boundaryTool': case 'pointerTool': case 'textTool': case 'panTool':
        case 'rectTool': case 'ellipseTool': case 'lineTool': case 'noteTool': {
          event.preventDefault(); setInsertKind(null);
          setCanvasTool(action === 'selectTool' ? 'select' : action === 'boundaryTool' ? 'boundary'
            : action === 'pointerTool' ? 'pointer' : action === 'textTool' ? 'text' : action === 'panTool' ? 'pan'
              : action === 'rectTool' ? 'rect' : action === 'ellipseTool' ? 'ellipse' : action === 'lineTool' ? 'line' : 'note');
          return;
        }
        case 'requestTool': case 'requestFromShape':
          // The canvas starts a request on the selection; with nothing selected F picks the tool.
          return;
        case 'askAI': event.preventDefault(); document.querySelector<HTMLElement>('[data-element-request]')?.focus(); return;
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
        case 'preview': if (!project) return; event.preventDefault(); previewSelectedBoundary(); return;
        case 'export':
          if (!project) return;
          event.preventDefault(); openExportPanel();
          if (exportDestination) void runExport(); else void chooseExportDestination();
          return;
        case 'undo': case 'redo':
          if (!project) return;
          event.preventDefault();
          if (action === 'undo' ? current.current?.canUndo : current.current?.canRedo) {
            void travel(action === 'undo' ? 'undo_project' : 'redo_project').catch(() => {});
          }
          return;
        case 'save': if (!project) return; event.preventDefault(); void save().catch(() => {}); return;
        case 'saveAs': if (!project) return; event.preventDefault(); void save(true).catch(() => {}); return;
        case 'closeProject': if (!project) return; event.preventDefault(); void closeProject().catch(() => {}); return;
      }
    };
    window.addEventListener('keydown', listener);
    return () => window.removeEventListener('keydown', listener);
  });

  const project = view?.project ?? null;
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
    const readImage = (file: File) => new Promise<string>((resolve, reject) => { const reader = new FileReader(); reader.onload = () => resolve(String(reader.result)); reader.onerror = () => reject(new Error(`${file.name} could not be read.`)); reader.readAsDataURL(file); });
    const attachments = [...request.attachments];
    const problems: string[] = [];
    for (const file of files) {
      try {
        let attachment: RequestAttachment;
        if (/^image\/(png|jpeg|webp|gif)$/.test(file.type)) attachment = { id: crypto.randomUUID().replace(/-/g, '').slice(0, 16), name: file.name.slice(0, 120), kind: 'image', dataUri: await readImage(file) };
        else if (file.type.startsWith('text/') || /\.(txt|md|csv|json)$/i.test(file.name)) attachment = { id: crypto.randomUUID().replace(/-/g, '').slice(0, 16), name: file.name.slice(0, 120), kind: 'text', text: await file.text() };
        else throw new Error(`${file.name}: only pictures (PNG, JPEG, WebP, GIF) and text files can be attached.`);
        attachments.push(attachment);
      } catch (reason) { problems.push(errorMessage(reason)); }
    }
    for (const url of links) {
      try { attachments.push({ id: crypto.randomUUID().replace(/-/g, '').slice(0, 16), name: new URL(url).hostname.slice(0, 120), kind: 'link', url }); }
      catch (reason) { problems.push(errorMessage(reason)); }
    }
    await mutate({ type: 'update_request', request: { ...request, attachments } });
    if (problems.length) setError(problems.join(' ')); else setNotice('Attached to the request.');
  }, [mutate]);
  async function switchAppearance(next: 'light' | 'dark') {
    setTheme(next);
    const target = current.current?.project; if (!target) return;
    for (const item of target.pages.filter(entry => entry.appearance !== next)) {
      await mutate(project => { const page = project.pages.find(entry => entry.id === item.id); return page && page.appearance !== next ? { type: 'update_page', page_id: page.id, name: page.name, path: page.path, appearance: next, included: page.included } : null; });
    }
  }
  // One confirmation covers a whole canvas selection, then each item removes itself.
  function removeCanvasSelection(referenceIds: string[], pointerIds: string[] = [], requestIds: string[] = [], elementIds: { pageId: string; elementId: string }[] = [], ask = true) {
    if (!referenceIds.length && !pointerIds.length && !requestIds.length && !elementIds.length) return;
    const action = async () => {
      await mutate({
        type: 'delete_items',
        elements: elementIds.map(item => [item.pageId, item.elementId]) as [string, string][],
        reference_ids: referenceIds,
        pointer_ids: pointerIds,
        request_ids: requestIds,
      });
      setSelected(null); setCanvasSelection([]);
      requestAnimationFrame(() => { if (!document.querySelector('.workspace-surface:focus-within')) document.querySelector<HTMLElement>('.project-canvas')?.focus(); });
    };
    const plural = referenceIds.length + pointerIds.length + requestIds.length + elementIds.length > 1;
    if (!ask || !askBeforeDelete()) { run(action); return; }
    setSkipNextAsk(false);
    setConfirmation({ title: plural ? 'Remove these items?' : 'Remove this item?', message: plural ? 'Remove these layers, reference items, pointers and requests?' : 'Remove this item?', action, skippable: true });
  }
  // A boundary is deleted as a whole page, or as the selected linked size.
  function deleteBoundary(pageId: string, sizeId: string | null) {
    const page = project?.pages.find(item => item.id === pageId);
    if (!page) return;
    const size = sizeId ? page.sizes.find(item => item.id === sizeId) : null;
    const removeSize = !!size && page.sizes.length > 1;
    const name = removeSize ? `${page.name} · ${size.label}` : page.name;
    const action = async () => {
      await mutate(removeSize ? { type: 'delete_items', size_ids: [[pageId, size!.id]] } : { type: 'delete_items', page_ids: [pageId] });
      setCanvasFocus(null); setCanvasFocusSize(null); setCanvasSelection([]);
      requestAnimationFrame(() => { if (!document.querySelector('.workspace-surface:focus-within')) document.querySelector<HTMLElement>('.project-canvas')?.focus(); });
    };
    if (!askBeforeDelete()) { run(action); return; }
    setSkipNextAsk(false);
    setConfirmation({ title: removeSize ? 'Remove this size?' : 'Remove this boundary?', message: removeSize ? `Remove ${name}? Its layers stay in the other sizes.` : `Remove ${name} and everything in it?`, action, skippable: true });
  }
  const run = (operation: () => Promise<unknown>) => { void operation().catch(() => {}); };
  const canEditProject = !!project && !onStartPage;

  const menuItems: MenuItem[] = [
    { id: 'home', label: 'Home', disabled: showStartPage || !project, action: () => setShowStartPage(true) },
    { id: 'back-to-canvas', label: 'Back to canvas', disabled: !project || !showStartPage, action: () => setShowStartPage(false) },
    { id: 'new', label: 'New project', shortcut: shortcutLabel('new'), action: () => run(newUntitled) },
    { id: 'open', label: 'Open project…', shortcut: shortcutLabel('open'), action: () => run(open) },
    { id: 'open-recent', label: 'Open recent', disabled: recentProjects.length === 0, children: recentProjects.length ? [
      ...recentProjects.map((entry, index) => ({ id: `open-recent-${index}`, label: entry.name, action: () => run(() => openRecent(entry.path)) })),
      { id: 'open-recent-separator', label: '', separator: true },
      { id: 'forget-recent', label: 'Forget recent project', children: recentProjects.map((entry, index) => ({ id: `forget-recent-${index}`, label: entry.name, action: () => run(() => forgetRecent(entry.path)) })) },
    ] : [] },
    { id: 'open-folder', label: 'Open project folder…', action: () => run(() => open(true)) },
    { id: 'save', label: 'Save', shortcut: shortcutLabel('save'), disabled: !canEditProject, action: () => run(() => save()) },
    { id: 'close-project', label: 'Close project', shortcut: shortcutLabel('closeProject'), disabled: !canEditProject, action: () => run(closeProject) },
    { id: 'save-as', label: 'Save as', shortcut: shortcutLabel('saveAs'), disabled: !canEditProject, action: () => run(() => save(true)) },
    { id: 'save-copy', label: 'Save a copy without history…', disabled: !canEditProject || pending > 0, action: () => run(saveCopy) },
    { id: 'rename-project', label: 'Rename project…', disabled: !canEditProject, action: () => setShowRename(true) },
    { id: 'compact-history', label: 'Compact history', disabled: !canEditProject || pending > 0, action: () => run(compactHistory) },
    ...(view?.recoveries.length ? [{ id: 'recover', label: 'Recover unsaved projects', action: () => setShowRecovery(true) }] : []),
    { id: 'file-divider', label: '', separator: true },
    { id: 'export', label: 'Export…', shortcut: shortcutLabel('export'), disabled: !canEditProject, action: openExportPanel },
    { id: 'open-browser', label: 'Open in browser', disabled: !canEditProject || !project?.lastExport, action: () => run(openInBrowser) },
    { id: 'publish', label: 'Publish online (coming soon)', disabled: true },
    { id: 'ai-divider', label: '', separator: true },
    { id: 'settings', label: 'AI settings', action: () => setShowAiSettings(true) },
    { id: 'mode', label: 'AI style', disabled: !canEditProject || working, children: (['fuzzy', 'strict'] as GenerationMode[]).map(mode => ({ id: mode, label: mode === 'fuzzy' ? 'Flexible' : 'Precise', checked: project?.generationMode === mode, action: () => run(() => mutate(current => current.generationMode === mode ? null : { type: 'set_generation_mode', mode })) })) },
    { id: 'style-guide', label: 'Style guide', disabled: !canEditProject, action: () => setShowStyleGuide(true) },
    { id: 'import', label: 'Import context…', disabled: !canEditProject, action: () => { setImportSource(null); setShowImport(true); } },
    { id: 'import-website', label: 'Import website…', disabled: !canEditProject, action: () => startWebsiteImport() },
    { id: 'dossier', label: 'Project brief', disabled: !canEditProject, action: () => setShowNotes('overview') },
    { id: 'help-divider', label: '', separator: true },
    { id: 'preferences', label: 'Preferences', children: [
      { id: 'show-start-page', label: 'Show start page on launch', checked: appSettings.showStartPage, action: () => run(() => changeShowStartPage(!appSettings.showStartPage)) },
      { id: 'preferences-divider', label: '', separator: true },
      { id: 'appearance', label: 'Editor theme', children: (['system', 'light', 'dark'] as Theme[]).map(value => ({ id: value, label: value === 'system' ? 'System' : value === 'light' ? 'Light' : 'Dark', checked: theme === value, action: () => setTheme(value) })) },
      { id: 'project-location', label: 'Default project location', action: () => setShowProjectLocation(true) },
      { id: 'ask-delete', label: 'Ask before deleting', checked: askDelete, action: () => setAskBeforeDelete(!askDelete) },
      { id: 'paste-default', label: 'Ask on paste', checked: !pasteMode, action: () => setPasteDefault(null) },
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
  return <div className="application editor-shell">
    {closeRequest && <WorkspaceSurface place="banner" title="Close Desegnos?" close={() => { closeRequest.resolve(false); setCloseRequest(null); }}><p>{closeRequest.message}</p><div className="form-actions"><button onClick={() => { closeRequest.resolve(true); setCloseRequest(null); }}>Close Desegnos</button><button onClick={() => { closeRequest.resolve(false); setCloseRequest(null); }}>Keep editing</button></div></WorkspaceSurface>}
    <header className="app-bar"><div className="wordmark" aria-label="Desegnos"><img className="brand-light" src={brandLight} alt="" /><img className="brand-dark" src={brandDark} alt="" /><span className="sr-only">Desegnos</span></div>
      {!onStartPage && project && !!view?.openProjects?.length && <nav className="project-tabs" aria-label="Open projects">
        {view.openProjects.map((entry, index) => <div className={entry.active ? 'project-tab active' : 'project-tab'} key={entry.id}>
          <button type="button" className="project-tab-name" title={entry.name} aria-current={entry.active ? 'page' : undefined} onClick={() => run(() => switchProjectTab(entry.id))}>
            <span className="project-dot" style={{ background: PROJECT_COLOURS[index % PROJECT_COLOURS.length] }} aria-hidden="true" />
            <span className="project-tab-label">{entry.name}</span>
            {entry.dirty && <span className="dirty-dot" aria-label="Unsaved changes" />}
          </button>
          <button type="button" className="project-tab-close icon-button sm" aria-label={`Close ${entry.name}`} title={`Close ${entry.name}`} onClick={() => run(() => closeProjectTab(entry.id))}><Icon name="close" size={12} /></button>
        </div>)}
      </nav>}
      <MenuButton label="Menu" ariaLabel="Editor menu" items={menuItems} />
      <div className="shell-primary-actions">
        <button type="button" className="theme-toggle icon-button" aria-label={resolvedTheme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'} title={!onStartPage && project ? (resolvedTheme === 'dark' ? 'Light mode for the editor and canvas' : 'Dark mode for the editor and canvas') : (resolvedTheme === 'dark' ? 'Light mode' : 'Dark mode')} onClick={() => onStartPage || !project ? setTheme(resolvedTheme === 'dark' ? 'light' : 'dark') : run(() => switchAppearance(resolvedTheme === 'dark' ? 'light' : 'dark'))}>{resolvedTheme === 'dark' ? <Icon name="sun" /> : <Icon name="moon" />}</button>
        {project && showsOpener({ available: true, open: showImport }) && <button type="button" className="icon-button" aria-label="Import context" title="Import context" onClick={() => { setImportSource(null); setShowImport(true); }}><Icon name="folder" /></button>}
        {!onStartPage && project && <>{view?.canUndo && <button aria-label="Undo last change" type="button" className="icon-button" title={withShortcut('Undo', 'undo')} onClick={() => run(() => travel('undo_project'))}><Icon name="undo" /></button>}{view?.canRedo && <button aria-label="Redo last change" type="button" className="icon-button" title={withShortcut('Redo', 'redo')} onClick={() => run(() => travel('redo_project'))}><Icon name="redo" /></button>}
          <button type="button" className={showHistory ? 'icon-button active' : 'icon-button'} aria-pressed={showHistory} aria-label="History and alternatives" title="History and alternatives" onClick={() => { setShowHistory(value => !value); if (panels.hidden) panelActions.toggleHidden(); if (panels.collapsed) panelActions.setCollapsed(false); }}><Icon name="history" /></button></>}
        {!onStartPage && project && <button type="button" className={playIsActive(play) ? 'icon-button active' : 'icon-button'} aria-pressed={playIsActive(play)} aria-label="Play" title={withShortcut('Play', 'play')} onClick={() => togglePlay()}><Icon name="play" /></button>}
        {!onStartPage && project && <button type="button" className="icon-button" aria-label="Preview export" title={withShortcut('Preview export', 'preview')} onClick={previewSelectedBoundary}><Icon name="monitor" /></button>}
        {!onStartPage && project && <button type="button" className={showExport ? 'icon-button active' : 'icon-button'} aria-pressed={showExport} aria-label="Export" title={withShortcut('Export', 'export')} onClick={() => openExportPanel()}><Icon name="download" /></button>}
        {showsOpener({ available: !aiConnected, open: showAiSettings }) && <button className="ai-shortcut needs-connection" onClick={() => setShowAiSettings(true)} aria-label="AI settings" title="Connect AI to generate">Connect AI</button>}
        {showsOpener({ available: !onStartPage && !!project && !!(generation.job || generation.operation), open: showLog }) && <button type="button" className="icon-button" aria-label="AI log" title="AI conversation" onClick={() => setShowLog(true)}><Icon name="note" /></button>}
        {!onStartPage && project && <button className={generation.canStop ? undefined : 'primary'} title={generation.canStop ? 'Stop generation (Esc)' : requestedAreas === 0 && pendingImages === 0 ? 'Describe at least one request first.' : 'Generate the described changes'} disabled={!generation.canStop && ((requestedAreas === 0 && pendingImages === 0) || working)} onClick={generation.canStop ? () => { void generation.cancel(); } : () => generate()}>{generation.canStop ? 'Stop' : 'Generate'}</button>}
      </div>
    </header>
    <SurfaceHost place="banner" />
    {onStartPage && <NoticeHost />}
    {error && <ProblemBanner message={userError(error)} dismiss={() => setError('')} action={problemAction} />}
    {openFailure && <Notice variant="warning" title="Couldn't open your browser" message={openFailure.message} action={{ label: 'Copy link', onClick: () => { const url = openFailure.url; void native('copy_text', { text: url }).then(() => setNotice('Link copied.')).catch(error => setError(errorMessage(error))); } }} dismiss={() => setOpenFailure(null)} />}
    {updateResult && <Notice variant={updateResult.variant} message={updateResult.message} action={updateResult.download ? { label: 'Download', onClick: () => { const url = updateResult.download; if (url) void openRelease(url); } } : undefined} dismiss={() => setUpdateResult(null)} />}
    {updateError && <ProblemBanner message={userError(updateError)} dismiss={() => setUpdateError('')} />}
    {notices.map(item => <Notice key={item.id} variant="info" message={item.message} dismiss={() => setNotices(items => items.filter(queued => queued.id !== item.id))} />)}
    <CrashReportNotice />
    {view?.recoveryError && !dismissedRecovery.includes(`error:${view.recoveryError}`) && <ProblemBanner message={userError(view.recoveryError)} dismiss={() => setDismissedRecovery(items => [...items, `error:${view.recoveryError}`])} />}
    {loading ? <main className="startup"><p role="status">Opening your workspace…</p></main> : !view ? <main className="startup"><h1>Open the Desegnos desktop application</h1><button onClick={() => { void load(); }}>Retry workspace connection</button></main> : onStartPage ? <>
      <SurfaceHost place="start" />
      <StartPage busy={pending > 0} create={template => run(() => newUntitled(template))}
        openProject={() => run(open)} showStartPageOnLaunch={appSettings.showStartPage} changeShowStartPage={value => run(() => changeShowStartPage(value))}
        recentProjects={recentProjects} openRecent={path => run(() => openRecent(path))}
        forgetRecent={path => run(() => forgetRecent(path))} pinRecent={(path, pinned) => run(() => pinRecent(path, pinned))}
        recovery={view.recoveries.length ? <RecoveryList items={view.recoveries} busy={pending > 0} recover={id => { void recover(id).catch(() => {}); }} dismiss={dismissRecoveries} /> : null} />
    </> : <>
      <div className="editor-body">
      {!panels.hidden && <ToolStrip tool={canvasTool} onTool={chooseTool}
        zoom={canvasZoom}
        onZoomStep={direction => canvasRef.current?.zoomStep(direction)}
        onZoom={value => canvasRef.current?.zoomTo(value)}
        onFit={() => canvasRef.current?.fit()}
        boundaryKind={boundaryKind} onBoundaryKind={setBoundaryKind}
        requestShape={requestToolShape} onRequestShape={setRequestToolShape}
        onInsert={armInsert} insertSignal={insertSignal} />}
      <div className="editor-work">
      <main className="workspace" id="project-panel">
      <div className="design-pane">
      <SurfaceHost place="canvas" />
      <NoticeHost />
      <ProjectCanvas ref={canvasRef} project={project} openPage={id => selectCanvasBoundary(id)} createBoundary={createCanvasBoundary} savePlacements={saveCanvasPlacements} selection={canvasSelection} focused={canvasFocus} focusedSize={canvasFocusSize} selectBoundary={selectCanvasBoundary} selectReference={selectCanvasReference} selectPointer={selectCanvasPointer} selectRequest={id => selectCanvasRequest(id)} clearSelection={clearCanvasSelection} removeSelection={removeCanvasSelection} addReference={addCanvasReference} updateReference={updateCanvasReference} elementToReference={elementToReference} referenceToElement={referenceToElement} attachToRequest={(requestId, files, links) => run(() => attachToRequest(requestId, files, links))} addRequest={addCanvasRequest} updateRequest={updateCanvasRequest} deleteBoundary={deleteBoundary} addPointer={addCanvasPointer} updatePointer={updateCanvasPointer} pointerStates={pointerStateMap} tool={canvasTool} insertKind={insertKind} onTool={next => { setCanvasTool(next); if (next !== 'insert') setInsertKind(null); }} boundaryKind={boundaryKind} requestShape={requestToolShape} onRequestShape={setRequestToolShape} onZoom={reportCanvasZoom} onLayout={setCanvasBoundaries}
        selectElement={selectCanvasElement} removeElements={(items, ask) => removeCanvasSelection([], [], [], items, ask)} insertElements={insertCanvasElements} updateElements={updateCanvasElements} moveElements={moveCanvasElements}
        groupElements={groupCanvasElements} ungroupElements={ungroupCanvasElements}
        linkElements={linkCanvasElements} unlinkElements={unlinkCanvasElements} pasteLinked={pasteLinkedInstances}
        play={play} onPlayElement={playElement} onPlayExit={() => setPlay(idlePlay)}
        preview={preview} previewClose={closePreview} previewRefresh={refreshPreview} />
      {showGenerationStatus ? <GenerationFeedback controller={generation} project={project} openSettings={() => setShowAiSettings(true)} openImport={openImportDialog} openBrief={() => setShowNotes('decisions')} changeRequest={editGenerationRequest} requestEditorOpen={requestEditorIsOpen} details={showsOpener({ available: true, open: showLog }) ? openGeneration : undefined} /> : draftCount > 0 && showsOpener({ available: true, open: showLog }) ? <Notice priority="result" message={`${draftCount} ${draftCount === 1 ? 'draft' : 'drafts'}`} action={{ label: 'Details', onClick: openGeneration }} /> : null}
      </div>
      {showLog && project && <GenerationLog key={`${project.id}:${logFocus}`} controller={generation} resultApplied={false} requestEditorOpen={requestEditorIsOpen} job={generation.job} operation={generation.operation} error={generation.error} project={project} settings={aiSettings} openSettings={() => setShowAiSettings(true)} openImport={openImportDialog} openBrief={() => setShowNotes('decisions')} manageUsage={manageChatgptUsage} editRequest={editGenerationRequest} reply={replyToGeneration} close={() => setShowLog(false)} />}
      <SurfaceHost place="panel" />
      </main>
      </div>
      <EditorDock state={panels} actions={panelActions}
        exportPanel={showExport && project
          ? <ExportPanel project={project} destination={exportDestination} busy={pending > 0} job={exportJob} sending={exportSending} error={exportError}
              chooseDestination={() => run(chooseExportDestination)} runExport={() => runExport()} sendExport={() => run(sendExport)}
              cancelExport={() => run(cancelExport)} retryExport={() => run(sendExport)}
              openFolder={openExportFolder} openInBrowser={() => run(openInBrowser)} previewExport={pageId => runExport(pageId)}
              close={() => setShowExport(false)} />
          : undefined}
        history={showHistory && project
          ? <HistoryPanel projectId={project.id} refreshKey={`${project.id}:${view?.dirty ? 'dirty' : 'saved'}:${view?.history?.version ?? 0}:${historyEpoch}`}
              apply={accept} notice={setNotice} error={setError} />
          : undefined}
        properties={<>
          {untitledSave && view?.untitled && <UntitledSaveForm key={project.id} project={project} location={projectLocation} busy={pending > 0}
            cancel={() => closeUntitledSave(null)}
            chooseLocation={name => native<string | null>('pick_project_save_path', { name })}
            save={saveUntitledProject} />}
          {selectedPointer
          ? <PointerProperties pointer={selectedPointer} project={project} state={selectedPointerState} busy={pending > 0} rewriting={false}
              aiConnected={aiConnected} connectAi={() => setShowAiSettings(true)}
              change={comment => updateCanvasPointer(selectedPointer.id, { comment })}
              keepComment={() => ["codeChanged", "needsExporting"].includes(selectedPointerState?.state ?? '') ? forgetExportPointer(selectedPointer.id) : keepPointerComment(selectedPointer.id)}
              keepCode={() => run(() => keepPointerCode(selectedPointer.id))}
              keepThis={() => selectedPointerOther && keepPointerThis(selectedPointer.id, selectedPointerOther.id)}
              merge={() => selectedPointerOther && mergePointerClash(selectedPointer, selectedPointerOther)}
              reattach={() => canvasRef.current?.armPointer(selectedPointer.id)} remove={() => removeCanvasSelection([], [selectedPointer.id])} />
          : selectedReferenceItem
          ? <ReferenceProperties item={selectedReferenceItem} busy={pending > 0} update={patch => updateCanvasReference(selectedReferenceItem.id, patch)} remove={() => removeCanvasSelection([selectedReferenceItem.id])} />
          : selectedRequest
          ? <RequestProperties request={selectedRequest} project={project} busy={pending > 0} update={updateCanvasRequest} remove={() => removeCanvasSelection([], [], [selectedRequest.id])} attach={attachReferenceContext} />
          : selectedElements.length && selectedElementPageId
            ? <ElementProperties project={project} page={project.pages.find(page => page.id === selectedElementPageId)!} pageId={selectedElementPageId} sizeId={selectedElementTargets[0].sizeId} elements={selectedElements} busy={pending > 0}
                update={patches => void updateCanvasElements(selectedElementPageId, patches)}
                rename={name => renameCanvasElement(selectedElementPageId, selectedElements[0].id, name)}
                link={() => canvasRef.current?.linkSelection()} unlink={() => canvasRef.current?.unlinkSelection()}
                canLink={selectedElementTargets.length >= 2}
                canUnlink={selectedElementTargets.length === 1 && (!!selectedElements[0]?.link || isNestedLayer(project, selectedElements[0]?.id ?? ''))}
                request={selectedElementRequest} setRequest={data => setCanvasElementRequest(selectedElements[0].id, data)} updateContext={updateCanvasElementContext}
                generate={() => generate(selectedElements.map(element => element.id))} error={setError} />
          : selectedBoundary
            ? <BoundaryProperties page={selectedBoundary} busy={pending > 0} commit={patch => updateCanvasPage(selectedBoundary.id, patch)}
                setOutput={output => void mutate({ type: 'set_page_output', page_id: selectedBoundary.id, output })}
                preview={() => previewSelectedBoundary()}
                updateSize={(sizeId, patch) => updateCanvasPageSize(selectedBoundary.id, sizeId, patch)}
                addSize={(preset, width, height) => addCanvasPageSize(selectedBoundary.id, preset, width, height)}
                removeSize={size => {
                  setConfirmation({ title: `Remove the ${size.label} size?`, message: 'Its layers stay in the other sizes; their overrides for this size are removed.', action: async () => { removeCanvasPageSize(selectedBoundary.id, size.id); } });
                }}
                remove={() => deleteBoundary(selectedBoundary.id, canvasFocusSize)} />
            : <ProjectProperties project={project} busy={pending > 0} rename={renameProject} setGrid={showDotGrid} setGridSettings={changeGridFlags}
                save={view?.untitled ? () => run(() => save()) : undefined} />}
        </>}
        layers={<CanvasLayers project={project} boundaries={canvasBoundaries} selection={canvasSelection} focused={canvasFocus} selectBoundary={selectCanvasBoundary} selectReference={selectCanvasReference} selectPointer={selectCanvasPointer}
          selectRequest={id => selectCanvasRequest(id)} selectElement={selectCanvasElement} updateElements={(pageId, sizeId, patches) => void updateCanvasElements(pageId, patches.map(patch => ({ ...patch, size_id: sizeId })))}
          moveElement={moveCanvasElementTo} removeLinked={removeLinkedDefinition}
          removeElements={ids => {
            const elementIds = ids.flatMap(id => { const page = project.pages.find(item => !!findElementIn(item.elements, id)); return page ? [{ pageId: page.id, elementId: id }] : []; });
            removeCanvasSelection([], [], [], elementIds);
          }}
          deleteBoundary={deleteBoundary} removeRequest={id => removeCanvasSelection([], [], [id])}
          removeReference={id => removeCanvasSelection([id])} removePointer={id => removeCanvasSelection([], [id])} />} />
      </div>
      <footer className="status-bar"><span className="status-selection" title={view.path ?? undefined}>{canvasSelection.length > 1 ? `${canvasSelection.length} selected` : ''}</span><div className="canvas-status-actions">{playIsActive(play) && <StatusChip tone="info" role="status" className="play-chip" onClick={() => setPlay(idlePlay)} ariaLabel="Stop Play">Playing · Esc to stop</StatusChip>}{clashPointers.length > 0 && <StatusChip tone="warning" className="clash-chip" onClick={selectNextPointerClash} role="status" ariaLabel="Select the next pointer clash">{clashPointers.length} clash{clashPointers.length === 1 ? '' : 'es'}</StatusChip>}{showsOpener({ available: true, open: showGridSettings }) && <button onClick={() => setShowGridSettings(true)} aria-label="Grid and rulers">{project.gridSettings.spacing}{project.gridSettings.unit === 'percent' ? '%' : ' px'} grid{project.gridSettings.snap ? ' · Snap' : ''}</button>}<StatusChip tone={pending > 0 ? 'neutral' : view.dirty ? 'warning' : 'success'} role="status" className="save-state">{pending > 0 ? 'Saving locally…' : view.dirty ? 'Unsaved' : 'Saved'}</StatusChip></div></footer>
    </>}
    {showShortcuts && <WorkspaceSurface place={onStartPage ? 'start' : 'panel'} title="Keyboard shortcuts" close={() => setShowShortcuts(false)}><div className="dialog-body shortcut-help">{SHORTCUT_GROUPS.map(group => <section key={group.group}><h3>{group.group}</h3><dl>{group.items.map(({ label, keys }) => <div key={label}><dt>{label}</dt><dd>{keys}</dd></div>)}</dl></section>)}<p className="field-help">Hold Space while dragging to pan. Alt+drag duplicates the selected layers. Escape stops Play, closes a preview, or cancels the current gesture.</p></div></WorkspaceSurface>}
    {recovery && <WorkspaceSurface place="banner" title="Unsaved changes" close={recovery.dismiss}><RecoveryChoice {...recovery} /></WorkspaceSurface>}
    {showProjectLocation && <WorkspaceSurface place={onStartPage ? 'start' : 'panel'} title="Project location" close={() => setShowProjectLocation(false)}><div className="dialog-body"><ProjectLocationSettings location={projectLocation} change={changeProjectLocation} /></div></WorkspaceSurface>}
    {showRecovery && view && <WorkspaceSurface place={onStartPage ? 'start' : 'panel'} title={project ? 'Recover unsaved projects' : 'Unsaved work'} close={() => setShowRecovery(false)}>
      <RecoveryList items={view.recoveries} busy={pending > 0} recover={id => { void recover(id).catch(() => {}); }} dismiss={dismissRecoveries} /></WorkspaceSurface>}
    {showImport && <WorkspaceSurface place={onStartPage ? 'start' : 'panel'} title="Import" close={() => setShowImport(false)}><ImportContext key={`${project?.id ?? 'new'}:${importSource?.id ?? ''}`} initialSource={importSource} busy={pending > 0} settings={aiSettings} openSettings={() => setShowAiSettings(true)} importInto={importContext} importWebsite={startWebsiteImport} close={() => setShowImport(false)} /></WorkspaceSurface>}
    {showWebsiteImport && <WorkspaceSurface place={onStartPage ? 'start' : 'panel'} title="Import website" close={() => setShowWebsiteImport(false)}><WebsiteImport busy={pending > 0} onImported={result => { accept(result.workspace); setCanvasSelection([]); setCanvasFocus(null); setShowWebsiteImport(false); setNotice(`Imported ${result.pages} ${result.pages === 1 ? 'page' : 'pages'} with ${result.layers} ${result.layers === 1 ? 'layer' : 'layers'}${result.warnings.length ? `; ${result.warnings.length} item${result.warnings.length === 1 ? '' : 's'} left out` : ''}.`); requestAnimationFrame(() => canvasRef.current?.fit()); }} close={() => setShowWebsiteImport(false)} /></WorkspaceSurface>}
    {showStyleGuide && project && <WorkspaceSurface place={onStartPage ? 'start' : 'panel'} title="Style guide" close={() => setShowStyleGuide(false)}><StyleGuide project={project} apply={command => mutate(command).then(() => undefined)} openBrief={() => { setShowStyleGuide(false); setShowNotes('decisions'); }} /></WorkspaceSurface>}
    {showNotes && project && <WorkspaceSurface place={onStartPage ? 'start' : 'panel'} title="Project brief" close={() => setShowNotes(null)}><Notes project={project} initialSection={showNotes} reviewGeneration={project.latestGeneration?.jobId === generation.job?.jobId ? openGeneration : undefined} apply={command => mutate(command).then(() => undefined)} changeBrief={changeBrief} /></WorkspaceSurface>}
    {showGridSettings && project && <WorkspaceSurface place={onStartPage ? 'start' : 'panel'} title="Grid and rulers" close={() => setShowGridSettings(false)}><GridForm rulerUnit={rulerUnit} changeRulerUnit={setRulerUnit} settings={project.gridSettings} showGrid={project.showGrid} busy={pending > 0} submit={async (settings, visible) => {
      if (visible !== project.showGrid) await mutate({ type: 'set_grid', enabled: visible });
      await mutate({ type: 'set_grid_settings', settings }); setShowGridSettings(false);
    }} /></WorkspaceSurface>}
    {showRename && project && <WorkspaceSurface place={onStartPage ? 'start' : 'panel'} title="Rename project" close={() => setShowRename(false)}><RenameProject name={project.name} submit={async name => { await mutate({ type: 'rename_project', name }); setShowRename(false); }} /></WorkspaceSurface>}
    {saveConflict && <SaveConflictDialog conflict={saveConflict} choose={chooseSaveConflict} />}
    {replacement && <WorkspaceSurface place="banner" title="Save your changes?" close={() => setReplacement(null)}><div className="dialog-body"><p>Save “{project?.name}” before you {replacement.label}?</p><p className="muted">Discard continues without saving these changes to the project file.</p><div className="form-actions">
      <button className="primary" disabled={pending > 0} onClick={() => { void (async () => { const saved = await save(); if (saved && !saved.dirty) { const run = replacement.run; setReplacement(null); await run(false); } })().catch(() => {}); }}>Save and continue</button>
      <button className="danger" disabled={pending > 0} onClick={() => { const run = replacement.run; setReplacement(null); void run(true).catch(() => {}); }}>Discard and continue</button><button onClick={() => setReplacement(null)}>Keep editing</button>
    </div></div></WorkspaceSurface>}
    {confirmation && <WorkspaceSurface place="banner" title={confirmation.title} showHeader={false} close={() => setConfirmation(null)}><div className="inline-notice"><span>{confirmation.message}</span><button className={confirmation.confirmLabel ? undefined : 'danger'} disabled={pending > 0} onClick={() => { if (confirmation.skippable && skipNextAsk) setAskBeforeDelete(false); void confirmation.action().then(() => setConfirmation(null)).catch(() => {}); }}>{confirmation.confirmLabel ?? 'Delete'}</button><button onClick={() => setConfirmation(null)}>Cancel</button>{confirmation.skippable && <details><summary>Options</summary><label className="check-label"><input type="checkbox" checked={skipNextAsk} onChange={event => setSkipNextAsk(event.target.checked)} />Don’t ask again</label></details>}</div></WorkspaceSurface>}
    {showAiSettings && <WorkspaceSurface place={onStartPage ? 'start' : 'panel'} title="AI settings" close={() => setShowAiSettings(false)}><AiSettings current={aiSettings} refresh={refreshAiSettings} update={updateAiSettings} manageUsage={manageChatgptUsage} pollingError={aiPollingError} /></WorkspaceSurface>}
  </div>;
}

function editedAgo(editedAtMs: number) {
  const minutes = Math.max(0, Math.floor((Date.now() - editedAtMs) / 60_000));
  return !Number.isFinite(minutes) ? 'Unsaved draft' : minutes < 1 ? 'Edited just now' : minutes < 60 ? `Edited ${minutes} min ago` : minutes < 1440 ? `Edited ${Math.floor(minutes / 60)} hr ago` : `Edited ${Math.floor(minutes / 1440)} days ago`;
}

function RecoveryList({ items, busy, recover, dismiss }: { items: RecoveryInfo[]; busy: boolean; recover: (id: string) => void; dismiss: (ids: string[]) => Promise<void> }) {
  const [confirmIds, setConfirmIds] = useState<string[] | null>(null);
  const submit = (ids: string[]) => { void dismiss(ids).then(() => setConfirmIds(null)).catch(() => {}); };
  const requestDismiss = (selected: RecoveryInfo[]) => {
    const ids = selected.map(item => item.projectId);
    if (selected.some(item => !item.savedPath)) setConfirmIds(ids);
    else submit(ids);
  };
  const confirmation = <div className="recovery-confirmation"><p>Never saved — dismiss anyway?</p><div className="form-actions">
    <button className="danger" disabled={busy} onClick={() => confirmIds && submit(confirmIds)}>Dismiss</button>
    <button disabled={busy} onClick={() => setConfirmIds(null)}>Cancel</button>
  </div></div>;
  return <div className="recovery-list">{items.map(item => <section className="recovery-row" key={item.projectId} aria-label={`Recover ${item.name}`}>
    <div className="recovery-details"><strong>{item.name}</strong>
    <p className="muted" title={item.savedPath ?? undefined}>{editedAgo(item.editedAtMs)}{item.savedPath ? ` · ${item.savedPath.split(/[\\/]/).at(-1)}` : ' · Never saved'}</p></div>
    {confirmIds?.length === 1 && confirmIds[0] === item.projectId ? confirmation : <div className="form-actions">
      <button className="primary" disabled={busy} onClick={() => recover(item.projectId)}>Recover</button>
      <button disabled={busy} onClick={() => requestDismiss([item])}>Dismiss</button>
    </div>}
  </section>)}
    {items.length >= 2 && (confirmIds && confirmIds.length > 1 ? confirmation : <div className="form-actions"><button disabled={busy} onClick={() => requestDismiss(items)}>Dismiss all</button></div>)}
  </div>;
}

function RenameProject({ name, submit }: { name: string; submit: (value: string) => Promise<void> }) {
  const [value, setValue] = useState(name);
  const [busy, setBusy] = useState(false);
  return <form className="dialog-body" onSubmit={event => { event.preventDefault(); setBusy(true); void submit(value.trim()).catch(() => {}).finally(() => setBusy(false)); }}><label>Project name<input autoFocus required maxLength={120} value={value} onChange={event => setValue(event.target.value)} /></label><div className="form-actions"><button className="primary" disabled={busy || !value.trim()}>Rename project</button></div></form>;
}

function canvasPageName(pages: Page[]) {
  const names = new Set(pages.map(page => page.name.trim().toLowerCase()));
  let number = 1;
  while (names.has(`page ${number}`)) number++;
  return `Page ${number}`;
}

function suggestedPagePath(name: string, paths: string[]) {
  let slug = name.trim().toLowerCase().replace(/[^\p{L}\p{N}]+/gu, '-').replace(/^-|-$/g, '') || 'page';
  if (/^(assets|src|node_modules|con|prn|aux|nul|com[1-9]|lpt[1-9])$/.test(slug)) slug += '-page';
  const base = `/${slug}`;
  let path = base; let suffix = 2;
  while (paths.some(existing => existing.toLowerCase() === path.toLowerCase())) path = `${base}-${suffix++}`;
  return path;
}

function GridForm({ settings, showGrid, busy, submit, rulerUnit, changeRulerUnit }: { rulerUnit: 'pixels' | 'percent'; changeRulerUnit: (value: 'pixels' | 'percent') => void; settings: GridSettings; showGrid: boolean; busy: boolean; submit: (settings: GridSettings, visible: boolean) => Promise<void> }) {
  const [draft, setDraft] = useState(settings);
  const [visible, setVisible] = useState(showGrid);
  const [saving, setSaving] = useState(false);
  const min = draft.unit === 'percent' ? 0.25 : 1;
  const max = draft.unit === 'percent' ? 100 : 512;
  const valid = Number.isFinite(draft.spacing) && draft.spacing >= min && draft.spacing <= max;
  return <form className="dialog-body grid-form" onSubmit={event => {
    event.preventDefault(); if (!valid) return; setSaving(true); void submit(draft, visible).catch(() => {}).finally(() => setSaving(false));
  }}>
    <label className="check-label"><input type="checkbox" checked={visible} onChange={event => setVisible(event.target.checked)} />Show grid</label>
    <div className="field-pair"><label>Grid spacing<input autoFocus type="number" required min={min} max={max} step="any" value={draft.spacing} onChange={event => setDraft({ ...draft, spacing: Number(event.target.value) })} /></label>
      <label>Unit<select value={draft.unit} onChange={event => {
        const unit = event.target.value as GridSettings['unit'];
        setDraft({ ...draft, unit, spacing: unit === 'percent' ? Math.max(0.25, Math.min(100, draft.spacing)) : Math.max(1, Math.min(512, draft.spacing)) });
      }}><option value="pixels">Pixels</option><option value="percent">Percent</option></select></label></div>
    <label className="check-label"><input type="checkbox" checked={draft.snap} onChange={event => setDraft({ ...draft, snap: event.target.checked })} />Snap requests to the grid</label>
    <label className="check-label"><input type="checkbox" checked={draft.rulers} onChange={event => setDraft({ ...draft, rulers: event.target.checked })} />Show rulers</label>
    <label>Ruler units<select value={rulerUnit} onChange={event => changeRulerUnit(event.target.value as 'pixels' | 'percent')}><option value="pixels">Pixels</option><option value="percent">Percent</option></select></label>
    <div className="form-actions"><button className="primary" disabled={busy || saving || !valid}>Save grid settings</button></div>
  </form>;
}
