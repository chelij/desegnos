import { useEffect, useState } from 'react';
import { native } from './api';
import { Notice, ProblemBanner } from './ProblemBanner.js';
import type { Page } from './types';

type Selection = { token: string; review: { updated: string[]; added: string[]; kept: Pick<Page, 'id' | 'name' | 'path'>[] } };
type Props = { sourceId: string; revision: number; homePageId: string; busy: boolean; beforePick: () => Promise<void>; apply: (token: string, remove: string[]) => Promise<void> };

export function ImportWebsite({ sourceId, revision, homePageId, busy, beforePick, apply }: Props) {
  const [selection, setSelection] = useState<Selection | null>(null);
  const [remove, setRemove] = useState<string[]>([]);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState('');
  const [imported, setImported] = useState(false);
  useEffect(() => {
    let stopped = false;
    setSelection(null); setRemove([]); setError('');
    if (imported) return;
    setWorking(true);
    void (async () => {
      await beforePick();
      return native<Selection | null>('prepare_website_import', { sourceId });
    })().then(value => { if (!stopped) setSelection(value); })
      .catch(error => { if (!stopped) setError(String(error)); })
      .finally(() => { if (!stopped) setWorking(false); });
    return () => { stopped = true; };
  }, [sourceId, revision, imported]);
  async function submit() {
    if (!selection) return;
    setWorking(true); setError('');
    try { await apply(selection.token, remove); setImported(true); setSelection(null); }
    catch (error) { setError(String(error)); }
    finally { setWorking(false); }
  }
  const locked = busy || working;
  // The home page always stays, so it is never offered for removal.
  const removable = selection?.review.kept.filter(page => page.id !== homePageId) ?? [];
  if (!selection && !error && !imported) return null;
  return <section aria-label="Pages">
    <h2>Pages</h2>
    {imported && <Notice message="Pages imported." />}
    {error && <ProblemBanner message={error} dismiss={() => setError('')} />}
    {selection && <>
      {selection.review.updated.length > 0 && <><h3>Update {selection.review.updated.length}</h3><ul className="import-pages">{selection.review.updated.map(path => <li key={path}>{path}</li>)}</ul></>}
      {selection.review.added.length > 0 && <><h3>Add {selection.review.added.length}</h3><ul className="import-pages">{selection.review.added.map(path => <li key={path}>{path}</li>)}</ul></>}
      {removable.length > 0 && <details className="import-remove">
        <summary>Remove pages not in this import{remove.length > 0 ? ` · ${remove.length}` : ''}</summary>
        <ul className="import-pages">{removable.map(page => <li key={page.id}><label className="check-label"><input type="checkbox" disabled={locked} checked={remove.includes(page.id)} onChange={event => setRemove(ids => event.target.checked ? [...ids, page.id] : ids.filter(id => id !== page.id))} />{page.path}</label></li>)}</ul>
      </details>}
      <div className="form-actions"><button type="button" className="primary" disabled={locked} onClick={() => void submit()}>Import pages</button></div>
    </>}
  </section>;
}
