import { readJsonPreference, writePreference } from '../preferences';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { defaultExportFolder } from './exportFolder';
import { native } from '../api';
import type { Command, PointerStateView, Project, WorkspaceView, ExportCheck, ExportJobView, PickDestination } from '../types';
import { idlePlay, playIsActive, type PlayState } from '../projectCanvas/play';
import type { PreviewState } from '../ProjectCanvas';
import { errorMessage } from '../editorShared';
import { projectHasPreviewPages } from '../visibleActions';

type Context = {
  view: WorkspaceView | null;
  panel: import('../session/useEditor').EditorPanel;
  openPanel: (panel: Exclude<import('../session/useEditor').EditorPanel, { kind: 'closed' }>) => void;
  closePanel: (kind: import('../session/useEditor').EditorPanel['kind']) => void;
  current: import("react").RefObject<WorkspaceView | null>;
  panels: import("../dock/panelState").PanelState;
  panelActions: import("../dock/panelState").PanelActions;
  accept: (next: WorkspaceView) => void;
  setNotice: (message: string, undo?: () => void) => void;
  settle: () => Promise<void>;
  setError: import("react").Dispatch<import("react").SetStateAction<string>>;
  setOpenFailure: import("react").Dispatch<import("react").SetStateAction<{ message: string; url: string; } | null>>;
  mutate: (command: Command | ((project: Project) => Command | null)) => Promise<WorkspaceView | null>;
  canvasFocus: string | null;
  selectedElementPageId: string | null;
  play: PlayState;
  setPlay: import("react").Dispatch<import("react").SetStateAction<PlayState>>;
  aiConnected: boolean;
  /** The AI spending limit; a Preview starts behaviour code by itself only within it. */
  spendingLimitUsd: number | null;
};

const EXPORT_FOLDERS_KEY = 'desegnos.export-folders';

function readExportFolders(): Record<string, string> {
  try {
    const raw: unknown = readJsonPreference(EXPORT_FOLDERS_KEY, {});
    if (!raw || typeof raw !== 'object') return {};
    return Object.fromEntries(Object.entries(raw).filter((entry): entry is [string, string] => typeof entry[1] === 'string'));
  } catch { return {}; }
}

