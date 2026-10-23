import { useState } from 'react';
import { native } from './api';
import type { Page } from './types';

type Selection = { token: string; review: { updated: string[]; added: string[]; kept: Pick<Page, 'id' | 'name' | 'path'>[] } };
type Props = { homePageId: string; busy: boolean; beforePick: () => Promise<void>; apply: (token: string, remove: string[]) => Promise<void> };

export function ImportWebsite({ homePageId, busy, beforePick, apply }: Props) {
  const [selection, setSelection] = useState<Selection | null>(null);
  const [remove, setRemove] = useState<string[]>([]);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState('');
  async function pick(kind: 'folder' | 'files') {
    setWorking(true); setError('');
    try {
      await beforePick();
      const picked = await native<Selection | null>('prepare_website_import', { kind });
      if (picked) { setSelection(picked); setRemove([]); }
    } catch (error) { setError(String(error)); }
    finally { setWorking(false); }
  }
  async function submit() {
    if (!selection) return;
    setWorking(true); setError('');
    try { await apply(selection.token, remove); }
    catch (error) { setError(String(error)); }
    finally { setWorking(false); }
  }
  const locked = busy || working;
  return <div className="dialog-body">
    <div className="form-actions"><button disabled={locked} onClick={() => void pick('folder')}>Choose folder…</button><button disabled={locked} onClick={() => void pick('files')}>Choose ZIP or HTML files…</button></div>
    {error && <p role="alert">{error}</p>}
    {selection && <>
      <h3>Update</h3><ul>{selection.review.updated.map(path => <li key={path}>{path}</li>)}</ul>
      <h3>Add</h3><ul>{selection.review.added.map(path => <li key={path}>{path}</li>)}</ul>
      <h3>Keep</h3><ul>{selection.review.kept.map(page => <li key={page.id}><label className="check-label"><input type="checkbox" disabled={locked || page.id === homePageId} title={page.id === homePageId ? 'The home page must be kept.' : undefined} checked={remove.includes(page.id)} onChange={event => setRemove(ids => event.target.checked ? [...ids, page.id] : ids.filter(id => id !== page.id))} />Remove {page.path}</label></li>)}</ul>
      <div className="form-actions"><button className="primary" disabled={locked} onClick={() => void submit()}>Import</button></div>
    </>}
  </div>;
}
