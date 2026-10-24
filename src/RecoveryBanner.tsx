import { Icon } from './icons';
export type PendingRecovery = { code: 'pendingRecovery'; projectId: string; path: string };

export function isPendingRecovery(error: unknown): error is PendingRecovery {
  return !!error && typeof error === 'object' && 'code' in error && error.code === 'pendingRecovery'
    && 'projectId' in error && typeof error.projectId === 'string'
    && 'path' in error && typeof error.path === 'string';
}

export type RecoveryChoices = { path: string; busy: boolean; recover: () => void; openSaved: () => void; dismiss: () => void };

// Shown where the user acted: on the recent project card they clicked, or in the recent projects area.
export function RecoveryChoice({ variant = 'row', busy, recover, openSaved, dismiss }: RecoveryChoices & { variant?: 'card' | 'row' }) {
  return <div className={`recovery-choice recovery-choice-${variant}`} role="alert">
    <span className="recovery-message" title="Unsaved changes from an earlier session">Unsaved changes from an earlier session</span>
    <span className="recovery-actions">
      <button type="button" className="primary" disabled={busy} onClick={recover}>Recover</button>
      <button type="button" disabled={busy} onClick={openSaved}>Open saved version</button>
    </span>
    <button type="button" className="recovery-dismiss icon-button sm" aria-label="Dismiss recovery choice" title="Dismiss" onClick={dismiss}><Icon name="close" /></button>
  </div>;
}
