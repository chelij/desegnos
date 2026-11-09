// Startup checks intentionally stay quiet unless an update exists. Installation is
// explicit and the editor confirms unsaved or running work before invoking Rust.
import type { UpdateCheck } from './types';

export type { UpdateCheck };
export type UpdateSummary = { variant: 'info' | 'success'; message: string; release: UpdateCheck['latest']; installSupported: boolean };

export function updateSummary(check: UpdateCheck): UpdateSummary {
  if (check.newer && check.latest) return { variant: 'success', message: `Desegnos ${check.latest.version} is available.`, release: check.latest, installSupported: check.installSupported };
  return { variant: 'info', message: `Desegnos is up to date (${check.current}).`, release: null, installSupported: false };
}
