import { usePoll } from './usePoll';
import { useCallback, useEffect, useRef, useState } from 'react';
import { native } from './api';
import { ownsJob, type OwnedJob } from './generationSession.js';
import type { GenerationJobView, GenerationStatus, WorkspaceView } from './types';

export type GenerationCall = <T>(command: string, args?: Record<string, unknown>, signal?: AbortSignal) => Promise<T>;

const activeStatuses: readonly GenerationStatus[] = ['generating', 'applying'];
// A restored job in one of these states can still finish on its own and be applied automatically.
const resumeStatuses: readonly GenerationStatus[] = ['generating', 'ready', 'applying'];
// A job in one of these states holds a result that must be dropped before preparing a fresh one.
const discardableStatuses: readonly GenerationStatus[] = ['prepared', 'ready', 'blocked'];

const inStatus = (job: GenerationJobView | null, statuses: readonly GenerationStatus[]) => !!job && statuses.includes(job.status);
export const activeGeneration = (job: GenerationJobView | null) => inStatus(job, activeStatuses);
const resumesOnItsOwn = (job: GenerationJobView | null) => inStatus(job, resumeStatuses);
const discardable = (job: GenerationJobView | null) => inStatus(job, discardableStatuses);
const describe = (error: unknown) => error instanceof Error ? error.message : String(error);

/** The one thing the hook is doing right now. The label the UI shows is derived from it, never stored. */
export type GenerationPhase =
  | { name: 'idle' }
  // Stopped before any job existed: there was nothing to cancel, so the notices say so.
  | { name: 'stopped' }
  // The explicit discard button. Dropping a stale job on the way to a new one is `preparing.discarding`.
  | { name: 'discarding' }
  | { name: 'preparing'; step: 'discarding' | 'reading' | 'starting' }
  | { name: 'starting' }
  | { name: 'retrying' }
  | { name: 'applying' }
  | { name: 'stopping' };

// Every user-facing label, keyed by phase. Nothing else turns a phase into text.
const labels = {
  idle: null,
  stopped: null,
  discarding: 'Discarding candidate…',
  preparing: { discarding: 'Preparing your changes…', reading: 'Preparing AI instructions…', starting: 'Starting generation…' },
  starting: 'Starting generation…',
  retrying: 'Sending approved retry…',
  applying: 'Updating your canvas…',
  stopping: 'Stopping…' } as const;

const operationLabel = (phase: GenerationPhase): string | null => phase.name === 'preparing' ? labels.preparing[phase.step] : labels[phase.name];

// The phases whose work Stop can interrupt.
const stopCapable = (phase: GenerationPhase) => phase.name === 'stopping' || phase.name === 'preparing' || phase.name === 'starting' || phase.name === 'retrying';

// Stable objects so settling back into the same phase does not rerender.
const idle: GenerationPhase = { name: 'idle' };
const stopped: GenerationPhase = { name: 'stopped' };
const preparing = (step: 'discarding' | 'reading' | 'starting'): GenerationPhase => ({ name: 'preparing', step });

