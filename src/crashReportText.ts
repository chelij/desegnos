import { browserOpenUrl, userError } from './userErrors.js';

export type PendingCrash = { kind: string; summary: string; body: string };

export const CRASH_NOTICE = 'Desegnos closed unexpectedly last time.';

/// A failed browser launch keeps the issue address so it can still be copied.
export function crashOpenFailure(error: unknown): { url: string | null; message: string } {
  const url = browserOpenUrl(error);
  if (url) return { url, message: 'Copy this address to report the crash.' };
  return { url: null, message: userError(error) || 'The crash report could not open. Try again.' };
}
