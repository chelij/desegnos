// A manual update check only reads the newest public release. It installs nothing and
// reports what it found in the editor's existing notice host.
import type { UpdateCheck } from './types';

export type { UpdateCheck };
export type UpdateSummary = { variant: 'info' | 'success'; message: string; download: string | null };

export function updateSummary(check: UpdateCheck): UpdateSummary {
  if (check.newer && check.latest) return { variant: 'success', message: `Desegnos ${check.latest.version} is available.`, download: check.latest.url };
  if (check.latest) return { variant: 'info', message: `Desegnos is up to date (${check.current}).`, download: null };
  return { variant: 'info', message: 'No releases yet.', download: null };
}
