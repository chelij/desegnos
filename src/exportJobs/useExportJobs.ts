import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { documentDir, join } from '@tauri-apps/api/path';
import { native } from '../api';
import { isTextField } from '../WorkspaceSurface';
import type { Command, PointerStateView, Project, WorkspaceView, ExportCheck, ExportJobView } from '../types';
import { idlePlay, playIsActive, type PlayState } from '../projectCanvas/play';
import type { PreviewState } from '../ProjectCanvas';
import { errorMessage } from '../editorShared';

type Context = {
  view: WorkspaceView | null;
  showHistory: boolean;
  current: import("react").RefObject<WorkspaceView | null>;
  panels: import("../EditorDock").PanelState;
  panelActions: import("../EditorDock").PanelActions;
  accept: (next: WorkspaceView) => void;
  setNotice: (message: string, undo?: () => void) => void;
  settle: () => Promise<void>;
  setError: import("react").Dispatch<import("react").SetStateAction<string>>;
  setOpenFailure: import("react").Dispatch<import("react").SetStateAction<{ message: string; url: string; } | null>>;
  setShowHistory: import("react").Dispatch<import("react").SetStateAction<boolean>>;
  mutate: (command: Command | ((project: Project) => Command | null), quiet?: boolean) => Promise<WorkspaceView>;
  canvasFocus: string | null;
  selectedElementPageId: string | null;
  play: PlayState;
  setPlay: import("react").Dispatch<import("react").SetStateAction<PlayState>>;
};

const EXPORT_FOLDERS_KEY = 'desegnos.export-folders';

/** Folders are remembered per project, so switching tabs keeps each project's own choice. */
function readExportFolders(): Record<string, string> {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(EXPORT_FOLDERS_KEY) ?? '{}');
    if (!raw || typeof raw !== 'object') return {};
    return Object.fromEntries(Object.entries(raw).filter((entry): entry is [string, string] => typeof entry[1] === 'string'));
  } catch { return {}; }
}

