import { needsAiSettings, ProblemBanner, Notice, NoticeProvider, NoticeHost } from './ProblemBanner.js';
import { summarizeOutsideChanges } from './noticeSummary.js';
import { userError } from './userErrors.js';
import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent, type CSSProperties, type PointerEvent as ReactPointerEvent } from 'react';
import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { native, edit } from './api';
import { Canvas, type CanvasHandle, type RequestPatch } from './Canvas';
import { CanvasTools, type CanvasTool, type RequestShapeKind } from './CanvasTools';
import { WorkspaceSurface, WorkspaceSurfaces, SurfaceHost, isTextField } from './WorkspaceSurface';
import { RecoveryBanner, isPendingRecovery, type PendingRecovery } from './RecoveryBanner';
import { SaveConflictDialog, type SaveConflict, type SaveOutcome, type SaveChoice } from './SaveConflict';
import { regionTargets, sameTargets } from './elementTargets';
import brandLight from './assets/brand/desegnos-wordmark-light.svg';
import brandDark from './assets/brand/desegnos-wordmark-dark.svg';
import { NewProject } from './NewProject';
import { ProjectLocationSettings } from './ProjectLocation';
import type { ProjectLocationView } from './types';
import { Notes, type DossierSection } from './Notes';
import { MenuButton, type MenuItem } from './Menus';
import './shell.css';
import { editorShortcut, SHORTCUT_HELP } from './shortcuts';
import { steppedZoom } from './canvasGeometry';
import { AiSettings } from './AiSettings';
import { GenerationFeedback } from './Generation';
import { CodePane, type CodeView, type SourceMapView, type CodePaneHandle } from './CodePane';
import { FirstRunTips } from './Onboarding';
import { StyleGuide } from './StyleGuide';
import { ImportContext } from './ImportContext';
import type { WebsiteFolderSelection } from './types';
import { pendingDrafts } from './generationDraftState';
import { GenerationLog } from './GenerationLog';
import { askBeforeDelete, setAskBeforeDelete, useAskBeforeDelete } from './deletePreference';
import { checkProject, PageCheckPanel, type CheckItem } from './PageCheck';
import { useHistory } from './useHistory';
import { activeGeneration, useGeneration } from './useGeneration';
import type { AiSettingsView, Appearance, Brief, Command, ElementStyleEdit, LayerTarget, LayerDirection, LayerPasteView, ElementBounds, ElementDetails, ElementEdit, ElementEditView, ElementSelection, ElementTarget, GenerationJobView, GenerationMode, GridSettings, Page, Project, Rect, RecoveryInfo, Region, RegionShape, Viewport, WorkspaceView, RecentProjectsView, RecentProject, WebsiteExport, SiteContent, RequestAttachment } from './types';

type Theme = 'system' | 'light' | 'dark';
type Replacement = { label: string; run: (discard: boolean) => Promise<void> };
type Confirmation = { title: string; message: string; action: () => Promise<void>; skippable?: boolean };

const PROJECT_COLOURS = ['#2f6fde', '#d9480f', '#2b8a3e', '#ae3ec9', '#0c8599', '#e67700'];
function errorMessage(error: unknown) { return error instanceof Error ? error.message : String(error); }
// Open each page fitted to the window: web pages by width, mobile pages by height.
function initialZoom(width: number, height = 900) { const fit = width > 600 ? (window.innerWidth - 96) / width : (window.innerHeight - 240) / height; return Math.max(0.1, Math.min(1, fit)); }
function readRequestShape(): RequestShapeKind { try { const value = localStorage.getItem('desegnos.request-shape'); return value === 'ellipse' || value === 'freehand' ? value : 'rect'; } catch { return 'rect'; } }
function readRulerUnit(): 'pixels' | 'percent' { try { return localStorage.getItem('desegnos.ruler-unit') === 'percent' ? 'percent' : 'pixels'; } catch { return 'pixels'; } }
function readCodeWidth() { try { return Math.min(0.75, Math.max(0.2, Number(localStorage.getItem('desegnos.code-width')) || 0.42)); } catch { return 0.42; } }
function readCodeView(projectId: string): CodeView { try { const value = localStorage.getItem(`desegnos.code-view.${projectId}`); return value === 'split' || value === 'code' ? value : 'design'; } catch { return 'design'; } }
function readTheme(): Theme {
  try { const saved = localStorage.getItem('desegni.editor-theme'); return saved === 'light' || saved === 'dark' ? saved : 'system'; }
  catch { return 'system'; }
}

export default function App() {
  return <NoticeProvider><WorkspaceSurfaces><Editor /></WorkspaceSurfaces></NoticeProvider>;
}

