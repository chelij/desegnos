import { userError } from './userErrors.js';
import { useState } from 'react';
import type { ProjectLocationView } from './types';

export function ProjectLocationSettings({ location, change }: { location: ProjectLocationView | null; change: (reset: boolean) => Promise<void> }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  async function update(reset: boolean) {
    setBusy(true); setError('');
    try { await change(reset); }
    catch (error) { setError(error instanceof Error ? error.message : String(error)); }
    finally { setBusy(false); }
  }
  return <details className="project-location-settings"><summary>Default project location</summary>
    <p className="field-help" style={{ overflowWrap: 'anywhere' }}>{location?.path ?? 'Loading project location…'}</p>
    <div className="form-actions"><button type="button" disabled={busy} onClick={() => { void update(false); }}>Change…</button><button type="button" disabled={busy || !location?.customized} title={location?.systemDefault} onClick={() => { void update(true); }}>Reset</button></div>
    {error && <p className="message error" role="alert">{userError(error)}</p>}
  </details>;
}
