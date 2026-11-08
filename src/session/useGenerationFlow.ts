import { useCallback, useMemo, useRef } from 'react';
import { native } from '../api';
import { announcesStatus } from '../generationSession.js';
import { activeGeneration, useGeneration, type GenerationNative } from '../useGeneration';
import { selectedReference, type CanvasTarget } from '../projectCanvas/canvasTargets';
import type { Boundary } from '../projectCanvas/boundaryLayout';
import { coveredTargets } from '../projectCanvas/requests';
import { baseSizeId } from '../projectCanvas/sizes';
import type { Command, GenerationJobView, PendingRequest, Project, RequestArea, WorkspaceView } from '../types';

/** Preparing, running and applying AI generations, and the checks that run before and after each native call. */
export function useGenerationFlow({ project, canGenerateFromBrief, canvasSelection, canvasBoundaries, queue, accept, mutate, requireAiReady, setNotice, setError, addFeedback }: {
  project: Project | null;
  canGenerateFromBrief: boolean;
  canvasSelection: CanvasTarget[];
  canvasBoundaries: Boundary[];
  queue: <T>(operation: (workspace: WorkspaceView | null) => Promise<T>, reportErrors?: boolean) => Promise<T>;
  accept: (next: WorkspaceView) => void;
  mutate: (command: Command) => Promise<WorkspaceView | null>;
  requireAiReady: () => Promise<void>;
  setNotice: (message: string) => void;
  setError: (message: string) => void;
  addFeedback: (targets: { pageId: string; elementId: string }[], text: string) => Promise<string[]>;
}) {
  const briefGenerateFlight = useRef(false);
  const activeProjectId = project?.id ?? null;

  const queued = useCallback(<T,>(run: () => Promise<T>, signal?: AbortSignal) => Promise.resolve().then(() => queue(async workspace => {
    signal?.throwIfAborted();
    if (!activeProjectId || workspace?.project?.id !== activeProjectId) throw new Error('Return to the project that owns this generation request.');
    return run();
  }, false)), [queue, activeProjectId]);
  const natives = useMemo<GenerationNative>(() => ({
    call: (command, args, signal) => queued(() => native(command, args), signal),
    prepare: (args, signal) => queued(async () => {
      await requireAiReady();
      signal?.throwIfAborted();
      return native<GenerationJobView>('prepare_generation', args);
    }, signal),
    apply: (args, signal) => queued(async () => {
      const result = await native<WorkspaceView>('apply_generation', args);
      if (!signal?.aborted) accept(result);
      return result;
    }, signal),
  }), [queued, accept, requireAiReady]);
  const applied = useCallback((workspace: WorkspaceView) => { if (workspace.project?.latestGeneration) setNotice('Generated result applied.'); }, []);
  const recorded = useCallback(() => queue(async () => {
    const refreshed = await native<WorkspaceView>('get_workspace');
    if (refreshed.project?.id === activeProjectId) accept(refreshed);
  }, false), [queue, accept, activeProjectId]);
  const generation = useGeneration(activeProjectId, natives, applied, recorded);
  const working = generation.canStop || activeGeneration(generation.job) || !!generation.operation;
  const showGenerationStatus = !!generation.error || announcesStatus(generation.job, { owns: generation.owns, stopped: generation.stopped, working });

  const requestedAreas = project ? (project.requests ?? []).filter(request => request.text.trim()).length : 0;
  // With nothing described, a filled brief captures the first empty page as one request area.
  const firstPage = project?.pages[0];
  const firstBoundary = firstPage && canvasBoundaries.find(item => item.pageId === firstPage.id && item.sizeId === baseSizeId(firstPage));
  const fromBrief = !!project && !!firstPage && !!firstBoundary && canGenerateFromBrief;
  const nothingToGenerate = requestedAreas === 0 && !fromBrief;

  // Progress shows in the generation notice; its Details opens the AI conversation.
  async function generate(elementIds?: string[]) {
    const contextItems = selectedReference(canvasSelection);
    setError('');
    if (!fromBrief || !project || !firstPage || !firstBoundary) { await generation.prepare(elementIds, contextItems); return; }
    // The request is added before the job is prepared, so a second click must not add another.
    if (briefGenerateFlight.current) return;
    briefGenerateFlight.current = true;
    try {
      const area: RequestArea = { page: firstPage.id, size: firstPage.sizes.length > 1 ? baseSizeId(firstPage) : null, shape: { kind: 'rect', rect: firstBoundary.rect } };
      const request: PendingRequest = {
        id: crypto.randomUUID(), text: 'Build this page from the project brief.', attachments: [],
        targets: coveredTargets(project, canvasBoundaries, area), area, requestedAt: new Date().toISOString() };
      if (!await mutate({ type: 'add_request', request })) return;
      await generation.prepare(elementIds, contextItems);
    } finally { briefGenerateFlight.current = false; }
  }

  async function replyToGeneration(targets: { pageId: string; elementId: string }[], text: string) {
    const elementIds = await addFeedback(targets, text);
    if (!elementIds.length) throw new Error('Those requests are no longer in the project.');
    await generation.prepare(elementIds);
  }

  return { generation, working, showGenerationStatus, requestedAreas, nothingToGenerate, generate, replyToGeneration };
}
