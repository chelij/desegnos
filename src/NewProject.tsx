import { useState, type FormEvent } from 'react';
import type { AiSettingsView, Viewport } from './types';
import './newProject.css';

const sizes = {
  web: [[1440, 900], [1920, 1080], [1536, 864], [1366, 768], [1280, 720], [2560, 1440]],
  mobile: [[390, 844], [414, 896], [360, 800], [384, 832], [393, 873], [360, 780]],
} as const;
function message(error: unknown) { return error instanceof Error ? error.message : String(error); }
type Props = { busy: boolean; create: (name: string, viewport: Viewport) => Promise<void>; open: () => Promise<void>; recentProjects?: { path: string; name: string }[]; openRecent?: (path: string) => Promise<void>; settings: AiSettingsView | null; openSettings: () => void };

export function NewProject({ busy, create, open, recentProjects = [], openRecent, settings, openSettings }: Props) {
  const [mode, setMode] = useState<'blank' | null>(null);
  const [name, setName] = useState('');
  const [platform, setPlatform] = useState<'web' | 'mobile'>('web');
  const [viewport, setViewport] = useState<Viewport>({ width: 1440, height: 900 });
  const [custom, setCustom] = useState(false);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState('');
  const locked = busy || working;
  const account = settings?.chatgpt.accounts.find(item => item.id === settings.chatgpt.activeAccountId);
  const connected = !settings?.configurationError && (settings?.authentication === 'chatgpt' ? account?.connected && account.planEnabled && !account.storageRepairRequired : settings?.credential.configured);
  async function run(operation: () => Promise<void>) {
    setWorking(true); setError('');
    try { await operation(); } catch (error) { setError(message(error)); }
    finally { setWorking(false); }
  }
  function submit(event: FormEvent) {
    event.preventDefault();
    if (!mode || locked) return;
    void run(() => create(name.trim(), viewport));
  }
  return <form className="new-project simple-new-project" onSubmit={submit}>
    <h1>{mode === 'blank' ? 'Start with a blank website' : 'Create your website'}</h1>
    {!mode ? <>
      <p className="new-project-intro">Place boxes where things go, describe them, generate your website with AI, then edit and export it. You can import your brand from an existing website later with Import context.</p>
      <div className="project-start-choices">
        <button type="button" aria-label="Blank website" disabled={locked} onClick={() => setMode('blank')}><span className="start-icon" aria-hidden="true"><svg viewBox="0 0 20 20"><path d="M10 4v12M4 10h12" /></svg></span><strong>Blank website</strong><span>Start from an empty page and place your own boxes.</span></button>
      </div>
      {!connected && <p className="start-ai-note">You’ll connect an AI account before your first Generate. <button type="button" className="link-button" onClick={openSettings}>Connect now</button></p>}
      <div className="start-open"><button type="button" className="link-button" onClick={() => { void run(open); }} disabled={locked}>Open project</button><span>a .desegnos file from your computer</span></div>
      {recentProjects.length > 0 && openRecent && <section className="recent-projects" aria-label="Recent projects"><h2>Recent projects</h2><ul>{recentProjects.map(project => <li key={project.path}><button type="button" disabled={locked} title={project.path} onClick={() => { void run(() => openRecent(project.path)); }}>
        <span className="recent-avatar" aria-hidden="true">{project.name.trim().charAt(0).toUpperCase() || '·'}</span>
        <span className="recent-text"><strong>{project.name}</strong><span>{project.path.split(/[\\/]/).slice(-2, -1)[0] ?? project.path}</span></span>
      </button></li>)}</ul></section>}
    </> : <>
      <label>Project name<input name="projectName" autoFocus required maxLength={120} value={name} onChange={event => setName(event.target.value)} autoComplete="off" disabled={locked} /></label>
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
    {error && <p className="message error" role="alert">{error}</p>}
  </form>;
}