function Editor() {
  const [closeRequest, setCloseRequest] = useState<{ message: string; resolve: (answer: boolean) => void } | null>(null);
  const [retryResult, setRetryResult] = useState(false);
  const [view, setView] = useState<WorkspaceView | null>(null);
  const current = useRef<WorkspaceView | null>(null);
  const canvas = useRef<CanvasHandle>(null);
  const code = useRef<CodePaneHandle>(null);
  const tail = useRef<Promise<void>>(Promise.resolve());
  const [pending, setPending] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [notices, setNotices] = useState<{ id: number; message: string }[]>([]);
  const noticeId = useRef(0);
  const setNotice = useCallback((message: string) => { const id = ++noticeId.current; setNotices(items => [...items, { id, message }]); }, []);
  const [dismissedMigration, setDismissedMigration] = useState('');
  // The recovery error can be put aside for this session; the recovery files stay on disk.
  const [dismissedRecovery, setDismissedRecovery] = useState<string[]>([]);
  const [theme, setTheme] = useState<Theme>(readTheme);
  const [pageId, setPageId] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [focusPrompt, setFocusPrompt] = useState(false);
  const [zoom, setZoom] = useState(0.75);
  const [tool, setTool] = useState<CanvasTool>('select');
  const [requestShape, setRequestShape] = useState<RequestShapeKind>(readRequestShape);
  useEffect(() => { try { localStorage.setItem('desegnos.request-shape', requestShape); } catch { /* Preference storage can be unavailable. */ } }, [requestShape]);
  const [rulerUnit, setRulerUnit] = useState<'pixels' | 'percent'>(readRulerUnit);
  const [recentProjects, setRecentProjects] = useState<RecentProject[]>([]);
  const [projectLocation, setProjectLocation] = useState<ProjectLocationView | null>(null);
  const [showProjectLocation, setShowProjectLocation] = useState(false);
  const [dismissedSiteWarnings, setDismissedSiteWarnings] = useState('');
  const [exported, setExported] = useState<WebsiteExport | null>(null);
  const [exporting, setExporting] = useState(false);
  const [resultReview, setResultReview] = useState<{ jobId: string; revision: number } | null>(null);
  const [highlightResult, setHighlightResult] = useState(false);
  const [showLayers, setShowLayers] = useState(false);
  const [activeLayer, setActiveLayer] = useState<ElementTarget | null>(null);
  const [showNew, setShowNew] = useState(false);
  const [pendingRecovery, setPendingRecovery] = useState<PendingRecovery | null>(null);
  async function requestProjectOpen<T>(operation: () => Promise<T>): Promise<T | null> {
    setPendingRecovery(null);
    try { return await operation(); }
    catch (error) {
      if (!isPendingRecovery(error)) throw error;
      setError(''); setPendingRecovery(error); return null;
    }
  }
  const [showNotes, setShowNotes] = useState<DossierSection | null>(null);
  const [showGridSettings, setShowGridSettings] = useState(false);
  const [codeView, setCodeView] = useState<CodeView>('design');
  const [showCheck, setShowCheck] = useState(false);
  const [showStyleGuide, setShowStyleGuide] = useState(false);
  const [showImport, setShowImport] = useState(false);
  const [importFolder, setImportFolder] = useState<WebsiteFolderSelection | null>(null);
  const [importAsNew, setImportAsNew] = useState(false);
  function startBrandImport() { setImportFolder(null); setImportAsNew(true); setShowNew(false); setShowImport(true); }
  async function editWebsite(folderId: string) {
    await synchronize(); await settle();
    await queue(async () => {
      const opened = await requestProjectOpen(() => native<WorkspaceView>('edit_website_folder', { folderId }));
      if (!opened) return;
      accept(opened); setSelected(null); setShowNew(false); setNotice('Website opened.');
      if (opened.project) setZoom(initialZoom(opened.project.viewport.width, opened.project.viewport.height));
      void refreshRecent().catch(() => {});
    });
  }
  const [offerRestyle, setOfferRestyle] = useState<string | false>(false);
  const [showLog, setShowLog] = useState(false);
  const [logFocus, setLogFocus] = useState(0);
  const [pendingImages, setPendingImages] = useState(0);
  // Live mode: the canvas page runs its scripts and takes clicks; requests can still be added.
  const [live, setLive] = useState(false);
  const [liveDevice, setLiveDevice] = useState<'desktop' | 'tablet' | 'phone'>('desktop');
  const [resolvedTheme, setResolvedTheme] = useState<'light' | 'dark'>('light');
  const askDelete = useAskBeforeDelete();
  const [skipNextAsk, setSkipNextAsk] = useState(false);
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
  useEffect(() => { void native<ProjectLocationView>('get_project_location').then(setProjectLocation).catch(error => setError(errorMessage(error))); }, []);
  async function changeProjectLocation(reset: boolean) {
    const path = reset ? null : await native<string | null>('pick_project_location');
    if (!reset && !path) return;
    setProjectLocation(await native<ProjectLocationView>('set_project_location', { path }));
  }
  useEffect(() => { try { localStorage.setItem('desegnos.ruler-unit', rulerUnit); } catch {} }, [rulerUnit]);

  const accept = useCallback((next: WorkspaceView) => { current.current = next; setView(next); }, []);
  // quiet: typing saves skip the busy count, which would re-render the whole editor twice per key.
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
    const update = () => { const value = theme === 'system' ? (media.matches ? 'dark' : 'light') : theme; document.documentElement.dataset.theme = value; setResolvedTheme(value); };
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
        if ((current.current?.anyDirty || current.current?.dirty || generating) && !await new Promise<boolean>(resolve => setCloseRequest({ resolve, message: `${generating ? `Generation is still running. Closing may interrupt the request, and ${plan ? 'submitted work may still use your ChatGPT plan or credits' : 'OpenAI may still charge for submitted work'}. Its outcome will be shown as uncertain when you return. ` : ''}${current.current?.dirty ? 'This project has changes that have not been saved to its file. ' : ''}Close Desegnos?` }))) return;
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
      accept(outcome.workspace); setDismissedSiteWarnings(''); setNotice('Project saved.'); void refreshRecent().catch(() => {});
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
  async function closeProject() {
    await replace('close this project', async discard => {
      await queue(async () => {
        accept(await native<WorkspaceView>('close_project', { discard }));
        setSelected(null); setNotice('Project closed.');
      });
    });
  }
  async function importContext(analysisId: string, findings: number[], assets: string[], photos: string[], items: number[], pages: string[], attachMatching: boolean) {
    await synchronize(); await settle();
    await queue(async workspace => {
      const next = importAsNew || !workspace?.project
        ? await native<WorkspaceView>('create_project', { name: 'Imported brand', viewport: { width: 1440, height: 900 }, discard: false, identityAnalysisId: analysisId, identityFindingIndices: findings, identityAssetIds: assets, identityPhotoIds: photos, identityItemIndices: items, identityPagePaths: pages })
        : await native<WorkspaceView>('import_context', { revision: workspace.project.revision, identityAnalysisId: analysisId, identityFindingIndices: findings, identityAssetIds: assets, identityPhotoIds: photos, identityItemIndices: items, identityPagePaths: pages, attachMatchingPages: attachMatching });
      accept(next);
      setNotice('Context added to the project brief.');
      // Pages generated before this import don't follow it yet: offer to rebuild them.
      if (next.project?.pages.some(item => item.content)) setOfferRestyle('Existing pages need restyling.');
    });
  }
  // Opening, creating or recovering adds a project tab group; the current project stays open.
  async function openAlongside(run: Replacement['run']) { await synchronize(); await settle(); await run(false); }
  async function switchProject(projectId: string, pageId?: string) {
    await synchronize(); await settle();
    await queue(async () => {
      const next = await native<WorkspaceView>('switch_project', { projectId });
      accept(next); setSelected(null);
      if (pageId) setPageId(pageId);
      if (next.project) setZoom(initialZoom(next.project.viewport.width, next.project.viewport.height));
    });
  }
  // One page-wide request per generated page, then Generate: the AI restyles each page to the
  // newly imported brand while keeping its content and layout.
  // Files and links dropped on a request travel with it to Generate.
  const [archiveReports, setArchiveReports] = useState<Record<string, import('./types').ArchiveReport[]>>({});
  async function attachToRequest(regionId: string, files: File[], links: string[]) {
    const owner = current.current?.project?.pages.find(item => item.regions.some(region => region.id === regionId));
    if (!owner) return;
    const readText = (file: File) => file.text();
    const readImage = (file: File) => new Promise<string>((resolve, reject) => { const reader = new FileReader(); reader.onload = () => resolve(String(reader.result)); reader.onerror = () => reject(new Error(`${file.name} could not be read.`)); reader.readAsDataURL(file); });
    const problems: string[] = [];
    for (const file of files) {
      try {
        if (/\.zip$/i.test(file.name)) {
          if (file.size > 32 * 1024 * 1024) throw new Error(`${file.name}: exceeds the 32 MiB archive limit.`);
          const count = current.current?.project?.pages.find(item => item.id === owner.id)?.regions.find(item => item.id === regionId)?.attachments?.length ?? 0;
          const data = (await readImage(file)).split(',')[1];
          const unpacked = await native<{ attachments: RequestAttachment[]; skipped: { name: string; reason: string }[] }>('unpack_request_zip', { name: file.name, data, remaining: Math.max(0, 16 - count) });
          let added = 0;
          for (const attachment of unpacked.attachments) {
            try { await mutate({ type: 'add_region_attachment', page_id: owner.id, region_id: regionId, attachment }); added++; }
            catch (reason) { unpacked.skipped.push({ name: attachment.name, reason: errorMessage(reason) }); }
          }
          const key = `${current.current?.project?.id}:${regionId}`;
          setArchiveReports(previous => ({ ...previous, [key]: [...(previous[key] ?? []), { name: file.name, added, skipped: unpacked.skipped }] }));
          continue;
        }
        let attachment: RequestAttachment;
        if (/^image\/(png|jpeg|webp|gif)$/.test(file.type)) {
          attachment = { id: 'new', name: file.name.slice(0, 120), kind: 'image', dataUri: await readImage(file) };
        } else if (file.type.startsWith('text/') || /\.(txt|md|csv|json|html?)$/i.test(file.name)) {
          const text = await readText(file);
          attachment = { id: 'new', name: file.name.slice(0, 120), kind: 'text', text };
        } else throw new Error(`${file.name}: only pictures (PNG, JPEG, WebP, GIF), text files, and ZIPs can be attached.`);
        await mutate({ type: 'add_region_attachment', page_id: owner.id, region_id: regionId, attachment });
      } catch (reason) { problems.push(errorMessage(reason)); }
    }
    for (const url of links) {
      try { await mutate({ type: 'add_region_attachment', page_id: owner.id, region_id: regionId, attachment: { id: 'new', name: new URL(url).hostname.slice(0, 120), kind: 'link', url } }); }
      catch (reason) { problems.push(errorMessage(reason)); }
    }
    if (problems.length) setError(problems.join(' ')); else setNotice('Attached to the request.');
  }
  // One switch for light/dark: the editor and every page of the open project.
  async function switchAppearance(next: 'light' | 'dark') {
    setTheme(next);
    const target = current.current?.project; if (!target) return;
    for (const item of target.pages.filter(entry => entry.appearance !== next)) {
      await mutate(project => { const page = project.pages.find(entry => entry.id === item.id); return page && page.appearance !== next ? { type: 'update_page', page_id: page.id, name: page.name, path: page.path, viewport: page.viewport, appearance: next, included: page.included } : null; });
    }
    if (target.pages.some(entry => entry.content && entry.appearance !== next)) setOfferRestyle(`Existing pages need restyling for ${next} mode.`);
  }
  async function regenerateWithContext() {
    await synchronize(true); await settle();
    setOfferRestyle(false);
    const target = current.current?.project; if (!target) return;
    for (const item of target.pages.filter(entry => entry.content)) {
      const before = new Set((current.current?.project?.pages.find(entry => entry.id === item.id)?.regions ?? []).map(region => region.id));
      const next = await mutate({ type: 'add_region', page_id: item.id, rect: { x: 0, y: 0, width: item.viewport.width, height: Math.min(item.viewport.height, 160) } });
      const added = next.project!.pages.find(entry => entry.id === item.id)!.regions.find(region => !before.has(region.id));
      if (added) await changeRegion(item.id, added.id, { prompt: `Update this page to follow the project brand rules (colours, fonts, tone and logo) and its ${item.appearance} appearance. Keep its content, structure and layout the same.` });
    }
    generate();
  }
  async function open(folder = false, path?: string, discardRecovery = false) {
    await openAlongside(async discard => {
      await queue(async () => {
        const opened = await requestProjectOpen(() => native<WorkspaceView | { website: WebsiteFolderSelection } | null>('open_project', { discard, folder, path: path ?? null, discardRecovery }));
        if (opened && 'website' in opened) { setImportFolder(opened.website); setImportAsNew(false); setShowNew(false); setShowImport(true); return; }
        if (opened) { accept(opened); setSelected(null); setNotice('Project opened.'); setShowNew(false); void refreshRecent().catch(() => {}); if (opened.project) setZoom(initialZoom(opened.project.viewport.width, opened.project.viewport.height)); }
      });
    });
  }
  async function openRecent(path: string) {
    await openAlongside(async discard => {
      await queue(async () => {
        const opened = await requestProjectOpen(() => native<WorkspaceView>('open_recent_project', { path, discard }));
        if (!opened) return;
        accept(opened); setSelected(null); setShowNew(false); setNotice('Project opened.');
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
  async function create(name: string, viewport: Viewport, parentDirectory?: string) {
    await openAlongside(async discard => {
      await queue(async () => {
        accept(await native<WorkspaceView>('create_project', { name, viewport, parentDirectory: parentDirectory ?? null, appearance: resolvedTheme, discard }));
        setSelected(null); setShowNew(false); setNotice('Blank project created.');
        setZoom(initialZoom(viewport.width, viewport.height));
        void refreshRecent().catch(() => {});
      });
    });
  }
  async function recover(projectId: string) {
    await openAlongside(async discard => {
      await queue(async () => {
        const recovered = await native<WorkspaceView>('recover_project', { projectId, discard });
        accept(recovered);
        if (recovered.project) setZoom(initialZoom(recovered.project.viewport.width, recovered.project.viewport.height));
        setSelected(null); setShowRecovery(false); setShowNew(false); setPendingRecovery(null);
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
      const shortcut = editorShortcut(event, editingText);
      if (!shortcut) return;
      // Side panels keep focus while editing, so app-wide commands still work there.
      if (event.target instanceof Element && event.target.closest('.workspace-surface') && !['save', 'saveAs', 'open', 'new', 'undo', 'redo', 'zoomIn', 'zoomOut', 'fit', 'actualSize', 'doubleSize', 'nextPage', 'previousPage'].includes(shortcut)) return;
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
      else if (shortcut === 'rectangleTool' || shortcut === 'circleTool' || shortcut === 'freehandTool') { event.preventDefault(); setRequestShape(shortcut === 'rectangleTool' ? 'rect' : shortcut === 'circleTool' ? 'ellipse' : 'freehand'); setTool('request'); return; }
      else if (shortcut === 'selectTool' || shortcut === 'requestTool' || shortcut === 'panTool') { event.preventDefault(); setTool(shortcut === 'selectTool' ? 'select' : shortcut === 'requestTool' ? 'request' : 'pan'); return; }
      else if (shortcut === 'cutLayer') { if (activeLayer || selected) { event.preventDefault(); void canvas.current?.cutLayer(); } return; }
      else if (shortcut === 'copyLayer') { if (window.getSelection()?.toString().trim()) return; if (activeLayer || selected) { event.preventDefault(); void canvas.current?.copyLayer(); } return; }
      else if (shortcut === 'pasteLayer') { event.preventDefault(); void canvas.current?.pasteLayer(); return; }
      else if (shortcut === 'layerForward' || shortcut === 'layerBackward' || shortcut === 'layerFront' || shortcut === 'layerBack') {
        if (activeLayer || selected) { event.preventDefault(); void canvas.current?.arrangeLayer(({ layerForward: 'forward', layerBackward: 'backward', layerFront: 'front', layerBack: 'back' } as const)[shortcut]); } return;
      }
      else if (shortcut === 'askAI') { event.preventDefault(); canvas.current?.askAI(); return; }
      else if (shortcut === 'undoElement') { event.preventDefault(); void canvas.current?.undoElement(); return; }
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
  const importedImages = [...(project?.importedIdentity?.assets ?? []), ...(project?.importedIdentity?.photos ?? [])];
  const storedImages = Object.entries(project?.pictures ?? {}).map(([id, dataUri], index) => ({ id, dataUri,
    name: importedImages.find(image => image.dataUri === dataUri)?.name ?? `Picture ${index + 1}` }));
  const projectImages = [...storedImages, ...importedImages.filter(image => !storedImages.some(stored => stored.dataUri === image.dataUri))];
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
  const generationCall = useCallback(<T,>(command: string, args?: Record<string, unknown>, signal?: AbortSignal) => Promise.resolve(command === 'prepare_generation' || command === 'start_generation' || command === 'retry_generation' || command === 'apply_generation' || command === 'generation_draft' && args?.action === 'apply' ? synchronize(['prepare_generation', 'start_generation', 'retry_generation'].includes(command)) : undefined).then(() => queue(async workspace => {
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
    if (command === 'generation_draft' && (result as import('./types').DraftReview).workspace) accept((result as import('./types').DraftReview).workspace!);
    return result;
  })), [queue, activeProjectId, accept, refreshAiSettings, synchronize]);
  const generationApplied = useCallback((workspace: WorkspaceView) => { setSelected(null); setFocusPrompt(false); setRetryResult(false); if (phoneFix.current) { phoneFix.current = false; setLiveDevice('phone'); setLive(true); } if (workspace.project?.latestGeneration) { setResultReview({ jobId: workspace.project.latestGeneration.jobId, revision: workspace.project.revision }); setHighlightResult(true); } }, []);
  const generationRecorded = useCallback(() => queue(async () => {
    const refreshed = await native<WorkspaceView>('get_workspace');
    if (refreshed.project?.id === activeProjectId) accept(refreshed);
  }), [queue, accept, activeProjectId]);
  const generation = useGeneration(activeProjectId, generationCall, generationApplied, generationRecorded);
  generationRef.current = generation.job;
  useEffect(() => { setLive(false); setShowLog(false); setOfferRestyle(false); setResultReview(null); setExported(null); setTool('select'); setCodeView(activeProjectId ? readCodeView(activeProjectId) : 'design'); }, [activeProjectId]);
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
    if (undone && tryAgain) {
      const repairs = generation.job?.areas.filter(area => project?.pages.find(page => page.id === area.pageId)?.regions.some(region => region.id === area.regionId && region.runtimeErrors?.length)).map(area => area.regionId);
      await generation.prepare(undefined, repairs?.length ? repairs : undefined);
    }
  }
  const working = generation.canStop || activeGeneration(generation.job) || !!generation.operation;
  useEffect(() => {
    // Escape leaves a text box once fields have had their own Escape handling.
    const leave = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || event.defaultPrevented || !isTextField(event.target)) return;
      event.preventDefault();
      (document.activeElement as HTMLElement | null)?.blur();
    };
    window.addEventListener('keydown', leave);
    return () => window.removeEventListener('keydown', leave);
  }, []);
  useEffect(() => { setRetryResult(false); }, [resultReview?.jobId]);
  useEffect(() => { if (generation.visible) { setShowLog(true); generation.close(); } }, [generation.visible]);
  const draftCount = pendingDrafts(generation.job).length;
  const showGenerationStatus = generation.stopped || working || !!generation.error || !!generation.job && (['prepared', 'blocked', 'failed', 'unknown', 'cancelled'].includes(generation.job.status) || generation.job.candidate?.regions.some(item => item.status === 'conflict'));
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
  function generate() {
    phoneFix.current = false;
    if (!requestedAreas && pendingImages) {
      void synchronize().then(() => setNotice('Pictures resized and updated.')).catch(() => {});
      return;
    }
    setShowLog(true);
    void generation.prepare().catch(() => {});
  }

  const [observationsVersion, setObservationsVersion] = useState(0);
  async function fixRuntimeErrors(errors: string[]) {
    if (!page?.content || working || !aiConnected) return;
    const pageId = page.id;
    await synchronize(true); await settle();
    const next = await mutate(current => {
      const target = current.pages.find(item => item.id === pageId);
      if (!target?.content) throw new Error('This page is no longer available.');
      return { type: 'add_region', page_id: pageId, rect: { x: 0, y: 0, width: target.viewport.width, height: target.viewport.height } };
    });
    const added = next.project!.pages.find(item => item.id === pageId)!.regions.at(-1)!;
    await mutate({ type: 'set_region_runtime_errors', page_id: pageId, region_id: added.id, errors: errors.slice(0, 10).map(error => error.slice(0, 4096)) });
    setSelected(null); setShowLog(true);
    await generation.prepare(undefined, [added.id]);
  }
  function keepResult() { setRetryResult(false); setObservationsVersion(value => value + 1); setResultReview(null); }

  function dismissEmpty(keep: string | null) {
    return mutate(project => project.pages.some(page => page.regions.some(region => !region.completed && region.id !== keep && !region.prompt.trim()))
      ? { type: 'dismiss_empty_regions', keep_region_id: keep } : null);
  }
  function selectRequest(id: string | null, cleanEmpty = true) {
    setSelected(id); setFocusPrompt(false);
    if (cleanEmpty) void dismissEmpty(id).catch(() => {});
  }
  async function selectPage(id: string, keep: string | null = null) { await synchronize(); await settle(); await dismissEmpty(keep); setPageId(id); setSelected(null); }
  async function addRegion(rect: Rect, shape?: RegionShape) {
    if (!page) return;
    const id = page.id;
    const next = await mutate(shape && shape.kind !== 'rect' ? { type: 'add_shaped_region', page_id: id, rect, shape } : { type: 'add_region', page_id: id, rect });
    setSelected(next.project!.pages.find(page => page.id === id)!.regions.at(-1)!.id); setFocusPrompt(true);
  }
  async function pickElement(selections: ElementSelection[]) {
    if (!page || !selections.length) return;
    const pageId = page.id, elements = selections.map(selection => selection.element);
    await synchronize(true); await settle();
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
  // Fix on phones: one phone-only request (a section, or the whole page), generated right away.
  // The native scope check only lets small-screen CSS change, so the desktop design stays identical.
  const phoneFix = useRef(false);
  async function fixOnPhones(selection?: ElementSelection) {
    if (!page?.content) return;
    const pageId = page.id;
    const others = page.regions.filter(region => !region.completed && region.prompt.trim() && !region.mobileOnly);
    if (others.length) { setError('Phone fixes run on their own. Generate or remove the other requests on this page first.'); return; }
    const before = new Set(page.regions.map(region => region.id));
    const next = await mutate(selection
      ? { type: 'add_element_regions', page_id: pageId, elements: [selection.element], bounds: [{ target: selection.element, rect: selection.rect }] }
      : { type: 'add_region', page_id: pageId, rect: { x: 0, y: 0, width: page.viewport.width, height: Math.min(page.viewport.height, 160) } });
    const added = next.project!.pages.find(item => item.id === pageId)!.regions.find(region => !before.has(region.id));
    if (!added) return;
    await changeRegion(pageId, added.id, { prompt: selection ? 'Make this section look good and easy to use on phones.' : 'Make this whole page look good and easy to use on phones.' });
    await mutate({ type: 'set_region_mobile_only', page_id: pageId, region_id: added.id, mobile_only: true });
    setSelected(null);
    generate(); phoneFix.current = true;
  }
  async function deleteElements(elements: ElementTarget[]) {
    if (!page) return;
    await mutate({ type: 'delete_elements', page_id: page.id, elements });
    setNotice('Items deleted.');
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
  async function undoElementNative(target: ElementTarget): Promise<ElementTarget | null> {
    if (!page || !project) throw new Error('Select an element first.');
    const projectId = project.id; const pageId = page.id; setError('');
    await synchronize(); await settle();
    return queue(async workspace => {
      if (workspace?.project?.id !== projectId) throw new Error('Return to the project that owns this element.');
      const result = await native<ElementEditView>('undo_element', { projectId, pageId, revision: workspace.project.revision, target });
      accept(result.workspace); setNotice('Element stepped back to its previous version.'); return result.element;
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
      if (patch.wholePage !== undefined) return { type: 'set_region_whole_page', page_id: targetPageId, region_id: regionId, whole_page: patch.wholePage };
      const rect = typeof patch.rect === 'function' ? patch.rect(area.rect) : { ...area.rect, ...patch.rect };
      const prompt = (typeof patch.prompt === 'function' ? patch.prompt() : patch.prompt) ?? area.prompt;
      if (rect.x === area.rect.x && rect.y === area.rect.y && rect.width === area.rect.width && rect.height === area.rect.height && prompt === area.prompt) return null;
      return { type: 'update_region', page_id: targetPageId, region_id: regionId, rect, prompt };
    }, typeof patch.prompt === 'function').then(next => next.project!.pages.find(page => page.id === targetPageId)!.regions.find(region => region.id === regionId)!);
  }
  function removeRegion(id = region?.id) {
    if (!page || !id) return;
    const page_id = page.id; const region_id = id;
    const action = async () => { await mutate({ type: 'remove_region', page_id, region_id }); setSelected(null); requestAnimationFrame(() => { if (!document.querySelector('.workspace-surface:focus-within')) document.getElementById('page-canvas')?.focus(); }); };
    if (!askBeforeDelete()) { run(action); return; }
    setSkipNextAsk(false);
    setConfirmation({ title: 'Remove request?', message: 'Remove this request? Remove any linked choices or exceptions from the project brief first.', action, skippable: true });
  }
  async function removePage() {
    if (!page) return;
    await synchronize();
    const page_id = page.id;
    setConfirmation({ title: 'Remove page?', message: `Remove “${page.name}” and its ${page.regions.length} requests? Remove any linked choices or exceptions from the project brief first.`,
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
    setSelected(added.id); setNotice('Request added.');
  }

  // Keep this menu short: only what someone building a website reaches for.
  // Canvas-level actions (copy, arrange, zoom) live on the canvas and its right-click menu.
  const menuItems: MenuItem[] = [
    { id: 'new', label: 'New project', shortcut: 'Ctrl/Cmd+N', action: () => setShowNew(true) },
    { id: 'open', label: 'Open project', shortcut: 'Ctrl/Cmd+O', action: () => run(open) },
    { id: 'open-folder', label: 'Open project folder…', action: () => run(() => open(true)) },
    { id: 'save', label: 'Save', shortcut: 'Ctrl/Cmd+S', disabled: !project, action: () => run(() => save()) },
    { id: 'close-project', label: 'Close project', disabled: !project, action: () => run(closeProject) },
    { id: 'save-as', label: 'Save as', shortcut: 'Ctrl/Cmd+Shift+S', disabled: !project, action: () => run(() => save(true)) },
    { id: 'move-project', label: 'Move into project folder', disabled: !project || !!view?.projectFolder, action: () => run(async () => {
      await synchronize(); await settle();
      await queue(async () => { accept(await native<WorkspaceView>('move_into_project_folder')); setNotice('Project moved into its folder. The original archive was kept as a backup.'); void refreshRecent().catch(() => {}); });
    }) },
    ...(view?.recoveries.length ? [{ id: 'recover', label: 'Recover unsaved projects', action: () => setShowRecovery(true) }] : []),
    { id: 'file-divider', label: '', separator: true },
    { id: 'add-page', label: 'Add page', disabled: !project || project.pages.length >= 100, action: () => setPageDialog('new') },
    { id: 'page-settings', label: 'Page settings', disabled: !page, action: () => setPageDialog(page) },
    { id: 'remove-page', label: 'Remove page', disabled: !page || page.id === project?.homePageId, action: () => run(removePage) },
    { id: 'page-divider', label: '', separator: true },
    { id: 'live', label: 'Live view', checked: live, disabled: !page?.content, action: () => { setLive(value => !value); setLiveDevice('desktop'); } },
    { id: 'code-view', label: 'View', disabled: !project, children: (['design', 'split', 'code'] as CodeView[]).map(value => ({ id: `view-${value}`, label: value === 'design' ? 'Design' : value === 'split' ? 'Split (design + code)' : 'Code', checked: codeView === value, action: () => run(() => changeCodeView(value)) })) },
    { id: 'export', label: 'Export website…', disabled: exporting || !project?.pages.some(page => page.content), action: () => run(exportWebsite) },
    { id: 'publish', label: 'Publish online (coming soon)', disabled: true },
    { id: 'ai-divider', label: '', separator: true },
    { id: 'settings', label: 'AI settings', action: () => setShowAiSettings(true) },
    { id: 'mode', label: 'AI style', disabled: !project || working, children: (['fuzzy', 'strict'] as GenerationMode[]).map(mode => ({ id: mode, label: mode === 'fuzzy' ? 'Flexible' : 'Precise', checked: project?.generationMode === mode, action: () => run(() => mutate(current => current.generationMode === mode ? null : { type: 'set_generation_mode', mode })) })) },
    { id: 'page-check', label: fixCount ? `Check page (${fixCount})` : 'Check page', disabled: !project?.pages.some(item => item.content), action: () => setShowCheck(true) },
    { id: 'style-guide', label: 'Style guide', disabled: !project, action: () => setShowStyleGuide(true) },
    { id: 'import', label: 'Import…', disabled: !project, action: () => { setImportFolder(null); setImportAsNew(false); setShowImport(true); } },
    { id: 'dossier', label: 'Project brief', disabled: !project, action: () => setShowNotes('overview') },
    { id: 'help-divider', label: '', separator: true },
    { id: 'appearance', label: 'Editor theme', children: (['system', 'light', 'dark'] as Theme[]).map(value => ({ id: value, label: value === 'system' ? 'System' : value === 'light' ? 'Light' : 'Dark', checked: theme === value, action: () => setTheme(value) })) },
    { id: 'project-location', label: 'Default project location', action: () => setShowProjectLocation(true) },
    { id: 'ask-delete', label: 'Ask before deleting', checked: askDelete, action: () => setAskBeforeDelete(!askDelete) },
    { id: 'help', label: 'Keyboard shortcuts', shortcut: 'F1', action: () => setShowShortcuts(true) },
  ];

  const problemAction = needsAiSettings(error) ? { label: 'AI settings', onClick: () => setShowAiSettings(true) }
    : project && /permission denied|not writable|read.only|access denied|can't save here/i.test(error) ? { label: 'Choose another location', disabled: pending > 0, onClick: () => run(() => save(true)) } : undefined;
  return <div className="application editor-shell">
    {closeRequest && <WorkspaceSurface place="banner" title="Close Desegnos?" close={() => { closeRequest.resolve(false); setCloseRequest(null); }}><p>{closeRequest.message}</p><div className="form-actions"><button onClick={() => { closeRequest.resolve(true); setCloseRequest(null); }}>Close Desegnos</button><button onClick={() => { closeRequest.resolve(false); setCloseRequest(null); }}>Keep editing</button></div></WorkspaceSurface>}
    <header className="app-bar"><div className="wordmark" aria-label="Desegnos"><img className="brand-light" src={brandLight} alt="" /><img className="brand-dark" src={brandDark} alt="" /><span className="sr-only">Desegnos</span></div>
      {project && <button className="project-title" onClick={() => setShowRename(true)} title="Rename project"><span>{project.name}</span>{view?.dirty && <span aria-label="Unsaved changes" className="dirty-dot">●</span>}</button>}
      <MenuButton label="Menu" ariaLabel="Editor menu" items={menuItems} />
      {project && <div className="view-switch segmented" role="group" aria-label="Editor view">{(['design', 'split', 'code'] as CodeView[]).map(value => <button key={value} type="button" aria-pressed={codeView === value} onClick={() => run(() => changeCodeView(value))}>{value === 'design' ? 'Design' : value === 'split' ? 'Split' : 'Code'}</button>)}</div>}
      <div className="shell-primary-actions">
        <button type="button" className="theme-toggle" aria-label={resolvedTheme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'} title={project ? (resolvedTheme === 'dark' ? 'Light mode for the editor and this site' : 'Dark mode for the editor and this site') : (resolvedTheme === 'dark' ? 'Light mode' : 'Dark mode')} onClick={() => run(() => switchAppearance(resolvedTheme === 'dark' ? 'light' : 'dark'))}>{resolvedTheme === 'dark' ? <svg viewBox="0 0 20 20" aria-hidden="true"><circle cx="10" cy="10" r="3.6" /><path d="M10 2v2M10 16v2M2 10h2M16 10h2M4.3 4.3l1.4 1.4M14.3 14.3l1.4 1.4M4.3 15.7l1.4-1.4M14.3 5.7l1.4-1.4" /></svg> : <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M15.5 12.5A6.5 6.5 0 0 1 7.5 4.5a6.5 6.5 0 1 0 8 8Z" /></svg>}</button>
        {project && <button type="button" onClick={() => { setImportFolder(null); setImportAsNew(false); setShowImport(true); }}>Import…</button>}
        {project && <><button aria-label="Undo last change" title="Undo · Ctrl/Cmd+Z" disabled={!view?.canUndo} onClick={() => run(() => travel('undo_project'))}>Undo</button><button aria-label="Redo last change" title="Redo · Ctrl/Cmd+Shift+Z" disabled={!view?.canRedo} onClick={() => run(() => travel('redo_project'))}>Redo</button>
          <MenuButton label="History" className="history-menu" ariaLabel="Change history" items={history.entries.length > 1 ? history.entries.map(entry => ({ id: `history-${entry.position}`, label: `${entry.label}${entry.position > (view?.history?.position ?? 0) ? ' (undone)' : ''}`, shortcut: new Date(entry.at).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' }), checked: entry.position === view?.history?.position, disabled: pending > 0 || working, action: () => run(() => jumpTo(entry.position)) })).reverse() : [{ id: 'history-empty', label: 'No changes yet', disabled: true }]} /></>}
        {page?.content && <button type="button" className="live-toggle" aria-pressed={live} title={live ? 'Back to editing' : 'Live view'} onClick={() => { setLive(value => !value); setLiveDevice('desktop'); }}>{live ? 'Live ●' : 'Live'}</button>}
        {project?.pages.some(item => item.content) && <button disabled={exporting} onClick={() => run(exportWebsite)}>{exporting ? 'Exporting…' : 'Export'}</button>}
        {!aiConnected && <button className="ai-shortcut needs-connection" onClick={() => setShowAiSettings(true)} aria-label="AI settings" title="Connect AI to generate">Connect AI</button>}
        {project && (generation.job || generation.operation) && <button type="button" aria-pressed={showLog} onClick={() => setShowLog(value => !value)}>AI log</button>}
        {project && <button className={generation.canStop ? undefined : 'primary'} title={generation.canStop ? 'Stop generation (Esc)' : requestedAreas === 0 && pendingImages === 0 ? 'Describe at least one request first.' : 'Generate the described changes'} disabled={!generation.canStop && ((requestedAreas === 0 && pendingImages === 0) || working)} onClick={generation.canStop ? () => { void generation.cancel(); } : generate}>{generation.canStop ? 'Stop' : 'Generate'}</button>}
      </div>
    </header>
    <SurfaceHost place="banner" />
    <NoticeHost />
    {pendingRecovery && <RecoveryBanner busy={pending > 0} recover={() => run(() => recover(pendingRecovery.projectId))} openSaved={() => run(() => open(false, pendingRecovery.path, true))} dismiss={() => setPendingRecovery(null)} />}
    {error && <ProblemBanner message={userError(error)} dismiss={() => setError('')} action={problemAction} />}
    {notices.map(item => <Notice key={item.id} variant="info" message={item.message} dismiss={() => setNotices(items => items.filter(queued => queued.id !== item.id))} />)}
    {exported && <Notice className="export-notice" priority="result" variant={exported.warnings.length ? 'warning' : 'success'} title="Website exported" message={`${exported.pageCount} ${exported.pageCount === 1 ? 'page' : 'pages'}${exported.warnings.length ? ` · ${[...new Set(exported.warnings)].join(' ')}` : ''}`} action={{ label: 'Open folder', onClick: () => run(() => native('open_export_folder', { path: exported.path })) }} dismiss={() => setExported(null)} />}
    {offerRestyle && project && <Notice variant="info" message={offerRestyle} action={{ label: 'Regenerate pages', disabled: pending > 0 || working, onClick: () => run(regenerateWithContext) }} dismiss={() => setOfferRestyle(false)} />}
    {view?.recoveryError && !dismissedRecovery.includes(`error:${view.recoveryError}`) && <ProblemBanner message={userError(view.recoveryError)} dismiss={() => setDismissedRecovery(items => [...items, `error:${view.recoveryError}`])} />}
    {project?.website?.framework && <p className="field-help" role="note">Edits don't change the {project.website.framework} source files.</p>}
    {!!project?.website?.skipped.length && <details className="field-help"><summary>Skipped {project.website.skipped.length} files</summary><p>{project.website.skipped.slice(0, 5).join(", ")}</p></details>}
    {view?.migrationNotice && dismissedMigration !== `${project?.id}:${view.migrationNotice}` && <Notice variant="warning" message={view.migrationNotice} dismiss={() => setDismissedMigration(`${project?.id}:${view.migrationNotice}`)} />}
    {loading ? <main className="startup"><p role="status">Opening your workspace…</p></main> : !view ? <main className="startup"><h1>Open the Desegnos desktop application</h1><button onClick={() => { void load(); }}>Retry workspace connection</button></main> : !project ? <main className="startup">
      <SurfaceHost place="start" />
      {!showNew && <NewProject importBrand={startBrandImport} busy={pending > 0} create={create} open={open} openFolder={() => open(true)} location={projectLocation} changeLocation={changeProjectLocation} settings={aiSettings} openSettings={() => setShowAiSettings(true)} recentProjects={recentProjects} openRecent={openRecent} forgetRecent={forgetRecent} />}
    </main> : <>
      <div className="page-strip"><div className="page-tabs" role="tablist" aria-label="Project pages">
        {(view.openProjects?.length ? view.openProjects : [{ id: project.id, name: project.name, dirty: !!view.dirty, active: true, pages: [] }]).map((open, groupIndex, groups) => <div key={open.id} data-project-id={open.id} className={`project-tab-group${open.active ? ' active' : ''}${groups.length > 1 ? ' labelled' : ''}`} style={{ '--project-colour': PROJECT_COLOURS[groupIndex % PROJECT_COLOURS.length] } as CSSProperties} role="group" aria-label={`Project ${open.name}`}>
          {groups.length > 1 && <span className="project-tab-label" title={open.name}>{open.name}{open.dirty ? ' •' : ''}</span>}
          <div className="project-tab-row">{open.active ? <>
        {project.pages.map((item, index) => <button key={item.id} id={`page-tab-${item.id}`} role="tab" aria-selected={page?.id === item.id} aria-controls="page-panel" tabIndex={page?.id === item.id ? 0 : -1}
          title={`${item.path}${item.included ? '' : ' · Hidden page'}`} onClick={() => run(() => selectPage(item.id))}
          onKeyDown={event => {
            if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
            event.preventDefault();
            const next = event.key === 'Home' ? 0 : event.key === 'End' ? project.pages.length - 1 : (index + (event.key === 'ArrowRight' ? 1 : -1) + project.pages.length) % project.pages.length;
            document.getElementById(`page-tab-${project.pages[next].id}`)?.focus(); run(() => selectPage(project.pages[next].id));
          }}><span>{item.name}</span>{!item.included && <span className="page-draft">Hidden</span>}</button>)}
          <button className="add-page-tab" aria-label="Add page" title="Add page" disabled={project.pages.length >= 100} onClick={() => setPageDialog('new')}>+</button></> : open.pages.map(item => <button key={item.id} type="button" className="parked-tab" title={`Switch to ${open.name}`} onClick={() => run(() => switchProject(open.id, item.id))}><span>{item.name}</span></button>)}</div>
        </div>)}
      </div></div>
      <main className={`workspace view-${codeView}`} id="page-panel" role="tabpanel" aria-labelledby={page ? `page-tab-${page.id}` : undefined} style={{ '--code-width': `${codeWidth * 100}%` } as CSSProperties}>
      <div className="design-pane">
      <SurfaceHost place="canvas" />
      {page && <Canvas generation={generation} fixRuntimeErrors={errors => run(() => fixRuntimeErrors(errors))} aiDisabled={working || !aiConnected || pending > 0} observationsVersion={observationsVersion} archiveReports={archiveReports} projectId={project.id} pendingImagesChanged={count => { setPendingImages(count); if (count) setNotice('Picture ready. Click Generate to resize and apply.'); }} projectImages={projectImages} ref={canvas} canUndo={!!view?.canUndo} canRedo={!!view?.canRedo} undo={() => run(() => travel('undo_project'))} redo={() => run(() => travel('redo_project'))} toggleGrid={() => run(() => mutate(current => ({ type: 'set_grid', enabled: !current.showGrid })))} configureGrid={() => setShowGridSettings(true)} page={page} layersOpen={showLayers} setLayersOpen={setShowLayers} selectionChanged={setActiveLayer} multiChanged={setMultiCount} liveView={live && page?.content ? (liveDevice === 'desktop' ? page.viewport : liveDevice === 'tablet' ? { width: 768, height: 1024 } : { width: 390, height: 844 }) : null} repick={repickRequest} attach={(regionId, files, links) => run(() => attachToRequest(regionId, files, links))} detach={(regionId, attachmentId) => { const owner = project.pages.find(item => item.regions.some(region => region.id === regionId)); if (owner) run(() => mutate({ type: 'remove_region_attachment', page_id: owner.id, region_id: regionId, attachment_id: attachmentId })); }} fixOnPhones={selection => run(() => fixOnPhones(selection))} arrangeRequest={arrangeRequest} copyNative={copyLayerNative} cutNative={cutLayerNative} pasteNative={pasteLayerNative} tool={tool} requestShape={requestShape} rulerUnit={rulerUnit} highlighted={highlightResult ? generation.job?.areas.filter(area => area.pageId === page.id) ?? [] : []} grid={project.showGrid} gridSettings={project.gridSettings} viewAction={canvasView} changing={working && generation.job && ['prepared', 'generating', 'ready', 'applying'].includes(generation.job.status) ? generation.job?.areas?.filter(area => area.pageId === page.id) ?? [] : []} zoom={zoom} refreshElementBounds={refreshElementBounds} openSource={target => openFoundSource(page.id, target)} inspectElement={inspectElement} editElement={editElement} undoElement={undoElementNative} changeZoom={setZoom} selected={selected} focusRequest={focusPrompt ? selected : null} select={selectRequest} add={(rect, shape) => { void addRegion(rect, shape).catch(() => {}); }} setTargetRemoval={(regionId, allowed) => run(() => mutate({ type: 'set_region_target_removal', page_id: page.id, region_id: regionId, allow_target_removal: allowed }))} deleteElements={deleteElements} pickElement={selection => { void pickElement(selection).catch(() => {}); }} change={(id, patch) => changeRegion(page.id, id, patch)} remove={id => removeRegion(id)} />}
      {resultReview && !showGenerationStatus ? <Notice className="generation-notice status-applied" variant={retryResult ? 'warning' : 'success'} priority="result" title={retryResult ? 'Try again?' : 'Page updated'}
        message={retryResult ? 'This uses your AI account again.' : summarizeOutsideChanges(generation.job?.validationEvents ?? [])}
        action={retryResult ? { label: 'Try again', disabled: pending > 0, onClick: () => { setRetryResult(false); run(() => rejectResult(true)); } } : { label: 'Undo', disabled: pending > 0, onClick: () => run(() => rejectResult()) }}
        secondaryAction={retryResult ? { label: 'Cancel', onClick: () => setRetryResult(false) } : { label: 'Details', onClick: openGeneration }}
        dismiss={keepResult} /> : showGenerationStatus ? <GenerationFeedback controller={generation} openSettings={() => setShowAiSettings(true)} details={openGeneration} /> : draftCount > 0 ? <Notice priority="result" message={`${draftCount} ${draftCount === 1 ? 'draft' : 'drafts'}`} action={{ label: 'Details', onClick: openGeneration }} /> : null}
      <FirstRunTips />
      {live && page?.content && <div className="live-bar" role="group" aria-label="Live view">
        <strong>Live</strong>
        <div className="segmented" role="group" aria-label="Live device">{(['desktop', 'tablet', 'phone'] as const).map(device => <button key={device} type="button" aria-pressed={liveDevice === device} onClick={() => setLiveDevice(device)}>{device === 'desktop' ? (page.viewport.width < 600 ? 'Page width' : 'Desktop') : device === 'tablet' ? 'Tablet' : 'Phone'}</button>)}</div>
        {liveDevice !== 'desktop' && (liveDevice === 'tablet' ? 768 : 390) !== page.viewport.width && <span className="live-note">Add requests at page width</span>}
        {liveDevice === 'phone' && <button type="button" title="Fix on phones" onClick={() => run(() => fixOnPhones())}>Fix on phones</button>}
        <button type="button" onClick={() => setLive(false)}>Exit live</button>
      </div>}
      {showCheck && <PageCheckPanel items={checks} busy={pending > 0 || working} close={() => setShowCheck(false)} show={item => run(() => showCheckItem(item))} fix={item => run(() => fixCheckItem(item))} source={item => run(async () => { const found = await showCheckItem(item); if (found) openFoundSource(item.pageId, found.element); })} />}
      <CanvasTools layersOpen={showLayers} toggleLayers={() => setShowLayers(value => !value)} tool={tool} setTool={setTool} requestShape={requestShape} setRequestShape={setRequestShape} zoom={zoom} setZoom={setZoom} fit={() => recenter('fit')} />
      </div>
      {codeView !== 'design' && <>
        {codeView === 'split' && <div className="code-divider" role="separator" aria-orientation="vertical" aria-label="Resize code pane" onPointerDown={dragDivider} />}
        <CodePane ref={code} saveProject={() => run(() => save())} tidy={page?.content ? () => run(async () => { await synchronize(); await mutate({ type: 'format_page_code', page_id: page.id }); setNotice('Code tidied. Undo puts it back.'); }) : undefined} key={page?.id} content={page?.content ?? null} target={activeLayer} map={sourceMap} save={page?.content ? saveSource : undefined} pick={(selector, id) => { canvas.current?.focusElement(selector, selector.split(' > ').at(-1)!.split(':')[0], id, page?.id); }} />
      </>}
      {showLog && project && <GenerationLog key={`${project.id}:${logFocus}`} controller={generation} retryResult={resultReview ? () => { setRetryResult(true); setShowLog(false); } : undefined} job={generation.job} operation={generation.operation} error={generation.error} project={project} settings={aiSettings} openSettings={() => setShowAiSettings(true)} manageUsage={manageChatgptUsage} editRequest={id => {
        const owner = project.pages.find(item => item.regions.some(region => region.id === id));
        if (owner) run(async () => { await selectPage(owner.id, id); setSelected(id); setFocusPrompt(true); });
      }} close={() => setShowLog(false)} />}
      <SurfaceHost place="panel" />
      </main>
      <footer className="status-bar"><span title={view.path ?? undefined}>{multiCount > 1 ? `${multiCount} elements selected` : region ? 'Request selected' : tool === 'request' ? requestShape === 'freehand' ? 'Draw around an area; release to close' : requestShape === 'ellipse' ? 'Drag an ellipse · Shift for a circle' : 'Click or drag to describe an area' : tool === 'pan' ? 'Drag to move around the canvas' : 'Double-click text to edit · Ask AI for changes'}</span><div className="canvas-status-actions">{fixCount > 0 && <button className="check-chip" aria-label="Open page check" onClick={() => setShowCheck(value => !value)}>{placeholderCount > 0 ? `${placeholderCount} placeholder${placeholderCount === 1 ? '' : 's'}` : `${fixCount} to check`}</button>}<button onClick={() => setShowGridSettings(true)} aria-label="Grid and rulers">{project.gridSettings.spacing}{project.gridSettings.unit === 'percent' ? '%' : ' px'} grid{project.gridSettings.snap ? ' · Snap' : ''}</button><span role="status" aria-live="polite">{pending > 0 ? 'Saving locally…' : view.dirty ? 'Unsaved' : 'Saved'}</span></div></footer>
    </>}
    {showShortcuts && <WorkspaceSurface place={project ? 'panel' : 'start'} title="Keyboard shortcuts" close={() => setShowShortcuts(false)}><div className="dialog-body shortcut-help"><dl>{SHORTCUT_HELP.map(([label, keys]) => <div key={label}><dt>{label}</dt><dd>{keys}</dd></div>)}</dl></div></WorkspaceSurface>}
    {showNew && <WorkspaceSurface place={project ? 'panel' : 'start'} title="New project" startHeading={false} close={() => setShowNew(false)}><NewProject importBrand={startBrandImport} busy={pending > 0} create={create} open={open} openFolder={() => open(true)} location={projectLocation} changeLocation={changeProjectLocation} settings={aiSettings} openSettings={() => setShowAiSettings(true)} recentProjects={recentProjects} openRecent={openRecent} forgetRecent={forgetRecent} /></WorkspaceSurface>}
    {!!view?.siteWarnings?.length && dismissedSiteWarnings !== `${project?.id}:${view.siteWarnings.join('\n')}` && <Notice variant="warning" title="Project saved" message={[...new Set(view.siteWarnings)].join(' ')} dismiss={() => setDismissedSiteWarnings(`${project?.id}:${view.siteWarnings?.join('\n')}`)} />}
    {showProjectLocation && <WorkspaceSurface place={project ? 'panel' : 'start'} title="Project location" close={() => setShowProjectLocation(false)}><div className="dialog-body"><ProjectLocationSettings location={projectLocation} change={changeProjectLocation} /></div></WorkspaceSurface>}
    {showRecovery && view && <WorkspaceSurface place={project ? 'panel' : 'start'} title={project ? 'Recover unsaved projects' : 'Unsaved work'} close={() => setShowRecovery(false)}>
      <RecoveryList items={view.recoveries} busy={pending > 0} recover={id => { void recover(id).catch(() => {}); }} dismiss={dismissRecoveries} /></WorkspaceSurface>}
    {showImport && <WorkspaceSurface place={project ? 'panel' : 'start'} title="Import" close={() => setShowImport(false)}><ImportContext key={`${project?.id ?? 'new'}:${importAsNew}`} initialFolder={importFolder} editWebsite={editWebsite} busy={pending > 0} settings={aiSettings} openSettings={() => setShowAiSettings(true)} importInto={importContext} close={() => setShowImport(false)} pages={project && !importAsNew ? { homePageId: project.homePageId, revision: project.revision, beforePick: async () => { await synchronize(); await settle(); }, apply: async (token, remove) => {
      await synchronize(); await settle();
      await queue(async () => { accept(await native<WorkspaceView>('apply_website_import', { token, remove })); });
    } } : undefined} /></WorkspaceSurface>}
    {showStyleGuide && project && <WorkspaceSurface place={project ? 'panel' : 'start'} title="Style guide" close={() => setShowStyleGuide(false)}><StyleGuide project={project} apply={command => mutate(command).then(() => undefined)} openBrief={() => { setShowStyleGuide(false); setShowNotes('decisions'); }} /></WorkspaceSurface>}
    {showNotes && project && <WorkspaceSurface place={project ? 'panel' : 'start'} title="Project brief" close={() => setShowNotes(null)}><Notes project={project} initialSection={showNotes} reviewGeneration={project.latestGeneration?.jobId === generation.job?.jobId ? openGeneration : undefined} apply={command => mutate(command).then(() => undefined)} changeBrief={changeBrief} /></WorkspaceSurface>}
    {showGridSettings && project && <WorkspaceSurface place={project ? 'panel' : 'start'} title="Grid and rulers" close={() => setShowGridSettings(false)}><GridForm rulerUnit={rulerUnit} changeRulerUnit={setRulerUnit} settings={project.gridSettings} showGrid={project.showGrid} busy={pending > 0} submit={async (settings, visible) => {
      if (visible !== project.showGrid) await mutate({ type: 'set_grid', enabled: visible });
      await mutate({ type: 'set_grid_settings', settings }); setShowGridSettings(false);
    }} /></WorkspaceSurface>}
    {pageDialog && project && <WorkspaceSurface place={project ? 'panel' : 'start'} title={pageDialog === 'new' ? 'Add page' : 'Page settings'} close={() => setPageDialog(null)}>
      <PageForm paths={project.pages.map(item => item.path)} page={pageDialog === 'new' ? null : pageDialog} home={pageDialog !== 'new' && pageDialog.id === project.homePageId} busy={pending > 0} submit={async values => {
        if (pageDialog === 'new') {
          await dismissEmpty(null);
          await synchronize();
          const next = await mutate({ type: 'add_page', name: values.name, path: values.path });
          setPageId(next.project!.pages.at(-1)!.id); setSelected(null);
        } else await mutate({ type: 'update_page', page_id: pageDialog.id, ...values });
        setPageDialog(null);
      }} />
    </WorkspaceSurface>}
    {showRename && project && <WorkspaceSurface place={project ? 'panel' : 'start'} title="Rename project" close={() => setShowRename(false)}><RenameProject name={project.name} submit={async name => { await mutate({ type: 'rename_project', name }); setShowRename(false); }} /></WorkspaceSurface>}
    {saveConflict && <SaveConflictDialog conflict={saveConflict} choose={chooseSaveConflict} />}
    {replacement && <WorkspaceSurface place="banner" title="Save your changes?" close={() => setReplacement(null)}><div className="dialog-body"><p>Save “{project?.name}” before you {replacement.label}?</p><p className="muted">Discard continues without saving these changes to the project file.</p><div className="form-actions">
      <button className="primary" disabled={pending > 0} onClick={() => { void (async () => { const saved = await save(); if (saved && !saved.dirty) { const run = replacement.run; setReplacement(null); await run(false); } })().catch(() => {}); }}>Save and continue</button>
      <button className="danger" disabled={pending > 0} onClick={() => { const run = replacement.run; setReplacement(null); void run(true).catch(() => {}); }}>Discard and continue</button><button onClick={() => setReplacement(null)}>Keep editing</button>
    </div></div></WorkspaceSurface>}
    {confirmation && <WorkspaceSurface place="banner" title={confirmation.title} showHeader={false} close={() => setConfirmation(null)}><div className="inline-notice"><span>{confirmation.message}</span><button className="danger" disabled={pending > 0} onClick={() => { if (confirmation.skippable && skipNextAsk) setAskBeforeDelete(false); void confirmation.action().then(() => setConfirmation(null)).catch(() => {}); }}>Delete</button><button onClick={() => setConfirmation(null)}>Cancel</button>{confirmation.skippable && <details><summary>Options</summary><label className="check-label"><input type="checkbox" checked={skipNextAsk} onChange={event => setSkipNextAsk(event.target.checked)} />Don’t ask again</label></details>}</div></WorkspaceSurface>}
    {showAiSettings && <WorkspaceSurface place={project ? 'panel' : 'start'} title="AI settings" close={() => setShowAiSettings(false)}><AiSettings current={aiSettings} refresh={refreshAiSettings} update={updateAiSettings} manageUsage={manageChatgptUsage} pollingError={aiPollingError} /></WorkspaceSurface>}
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
    <div className="form-actions"><button className="primary" disabled={busy || saving || !valid}>Save grid settings</button></div>
  </form>;
}
