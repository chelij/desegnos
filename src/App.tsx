import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent, type CSSProperties, type PointerEvent as ReactPointerEvent } from 'react';
import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { confirm } from '@tauri-apps/plugin-dialog';
import { native, edit } from './api';
import { Canvas, type CanvasHandle, type RequestPatch } from './Canvas';
import { CanvasTools, type CanvasTool } from './CanvasTools';
import { Modal } from './Modal';
import { SaveConflictDialog, type SaveConflict, type SaveOutcome, type SaveChoice } from './SaveConflict';
import { regionTargets, sameTargets } from './elementTargets';
import brandLight from './assets/brand/desegnos-wordmark-light.svg';
import brandDark from './assets/brand/desegnos-wordmark-dark.svg';
import { NewProject } from './NewProject';
import { Notes, type DossierSection } from './Notes';
import { MenuButton, type MenuItem } from './Menus';
import './shell.css';
import { editorShortcut, SHORTCUT_HELP } from './shortcuts';
import { steppedZoom } from './canvasGeometry';
import { AiSettings } from './AiSettings';
import { GenerationDialog, GenerationFeedback } from './Generation';
import { PagePreview } from './Preview';
import { CodePane, type CodeView, type SourceMapView, type CodePaneHandle } from './CodePane';
import { FirstRunTips } from './Onboarding';
import { checkProject, PageCheckPanel, type CheckItem } from './PageCheck';
import { useHistory } from './useHistory';
import { activeGeneration, useGeneration } from './useGeneration';
import type { AiSettingsView, Appearance, Brief, Command, ElementStyleEdit, LayerTarget, LayerDirection, LayerPasteView, ElementBounds, ElementDetails, ElementEdit, ElementEditView, ElementSelection, ElementTarget, GenerationJobView, GenerationMode, GridSettings, NewProjectIdentity, Page, Project, Rect, RecoveryInfo, Region, Viewport, WorkspaceView, RecentProjectsView, RecentProject, WebsiteExport, SiteContent } from './types';

type Theme = 'system' | 'light' | 'dark';
type Replacement = { label: string; run: (discard: boolean) => Promise<void> };
type Confirmation = { title: string; message: string; action: () => Promise<void> };

function errorMessage(error: unknown) { return error instanceof Error ? error.message : String(error); }
// Open each page fitted to the window: web pages by width, mobile pages by height.
function initialZoom(width: number, height = 900) { const fit = width > 600 ? (window.innerWidth - 96) / width : (window.innerHeight - 240) / height; return Math.max(0.1, Math.min(1, fit)); }
function readRulerUnit(): 'pixels' | 'percent' { try { return localStorage.getItem('desegnos.ruler-unit') === 'percent' ? 'percent' : 'pixels'; } catch { return 'pixels'; } }
function readCodeWidth() { try { return Math.min(0.75, Math.max(0.2, Number(localStorage.getItem('desegnos.code-width')) || 0.42)); } catch { return 0.42; } }
function readCodeView(projectId: string): CodeView { try { const value = localStorage.getItem(`desegnos.code-view.${projectId}`); return value === 'split' || value === 'code' ? value : 'design'; } catch { return 'design'; } }
function readTheme(): Theme {
  try { const saved = localStorage.getItem('desegni.editor-theme'); return saved === 'light' || saved === 'dark' ? saved : 'system'; }
  catch { return 'system'; }
}

