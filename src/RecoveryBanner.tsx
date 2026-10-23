export type PendingRecovery = { code: 'pendingRecovery'; projectId: string; path: string };

export function isPendingRecovery(error: unknown): error is PendingRecovery {
  return !!error && typeof error === 'object' && 'code' in error && error.code === 'pendingRecovery'
    && 'projectId' in error && typeof error.projectId === 'string'
    && 'path' in error && typeof error.path === 'string';
}

export function RecoveryBanner({ busy, recover, openSaved, dismiss }: {
  busy: boolean; recover: () => void; openSaved: () => void; dismiss: () => void;
}) {
  return <div className="message warning recovery-banner" role="status">
    <span>This project has unsaved changes from an earlier session.</span>
    <button type="button" disabled={busy} onClick={recover}>Recover</button>
    <button type="button" disabled={busy} onClick={openSaved}>Open saved version</button>
    <button type="button" onClick={dismiss} aria-label="Dismiss recovery banner">Dismiss</button>
  </div>;
}
