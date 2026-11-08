import { useState } from 'react';
import type { RecoveryInfo } from './types';

function editedAgo(editedAtMs: number) {
  const minutes = Math.max(0, Math.floor((Date.now() - editedAtMs) / 60_000));
  return !Number.isFinite(minutes) ? 'Unsaved draft' : minutes < 1 ? 'Edited just now' : minutes < 60 ? `Edited ${minutes} min ago` : minutes < 1440 ? `Edited ${Math.floor(minutes / 60)} hr ago` : `Edited ${Math.floor(minutes / 1440)} days ago`;
}

export function RecoveryList({ items, busy, recover, dismiss }: { items: RecoveryInfo[]; busy: boolean; recover: (id: string) => void; dismiss: (ids: string[]) => Promise<void> }) {
  const [confirmIds, setConfirmIds] = useState<string[] | null>(null);
  const submit = (ids: string[]) => { void dismiss(ids).then(() => setConfirmIds(null)); };
  const requestDismiss = (selected: RecoveryInfo[]) => {
    const ids = selected.map(item => item.projectId);
    if (selected.some(item => !item.savedPath)) setConfirmIds(ids);
    else submit(ids);
  };
  const confirmation = <div className="recovery-confirmation"><p>Never saved — dismiss anyway?</p><div className="form-actions">
    <button className="danger" disabled={busy} onClick={() => confirmIds && submit(confirmIds)}>Dismiss</button>
    <button disabled={busy} onClick={() => setConfirmIds(null)}>Cancel</button>
  </div></div>;
  return <div className="recovery-list">{items.map(item => <section className="recovery-row" key={item.projectId} aria-label={`Recover ${item.name}`}>
    <div className="recovery-details"><strong>{item.name}</strong>
    <p className="muted" title={item.savedPath ?? undefined}>{editedAgo(item.editedAtMs)}{item.savedPath ? ` · ${item.savedPath.split(/[\\/]/).at(-1)}` : ' · Never saved'}</p></div>
    {confirmIds?.length === 1 && confirmIds[0] === item.projectId ? confirmation : <div className="form-actions">
      <button className="primary" disabled={busy} onClick={() => recover(item.projectId)}>Recover</button>
      <button disabled={busy} onClick={() => requestDismiss([item])}>Dismiss</button>
    </div>}
  </section>)}
    {items.length >= 2 && (confirmIds && confirmIds.length > 1 ? confirmation : <div className="form-actions"><button disabled={busy} onClick={() => requestDismiss(items)}>Dismiss all</button></div>)}
  </div>;
}