/** A project name is a folder name here, never a path. */
function exportFolderName(name: string): string {
  const clean = name.trim().replace(/[\\/:*?"<>|\u0000-\u001f]/g, ' ').replace(/\s+/g, ' ').replace(/^[.\s]+|[.\s]+$/g, '').slice(0, 80);
  return clean || 'Untitled project';
}

/** The platform's Documents folder, then Desegnos/<project name>. */
async function defaultExportFolder(name: string): Promise<string> {
  return join(await documentDir(), 'Desegnos', exportFolderName(name));
}

export function useExportJobs(input: Pick<Context, 'current' | 'view' | 'showHistory' | 'play' | 'setPlay'>, context: () => Context) {
  const { current, view, showHistory, play, setPlay } = input;
  const [showExport, setShowExport] = useState(false);
  const [exportFolders, setExportFolders] = useState<Record<string, string>>(readExportFolders);
  const projectId = view?.project?.id ?? null;
  const exportDestination = projectId ? exportFolders[projectId] ?? null : null;
  const [exportJob, setExportJob] = useState<ExportJobView | null>(null);
  const [exportError, setExportError] = useState('');
  const [exportSending, setExportSending] = useState(false);
  const [checkedStates, setCheckedStates] = useState<PointerStateView[] | null>(null);
  const [checkedProject, setCheckedProject] = useState<string | null>(null);
  const [previewResult, setPreview] = useState<(Omit<PreviewState, 'stale'> & { revision: number | null }) | null>(null);
  const preview = useMemo<PreviewState | null>(() => previewResult ? {
    pageId: previewResult.pageId, url: previewResult.url,
    stale: previewResult.revision !== null && view?.project?.revision !== undefined && previewResult.revision !== view.project.revision,
  } : null, [previewResult, view?.project?.revision]);
  const checkTimer = useRef<number | null>(null);

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
  useEffect(() => { setShowExport(false); setExportJob(null); setExportError(''); setPreview(null); setPlay(idlePlay); }, [view?.project?.id]);
  // An error about a boundary that was added or removed no longer describes the current export.
  const pageIds = view?.project?.pages.map(page => page.id).join() ?? '';
  useEffect(() => { setExportError(''); }, [pageIds]);
  useEffect(() => { try { localStorage.setItem(EXPORT_FOLDERS_KEY, JSON.stringify(exportFolders)); } catch { /* Preferences can be unavailable. */ } }, [exportFolders]);
  // A new project starts in Documents/Desegnos/<project name>, so Export is one click.
  useEffect(() => {
    const project = view?.project;
    if (!project) return;
    let live = true;
    void defaultExportFolder(project.name).then(path => {
      if (live) setExportFolders(current => current[project.id] ? current : { ...current, [project.id]: path });
    }).catch(() => { /* Without a Documents folder, Choose folder… still works. */ });
    return () => { live = false; };
  }, [view?.project?.id, view?.project?.name]);
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

  function openExportPanel(restore = true) {
    const { current, panels, panelActions } = context();
    const project = current.current?.project;
    if (!project) return;
    setShowExport(true);
    if (restore && !exportJob) void native<ExportJobView | null>('get_export_job', { projectId: project.id }).then(job => { if (job) setExportJob(job); }).catch(() => {});
    if (panels.hidden) panelActions.toggleHidden();
    if (panels.collapsed) panelActions.setCollapsed(false);
  }

  async function ensureExportDestination(project: Project): Promise<string | null> {
    const saved = exportFolders[project.id];
    if (saved) return saved;
    const path = await defaultExportFolder(project.name).catch(() => null);
    if (!path) return null;
    setExportFolders(current => current[project.id] ? current : { ...current, [project.id]: path });
    return path;
  }

  async function chooseExportDestination() {
    const project = current.current?.project;
    if (!project) return;
    try {
      const picked = await native<{ path: string } | null>('pick_export_destination');
      if (!picked) return;
      setExportFolders(current => ({ ...current, [project.id]: picked.path }));
    } catch (error) { setExportError(errorMessage(error)); }
  }

  async function refreshWorkspace() {
    const { accept } = context();
    try { accept(await native<WorkspaceView>('get_workspace')); } catch { /* The next edit reports its own error. */ }
  }

  function showJobResult(job: ExportJobView, previewPageId?: string | null) {
    const { current, setNotice } = context();
    if (job.status === 'ready') void refreshWorkspace();
    if (job.result?.previewUrl) {
      const pageId = previewPageId ?? job.pages[0]?.pageId ?? null;
      if (pageId) { setPreview({ pageId, url: job.result.previewUrl, revision: current.current?.project?.revision ?? null }); }
    }
    if (job.status === 'ready' && !job.result?.previewUrl) setNotice(job.message);
  }

  async function runExport(previewPageId: string | null = null) {
    const { settle, current } = context();
    await settle();
    const project = current.current?.project;
    if (!project) return;
    const destination = previewPageId ? null : await ensureExportDestination(project);
    if (!previewPageId && !destination) { await chooseExportDestination(); return; }
    setExportError(''); setShowExport(true); setExportSending(false); setExportJob(null);
    try {
      const job = await native<ExportJobView>('prepare_export', {
        projectId: project.id, revision: project.revision,
        destination,
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
    const { current, setError } = context();
    const project = current.current?.project;
    if (!project) return;
    void native('open_export_folder', { projectId: project.id, path }).catch(error => setError(errorMessage(error)));
  }

  async function openInBrowser() {
    const { current, setError, setOpenFailure } = context();
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
    const { current, setShowHistory, setError } = context();
    const project = current.current?.project;
    if (!project) return;
    try {
      const job = await native<ExportJobView>('keep_pointer_code', { projectId: project.id, pointerId });
      setExportJob(job); openExportPanel(false); setShowHistory(false);
    } catch (error) { setError(errorMessage(error)); }
  }

  function forgetExportPointer(pointerId: string) {
    const { mutate } = context();
    void mutate({ type: 'forget_export_pointer', pointer_id: pointerId }).catch(() => {});
  }

  function previewSelectedBoundary() {
    const { canvasFocus, selectedElementPageId, current, setNotice, play, setPlay } = context();
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

  return {
    showExport, setShowExport, exportDestination, exportJob, exportError, exportSending, checkedStates,
    checkedProject, preview, openExportPanel, chooseExportDestination, refreshWorkspace, showJobResult,
    runExport, sendExport, cancelExport, openExportFolder, openInBrowser, keepPointerCode, forgetExportPointer,
    previewSelectedBoundary, closePreview, refreshPreview,
  };
}
