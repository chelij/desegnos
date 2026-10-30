import { useCallback, useEffect, useRef, useState } from 'react';
import { native } from './api';
import type { BranchSummary, ProjectHistoryView, VersionSummary, WorkspaceView } from './types';

function editedAgo(createdAt: number) {
  const elapsed = Date.now() - createdAt;
  if (!Number.isFinite(elapsed) || elapsed < 0) return 'just now';
  const minutes = Math.floor(elapsed / 60_000);
  if (minutes < 1) return 'just now';
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} hr ago`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days} ${days === 1 ? 'day' : 'days'} ago`;
  return new Date(createdAt).toLocaleDateString([], { month: 'short', day: 'numeric' });
}

// Versions of the current branch, newest first, with the alternatives they belong to.
// Restoring never edits history: it becomes the current state as one undo step.
export function HistoryPanel({ projectId, refreshKey, apply, notice, error, compact }: {
  projectId: string;
  refreshKey: string;
  apply: (workspace: WorkspaceView) => void;
  notice: (message: string) => void;
  error: (message: string) => void;
  compact: () => void;
}) {
  const [data, setData] = useState<ProjectHistoryView | null>(null);
  const [busy, setBusy] = useState(false);
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState('');
  const [renaming, setRenaming] = useState<string | null>(null);
  const [renameTo, setRenameTo] = useState('');
  const [deleting, setDeleting] = useState<string | null>(null);
  const request = useRef(0);

  const load = useCallback(async () => {
    const current = ++request.current;
    try {
      const history = await native<ProjectHistoryView | null>('get_project_history');
      if (current === request.current) setData(history);
    } catch (problem) {
      if (current === request.current) { setData(null); error(problem instanceof Error ? problem.message : String(problem)); }
    }
  }, [error]);

  useEffect(() => { void load(); }, [load, projectId, refreshKey]);

  const alternatives = (data?.branches ?? []).filter(branch => !branch.primary);
  const fail = (problem: unknown) => error(problem instanceof Error ? problem.message : String(problem));
  const act = (operation: () => Promise<WorkspaceView>, message: string | ((workspace: WorkspaceView) => string)) => {
    if (busy) return;
    setBusy(true);
    void operation()
      .then(workspace => {
        apply(workspace);
        notice(typeof message === 'string' ? message : message(workspace));
        setCreating(false);
        setName('');
        setRenaming(null);
        setDeleting(null);
      })
      .catch(fail)
      .finally(() => { setBusy(false); void load(); });
  };

  const restore = (version: VersionSummary) => act(
    () => native<WorkspaceView>('restore_version', { version: version.id }),
    'Version restored. Undo puts the newer work back.',
  );
  const switchTo = (branch: string) => {
    if (branch === data?.branch) return;
    act(() => native<WorkspaceView>('switch_alternative', { branch }), `Switched to “${branch}”.`);
  };
  const alternativeRow = (branch: BranchSummary) => <li key={branch.name}>
    {renaming === branch.name
      ? <form className="history-inline" onSubmit={event => {
          event.preventDefault();
          const value = renameTo.trim();
          if (!value) return;
          act(() => native<WorkspaceView>('rename_alternative', { from: branch.name, to: value }), `Renamed the alternative to “${value}”.`);
        }}>
          <input autoFocus aria-label="Alternative name" maxLength={60} value={renameTo} onChange={event => setRenameTo(event.target.value)} />
          <button className="primary" disabled={busy || !renameTo.trim()}>Rename</button>
          <button type="button" onClick={() => setRenaming(null)}>Cancel</button>
        </form>
      : deleting === branch.name
        ? <div className="history-inline">
            <span className="muted">Delete “{branch.name}” and its versions?</span>
            <button type="button" className="danger" disabled={busy} onClick={() => act(() => native<WorkspaceView>('delete_alternative', { name: branch.name }), `Deleted “${branch.name}”.`)}>Delete</button>
            <button type="button" onClick={() => setDeleting(null)}>Cancel</button>
          </div>
        : <div className="history-inline">
            <span className="history-alternative-name">{branch.name}</span>
            <button type="button" className="history-action" disabled={busy} onClick={() => { setRenaming(branch.name); setRenameTo(branch.name); setDeleting(null); }}>Rename</button>
            <button type="button" className="history-action" disabled={busy} onClick={() => { setDeleting(branch.name); setRenaming(null); }}>Delete</button>
          </div>}
  </li>;

  return <div className="history-panel">
    <label className="history-branch">Alternative
      <select value={data?.branch ?? ''} disabled={busy || !data} onChange={event => switchTo(event.target.value)}>
        {(data?.branches ?? []).map(branch => <option key={branch.name} value={branch.name}>{branch.primary ? `${branch.name} (main)` : branch.name}</option>)}
      </select>
    </label>
    {creating
      ? <form className="history-inline" onSubmit={event => {
          event.preventDefault();
          const value = name.trim();
          if (!value) return;
          act(() => native<WorkspaceView>('create_alternative', { name: value }), `Created “${value}”.`);
        }}>
          <input autoFocus aria-label="Alternative name" maxLength={60} placeholder="Alternative name" value={name} onChange={event => setName(event.target.value)} />
          <button className="primary" disabled={busy || !name.trim()}>Create</button>
          <button type="button" onClick={() => { setCreating(false); setName(''); }}>Cancel</button>
        </form>
      : <button type="button" className="history-action" disabled={busy || !data} onClick={() => { setCreating(true); setRenaming(null); setDeleting(null); }}>New alternative</button>}
    <button type="button" className="history-action" disabled={busy || !data} onClick={() => compact()}>Compact history</button>
    {alternatives.length > 0 && <ul className="history-alternatives">{alternatives.map(alternativeRow)}</ul>}
    {data && data.versions.length > 0
      ? <ol className="history-list">
          {data.versions.map(version => <li key={version.id} className={version.head ? 'current' : undefined}>
            <div className="history-version">
              <strong>{version.label || 'Saved'}</strong>
              <span className="muted">{editedAgo(version.createdAt)}{version.head ? ' · Current' : ''}</span>
            </div>
            {!version.head && <button type="button" disabled={busy} onClick={() => restore(version)}>Restore</button>}
          </li>)}
        </ol>
      : <p className="muted history-empty">Saving the project adds the first version here.</p>}
  </div>;
}
