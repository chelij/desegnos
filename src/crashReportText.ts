import { browserOpenUrl, userError } from './userErrors.js';

import type { OpenFailure, PendingCrash } from './types';

export type { PendingCrash };

export const CRASH_NOTICE = 'Desegnos closed unexpectedly last time.';

/// A failed browser launch keeps the issue address so it can still be copied.
export function crashOpenFailure(error: unknown): OpenFailure {
  const url = browserOpenUrl(error);
  if (url) return { url, message: 'Copy this address to report the crash.' };
  return { url: null, message: userError(error) || 'The crash report could not open. Try again.' };
}
