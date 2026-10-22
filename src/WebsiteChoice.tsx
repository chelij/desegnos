import { useEffect, useRef, useState } from 'react';
import { native } from './api';
import type { WebsiteFolderSelection } from './types';

type BuildStatus = { running: boolean; line: string; error: string | null; folder: WebsiteFolderSelection | null };
export function WebsiteChoice({ folder, brand, disabled, chooseBrand, edit, update }: {
  folder: WebsiteFolderSelection; brand: boolean; disabled: boolean; chooseBrand: () => void;
  edit: () => Promise<void>; update: (folder: WebsiteFolderSelection) => void;
}) {
  const [build, setBuild] = useState<BuildStatus | null>(null);
  const [error, setError] = useState('');
  const running = useRef(false);
  running.current = !!build?.running;
  useEffect(() => () => { if (running.current) void native('website_build', { folderId: folder.id, action: 'stop' }).catch(() => {}); }, [folder.id]);
  useEffect(() => {
    if (!build?.running) return;
    let stopped = false;
    const timer = setInterval(() => { void native<BuildStatus>('website_build', { folderId: folder.id, action: 'status' }).then(value => {
      if (stopped) return;
      setBuild(value); if (value.folder) update(value.folder);
    }).catch(error => { if (!stopped) setError(String(error)); }); }, 500);
    return () => { stopped = true; clearInterval(timer); };
  }, [build?.running, folder.id, update]);
  async function action(action: string) {
    setError('');
    try { setBuild(await native<BuildStatus>('website_build', { folderId: folder.id, action })); }
    catch (error) { setError(String(error)); }
  }
  const site = folder.website;
  if (!site) return null;
  return <section aria-label="Website import" className="website-choice">
    <div className="segmented" role="group" aria-label="Use this folder">
      <button type="button" aria-pressed={!brand} disabled={disabled || build?.running} onClick={() => { void edit().catch(error => setError(String(error))); }}>Edit this website</button>
      <button type="button" aria-pressed={brand} disabled={disabled || build?.running} onClick={chooseBrand}>Use it for my brand only</button>
    </div>
    {!brand && <>
      <p className="field-help">{folder.name} · {site.pages.length} page{site.pages.length === 1 ? '' : 's'}</p>
      {site.framework && <p className="field-help">Edits don't change the {site.framework} source files.</p>}
      {site.buildUnavailable && <p role="status">{site.buildUnavailable}{site.output ? " Using the existing build output; it may be out of date." : ""}</p>}
      {site.fresh || (site.output && site.buildUnavailable) ? <button type="button" className="primary" disabled={disabled || build?.running} onClick={() => { void edit().catch(error => setError(String(error))); }}>Open website for editing</button>
        : site.buildUnavailable ? null
        : site.installCommand ? <p role="status">Run <code>{site.installCommand}</code> in this folder first: {site.path}</p>
        : <button type="button" disabled={disabled || build?.running || !site.buildCommand} onClick={() => { void action('start'); }}>Build site</button>}
    </>}
    {build && <p role="status">{build.line} {build.running && <button type="button" onClick={() => { void action('stop'); }}>Stop</button>}</p>}
    {(error || build?.error) && <p role="alert" className="message error">{error || build?.error}</p>}
  </section>;
}