export function useExportJobs(context: Context) {
  const { current, view, panel, setPlay, openPanel, closePanel } = context;
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
    stale: previewResult.revision !== null && view?.project?.revision !== undefined && previewResult.revision !== view.project.revision } : null, [previewResult, view?.project?.revision]);
  const checkTimer = useRef<number | null>(null);

  const recheckExport = useCallback(async () => {
    const project = current.current?.project;
    if (!project) { setCheckedStates(null); setCheckedProject(null); return; }
    try {
      const check = await native<ExportCheck>('recheck_export', { projectId: project.id });
      setCheckedStates(check.states); setCheckedProject(project.id);
    } catch (error) { setCheckedStates(null); setCheckedProject(null); setExportError(errorMessage(error)); }
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
  useEffect(() => { if (panel.kind === 'history') scheduleExportCheck(); }, [panel.kind, scheduleExportCheck]);
  useEffect(() => { closePanel('export'); setExportJob(null); setExportError(''); setPreview(null); setPlay(idlePlay); }, [view?.project?.id]);
  // An error about a boundary that was added or removed no longer describes the current export.
  const pageIds = view?.project?.pages.map(page => page.id).join() ?? '';
  useEffect(() => { setExportError(''); }, [pageIds]);
  useEffect(() => { writePreference(EXPORT_FOLDERS_KEY, JSON.stringify(exportFolders)); }, [exportFolders]);
  // A new export creates one project folder inside Documents/Desegnos.
  useEffect(() => {
    const project = view?.project;
    if (!project) return;
    let live = true;
    void defaultExportFolder(exportFolders[project.id]).then(path => {
      if (live) setExportFolders(current => current[project.id] === exportFolders[project.id] ? { ...current, [project.id]: path } : current);
    }).catch(error => { if (live) setExportError(errorMessage(error)); });
    return () => { live = false; };
  }, [view?.project?.id, view?.project?.name]);

  function openExportPanel(restore = true) {
    const { current, panels, panelActions } = context;
    const project = current.current?.project;
    if (!project) return;
    openPanel({ kind: 'export' });
    if (restore && !exportJob) void native<ExportJobView | null>('get_export_job', { projectId: project.id }).then(job => { if (job) setExportJob(job); }).catch(error => setExportError(errorMessage(error)));
    if (panels.hidden) panelActions.toggleHidden();
    if (panels.collapsed) panelActions.setCollapsed(false);
  }

  async function ensureExportDestination(project: Project): Promise<string | null> {
    const saved = exportFolders[project.id];
    const path = await defaultExportFolder(saved).catch(error => { setExportError(errorMessage(error)); return saved ?? null; });
    if (!path) return null;
    setExportFolders(current => current[project.id] === saved ? { ...current, [project.id]: path } : current);
    return path;
  }

  async function chooseExportDestination() {
    const project = current.current?.project;
    if (!project) return;
    try {
      const picked = await native<PickDestination | null>('pick_export_destination');
      if (!picked) return;
      setExportFolders(current => ({ ...current, [project.id]: picked.path }));
    } catch (error) { setExportError(errorMessage(error)); }
  }

  /** A typed folder. Export itself checks it is safe to write; only its shape is checked here. */
  function setExportDestination(path: string) {
    const project = current.current?.project;
    const trimmed = path.trim();
    if (!project || !trimmed) return;
    if (!/^(\/|[A-Za-z]:[\\/]|\\\\)/.test(trimmed)) { setExportError('Enter the full path of the folder.'); return; }
    setExportError('');
    setExportFolders(current => ({ ...current, [project.id]: trimmed.replace(/[\\/]+$/, '') || trimmed }));
  }

  async function refreshWorkspace() {
    const { accept } = context;
    try { accept(await native<WorkspaceView>('get_workspace')); } catch (error) { setExportError(errorMessage(error)); }
  }

  function showJobResult(job: ExportJobView, previewPageId?: string | null) {
    const { current, setNotice } = context;
    if (job.status === 'ready') void refreshWorkspace();
    if (job.result?.previewUrl) {
      const pageId = previewPageId ?? job.pages[0]?.pageId ?? null;
      if (pageId) { setPreview({ pageId, url: job.result.previewUrl, revision: current.current?.project?.revision ?? null }); }
    }
    if (job.status === 'ready' && !job.result?.previewUrl) setNotice(job.message);
  }

  async function runExport(previewPageId: string | null = null) {
    const { settle, current, aiConnected, spendingLimitUsd } = context;
    await settle();
    const project = current.current?.project;
    if (!project) return;
    const destination = previewPageId ? null : await ensureExportDestination(project);
    if (!previewPageId && !destination) { await chooseExportDestination(); return; }
    setExportError(''); openPanel({ kind: 'export' }); setExportSending(false); setExportJob(null);
    try {
      const job = await native<ExportJobView>('prepare_export', {
        projectId: project.id, revision: project.revision,
        destination,
        pageIds: previewPageId ? [previewPageId] : null,
        preview: !!previewPageId,
        // A preview asks the AI only for behaviours it cannot otherwise show, and still waits for Send.
        buildBehaviours: !!previewPageId && aiConnected });
      setExportJob(job);
      showJobResult(job, previewPageId);
      // Pressing Preview is the go-ahead: start the behaviour code straight away when it fits
      // the spending limit. Anything else waits on the panel's button as before.
      if (previewPageId && job.status === 'prepared' && job.kind === 'code' && aiConnected
        && spendingLimitUsd !== null && job.estimateUsd <= spendingLimitUsd) await sendExport(job);
    } catch (error) { setExportError(errorMessage(error)); }
  }

  async function sendExport(prepared: ExportJobView | null = null) {
    // Buttons call this with their click event; only a prepared job overrides the shown one.
    const target = prepared && typeof prepared === 'object' && 'jobId' in prepared ? prepared : exportJob;
    if (!target) return;
    setExportSending(true); setExportError('');
    try {
      const job = await native<ExportJobView>('start_export', { jobId: target.jobId });
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
    const { current, setError } = context;
    const project = current.current?.project;
    if (!project) return;
    void native('open_export_folder', { projectId: project.id, path }).catch(error => setError(errorMessage(error)));
  }

  async function openInBrowser() {
    const { current, setError, setOpenFailure } = context;
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
    const { current, setError } = context;
    const project = current.current?.project;
    if (!project) return;
    try {
      const job = await native<ExportJobView>('keep_pointer_code', { projectId: project.id, pointerId });
      setExportJob(job); openExportPanel(false);
    } catch (error) { setError(errorMessage(error)); }
  }

  function forgetExportPointer(pointerId: string) {
    const { mutate } = context;
    void mutate({ type: 'forget_export_pointer', pointer_id: pointerId });
  }

  function previewSelectedBoundary(preferredPageId?: string | null) {
    const { canvasFocus, selectedElementPageId, current, play, setPlay } = context;
    const project = current.current?.project;
    if (!project || !projectHasPreviewPages(project)) return;
    const previewable = (pageId: string | null | undefined) => project.pages.find(page =>
      page.id === pageId && (page.output === 'webpage' || page.output === 'appScreen'))?.id;
    const target = previewable(preferredPageId) ?? previewable(canvasFocus) ?? previewable(selectedElementPageId) ?? previewable(project.homePageId)
      ?? project.pages.find(page => page.output === 'webpage' || page.output === 'appScreen')?.id;
    if (!target) return;
    if (playIsActive(play)) setPlay(idlePlay);
    void runExport(target);
  }

  function closePreview() {

    setPreview(null);
    void native('close_preview').catch(error => setExportError(errorMessage(error)));
  }

  function refreshPreview() {

    const pageId = preview?.pageId;
    if (!pageId) return;
    closePreview();
    void runExport(pageId);
  }

  return {
    destination: exportDestination, job: exportJob, error: exportError, sending: exportSending,
    checkedStates, checkedProject, preview,
    openPanel: openExportPanel, chooseDestination: chooseExportDestination, setDestination: setExportDestination,
    /** Exports the included boundaries, or previews one page when given its id. */
    run: runExport, send: sendExport, cancel: cancelExport,
    openFolder: openExportFolder, openInBrowser, keepPointerCode, forgetPointer: forgetExportPointer,
    previewBoundary: previewSelectedBoundary, closePreview, refreshPreview };
}

/** Export and Preview: the job, its folder and every action on it. */
export type ExportJobs = ReturnType<typeof useExportJobs>;