export function useGeneration(projectId: string | null, call: GenerationCall, applied: (view: WorkspaceView) => void, recorded?: () => Promise<void>) {
  const [job, setJob] = useState<GenerationJobView | null>(null);
  // The job this app session created; a restored job never owns the automatic notices.
  const [owned, setOwned] = useState<OwnedJob>(null);
  const [visible, setVisible] = useState(false);
  const [phase, setPhase] = useState<GenerationPhase>(idle);
  const [operationStartedAt, setOperationStartedAt] = useState(0);
  const [clock, setClock] = useState(Date.now);
  const [error, setError] = useState('');
  // The abort scope of the current flow. It outlives the phase on purpose: a later canvas apply
  // reuses it so Stop also blocks that, and only a project switch drops it.
  const flow = useRef<{ controller: AbortController; jobId?: string; done: Promise<void> } | null>(null);
  // The job this session applies without being asked. Independent of the phase: it stays set while
  // the provider works in the background, when the phase is already idle.
  const automatic = useRef<string | null>(null);
  const context = useRef(projectId);
  const epoch = useRef(0);
  const action = useRef(0);
  context.current = projectId;
  const operation = operationLabel(phase);
  const working = operation !== null;
  const canStop = stopCapable(phase) || activeGeneration(job) || job?.status === 'ready';
  useEffect(() => {
    if (!recorded || !job) return;
    let active = true;
    void recorded().catch(error => { if (active) setError(`The project brief could not be refreshed: ${describe(error)}`); });
    return () => { active = false; };
  }, [job?.jobId, job?.status, recorded]);
  const busy = working || activeGeneration(job);
  useEffect(() => {
    if (!busy) return;
    setClock(Date.now());
    const timer = setInterval(() => setClock(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [busy]);
  const attempt = job?.attempts.at(-1);
  const startedAt = working ? operationStartedAt : attempt ? Date.parse(attempt.startedAt) : NaN;
  const finishedAt = busy ? clock : attempt?.finishedAt ? Date.parse(attempt.finishedAt) : NaN;
  const elapsedSeconds = Number.isFinite(startedAt) && Number.isFinite(finishedAt) ? Math.max(0, Math.floor((finishedAt - startedAt) / 1000)) : null;
  useEffect(() => {
    let active = true;
    const version = ++epoch.current; action.current += 1; setJob(null); setOwned(null); setVisible(false); setPhase(idle); setError(''); flow.current?.controller.abort(); flow.current = null;
    if (projectId) void native<GenerationJobView | null>('get_generation', { projectId }).then(value => {
      if (active && context.current === projectId && version === epoch.current) { setJob(value); if (value && resumesOnItsOwn(value)) automatic.current = value.jobId; }
    }, error => { if (active && version === epoch.current) setError(describe(error)); });
    return () => { active = false; };
  }, [projectId]);
  usePoll(!!projectId && activeGeneration(job) && !flow.current?.controller.signal.aborted, `${projectId}:${job?.jobId}:${job?.status}`, 700,
    async active => {
      const version = epoch.current;
      const jobId = job?.jobId;
      const value = await native<GenerationJobView | null>('get_generation', { projectId });
      if (active() && context.current === projectId && version === epoch.current && value?.jobId === jobId) { setJob(value); setError(''); }
    }, error => setError(`Progress could not be refreshed: ${describe(error)}`));
  const refresh = useCallback(async () => {
    if (!projectId) return;
    const version = ++epoch.current;
    try { const value = await native<GenerationJobView | null>('get_generation', { projectId }); if (context.current === projectId && version === epoch.current) { setJob(value); setError(''); } }
    catch (error) { if (context.current === projectId && version === epoch.current) setError(describe(error)); }
  }, [projectId]);
  async function perform(command: string, next: GenerationPhase, args?: Record<string, unknown>, signal?: AbortSignal) {
    const id = projectId;
    const version = ++epoch.current;
    const token = ++action.current; setOperationStartedAt(Date.now()); setPhase(next); setError('');
    try { const value = await call<GenerationJobView>(command, args, signal); if (context.current === id && version === epoch.current) { setJob(value); if (id) setOwned({ projectId: id, jobId: value.jobId }); } return value; }
    catch (error) { if (!signal?.aborted && context.current === id && version === epoch.current) setError(describe(error)); throw error; }
    finally { if (!signal?.aborted && context.current === id && token === action.current) setPhase(idle); }
  }
  async function prepare(elementIds?: string[], referenceIds?: string[]) {
    if (phase.name === 'preparing') return;
    const replacing = discardable(job) ? job : null;
    setVisible(false);
    let finish!: () => void;
    const current = { controller: new AbortController(), jobId: undefined as string | undefined, done: new Promise<void>(resolve => { finish = resolve; }) };
    flow.current = current;
    const signal = current.controller.signal;
    setPhase(preparing(replacing ? 'discarding' : 'reading'));
    try {
      if (replacing) await perform('discard_generation', preparing('discarding'), { jobId: replacing.jobId }, signal);
      signal.throwIfAborted();
      const prepared = await perform('prepare_generation', preparing('reading'), elementIds || referenceIds ? { elementIds, referenceIds } : undefined, signal);
      current.jobId = prepared.jobId;
      signal.throwIfAborted();
      automatic.current = prepared.jobId;
      const started = await perform('start_generation', preparing('starting'), { jobId: prepared.jobId }, signal);
      current.jobId = started.jobId; automatic.current = started.jobId;
    } catch (error) { if (!signal.aborted && context.current === projectId) setError(describe(error)); } finally {
      finish();
    }
  }
  async function cancel() {
    if (phase.name === 'stopping') return;
    const id = projectId;
    const current = flow.current;
    current?.controller.abort();
    automatic.current = null;
    epoch.current += 1;
    const token = ++action.current;
    let settled: GenerationPhase = idle;
    setPhase({ name: 'stopping' }); setError('');
    try {
      // Preparation may still be creating the job. Never start it after Stop.
      if (current && !current.jobId) await current.done;
      const jobId = current?.jobId ?? (!current ? job?.jobId : undefined);
      if (jobId) {
        // Cancellation must bypass the workspace queue, including a pending start/apply.
        const value = await native<GenerationJobView>('cancel_generation', { jobId });
        if (context.current === id) setJob(value);
      } else if (context.current === id) { setJob(null); settled = stopped; }
    } catch (error) { if (context.current === id) setError(describe(error)); }
    finally { if (context.current === id && token === action.current) setPhase(settled); }
  }
  async function launch(command: string, next: GenerationPhase) {
    let finish!: () => void;
    const current = { controller: new AbortController(), jobId: job!.jobId, done: new Promise<void>(resolve => { finish = resolve; }) };
    flow.current = current; automatic.current = current.jobId;
    try {
      const submitted = await perform(command, next, { jobId: current.jobId }, current.controller.signal);
      current.jobId = submitted.jobId; automatic.current = submitted.jobId;
      return submitted;
    }
    catch (error) { if (!current.controller.signal.aborted && context.current === projectId) setError(describe(error)); }
    finally { finish(); }
  }
  const args = () => ({ jobId: job!.jobId });
  async function apply(indices: number[]) {
    const id = projectId;
    flow.current ??= { controller: new AbortController(), jobId: job!.jobId, done: Promise.resolve() };
    epoch.current += 1;
    const token = ++action.current; setOperationStartedAt(Date.now()); setPhase({ name: 'applying' }); setError('');
    try {
      const value = await call<WorkspaceView>('apply_generation', { ...args(), acceptedDecisionIndices: indices }, flow.current?.controller.signal);
      if (!flow.current?.controller.signal.aborted && context.current === id) { applied(value); await refresh(); setVisible(false); }
    } catch (error) {
      if (!flow.current?.controller.signal.aborted && context.current === id) {
        await refresh();
        if (context.current === id) setError(describe(error));
      }
    }
    finally { if (context.current === id && token === action.current) setPhase(idle); }
  }
  const appliedAttempts = useRef(new Set<string>());
  useEffect(() => {
    if (!job || automatic.current !== job.jobId || job.status !== 'ready' || working) return;
    const key = `${job.jobId}:${job.attempts.length}`;
    if (appliedAttempts.current.has(key)) return;
    appliedAttempts.current.add(key);
    void apply([]);
  }, [job?.jobId, job?.status, job?.attempts.length, phase]);
  const owns = ownsJob(projectId, job, owned);
  return { owns, job, phase, visible, operation, error, elapsedSeconds, stopped: phase.name === 'stopped', canStop, open: () => setVisible(true), close: () => setVisible(false), refresh, prepare, apply,
    start: () => launch('start_generation', { name: 'starting' }),
    cancel,
    discard: () => perform('discard_generation', { name: 'discarding' }, args()).catch(error => { setError(describe(error)); }),
    retry: () => launch('retry_generation', { name: 'retrying' }) };
}

export type GenerationController = ReturnType<typeof useGeneration>;