export default function App() {
  const [view, setView] = useState<WorkspaceView | null>(null);
  const current = useRef<WorkspaceView | null>(null);
  const canvas = useRef<CanvasHandle>(null);
  const code = useRef<CodePaneHandle>(null);
  const tail = useRef<Promise<void>>(Promise.resolve());
  const [pending, setPending] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [dismissedMigration, setDismissedMigration] = useState('');
  const [theme, setTheme] = useState<Theme>(readTheme);
  const [pageId, setPageId] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [focusPrompt, setFocusPrompt] = useState(false);
  const [zoom, setZoom] = useState(0.75);
  const [tool, setTool] = useState<CanvasTool>('select');
  const [rulerUnit, setRulerUnit] = useState<'pixels' | 'percent'>(readRulerUnit);
  const [recentProjects, setRecentProjects] = useState<RecentProject[]>([]);
  const [exported, setExported] = useState<WebsiteExport | null>(null);
  const [exporting, setExporting] = useState(false);
  const [resultReview, setResultReview] = useState<{ jobId: string; revision: number } | null>(null);
  const [highlightResult, setHighlightResult] = useState(false);
  const [showLayers, setShowLayers] = useState(false);
  const [activeLayer, setActiveLayer] = useState<ElementTarget | null>(null);
  const [showNew, setShowNew] = useState(false);
  const [showNotes, setShowNotes] = useState<DossierSection | null>(null);
  const [showGridSettings, setShowGridSettings] = useState(false);
  const [codeView, setCodeView] = useState<CodeView>('design');
  const [showCheck, setShowCheck] = useState(false);
  const [multiCount, setMultiCount] = useState(0);
  const history = useHistory();
  const [sourceMap, setSourceMap] = useState<SourceMapView | null>(null);
  const [codeWidth, setCodeWidth] = useState(readCodeWidth);
  const [showShortcuts, setShowShortcuts] = useState(false);
  const [canvasView, setCanvasView] = useState<{ serial: number; kind: 'guide' | 'selection' | 'fit' }>({ serial: 0, kind: 'guide' });
  const [showRecovery, setShowRecovery] = useState(false);
  const [pageDialog, setPageDialog] = useState<'new' | Page | null>(null);
  const [showRename, setShowRename] = useState(false);
  const [showAiSettings, setShowAiSettings] = useState(false);
  const [aiSettings, setAiSettings] = useState<AiSettingsView | null>(null);
  const aiCurrent = useRef<AiSettingsView | null>(null);
  const aiEpoch = useRef(0);
  const aiRead = useRef(0);
  const aiTail = useRef<Promise<void>>(Promise.resolve());
  const [aiPollingError, setAiPollingError] = useState('');
  const [showPreview, setShowPreview] = useState(false);
  const [replacement, setReplacement] = useState<Replacement | null>(null);
  const [confirmation, setConfirmation] = useState<Confirmation | null>(null);
  const closing = useRef(false);
  const [saveConflict, setSaveConflict] = useState<SaveConflict | null>(null);
  const saveChoice = useRef<((choice: SaveChoice) => void) | null>(null);
  const saveFlight = useRef<Promise<WorkspaceView | null> | null>(null);
  const generationRef = useRef<GenerationJobView | null>(null);

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
  useEffect(() => { try { localStorage.setItem('desegnos.ruler-unit', rulerUnit); } catch {} }, [rulerUnit]);

  const accept = useCallback((next: WorkspaceView) => { current.current = next; setView(next); }, []);
  const queue = useCallback(<T,>(operation: (workspace: WorkspaceView | null) => Promise<T>, reportErrors = true): Promise<T> => {
    setPending(count => count + 1);
    const result = tail.current.then(() => operation(current.current));
    tail.current = result.then(() => undefined, () => undefined);
    return result.catch(error => { if (reportErrors) setError(errorMessage(error)); throw error; }).finally(() => setPending(count => count - 1));
  }, []);
  const mutate = useCallback((command: Command | ((project: Project) => Command | null)) => queue(async workspace => {
    const project = workspace?.project;
    if (!project) throw new Error('Create or open a project first.');
    const resolved = typeof command === 'function' ? command(project) : command;
    if (!resolved) return workspace!;
    const next = await edit(project.revision, resolved);
    accept(next); return next;
  }), [queue, accept]);
  const load = useCallback(async () => {
    setLoading(true); setError('');
    try { const loaded = await native<WorkspaceView>('get_workspace'); accept(loaded); if (loaded.project) setZoom(initialZoom(loaded.project.viewport.width, loaded.project.viewport.height)); }
    catch (error) { setError(errorMessage(error)); }
    finally { setLoading(false); }
  }, [accept]);
  useEffect(() => { void load(); }, [load]);
  useEffect(() => { history.observe(view); }, [view, history.observe]);
  useEffect(() => {
    const media = window.matchMedia('(prefers-color-scheme: dark)');
    const update = () => { document.documentElement.dataset.theme = theme === 'system' ? (media.matches ? 'dark' : 'light') : theme; };
    update(); media.addEventListener('change', update);
    try { localStorage.setItem('desegni.editor-theme', theme); } catch {}
    return () => media.removeEventListener('change', update);
  }, [theme]);
  useEffect(() => {
    const project = view?.project;
    if (!project) { setPageId(null); setSelected(null); return; }
    const page = project.pages.find(page => page.id === pageId);
    if (!page) { setPageId(project.homePageId); setSelected(null); }
    else if (!page.regions.some(region => region.id === selected)) setSelected(null);
  }, [view, pageId, selected]);

  const settle = useCallback(async () => {
    await tail.current;
  }, []);
  const synchronize = useCallback(async (requireBounds = false) => {
    try { await code.current?.synchronize(); await canvas.current?.synchronize(requireBounds); }
    catch (error) { setError(errorMessage(error)); throw error; }
  }, []);
  useEffect(() => {
    if (!isTauri()) return;
    const heartbeat = () => { void native('editor_heartbeat').catch(() => {}); };
    heartbeat();
    const timer = setInterval(heartbeat, 2000);
    return () => clearInterval(timer);
  }, []);
  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void getCurrentWindow().onCloseRequested(async event => {
      event.preventDefault();
      if (closing.current) return;
      closing.current = true;
      try {
        await synchronize();
        await settle();
        const identity = await native<import('./types').IdentityAnalysis | null>('get_identity_analysis').catch(() => null);
        const generating = generationRef.current?.status === 'generating' || identity?.status === 'analyzing';
        const plan = identity?.status === 'analyzing' ? (identity.authentication ?? aiCurrent.current?.authentication) === 'chatgpt' : generationRef.current?.authentication === 'chatgpt';
        if ((current.current?.dirty || generating) && !await confirm(`${generating ? `Generation is still running. Closing may interrupt the request, and ${plan ? 'submitted work may still use your ChatGPT plan or credits' : 'OpenAI may still charge for submitted work'}. Its outcome will be shown as uncertain when you return. ` : ''}${current.current?.dirty ? 'This project has changes that have not been saved to its file. ' : ''}Close Desegnos?`, {
          title: generating ? 'Generation is running' : 'Unsaved project changes', kind: 'warning', okLabel: 'Close Desegnos', cancelLabel: 'Keep editing',
        })) return;
        await getCurrentWindow().destroy();
      } catch (error) { setError(errorMessage(error)); }
      finally { closing.current = false; }
    }).then(stop => { if (disposed) stop(); else unlisten = stop; }, error => setError(errorMessage(error)));
    return () => { disposed = true; unlisten?.(); };
  }, [settle, synchronize]);

  function chooseSaveConflict(choice: SaveChoice) { const resolve = saveChoice.current; saveChoice.current = null; setSaveConflict(null); resolve?.(choice); }
  function save(saveAs = false): Promise<WorkspaceView | null> {
    if (saveFlight.current) return saveFlight.current;
    const operation = async () => {
      await synchronize();
      await settle();
      let outcome = await queue(() => native<SaveOutcome>('save_project_checked', { saveAs }));
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
    await synchronize();
    await settle();
    if (current.current?.dirty) setReplacement({ label, run });
    else await run(false);
  }
  async function open() {
    await replace('open another project', async discard => {
      await queue(async () => {
        const opened = await native<WorkspaceView | null>('open_project', { discard });
        if (opened) { accept(opened); setSelected(null); setNotice('Project opened.'); setShowNew(false); void refreshRecent().catch(() => {}); if (opened.project) setZoom(initialZoom(opened.project.viewport.width, opened.project.viewport.height)); }
      });
    });
  }
  async function openRecent(path: string) {
    await replace('open another project', async discard => {
      await queue(async () => {
        const opened = await native<WorkspaceView>('open_recent_project', { path, discard });
        accept(opened); setSelected(null); setShowNew(false); setNotice('Project opened.');
        if (opened.project) setZoom(initialZoom(opened.project.viewport.width, opened.project.viewport.height));
        void refreshRecent().catch(() => {});
      });
    });
  }
  async function exportWebsite() {
    await synchronize(); await settle();
    setExporting(true); setError('');
    try {
      const project = current.current?.project;
      if (!project) return;
      const result = await native<WebsiteExport | null>('export_website', { projectId: project.id, revision: project.revision });
      if (result) setExported(result);
    } catch (error) { setError(errorMessage(error)); }
    finally { setExporting(false); }
  }
  async function create(name: string, viewport: Viewport, identity?: NewProjectIdentity) {
    await replace('create a new project', async discard => {
      await queue(async () => {
        accept(await native<WorkspaceView>('create_project', { name, viewport, appearance: 'light', discard, ...identity }));
        setSelected(null); setShowNew(false); setNotice(identity ? 'Blank website created with your brand.' : 'Blank project created.');
        setZoom(initialZoom(viewport.width, viewport.height));
      });
    });
  }
  async function recover(projectId: string) {
    await replace('recover another project', async discard => {
      await queue(async () => {
        const recovered = await native<WorkspaceView>('recover_project', { projectId, discard });
        accept(recovered);
        if (recovered.project) setZoom(initialZoom(recovered.project.viewport.width, recovered.project.viewport.height));
        setSelected(null); setShowRecovery(false);
        setNotice('Project recovered. Save it to keep a project file.');
      });
    });
  }
  async function travel(command: 'undo_project' | 'redo_project') {
    await synchronize();
    await settle();
    await queue(async () => { accept(await native<WorkspaceView>(command)); setNotice(command === 'undo_project' ? 'Change undone.' : 'Change redone.'); });
  }
  // Hold the mutation queue while travelling so edits cannot interleave with the jump.
  async function jumpTo(position: number) {
    const sessionId = current.current?.history?.sessionId;
    await synchronize(); await settle();
    await queue(async workspace => {
      let next = workspace;
      if (!next?.history || next.history.sessionId !== sessionId || position < next.history.start || position > next.history.end) throw new Error('That change is no longer available in this history.');
      while (next.history!.position !== position) {
        next = await native<WorkspaceView>(next.history!.position > position ? 'undo_project' : 'redo_project');
        accept(next);
      }
      setNotice('Returned to the selected change.');
    });
  }
  const getNotes = useCallback(() => queue(() => native<string>('get_project_notes')), [queue]);
  const changeBrief = useCallback((patch: Partial<Brief>) => mutate(project => ({ type: 'set_brief', brief: { ...project.brief, ...patch } }))
    .then(workspace => workspace.project!.brief), [mutate]);

  useEffect(() => {
    const listener = (event: KeyboardEvent) => {
      const target = event.target;
      if (event.defaultPrevented || target instanceof HTMLElement && target.closest('.cm-editor')) return;
      const editingText = target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || (target instanceof HTMLElement && target.isContentEditable);
      // WebKitGTK lacks these text key bindings with the pinned muda 0.20 stack.
      // Remove when the native stack supplies them and the trusted-input test passes.
      // https://github.com/tauri-apps/tauri/issues/16299
      if (editingText && navigator.userAgent.includes('Linux')) {
        const textAction = editorShortcut(event, false);
        if (textAction === 'undo' || textAction === 'redo') {
          event.preventDefault(); document.execCommand(textAction); return;
        }
      }
      if (document.querySelector('dialog[open]')) return;
      const shortcut = editorShortcut(event, editingText);
      if (!shortcut) return;
      if (event.repeat && ['new', 'open', 'saveAs', 'help', 'remove'].includes(shortcut)) { event.preventDefault(); return; }
      const project = current.current?.project;
      let action: (() => Promise<unknown>) | undefined;
      if (shortcut === 'help') { event.preventDefault(); setShowShortcuts(true); return; }
      if (shortcut === 'new') { event.preventDefault(); setShowNew(true); return; }
      if (shortcut === 'open') action = open;
      else if (!project) return;
      else if (shortcut === 'save' || shortcut === 'saveAs') action = () => save(shortcut === 'saveAs');
      else if (shortcut === 'undo' || shortcut === 'redo') {
        event.preventDefault();
        if (shortcut === 'undo' ? current.current?.canUndo : current.current?.canRedo) action = () => travel(shortcut === 'undo' ? 'undo_project' : 'redo_project');
      } else if (shortcut === 'zoomIn' || shortcut === 'zoomOut') { event.preventDefault(); setZoom(value => steppedZoom(value, shortcut === 'zoomIn' ? 1 : -1)); return; }
      else if (shortcut === 'actualSize' || shortcut === 'doubleSize') { event.preventDefault(); setZoom(shortcut === 'actualSize' ? 1 : 2); return; }
      else if (shortcut === 'selectTool' || shortcut === 'requestTool' || shortcut === 'panTool') { event.preventDefault(); setTool(shortcut === 'selectTool' ? 'select' : shortcut === 'requestTool' ? 'request' : 'pan'); return; }
      else if (shortcut === 'cutLayer') { if (activeLayer || selected) { event.preventDefault(); void canvas.current?.cutLayer(); } return; }
      else if (shortcut === 'copyLayer') { if (activeLayer || selected) { event.preventDefault(); void canvas.current?.copyLayer(); } return; }
      else if (shortcut === 'pasteLayer') { event.preventDefault(); void canvas.current?.pasteLayer(); return; }
      else if (shortcut === 'layerForward' || shortcut === 'layerBackward' || shortcut === 'layerFront' || shortcut === 'layerBack') {
        if (activeLayer || selected) { event.preventDefault(); void canvas.current?.arrangeLayer(({ layerForward: 'forward', layerBackward: 'backward', layerFront: 'front', layerBack: 'back' } as const)[shortcut]); } return;
      }
      else if (shortcut === 'askAI') { event.preventDefault(); canvas.current?.askAI(); return; }
      else if (shortcut === 'fit') { event.preventDefault(); recenter('fit'); return; }
      else if (shortcut === 'remove' && selected) { event.preventDefault(); removeRegion(selected); return; }
      else if (shortcut === 'nextPage' || shortcut === 'previousPage') {
        event.preventDefault();
        const index = project.pages.findIndex(page => page.id === (pageId ?? project.homePageId));
        const id = project.pages[(Math.max(0, index) + (shortcut === 'nextPage' ? 1 : project.pages.length - 1)) % project.pages.length].id;
        action = async () => { await selectPage(id); requestAnimationFrame(() => document.getElementById(`page-tab-${id}`)?.focus()); };
      }
      if (action) { event.preventDefault(); void action().catch(() => {}); }
    };
    window.addEventListener('keydown', listener);
    return () => window.removeEventListener('keydown', listener);
  });

  const project = view?.project ?? null;
  const page = project?.pages.find(page => page.id === pageId) ?? project?.pages.find(page => page.id === project.homePageId) ?? null;
  const region = page?.regions.find(region => region.id === selected) ?? null;
  const requestedAreas = project?.pages.reduce((count, page) => count + page.regions.filter(region => !region.completed && region.prompt.trim()).length, 0) ?? 0;
  const checks = useMemo(() => project ? checkProject(project) : [], [project]);
  const placeholderCount = checks.filter(item => item.kind === 'placeholder').length;
  const fixCount = checks.filter(item => item.kind !== 'found').length;
  // The native source map ties code ranges to canvas elements; it is only valid for the exact HTML it was built from.
  const mapHtml = codeView !== 'design' ? page?.content?.html ?? null : null;
  useEffect(() => {
    if (!mapHtml || !project || !page) { setSourceMap(null); return; }
    let active = true; const projectId = project.id, pageId = page.id, html = mapHtml;
    const timer = setTimeout(() => { void native<{ ranges: SourceMapView['ranges'] }>('get_page_source_map', { projectId, pageId }).then(value => { if (active) setSourceMap({ html, ranges: value.ranges }); }, () => { if (active) setSourceMap(null); }); }, 150);
    return () => { active = false; clearTimeout(timer); };
  }, [mapHtml, project?.id, page?.id]);
  const activeProjectId = project?.id ?? null;
  const generationCall = useCallback(<T,>(command: string, args?: Record<string, unknown>) => Promise.resolve(command === 'prepare_generation' || command === 'apply_generation' ? synchronize(command === 'prepare_generation') : undefined).then(() => queue(async workspace => {
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
    const result = await native<T>(command, args);
    if (command === 'apply_generation') accept(result as WorkspaceView);
    return result;
  })), [queue, activeProjectId, accept, refreshAiSettings, synchronize]);
  const generationApplied = useCallback((workspace: WorkspaceView) => { setSelected(null); setFocusPrompt(false); setNotice('Website updated.'); if (workspace.project?.latestGeneration) { setResultReview({ jobId: workspace.project.latestGeneration.jobId, revision: workspace.project.revision }); setHighlightResult(true); } }, []);
  const generationRecorded = useCallback(() => queue(async () => {
    const refreshed = await native<WorkspaceView>('get_workspace');
    if (refreshed.project?.id === activeProjectId) accept(refreshed);
  }), [queue, accept, activeProjectId]);
  const generation = useGeneration(activeProjectId, generationCall, generationApplied, generationRecorded);
  generationRef.current = generation.job;
  useEffect(() => { setResultReview(null); setExported(null); setTool('select'); setCodeView(activeProjectId ? readCodeView(activeProjectId) : 'design'); }, [activeProjectId]);
  async function changeCodeView(value: CodeView) { if (value === codeView) return; await synchronize(); setCodeView(value); if (value !== 'code') requestAnimationFrame(() => recenter('fit')); try { if (activeProjectId) localStorage.setItem(`desegnos.code-view.${activeProjectId}`, value); } catch {} }
  function dragDivider(event: ReactPointerEvent<HTMLDivElement>) {
    const host = event.currentTarget.parentElement; if (!host) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    const bounds = host.getBoundingClientRect();
    const moveTo = (x: number) => { const width = Math.min(0.75, Math.max(0.2, (bounds.right - x) / bounds.width)); setCodeWidth(width); try { localStorage.setItem('desegnos.code-width', String(width)); } catch {} };
    const onMove = (next: PointerEvent) => moveTo(next.clientX);
    const stop = () => { window.removeEventListener('pointermove', onMove); window.removeEventListener('pointerup', stop); };
    window.addEventListener('pointermove', onMove); window.addEventListener('pointerup', stop);
  }
  useEffect(() => { if (resultReview && project?.revision !== resultReview.revision) setResultReview(null); }, [project?.revision, resultReview]);
  useEffect(() => { if (!highlightResult) return; const timer = setTimeout(() => setHighlightResult(false), 2000); return () => clearTimeout(timer); }, [highlightResult, resultReview?.jobId]);
  async function rejectResult(tryAgain = false) {
    const review = resultReview;
    if (!review) return;
    await synchronize(); await settle();
    const undone = await queue(async workspace => {
      setResultReview(null);
      if (workspace?.project?.revision !== review.revision || workspace.project.latestGeneration?.jobId !== review.jobId) return false;
      accept(await native<WorkspaceView>('undo_project')); setNotice('Generated changes undone.'); return true;
    });
    if (undone && tryAgain) await generation.prepare();
  }
  const working = activeGeneration(generation.job) || !!generation.operation;
  const showGenerationStatus = working || !!generation.error || !!generation.job && ['prepared', 'blocked', 'failed', 'unknown', 'cancelled'].includes(generation.job.status);
  const composeJob = generation.job && ['prepared', 'generating', 'ready', 'blocked', 'applying'].includes(generation.job.status) ? generation.job : null;
  const composeUsesChatgpt = (composeJob?.authentication ?? aiSettings?.authentication) === 'chatgpt';
  const composeAccountId = composeJob ? composeJob.chatgptAccountId : aiSettings?.chatgpt.activeAccountId;
  const composeAccount = aiSettings?.chatgpt.accounts.find(account => account.id === composeAccountId);
  const aiConnected = !!aiSettings && !aiSettings.configurationError && (composeUsesChatgpt ? !!composeAccount?.connected && composeAccount.planEnabled && !composeAccount.storageRepairRequired : aiSettings.credential.configured);
  function openGeneration() {
    generation.open();
    void generation.refresh();
    void refreshAiSettings().catch(() => {});
  }
  function closeGeneration() { generation.close(); }
  function generate() {
    void generation.prepare().catch(() => {});
  }

  function dismissEmpty(keep: string | null) {
    return mutate(project => project.pages.some(page => page.regions.some(region => !region.completed && region.id !== keep && !region.prompt.trim()))
      ? { type: 'dismiss_empty_regions', keep_region_id: keep } : null);
  }
  function selectRequest(id: string | null, cleanEmpty = true) {
    setSelected(id); setFocusPrompt(false);
    if (cleanEmpty) void dismissEmpty(id).catch(() => {});
  }
  async function selectPage(id: string, keep: string | null = null) { await synchronize(); await settle(); await dismissEmpty(keep); setPageId(id); setSelected(null); }
  async function addRegion(rect: Rect) {
    if (!page) return;
    const id = page.id;
    const next = await mutate({ type: 'add_region', page_id: id, rect });
    setSelected(next.project!.pages.find(page => page.id === id)!.regions.at(-1)!.id); setFocusPrompt(true);
  }
  async function pickElement(selections: ElementSelection[]) {
    if (!page || !selections.length) return;
    const pageId = page.id, elements = selections.map(selection => selection.element);
    // Resolve against the queued current state, so repeated clicks cannot create duplicate requests.
    const next = await mutate(project => {
      const currentPage = project.pages.find(item => item.id === pageId);
      if (!currentPage) throw new Error('This page was removed.');
      if (currentPage.regions.some(region => !region.completed && sameTargets(regionTargets(region), elements))) return null;
      return { type: 'add_element_regions', page_id: pageId, elements, bounds: selections.map(selection => ({ target: selection.element, rect: selection.rect })) };
    });
    const request = next.project!.pages.find(item => item.id === pageId)?.regions.find(region => !region.completed && sameTargets(regionTargets(region), elements));
    if (request) { await dismissEmpty(request.id); setSelected(request.id); setFocusPrompt(true); }
  }
  async function deleteElements(elements: ElementTarget[]) {
    if (!page) return;
    await mutate({ type: 'delete_elements', page_id: page.id, elements });
    setNotice('Items deleted. You can undo this.');
  }
  async function inspectElement(target: ElementTarget): Promise<ElementDetails> {
    if (!page || !project) throw new Error('Select an element first.');
    const projectId = project.id; const pageId = page.id;
    return queue(async workspace => {
      if (workspace?.project?.id !== projectId) throw new Error('Return to the project that owns this element.');
      return native<ElementDetails>('inspect_element', { projectId, pageId, target });
    }, false);
  }
  async function editElement(target: ElementTarget, change: ElementEdit): Promise<ElementTarget | null> {
    if (!page || !project) throw new Error('Select an element first.');
    const projectId = project.id; const pageId = page.id; setError('');
    return queue(async workspace => {
      if (workspace?.project?.id !== projectId) throw new Error('Return to the project that owns this element.');
      const result = await native<ElementEditView>('edit_element', { projectId, pageId, revision: workspace.project.revision, target, change });
      accept(result.workspace); setNotice('Element updated.'); return result.element;
    }, false);
  }
  async function copyLayerNative(target: LayerTarget, styles: ElementStyleEdit[]) {
    const projectId = project?.id, targetPageId = page?.id;
    await settle();
    await queue(async workspace => {
      if (!projectId || !targetPageId || workspace?.project?.id !== projectId) throw new Error('The project changed before the layer was copied.');
      await native<void>('copy_layer', { projectId, pageId: targetPageId, revision: workspace.project.revision, target, visualStyles: styles });
      setNotice('Layer copied.');
    }, false);
  }
  async function pasteLayerNative(after: ElementTarget | null): Promise<LayerPasteView> {
    const projectId = project?.id, targetPageId = page?.id;
    await settle();
    return queue(async workspace => {
      if (!projectId || !targetPageId || workspace?.project?.id !== projectId) throw new Error('The project changed before the layer was pasted.');
      const result = await native<LayerPasteView>('paste_layer', { projectId, pageId: targetPageId, revision: workspace.project.revision, after });
      accept(result.workspace); setNotice('Layer pasted.'); return result;
    }, false);
  }
  async function arrangeRequest(id: string, direction: LayerDirection) {
    if (!page) return;
    await mutate({ type: 'arrange_region', page_id: page.id, region_id: id, direction });
  }
  async function cutLayerNative(target: LayerTarget, styles: ElementStyleEdit[]) {
    const projectId = project?.id, targetPageId = page?.id;
    await settle();
    await queue(async workspace => {
      if (!projectId || !targetPageId || workspace?.project?.id !== projectId) throw new Error('The project changed before the layer was cut.');
      const next = await native<WorkspaceView>('cut_layer', { projectId, pageId: targetPageId, revision: workspace.project.revision, target, visualStyles: styles });
      accept(next); setNotice('Layer cut.');
    }, false);
  }
  async function refreshElementBounds(pageId: string, viewport: Viewport, bounds: ElementBounds[]) {
    const projectId = project?.id;
    await queue(async workspace => {
      if (!projectId || workspace?.project?.id !== projectId) throw new Error('The project changed before its element positions were recorded.');
      const next = await native<WorkspaceView>('refresh_element_target_bounds', { projectId, pageId, viewport, bounds });
      accept(next);
    }, false);
  }
  function changeRegion(targetPageId: string, regionId: string, patch: RequestPatch): Promise<Region> {
    return mutate(project => {
      const area = project.pages.find(page => page.id === targetPageId)?.regions.find(region => region.id === regionId);
      if (!area) throw new Error('This area is no longer in the project.');
      const rect = typeof patch.rect === 'function' ? patch.rect(area.rect) : { ...area.rect, ...patch.rect };
      const prompt = patch.prompt ?? area.prompt;
      if (rect.x === area.rect.x && rect.y === area.rect.y && rect.width === area.rect.width && rect.height === area.rect.height && prompt === area.prompt) return null;
      return { type: 'update_region', page_id: targetPageId, region_id: regionId, rect, prompt };
    }).then(next => next.project!.pages.find(page => page.id === targetPageId)!.regions.find(region => region.id === regionId)!);
  }
  function removeRegion(id = region?.id) {
    if (!page || !id) return;
    const page_id = page.id; const region_id = id;
    setConfirmation({ title: 'Remove request?', message: 'Remove this request? You can undo this. Remove any linked choices or exceptions from the project brief first.',
      action: async () => { await mutate({ type: 'remove_region', page_id, region_id }); setSelected(null); requestAnimationFrame(() => { if (!document.querySelector('dialog[open]')) document.getElementById('page-canvas')?.focus(); }); } });
  }
  async function removePage() {
    if (!page) return;
    await synchronize();
    const page_id = page.id;
    setConfirmation({ title: 'Remove page?', message: `Remove “${page.name}” and its ${page.regions.length} requests? You can undo this. Remove any linked choices or exceptions from the project brief first.`,
      action: async () => { await mutate({ type: 'delete_page', page_id }); setPageId(project!.homePageId); setSelected(null); } });
  }

  const run = (operation: () => Promise<unknown>) => { void operation().catch(() => {}); };
  const recenter = (kind: 'guide' | 'selection' | 'fit') => setCanvasView(value => ({ serial: value.serial + 1, kind }));
  // Page check: select the item on the canvas, switching pages first and waiting for the page to render.
  async function showCheckItem(item: CheckItem) {
    if (page?.id !== item.pageId) await selectPage(item.pageId);
    if (!item.selector || !item.tag) return null;
    for (let attempt = 0; attempt < 30; attempt += 1) {
      const found = canvas.current?.focusElement(item.selector, item.tag, item.elementId ?? undefined, item.pageId);
      if (found) return found;
      await new Promise(resolve => setTimeout(resolve, 150));
    }
    setError('That item could not be found on the canvas. The page may have changed.'); return null;
  }
  function openFoundSource(pageId: string, target: ElementTarget) {
    const projectId = current.current?.project?.id; if (!projectId) return;
    void native('open_found_source', { projectId, pageId, target }).catch(error => setError(errorMessage(error)));
  }
  async function saveSource(language: keyof SiteContent, text: string, expected: string) {
    const projectId = project?.id, pageId = page?.id;
    await canvas.current?.synchronize(); await settle();
    return queue(async workspace => {
      const target = workspace?.project?.pages.find(item => item.id === pageId);
      // Never write over code that changed elsewhere since this draft began.
      if (!workspace?.project || workspace.project.id !== projectId || !target || (target.content?.[language] ?? '') !== expected) throw { conflict: true, message: 'This page changed elsewhere while you were editing.' };
      const next = await native<WorkspaceView>('edit_page_source', { projectId, pageId, revision: workspace.project.revision, [language]: text });
      accept(next);
      const applied = next.project?.pages.find(item => item.id === pageId)?.content;
      if (!applied) throw new Error('The edited page is no longer available. Your draft is still here.');
      return applied;
    }, false);
  }
  // Retarget atomically so the request description and linked brief rules stay together.
  async function repickRequest(regionId: string, selection: ElementSelection) {
    const target = current.current?.project?.pages.find(item => item.regions.some(region => region.id === regionId));
    if (!target) return;
    await mutate({ type: 'retarget_region', page_id: target.id, region_id: regionId, elements: [selection.element], bounds: [{ target: selection.element, rect: selection.rect }] });
    setSelected(regionId); setNotice('Request moved to the new element.');
  }
  async function fixCheckItem(item: CheckItem) {
    const found = await showCheckItem(item);
    if (item.selector && !found) return;
    const target = current.current?.project?.pages.find(entry => entry.id === item.pageId);
    if (!target) return;
    // Reuse an open request on the same element instead of stacking a second one.
    const existing = found && target.regions.find(region => !region.completed && sameTargets(regionTargets(region), [found.element]));
    const before = new Set(target.regions.map(region => region.id));
    const added = existing || await (found ? mutate({ type: 'add_element_region', page_id: target.id, ...found })
      : mutate({ type: 'add_region', page_id: target.id, rect: { x: 0, y: 0, width: target.viewport.width, height: 120 } }))
      .then(next => next.project!.pages.find(entry => entry.id === target.id)!.regions.find(region => !before.has(region.id)));
    if (!added) return;
    if (added.prompt.includes(item.fix)) { setSelected(added.id); return; }
    await changeRegion(target.id, added.id, { prompt: added.prompt.trim() ? `${added.prompt.trim()}\n${item.fix}` : item.fix });
    setSelected(added.id); setNotice('Added a request. Press Generate when you’re ready.');
  }

  // Keep this menu short: only what someone building a website reaches for.
  // Canvas-level actions (copy, arrange, zoom) live on the canvas and its right-click menu.
  const menuItems: MenuItem[] = [
    { id: 'new', label: 'New project', shortcut: 'Ctrl/Cmd+N', action: () => setShowNew(true) },
    { id: 'open', label: 'Open project', shortcut: 'Ctrl/Cmd+O', action: () => run(open) },
    { id: 'save', label: 'Save', shortcut: 'Ctrl/Cmd+S', disabled: !project, action: () => run(() => save()) },
    { id: 'save-as', label: 'Save as', shortcut: 'Ctrl/Cmd+Shift+S', disabled: !project, action: () => run(() => save(true)) },
    ...(view?.recoveries.length ? [{ id: 'recover', label: 'Recover unsaved projects', action: () => setShowRecovery(true) }] : []),
    { id: 'file-divider', label: '', separator: true },
    { id: 'add-page', label: 'Add page', disabled: !project || project.pages.length >= 100, action: () => setPageDialog('new') },
    { id: 'page-settings', label: 'Page settings', disabled: !page, action: () => setPageDialog(page) },
    { id: 'remove-page', label: 'Remove page', disabled: !page || page.id === project?.homePageId, action: () => run(removePage) },
    { id: 'page-divider', label: '', separator: true },
    { id: 'preview', label: 'Preview website', disabled: !page?.content, action: () => setShowPreview(true) },
    { id: 'code-view', label: 'View', disabled: !project, children: (['design', 'split', 'code'] as CodeView[]).map(value => ({ id: `view-${value}`, label: value === 'design' ? 'Design' : value === 'split' ? 'Split (design + code)' : 'Code', checked: codeView === value, action: () => run(() => changeCodeView(value)) })) },
    { id: 'export', label: 'Export website…', disabled: exporting || !project?.pages.some(page => page.content), action: () => run(exportWebsite) },
    { id: 'publish', label: 'Publish online (coming soon)', disabled: true },
    { id: 'ai-divider', label: '', separator: true },
    { id: 'settings', label: 'AI settings', action: () => setShowAiSettings(true) },
    { id: 'mode', label: 'AI style', disabled: !project || working, children: (['fuzzy', 'strict'] as GenerationMode[]).map(mode => ({ id: mode, label: mode === 'fuzzy' ? 'Flexible — small adjustments only' : 'Precise — follow placement closely', checked: project?.generationMode === mode, action: () => run(() => mutate(current => current.generationMode === mode ? null : { type: 'set_generation_mode', mode })) })) },
    { id: 'page-check', label: fixCount ? `Check page (${fixCount})` : 'Check page', disabled: !project?.pages.some(item => item.content), action: () => setShowCheck(true) },
    { id: 'dossier', label: 'Project brief', disabled: !project, action: () => setShowNotes('overview') },
    { id: 'help-divider', label: '', separator: true },
    { id: 'appearance', label: 'Editor theme', children: (['system', 'light', 'dark'] as Theme[]).map(value => ({ id: value, label: value === 'system' ? 'System' : value === 'light' ? 'Light' : 'Dark', checked: theme === value, action: () => setTheme(value) })) },
    { id: 'help', label: 'Keyboard shortcuts', shortcut: 'F1', action: () => setShowShortcuts(true) },
  ];

  return <div className="application editor-shell">
    <header className="app-bar"><div className="wordmark" aria-label="Desegnos"><img className="brand-light" src={brandLight} alt="" /><img className="brand-dark" src={brandDark} alt="" /><span className="sr-only">Desegnos</span></div>
      {project && <button className="project-title" onClick={() => setShowRename(true)} title="Rename project"><span>{project.name}</span>{view?.dirty && <span aria-label="Unsaved changes" className="dirty-dot">●</span>}</button>}
      <MenuButton label="Menu" ariaLabel="Editor menu" items={menuItems} />
      {project && <div className="view-switch segmented" role="group" aria-label="Editor view">{(['design', 'split', 'code'] as CodeView[]).map(value => <button key={value} type="button" aria-pressed={codeView === value} onClick={() => run(() => changeCodeView(value))}>{value === 'design' ? 'Design' : value === 'split' ? 'Split' : 'Code'}</button>)}</div>}
      <div className="shell-primary-actions">
        {project && <><button aria-label="Undo last change" title="Undo · Ctrl/Cmd+Z" disabled={!view?.canUndo || pending > 0} onClick={() => run(() => travel('undo_project'))}>Undo</button><button aria-label="Redo last change" title="Redo · Ctrl/Cmd+Shift+Z" disabled={!view?.canRedo || pending > 0} onClick={() => run(() => travel('redo_project'))}>Redo</button>
          <MenuButton label="History" className="history-menu" ariaLabel="Change history" title="Go back to an earlier point" items={history.entries.length > 1 ? history.entries.map(entry => ({ id: `history-${entry.position}`, label: `${entry.label}${entry.position > (view?.history?.position ?? 0) ? ' (undone)' : ''}`, shortcut: new Date(entry.at).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' }), checked: entry.position === view?.history?.position, disabled: pending > 0 || working, action: () => run(() => jumpTo(entry.position)) })).reverse() : [{ id: 'history-empty', label: 'No changes yet', disabled: true }]} /></>}
        {page?.content && <button onClick={() => setShowPreview(true)}>Preview</button>}
        {project?.pages.some(item => item.content) && <button disabled={exporting} onClick={() => run(exportWebsite)}>{exporting ? 'Exporting…' : 'Export'}</button>}
        {!aiConnected && <button className="ai-shortcut needs-connection" onClick={() => setShowAiSettings(true)} aria-label="AI settings" title="Connect AI to generate">Connect AI</button>}
        {project && <button className="primary" title={requestedAreas === 0 ? 'Describe at least one request first.' : 'Generate the described changes'} disabled={requestedAreas === 0 || working} onClick={generate}>{working ? 'Generating…' : 'Generate'}</button>}
      </div>
    </header>
    {error && <div className="message error" role="alert"><span>{error}</span><button onClick={() => setError('')} aria-label="Dismiss error">Dismiss</button></div>}
    {exported && <section className="export-notice" role="status"><div><strong>Website exported</strong><span title={exported.path}>{exported.pageCount} {exported.pageCount === 1 ? 'page' : 'pages'}{exported.warnings.length > 0 && ` · ${exported.warnings.length} to check`}</span>{exported.warnings.length > 0 && <details><summary>Things to check</summary><ul>{exported.warnings.map((warning, index) => <li key={index}>{warning}</li>)}</ul></details>}</div><button onClick={() => run(() => native('open_export_folder', { path: exported.path }))}>Open folder</button><button aria-label="Dismiss export notice" title="Dismiss" onClick={() => setExported(null)}>×</button></section>}
    {view?.recoveryError && <div className="message error" role="alert">Recovery could not be loaded: {view.recoveryError}</div>}
    {view?.migrationNotice && dismissedMigration !== `${project?.id}:${view.migrationNotice}` && <div className="message warning migration-notice" role="status"><span>{view.migrationNotice}</span><button onClick={() => setDismissedMigration(`${project?.id}:${view.migrationNotice}`)} aria-label="Dismiss migration notice">Dismiss</button></div>}
    {loading ? <main className="startup"><p role="status">Opening your workspace…</p></main> : !view ? <main className="startup"><h1>Open the Desegnos desktop application</h1><p className="muted">Projects are managed by the native application.</p><button onClick={() => { void load(); }}>Retry workspace connection</button></main> : !project ? <main className="startup">
      {view.recoveries.length > 0 && <section className="recovery-notice" aria-label="Project recovery"><h2>Recover unsaved projects</h2>
        <RecoveryList items={view.recoveries} busy={pending > 0} recover={id => { void recover(id).catch(() => {}); }} /></section>}
      <NewProject busy={pending > 0} create={create} open={open} settings={aiSettings} openSettings={() => setShowAiSettings(true)} recentProjects={recentProjects} openRecent={openRecent} />
    </main> : <>
      <div className="page-strip"><div className="page-tabs" role="tablist" aria-label="Project pages">
        {project.pages.map((item, index) => <button key={item.id} id={`page-tab-${item.id}`} role="tab" aria-selected={page?.id === item.id} aria-controls="page-panel" tabIndex={page?.id === item.id ? 0 : -1}
          title={`${item.path}${item.included ? '' : ' · Hidden page'}`} onClick={() => run(() => selectPage(item.id))}
          onKeyDown={event => {
            if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
            event.preventDefault();
            const next = event.key === 'Home' ? 0 : event.key === 'End' ? project.pages.length - 1 : (index + (event.key === 'ArrowRight' ? 1 : -1) + project.pages.length) % project.pages.length;
            document.getElementById(`page-tab-${project.pages[next].id}`)?.focus(); run(() => selectPage(project.pages[next].id));
          }}><span>{item.name}</span>{!item.included && <span className="page-draft">Hidden</span>}</button>)}
      </div><button className="add-page-tab" aria-label="Add page" title="Add page" disabled={project.pages.length >= 100} onClick={() => setPageDialog('new')}>+</button></div>
      <main className={`workspace view-${codeView}`} id="page-panel" role="tabpanel" aria-labelledby={page ? `page-tab-${page.id}` : undefined} style={{ '--code-width': `${codeWidth * 100}%` } as CSSProperties}>
      <div className="design-pane">
      {page && <Canvas projectImages={project.importedIdentity?.assets ?? []} ref={canvas} canUndo={!!view?.canUndo && pending === 0} canRedo={!!view?.canRedo && pending === 0} undo={() => run(() => travel('undo_project'))} redo={() => run(() => travel('redo_project'))} toggleGrid={() => run(() => mutate(current => ({ type: 'set_grid', enabled: !current.showGrid })))} configureGrid={() => setShowGridSettings(true)} page={page} layersOpen={showLayers} setLayersOpen={setShowLayers} selectionChanged={setActiveLayer} multiChanged={setMultiCount} repick={repickRequest} arrangeRequest={arrangeRequest} copyNative={copyLayerNative} cutNative={cutLayerNative} pasteNative={pasteLayerNative} tool={tool} rulerUnit={rulerUnit} highlighted={highlightResult ? generation.job?.areas.filter(area => area.pageId === page.id).map(area => area.rect) ?? [] : []} grid={project.showGrid} gridSettings={project.gridSettings} viewAction={canvasView} changing={working && generation.job && ['prepared', 'generating', 'ready', 'applying'].includes(generation.job.status) ? generation.job?.areas?.filter(area => area.pageId === page.id).map(area => area.rect) ?? [] : []} zoom={zoom} refreshElementBounds={refreshElementBounds} openSource={target => openFoundSource(page.id, target)} inspectElement={inspectElement} editElement={editElement} changeZoom={setZoom} selected={selected} focusRequest={focusPrompt ? selected : null} select={selectRequest} add={rect => { void addRegion(rect).catch(() => {}); }} setTargetRemoval={(regionId, allowed) => run(() => mutate({ type: 'set_region_target_removal', page_id: page.id, region_id: regionId, allow_target_removal: allowed }))} deleteElements={deleteElements} pickElement={selection => { void pickElement(selection).catch(() => {}); }} change={(id, patch) => changeRegion(page.id, id, patch)} remove={id => removeRegion(id)} />}
      {(showGenerationStatus || resultReview) && <div className="canvas-pill" role="region" aria-label="Generation status">
        {resultReview && !showGenerationStatus ? <><strong role="status">Page updated</strong>
          <button disabled={pending > 0} onClick={() => run(() => rejectResult())}>Undo</button>
          <button disabled={pending > 0} title="Undo this result and build a new version; this uses your AI account again" onClick={() => run(() => rejectResult(true))}>Try again</button>
          <button className="pill-close" aria-label="Keep changes" title="Keep" onClick={() => setResultReview(null)}>×</button>
        </> : <GenerationFeedback controller={generation} compact>
          {generation.job?.status === 'generating' && <button disabled={!!generation.operation} onClick={() => run(generation.cancel)}>Cancel</button>}
          {!working && generation.job && ['failed', 'unknown', 'cancelled', 'blocked'].includes(generation.job.status) && <button className="primary" disabled={requestedAreas === 0} onClick={generate}>{generation.job.status === 'blocked' ? 'Build draft' : 'Try again'}</button>}
          {generation.error && <button onClick={() => setShowAiSettings(true)}>AI settings</button>}
          {!working && <button onClick={openGeneration}>Details</button>}
        </GenerationFeedback>}
      </div>}
      <FirstRunTips />
      {showCheck && <PageCheckPanel items={checks} busy={pending > 0 || working} close={() => setShowCheck(false)} show={item => run(() => showCheckItem(item))} fix={item => run(() => fixCheckItem(item))} source={item => run(async () => { const found = await showCheckItem(item); if (found) openFoundSource(item.pageId, found.element); })} />}
      <CanvasTools layersOpen={showLayers} toggleLayers={() => setShowLayers(value => !value)} tool={tool} setTool={setTool} zoom={zoom} setZoom={setZoom} fit={() => recenter('fit')} />
      </div>
      {codeView !== 'design' && <>
        {codeView === 'split' && <div className="code-divider" role="separator" aria-orientation="vertical" aria-label="Resize code pane" onPointerDown={dragDivider} />}
        <CodePane ref={code} saveProject={() => run(() => save())} key={page?.id} content={page?.content ?? null} target={activeLayer} map={sourceMap} save={page?.content ? saveSource : undefined} pick={(selector, id) => { canvas.current?.focusElement(selector, selector.split(' > ').at(-1)!.split(':')[0], id, page?.id); }} />
      </>}
      </main>
      <footer className="status-bar"><span title={view.path ?? undefined}>{multiCount > 1 ? `${multiCount} elements selected` : region ? 'Request selected' : tool === 'request' ? 'Click or drag to describe an area' : tool === 'pan' ? 'Drag to move around the canvas' : 'Double-click text to edit · Ask AI for changes'}</span><div className="canvas-status-actions">{fixCount > 0 && <button className="check-chip" aria-label="Open page check" onClick={() => setShowCheck(value => !value)}>{placeholderCount > 0 ? `${placeholderCount} placeholder${placeholderCount === 1 ? '' : 's'}` : `${fixCount} to check`}</button>}<button onClick={() => setShowGridSettings(true)} aria-label="Grid and rulers">{project.gridSettings.spacing}{project.gridSettings.unit === 'percent' ? '%' : ' px'} grid{project.gridSettings.snap ? ' · Snap' : ''}</button><span role="status" aria-live="polite">{pending > 0 ? 'Saving locally…' : view.dirty ? 'Unsaved' : 'Saved'}{notice && <span className="sr-only"> · {notice}</span>}</span></div></footer>
    </>}
    {showShortcuts && <Modal title="Keyboard shortcuts" close={() => setShowShortcuts(false)}><div className="dialog-body shortcut-help"><p className="field-help">Text selection, copy, paste and text undo keep their normal behavior inside prompts.</p><dl>{SHORTCUT_HELP.map(([label, keys]) => <div key={label}><dt>{label}</dt><dd>{keys}</dd></div>)}</dl></div></Modal>}
    {showNew && <Modal title="New project" close={() => setShowNew(false)}><DialogError error={error} dismiss={() => setError('')} /><NewProject busy={pending > 0} create={create} open={open} settings={aiSettings} openSettings={() => setShowAiSettings(true)} recentProjects={recentProjects} openRecent={openRecent} /></Modal>}
    {showRecovery && view && <Modal title="Recover unsaved projects" close={() => setShowRecovery(false)}><DialogError error={error} dismiss={() => setError('')} />
      <RecoveryList items={view.recoveries} busy={pending > 0} recover={id => { void recover(id).catch(() => {}); }} /></Modal>}
    {showNotes && project && <Modal title="Project brief" wide close={() => setShowNotes(null)}><DialogError error={error} dismiss={() => setError('')} /><Notes project={project} initialSection={showNotes} reviewGeneration={project.latestGeneration?.jobId === generation.job?.jobId ? openGeneration : undefined} apply={command => mutate(command).then(() => undefined)} changeBrief={changeBrief} markdown={getNotes} /></Modal>}
    {showGridSettings && project && <Modal title="Grid and rulers" close={() => setShowGridSettings(false)}><DialogError error={error} dismiss={() => setError('')} /><GridForm rulerUnit={rulerUnit} changeRulerUnit={setRulerUnit} settings={project.gridSettings} showGrid={project.showGrid} busy={pending > 0} submit={async (settings, visible) => {
      if (visible !== project.showGrid) await mutate({ type: 'set_grid', enabled: visible });
      await mutate({ type: 'set_grid_settings', settings }); setShowGridSettings(false);
    }} /></Modal>}
    {pageDialog && project && <Modal title={pageDialog === 'new' ? 'Add page' : 'Page settings'} close={() => setPageDialog(null)}>
      <DialogError error={error} dismiss={() => setError('')} /><PageForm paths={project.pages.map(item => item.path)} page={pageDialog === 'new' ? null : pageDialog} home={pageDialog !== 'new' && pageDialog.id === project.homePageId} busy={pending > 0} submit={async values => {
        if (pageDialog === 'new') {
          await dismissEmpty(null);
          await synchronize();
          const next = await mutate({ type: 'add_page', name: values.name, path: values.path });
          setPageId(next.project!.pages.at(-1)!.id); setSelected(null);
        } else await mutate({ type: 'update_page', page_id: pageDialog.id, ...values });
        setPageDialog(null);
      }} />
    </Modal>}
    {showRename && project && <Modal title="Rename project" close={() => setShowRename(false)}><DialogError error={error} dismiss={() => setError('')} /><RenameProject name={project.name} submit={async name => { await mutate({ type: 'rename_project', name }); setShowRename(false); }} /></Modal>}
    {saveConflict && <SaveConflictDialog conflict={saveConflict} choose={chooseSaveConflict} />}
    {replacement && <Modal title="Save your changes?" close={() => setReplacement(null)}><DialogError error={error} dismiss={() => setError('')} /><div className="dialog-body"><p>Save “{project?.name}” before you {replacement.label}?</p><p className="muted">Discard continues without saving these changes to the project file.</p><div className="form-actions">
      <button className="primary" disabled={pending > 0} onClick={() => { void (async () => { const saved = await save(); if (saved && !saved.dirty) { const run = replacement.run; setReplacement(null); await run(false); } })().catch(() => {}); }}>Save and continue</button>
      <button className="danger" disabled={pending > 0} onClick={() => { const run = replacement.run; setReplacement(null); void run(true).catch(() => {}); }}>Discard and continue</button><button onClick={() => setReplacement(null)}>Keep editing</button>
    </div></div></Modal>}
    {confirmation && <Modal title={confirmation.title} close={() => setConfirmation(null)}><DialogError error={error} dismiss={() => setError('')} /><div className="dialog-body"><p>{confirmation.message}</p><div className="form-actions"><button className="danger" disabled={pending > 0} onClick={() => { void confirmation.action().then(() => setConfirmation(null)).catch(() => {}); }}>Remove</button><button onClick={() => setConfirmation(null)}>Cancel</button></div></div></Modal>}
    {showPreview && page?.content && <Modal title="Site preview" wide close={() => setShowPreview(false)}><div className="accepted-preview"><PagePreview key={`${project?.id}:${page.id}`} page={page} revision={project!.revision} /></div></Modal>}
    {generation.visible && project && <GenerationDialog controller={generation} project={project} settings={aiSettings} openSettings={() => setShowAiSettings(true)} manageUsage={manageChatgptUsage} editRequest={id => {
      const owner = project.pages.find(item => item.regions.some(region => region.id === id));
      if (!owner) return;
      generation.close();
      run(async () => { await selectPage(owner.id, id); setSelected(id); setFocusPrompt(true); });
    }} close={closeGeneration} />}
    {showAiSettings && <Modal title="AI settings" wide close={() => setShowAiSettings(false)}><AiSettings current={aiSettings} refresh={refreshAiSettings} update={updateAiSettings} manageUsage={manageChatgptUsage} pollingError={aiPollingError} /></Modal>}
  </div>;
}

function DialogError({ error, dismiss }: { error: string; dismiss: () => void }) {
  return error ? <div className="message error" role="alert"><span>{error}</span><button onClick={dismiss} aria-label="Dismiss dialog error">Dismiss</button></div> : null;
}

function editedAgo(editedAtMs: number) {
  const minutes = Math.max(0, Math.floor((Date.now() - editedAtMs) / 60_000));
  return !Number.isFinite(minutes) ? 'Unsaved draft' : minutes < 1 ? 'Edited just now' : minutes < 60 ? `Edited ${minutes} min ago` : minutes < 1440 ? `Edited ${Math.floor(minutes / 60)} hr ago` : `Edited ${Math.floor(minutes / 1440)} days ago`;
}

function RecoveryList({ items, busy, recover }: { items: RecoveryInfo[]; busy: boolean; recover: (id: string) => void }) {
  return <div className="recovery-list">{items.map(item => <section key={item.projectId} aria-label={`Recover ${item.name}`}>
    <p>“{item.name}” has unsaved changes.</p>
    <p className="muted" title={item.savedPath ?? undefined}>{editedAgo(item.editedAtMs)}{item.savedPath ? ` · ${item.savedPath.split(/[\\/]/).at(-1)}` : ' · Never saved'}</p>
    <button disabled={busy} onClick={() => recover(item.projectId)}>Recover project</button>
  </section>)}</div>;
}

function RenameProject({ name, submit }: { name: string; submit: (value: string) => Promise<void> }) {
  const [value, setValue] = useState(name);
  const [busy, setBusy] = useState(false);
  return <form className="dialog-body" onSubmit={event => { event.preventDefault(); setBusy(true); void submit(value.trim()).catch(() => {}).finally(() => setBusy(false)); }}><label>Project name<input autoFocus required maxLength={120} value={value} onChange={event => setValue(event.target.value)} /></label><div className="form-actions"><button className="primary" disabled={busy || !value.trim()}>Rename project</button></div></form>;
}

function suggestedPagePath(name: string, paths: string[]) {
  let slug = name.trim().toLowerCase().replace(/[^\p{L}\p{N}]+/gu, '-').replace(/^-|-$/g, '') || 'page';
  if (/^(assets|src|node_modules|con|prn|aux|nul|com[1-9]|lpt[1-9])$/.test(slug)) slug += '-page';
  const base = `/${slug}`;
  let path = base; let suffix = 2;
  while (paths.some(existing => existing.toLowerCase() === path.toLowerCase())) path = `${base}-${suffix++}`;
  return path;
}

function PageForm({ page, paths, home, busy, submit }: { page: Page | null; paths: string[]; home: boolean; busy: boolean; submit: (values: Pick<Page, 'name' | 'path' | 'viewport' | 'appearance' | 'included'>) => Promise<void> }) {
  const [name, setName] = useState(page?.name ?? '');
  const [path, setPath] = useState(page?.path ?? '/page');
  const [customPath, setCustomPath] = useState(false);
  const [viewport, setViewport] = useState<Viewport>(page?.viewport ?? { width: 1440, height: 900 });
  const [appearance, setAppearance] = useState<Appearance>(page?.appearance ?? 'light');
  const [included, setIncluded] = useState(page?.included ?? true);
  const [saving, setSaving] = useState(false);
  function save(event: FormEvent) { event.preventDefault(); setSaving(true); void submit({ name: name.trim(), path: path.trim(), viewport, appearance, included }).catch(() => {}).finally(() => setSaving(false)); }
  return <form className="dialog-body page-form" onSubmit={save} onInvalid={event => { if (event.target instanceof HTMLElement) { const details = event.target.closest('details'); if (details) details.open = true; } }}>
    <label>Page name<input autoFocus required maxLength={120} value={name} onChange={event => {
      const value = event.target.value; setName(value); if (!page && !customPath) setPath(suggestedPagePath(value, paths));
    }} /></label>
    {page && <>
      <fieldset><legend>Screen guide</legend><div className="segmented">{([{ label: 'Desktop', width: 1440, height: 900 }, { label: 'Mobile', width: 390, height: 844 }]).map(size => <button key={size.label} type="button" aria-pressed={viewport.width === size.width && viewport.height === size.height} onClick={() => setViewport({ width: size.width, height: size.height })}>{size.label}</button>)}</div></fieldset>
      <details className="custom-screen"><summary>Custom screen size · {viewport.width} × {viewport.height}</summary><div className="field-pair">
        <label>Screen width<input type="number" min={1} max={32768} required value={viewport.width} onChange={event => setViewport({ ...viewport, width: Number(event.target.value) })} /></label><label>Screen height<input type="number" min={1} max={100000} required value={viewport.height} onChange={event => setViewport({ ...viewport, height: Number(event.target.value) })} /></label>
      </div></details>
      <label>Website appearance<select value={appearance} onChange={event => setAppearance(event.target.value as Appearance)}><option value="light">Light page</option><option value="dark">Dark page</option></select></label>
      <label className="check-label"><input type="checkbox" checked={included} disabled={home} onChange={event => setIncluded(event.target.checked)} />Include this page in export</label>
    </>}
    <details className="page-address"><summary>Page address · {path}</summary><label>Page path<input required maxLength={1024} disabled={home} value={path} onChange={event => { setPath(event.target.value); setCustomPath(true); }} aria-describedby="page-path-help" /></label><p id="page-path-help" className="field-help">{home ? 'The home page uses /.' : 'Start with /, such as /about.'}</p></details>
    <div className="form-actions"><button className="primary" disabled={busy || saving || !name.trim() || !path.trim()}>{page ? 'Save page settings' : 'Add page'}</button></div>
  </form>;
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
    <p className="field-help">Percentages follow the screen guide. Hold Alt to place freely.</p>
    <div className="form-actions"><button className="primary" disabled={busy || saving || !valid}>Save grid settings</button></div>
  </form>;
}
