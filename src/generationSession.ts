import type { GenerationJobView } from './types';
import { jobOutcome } from './generationProblems.js';

// Automatic notices describe a generation this app session started. A job restored from the
// saved latest generation (opening or switching to a project) stays silent; it is still in the
// AI conversation and the Past generations list.
export type OwnedJob = { projectId: string; jobId: string } | null;

export function ownsJob(projectId: string | null, job: GenerationJobView | null, owned: OwnedJob): boolean {
  return !!projectId && !!job && owned?.projectId === projectId && owned.jobId === job.jobId;
}

const statusNotices = ['prepared', 'blocked', 'failed', 'unknown', 'cancelled'];

/// Whether the editor banner should describe this job automatically.
export function announcesStatus(job: GenerationJobView | null, state: { owns: boolean; stopped: boolean; working: boolean }): boolean {
  if (!state.owns) return false;
  return state.stopped || state.working || !!job && (statusNotices.includes(job.status) || !!job.candidate?.requests.some(item => item.status === 'conflict') || job.status === 'applied' && !!jobOutcome(job).reason);
}
