import type { AiSettingsView, GenerationJobView } from '../types';

/** Whether the AI can take work now: a live job is judged by the sign-in it started with, anything else by the current settings. */
export function aiReady(settings: AiSettingsView | null, job: GenerationJobView | null): boolean {
  if (!settings) return false;
  const { readiness } = settings;
  if (!job) return readiness.ready;
  if ((job.authentication ?? settings.authentication) !== 'chatgpt') return readiness.apiKeyReady;
  return !!job.chatgptAccountId && !!readiness.accounts[job.chatgptAccountId];
}
