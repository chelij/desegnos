import { useCallback, useEffect, useRef, useState } from 'react';
import { native } from './api';
import type { GenerationJobView, WorkspaceView } from './types';

export type GenerationCall = <T>(command: string, args?: Record<string, unknown>, signal?: AbortSignal) => Promise<T>;
export const activeGeneration = (job: GenerationJobView | null) => !!job && (job.status === 'generating' || job.status === 'applying');
const describe = (error: unknown) => error instanceof Error ? error.message : String(error);

export function useGeneration(projectId: string | null, call: GenerationCall, applied: (view: WorkspaceView) => void, recorded?: () => Promise<void>) {
  const [job, setJob] = useState<GenerationJobView | null>(null);
  const [visible, setVisible] = useState(false);
  const [operation, setOperation] = useState<string | null>(null);
  const [operationStartedAt, setOperationStartedAt] = useState(0);
  const [clock, setClock] = useState(Date.now);
  const [error, setError] = useState('');
  const [running, setRunning] = useState(false);
  const [stopped, setStopped] = useState(false);
  const flow = useRef<{ controller: AbortController; jobId?: string; done: Promise<void> } | null>(null);
  const context = useRef(projectId);
  const epoch = useRef(0);
  const action = useRef(0);
  const automatic = useRef<string | null>(null);
  context.current = projectId;
  useEffect(() => {
    if (!recorded || !job) return;
    let active = true;
    void recorded().catch(error => { if (active) setError(`The project brief could not be refreshed: ${describe(error)}`); });
    return () => { active = false; };
  }, [job?.jobId, job?.status, recorded]);
  const busy = !!operation || activeGeneration(job);
  useEffect(() => {
    if (!busy) return;
    setClock(Date.now());
    const timer = setInterval(() => setClock(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [busy]);
  const attempt = job?.attempts.at(-1);
  const startedAt = operation ? operationStartedAt : attempt ? Date.parse(attempt.startedAt) : NaN;
  const finishedAt = busy ? clock : attempt?.finishedAt ? Date.parse(attempt.finishedAt) : NaN;
  const elapsedSeconds = Number.isFinite(startedAt) && Number.isFinite(finishedAt) ? Math.max(0, Math.floor((finishedAt - startedAt) / 1000)) : null;
  useEffect(() => {
    let active = true;
    const version = ++epoch.current; action.current += 1; setJob(null); setVisible(false); setOperation(null); setError(''); setRunning(false); setStopped(false); flow.current?.controller.abort(); flow.current = null;
    if (projectId) void native<GenerationJobView | null>('get_generation', { projectId }).then(value => {
      if (active && context.current === projectId && version === epoch.current) { setJob(value); if (value && ['generating', 'ready', 'applying'].includes(value.status)) { automatic.current = value.jobId; } }
    }, error => { if (active && version === epoch.current) setError(describe(error)); });
    return () => { active = false; };
  }, [projectId]);
  useEffect(() => {
    if (!projectId || !activeGeneration(job) || flow.current?.controller.signal.aborted) return;
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    const jobId = job!.jobId;
    async function poll() {
      const version = epoch.current;
      try {
        const value = await native<GenerationJobView | null>('get_generation', { projectId });
        if (active && context.current === projectId && version === epoch.current && value?.jobId === jobId) { setJob(value); setError(''); }
      } catch (error) { if (active && context.current === projectId && version === epoch.current) setError(`Progress could not be refreshed: ${describe(error)}`); }
      finally { if (active) timer = setTimeout(() => { void poll(); }, 700); }
    }
    timer = setTimeout(() => { void poll(); }, 700);
    return () => { active = false; clearTimeout(timer); };
  }, [projectId, job?.jobId, job?.status]);
  const refresh = useCallback(async () => {
    if (!projectId) return;
    const version = ++epoch.current;
    try { const value = await native<GenerationJobView | null>('get_generation', { projectId }); if (context.current === projectId && version === epoch.current) { setJob(value); setError(''); } }
    catch (error) { if (context.current === projectId && version === epoch.current) setError(describe(error)); }
  }, [projectId]);
  async function perform(command: string, label: string, args?: Record<string, unknown>, signal?: AbortSignal) {
    const id = projectId;
    const version = ++epoch.current;
    const token = ++action.current; setOperationStartedAt(Date.now()); setOperation(label); setError('');
    try { const value = await call<GenerationJobView>(command, args, signal); if (context.current === id && version === epoch.current) setJob(value); return value; }
    catch (error) { if (!signal?.aborted && context.current === id && version === epoch.current) setError(describe(error)); throw error; }
    finally { if (!signal?.aborted && context.current === id && token === action.current) setOperation(null); }
  }
  async function prepare(draft?: { jobId: string; draftId: string }, regionIds?: string[]) {
    if (running) return;
    setVisible(false); setRunning(true); setStopped(false);
    let finish!: () => void;
    const current = { controller: new AbortController(), jobId: undefined as string | undefined, done: new Promise<void>(resolve => { finish = resolve; }) };
    flow.current = current;
    const signal = current.controller.signal;
    try {
      if (job && ['prepared', 'ready', 'blocked'].includes(job.status)) await perform('discard_generation', 'Preparing your changes…', { jobId: job.jobId }, signal);
      signal.throwIfAborted();
      const prepared = await perform('prepare_generation', 'Preparing AI instructions…', draft ? { repairJobId: draft.jobId, repairDraftId: draft.draftId } : regionIds ? { regionIds } : undefined, signal);
      current.jobId = prepared.jobId;
      signal.throwIfAborted();
      automatic.current = prepared.jobId;
      const started = await perform('start_generation', 'Starting generation…', { jobId: prepared.jobId }, signal);
      current.jobId = started.jobId; automatic.current = started.jobId;
    } finally {
      finish();
      if (!signal.aborted && flow.current === current) setRunning(false);
    }
  }
  async function cancel() {
    if (operation === 'Stopping…') return;
    const id = projectId;
    const current = flow.current;
    current?.controller.abort();
    automatic.current = null;
    epoch.current += 1;
    const token = ++action.current;
    setOperation('Stopping…'); setError('');
    try {
      // Preparation may still be creating the job. Never start it after Stop.
      if (current && !current.jobId) await current.done;
      const jobId = current?.jobId ?? (!current ? job?.jobId : undefined);
      if (jobId) {
        // Cancellation must bypass the workspace queue, including a pending start/apply.
        const value = await native<GenerationJobView>('cancel_generation', { jobId });
        if (context.current === id) setJob(value);
      } else if (context.current === id) { setJob(null); setStopped(true); }
    } catch (error) { if (context.current === id) setError(describe(error)); }
    finally { if (context.current === id && token === action.current) { setOperation(null); setRunning(false); } }
  }
  async function launch(command: string, label: string) {
    let finish!: () => void;
    const current = { controller: new AbortController(), jobId: job!.jobId, done: new Promise<void>(resolve => { finish = resolve; }) };
    flow.current = current; setStopped(false); automatic.current = current.jobId;
    try {
      const submitted = await perform(command, label, { jobId: current.jobId }, current.controller.signal);
      current.jobId = submitted.jobId; automatic.current = submitted.jobId;
      return submitted;
    }
    finally { finish(); }
  }
  const args = () => ({ jobId: job!.jobId });
  async function apply(indices: number[]) {
    const id = projectId;
    flow.current ??= { controller: new AbortController(), jobId: job!.jobId, done: Promise.resolve() };
    epoch.current += 1;
    const token = ++action.current; setOperationStartedAt(Date.now()); setOperation('Updating your canvas…'); setError('');
    try {
      const value = await call<WorkspaceView>('apply_generation', { ...args(), acceptedDecisionIndices: indices }, flow.current?.controller.signal);
      if (!flow.current?.controller.signal.aborted && context.current === id) { applied(value); await refresh(); setVisible(false); }
    } catch (error) {
      if (!flow.current?.controller.signal.aborted && context.current === id) {
        await refresh();
        if (context.current === id) setError(describe(error));
      }
      throw error;
    }
    finally { if (context.current === id && token === action.current) setOperation(null); }
  }
  const appliedAttempts = useRef(new Set<string>());
  useEffect(() => {
    if (!job || automatic.current !== job.jobId || job.status !== 'ready' || operation) return;
    const key = `${job.jobId}:${job.attempts.length}`;
    if (appliedAttempts.current.has(key)) return;
    appliedAttempts.current.add(key);
    void apply([]).catch(() => {});
  }, [job?.jobId, job?.status, job?.attempts.length, operation]);
  async function draftAction(jobId: string, draftId: string, action: string, confirmedRevision?: number, regionId?: string) {
    const id = projectId;
    try {
      const value = await call<import('./types').DraftReview>('generation_draft', { jobId, draftId, action, confirmedRevision, regionId });
      if (context.current === id) {
        setError('');
        if (value.workspace) applied(value.workspace);
        if (jobId === job?.jobId) setJob(value.job);
      }
      return value;
    } catch (error) {
      if (context.current === id) setError(describe(error));
      throw error;
    }
  }
  return { draftAction, repairDraft: (jobId: string, draftId: string) => prepare({ jobId, draftId }), job, visible, operation, error, elapsedSeconds, stopped, canStop: operation === 'Stopping…' || running || activeGeneration(job) || job?.status === 'ready' || operation === 'Starting generation…' || operation === 'Sending approved retry…', open: () => setVisible(true), close: () => setVisible(false), refresh, prepare, apply,
    start: () => launch('start_generation', 'Starting generation…'),
    cancel,
    discard: () => perform('discard_generation', 'Discarding candidate…', args()),
    retry: () => launch('retry_generation', 'Sending approved retry…'),
  };
}

export type GenerationController = ReturnType<typeof useGeneration>;
