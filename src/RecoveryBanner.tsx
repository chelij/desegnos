import { ProblemBanner } from './ProblemBanner.js';
export type PendingRecovery = { code: 'pendingRecovery'; projectId: string; path: string };

export function isPendingRecovery(error: unknown): error is PendingRecovery {
  return !!error && typeof error === 'object' && 'code' in error && error.code === 'pendingRecovery'
    && 'projectId' in error && typeof error.projectId === 'string'
    && 'path' in error && typeof error.path === 'string';
}

export function RecoveryBanner({ busy, recover, openSaved, dismiss }: {
  busy: boolean; recover: () => void; openSaved: () => void; dismiss: () => void;
}) {
  return <ProblemBanner className="recovery-banner" message="This project has unsaved changes from an earlier session."
    action={{ label: 'Recover', disabled: busy, onClick: recover }} dismiss={dismiss}>
    <button type="button" disabled={busy} onClick={openSaved}>Open saved version</button>
  </ProblemBanner>;
}
