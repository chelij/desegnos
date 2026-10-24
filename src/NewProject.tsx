import { ProblemBanner } from './ProblemBanner.js';
import { RecoveryChoice, type RecoveryChoices } from './RecoveryBanner.js';
import { userError } from './userErrors.js';
import { useState, type FormEvent } from 'react';
import type { ProjectLocationView, Viewport } from './types';
import { native } from './api';
import { ProjectLocationSettings } from './ProjectLocation';
import { Icon } from './icons';
import './newProject.css';

const sizes = {
  web: [[1440, 900], [1920, 1080], [1536, 864], [1366, 768], [1280, 720], [2560, 1440]],
  mobile: [[390, 844], [414, 896], [360, 800], [384, 832], [393, 873], [360, 780]],
} as const;
function message(error: unknown) { return error && typeof error === 'object' && 'message' in error ? String(error.message) : String(error); }
type Props = { importBrand: () => void; busy: boolean; create: (name: string, viewport: Viewport, parentDirectory?: string) => Promise<void>; open: () => Promise<void>; openFolder: () => Promise<void>; location: ProjectLocationView | null; changeLocation: (reset: boolean) => Promise<void>; recentProjects?: { path: string; name: string }[]; openRecent?: (path: string) => Promise<void>; forgetRecent: (path: string) => Promise<void>; recovery?: RecoveryChoices | null };

export function NewProject({ importBrand, busy, create, open, openFolder, location, changeLocation, recentProjects = [], openRecent, forgetRecent, recovery = null }: Props) {
  const [mode, setMode] = useState<'blank' | null>(null);
  const [name, setName] = useState('');
  const [platform, setPlatform] = useState<'web' | 'mobile'>('web');
  const [viewport, setViewport] = useState<Viewport>({ width: 1440, height: 900 });
  const [custom, setCustom] = useState(false);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState('');
  const [missingRecent, setMissingRecent] = useState<string | null>(null);
  const [parentDirectory, setParentDirectory] = useState<string>();
  const locked = busy || working;
  async function run(operation: () => Promise<void>, recentPath?: string) {
    setWorking(true); setError(''); setMissingRecent(null);
    try { await operation(); } catch (error) {
      setError(message(error));
      if (recentPath && error && typeof error === 'object' && 'missing' in error && error.missing === true) setMissingRecent(recentPath);
    }
    finally { setWorking(false); }
  }
  function submit(event: FormEvent) {
    event.preventDefault();
    if (!mode || locked) return;
    void run(() => create(name.trim(), viewport, parentDirectory));
  }
  return <form className="new-project simple-new-project" onSubmit={submit}>
    <h1>{mode === 'blank' ? 'Start with a blank website' : 'Create your website'}</h1>
    {error && <ProblemBanner message={userError(error)} dismiss={() => setError('')} action={missingRecent ? { label: 'Remove from recent', disabled: locked, onClick: () => { void run(() => forgetRecent(missingRecent)); } } : mode === 'blank' ? { label: 'Choose another location', disabled: locked, onClick: () => { void run(async () => { const chosen = await native<string | null>('pick_project_location'); if (chosen) setParentDirectory(chosen); }); } } : { label: 'Choose another location', disabled: locked, onClick: () => { void run(open); } }} />}
    {!mode ? <>
      <div className="project-start-choices">
        <button type="button" aria-label="Blank website" disabled={locked} onClick={() => setMode('blank')}><span className="start-icon" aria-hidden="true"><Icon name="plus" /></span><strong>Blank website</strong></button>
        <button type="button" disabled={locked} onClick={importBrand}><span className="start-icon" aria-hidden="true"><Icon name="fill-color" /></span><strong>Use my existing brand</strong></button>
      </div>
      <div className="start-open"><button type="button" onClick={() => { void run(open); }} disabled={locked}>Open project</button><button type="button" onClick={() => { void run(openFolder); }} disabled={locked}>Open folder…</button></div>
      {((recentProjects.length > 0 && openRecent) || recovery) && <section className="recent-projects" aria-label="Recent projects"><h2>Recent projects</h2>
        {recovery && !recentProjects.some(project => project.path === recovery.path) && <RecoveryChoice {...recovery} />}
        <ul>{recentProjects.map(project => { const recovering = !!recovery && project.path === recovery.path; return <li key={project.path}>
        {recovering && recovery ? <RecoveryChoice variant="card" {...recovery} /> : <><button className="recent-open" type="button" disabled={locked} title={project.path} onClick={() => { if (openRecent) void run(() => openRecent(project.path), project.path); }}>
        <span className="recent-avatar" aria-hidden="true">{project.name.trim().charAt(0).toUpperCase() || '·'}</span>
        <span className="recent-text"><strong>{project.name}</strong><span>{project.path.split(/[\\/]/).slice(-2, -1)[0] ?? project.path}</span></span>
      </button><button className="recent-remove icon-button sm" type="button" disabled={locked} aria-label={`Remove ${project.name} from recent projects`} title="Remove from recent" onClick={() => { void run(() => forgetRecent(project.path)); }}><Icon name="close" /></button></>}</li>; })}</ul></section>}
    </> : <>
      <label>Project name<input name="projectName" autoFocus required maxLength={120} value={name} onChange={event => setName(event.target.value)} autoComplete="off" disabled={locked} /></label>
      <div><strong>Location</strong><p className="field-help" style={{ overflowWrap: 'anywhere' }}>{parentDirectory ?? location?.path ?? 'Loading project location…'}</p>
        <button type="button" className="link-button" disabled={locked} onClick={() => { void run(async () => { const chosen = await native<string | null>('pick_project_location'); if (chosen) setParentDirectory(chosen); }); }}>Choose folder…</button>
        {parentDirectory && <> · <button type="button" className="link-button" disabled={locked} onClick={() => setParentDirectory(undefined)}>Use default</button></>}
      </div>
      <ProjectLocationSettings location={location} change={changeLocation} />
      <fieldset disabled={locked}><legend>Screen guide</legend><div className="segmented">
        {(['web', 'mobile'] as const).map(value => <button type="button" key={value} aria-pressed={platform === value} onClick={() => { setPlatform(value); setCustom(false); setViewport({ width: sizes[value][0][0], height: sizes[value][0][1] }); }}>{value === 'web' ? 'Web' : 'Mobile'}</button>)}
      </div></fieldset>
      <label>Resolution<select disabled={locked} value={custom ? 'custom' : `${viewport.width}x${viewport.height}`} onChange={event => {
        setCustom(event.target.value === 'custom');
        if (event.target.value !== 'custom') { const [width, height] = event.target.value.split('x').map(Number); setViewport({ width, height }); }
      }}>{sizes[platform].map(([width, height]) => <option key={`${width}x${height}`} value={`${width}x${height}`}>{width} × {height}</option>)}<option value="custom">Custom size</option></select></label>
      {custom && <div className="field-pair custom-screen"><label>Width <span className="unit">px</span><input name="width" type="number" required min={1} max={32768} value={viewport.width} disabled={locked} onChange={event => setViewport({ ...viewport, width: Number(event.target.value) })} /></label><label>Height <span className="unit">px</span><input name="height" type="number" required min={1} max={100000} value={viewport.height} disabled={locked} onChange={event => setViewport({ ...viewport, height: Number(event.target.value) })} /></label></div>}
      <div className="form-actions"><button type="button" disabled={locked} onClick={() => { setMode(null); setError(''); }}>Back</button><button className="primary" disabled={locked || !name.trim()} type="submit">{busy ? 'Creating…' : 'Create project'}</button></div>
    </>}
  </form>;
}
