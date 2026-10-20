import { useCallback, useEffect, useRef, useState, type FormEvent } from 'react';
import { native } from './api';
import type { AiSettingsView, IdentityAnalysis, IdentitySource, NewProjectIdentity, Viewport } from './types';
import './newProject.css';

const sizes = {
  web: [[1440, 900], [1920, 1080], [1536, 864], [1366, 768], [1280, 720], [2560, 1440]],
  mobile: [[390, 844], [414, 896], [360, 800], [384, 832], [393, 873], [360, 780]],
} as const;
function message(error: unknown) { return error instanceof Error ? error.message : String(error); }
function initialFiles(source: IdentitySource) {
  let total = 0;
  return source.files.filter(file => { if (total + file.textBytes > 480 * 1024) return false; total += file.textBytes; return true; }).map(file => file.id);
}
type Props = { busy: boolean; create: (name: string, viewport: Viewport, identity?: NewProjectIdentity) => Promise<void>; open: () => Promise<void>; recentProjects?: { path: string; name: string }[]; openRecent?: (path: string) => Promise<void>; settings: AiSettingsView | null; openSettings: () => void };

export function NewProject({ busy, create, open, recentProjects = [], openRecent, settings, openSettings }: Props) {
  const [mode, setMode] = useState<'blank' | 'identity' | null>(null);
  const [name, setName] = useState('');
  const [platform, setPlatform] = useState<'web' | 'mobile'>('web');
  const [viewport, setViewport] = useState<Viewport>({ width: 1440, height: 900 });
  const [custom, setCustom] = useState(false);
  const [source, setSource] = useState<IdentitySource | null>(null);
  const [files, setFiles] = useState<string[]>([]);
  const [analysis, setAnalysis] = useState<IdentityAnalysis | null>(null);
  const [previous, setPrevious] = useState<IdentityAnalysis | null>(null);
  const [findings, setFindings] = useState<number[]>([]);
  const [assets, setAssets] = useState<string[]>([]);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState('');
  const [recoveryError, setRecoveryError] = useState('');
  const [readingPrevious, setReadingPrevious] = useState(false);
  const mounted = useRef(false);
  const previousRead = useRef(0);
  const active = useRef<string | null>(null);
  const picker = useRef(0);
  const analyzing = analysis?.status === 'analyzing';
  const locked = busy || working || analyzing;
  const analysisMethod = analysis?.authentication ?? settings?.authentication;
  const analysisAccount = settings?.chatgpt.accounts.find(item => item.id === analysis?.chatgptAccountId);
  const candidate = analysis?.status === 'complete' ? analysis.candidate : null;
  const account = settings?.chatgpt.accounts.find(item => item.id === settings.chatgpt.activeAccountId);
  const connected = !settings?.configurationError && (settings?.authentication === 'chatgpt' ? account?.connected && account.planEnabled && !account.storageRepairRequired : settings?.credential.configured);

  const readPrevious = useCallback(async () => {
    const serial = ++previousRead.current;
    setReadingPrevious(true);
    try {
      const value = await native<IdentityAnalysis | null>('get_identity_analysis');
      if (mounted.current && serial === previousRead.current) { setPrevious(value); setRecoveryError(''); }
    } catch (error) { if (mounted.current && serial === previousRead.current) setRecoveryError(`Your previous brand review could not be loaded: ${message(error)}`); }
    finally { if (mounted.current && serial === previousRead.current) setReadingPrevious(false); }
  }, []);
  useEffect(() => {
    mounted.current = true;
    void readPrevious();
    return () => { mounted.current = false; previousRead.current += 1; if (active.current) void native('cancel_identity_analysis', { jobId: active.current }).catch(() => {}); };
  }, [readPrevious]);
  function acceptAnalysis(value: IdentityAnalysis) {
    setAnalysis(value);
    active.current = value.status === 'analyzing' ? value.jobId : null;
    if (value.status === 'complete' && value.candidate) {
      setFindings(value.candidate.findings.flatMap((finding, index) => finding.confidence === 'low' ? [] : [index]));
      setAssets(value.candidate.logoAssetIds);
    }
  }
  useEffect(() => {
    if (!analyzing) return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        const value = await native<IdentityAnalysis | null>('get_identity_analysis');
        if (!stopped && value?.jobId === active.current) { acceptAnalysis(value); setError(''); }
      } catch (error) { if (!stopped) setError(`Could not refresh analysis: ${message(error)}`); }
      if (!stopped) timer = setTimeout(() => { void poll(); }, 900);
    }
    timer = setTimeout(() => { void poll(); }, 500);
    return () => { stopped = true; clearTimeout(timer); };
  }, [analyzing]);
  async function run(operation: () => Promise<void>) {
    setWorking(true); setError('');
    try { await operation(); } catch (error) { if (mounted.current) setError(message(error)); }
    finally { if (mounted.current) setWorking(false); }
  }
  async function pick(kind: 'file' | 'folder') {
    const serial = ++picker.current;
    await run(async () => {
      const value = await native<IdentitySource | null>('pick_identity_source', { kind });
      if (!mounted.current || serial !== picker.current || !value) return;
      setSource(value); setFiles(initialFiles(value)); setAnalysis(null); setFindings([]); setAssets([]);
    });
  }
  const [address, setAddress] = useState('');
  async function fetchSite(event?: FormEvent) {
    event?.preventDefault();
    const serial = ++picker.current;
    await run(async () => {
      const value = await native<IdentitySource>('fetch_identity_source', { url: address.trim() });
      if (!mounted.current || serial !== picker.current) return;
      setSource(value); setFiles(initialFiles(value)); setAnalysis(null); setFindings([]); setAssets([]);
    });
  }
  async function analyze() {
    if (!source) return;
    await run(async () => {
      const value = await native<IdentityAnalysis>('analyze_identity', { sourceId: source.id, fileIds: files });
      if (!mounted.current) { if (value.status === 'analyzing') await native('cancel_identity_analysis', { jobId: value.jobId }); return; }
      acceptAnalysis(value);
    });
  }
  async function cancel() {
    if (!analysis) return;
    await run(async () => acceptAnalysis(await native<IdentityAnalysis>('cancel_identity_analysis', { jobId: analysis.jobId })));
  }
  function submit(event: FormEvent) {
    event.preventDefault();
    if (!mode || locked || (mode === 'identity' && (!candidate || !findings.length && !assets.length))) return;
    void run(() => create(name.trim(), viewport, mode === 'identity' && analysis ? { identityAnalysisId: analysis.jobId, identityFindingIndices: findings, identityAssetIds: assets } : undefined));
  }
  const selectedBytes = source?.files.filter(file => files.includes(file.id)).reduce((total, file) => total + file.textBytes, 0) ?? 0;
  const logos = source?.files.flatMap(file => file.asset && candidate?.logoAssetIds.includes(file.asset.id) ? [file.asset] : []) ?? [];
  return <form className="new-project simple-new-project" onSubmit={submit}>
    <h1>{mode === 'identity' ? 'Use your existing brand' : mode === 'blank' ? 'Start with a blank website' : 'Create your website'}</h1>
    {!mode ? <>
      <p className="new-project-intro">Place boxes where things go, describe them, generate your website with AI, then edit and export it.</p>
      <div className="project-start-choices">
        <button type="button" aria-label="Blank website" disabled={locked} onClick={() => setMode('blank')}><span className="start-icon" aria-hidden="true"><svg viewBox="0 0 20 20"><path d="M10 4v12M4 10h12" /></svg></span><strong>Blank website</strong><span>Start from an empty page and place your own boxes.</span></button>
        <button type="button" aria-label="Use my existing brand" disabled={locked} onClick={() => setMode('identity')}><span className="start-icon" aria-hidden="true"><svg viewBox="0 0 20 20"><circle cx="10" cy="10" r="6.5" /><path d="M10 3.5a6.5 6.5 0 0 1 0 13Z" fill="currentColor" /></svg></span><strong>Use my existing brand</strong><span>Read colors, fonts and logos from a website or project.</span></button>
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
      {mode === 'identity' && <section className="identity-import" aria-label="Use my existing brand">
        <div className="identity-address"><label>Website address<input type="url" inputMode="url" placeholder="example.com" value={address} disabled={locked} onChange={event => setAddress(event.target.value)} onKeyDown={event => { if (event.key === 'Enter') { event.preventDefault(); if (address.trim()) void fetchSite(); } }} /></label>
          <button type="button" disabled={locked || !address.trim()} onClick={() => { void fetchSite(); }}>{working && address.trim() ? 'Reading…' : 'Read website'}</button></div>
        <p className="field-help">Reads the home page, its styles and logo images over the public web. Nothing on the website is changed.</p>
        <div className="identity-source-actions"><span className="identity-or">or</span><button type="button" disabled={locked} onClick={() => { void pick('file'); }}>Choose project or ZIP</button><button type="button" disabled={locked} onClick={() => { void pick('folder'); }}>Choose website folder</button></div>
        {!source && recoveryError && <div className="message error" role="alert"><span>{recoveryError}</span><button type="button" disabled={locked || readingPrevious} onClick={() => { void readPrevious(); }}>Retry loading brand review</button></div>}
        {!source && previous && <button type="button" disabled={locked} onClick={() => { setSource(previous.source); setFiles(previous.fileIds); acceptAnalysis(previous); setPrevious(null); }}>Continue previous brand review</button>}
        {source && <>
          <strong className="identity-source-name">{source.name}</strong>
          <details className="identity-file-review"><summary>Files to read · {files.length} selected</summary><div className="identity-files">{source.files.map(file => <label className="identity-choice" key={file.id}><input type="checkbox" disabled={locked || !!candidate} checked={files.includes(file.id)} onChange={event => { setFiles(current => event.target.checked ? [...current, file.id] : current.filter(id => id !== file.id)); setAnalysis(null); }} /><span>{file.name}<small>{file.textBytes ? `${Math.ceil(file.textBytes / 1024)} KB` : 'Image'}{file.asset ? ' · possible logo' : ''}</small></span></label>)}</div><p className="field-help">Selected files and logo images go to {settings?.authentication === 'chatgpt' ? 'ChatGPT' : 'OpenAI'} for analysis.</p></details>
          {source.skipped.length > 0 && <details><summary>Skipped files · {source.skipped.length}</summary><ul className="identity-skipped">{source.skipped.map((reason, index) => <li key={index}>{reason}</li>)}</ul></details>}
          {selectedBytes >= 512 * 1024 && <p role="alert">Choose fewer files to keep this analysis under 512 KB.</p>}
          {!candidate && <p className="field-help">Analysis sends the selected files and logos to {analysisMethod === 'chatgpt' ? `ChatGPT using ${analysisAccount?.label ? `${analysisAccount.label}’s` : 'your'} plan` : 'OpenAI using your API key'}.</p>}
          {analyzing ? <div className="identity-progress" role="status"><span>{analysis.message || 'Reading your brand…'}</span><button type="button" disabled={working} onClick={() => { void cancel(); }}>Cancel analysis</button></div> : !candidate ? <>
            {analysis && <p role={analysis.status === 'failed' || analysis.status === 'unknown' ? 'alert' : 'status'}>{analysis.error || analysis.message}</p>}
            <div className="identity-source-actions"><button type="button" className="primary" disabled={locked || !files.length || selectedBytes >= 512 * 1024 || !connected} onClick={() => { void analyze(); }}>Analyze brand</button><button type="button" disabled={locked} onClick={openSettings}>{connected ? 'AI settings' : 'Connect AI'}</button></div>
          </> : <div className="identity-result" aria-label="Brand review">
            <h2>{candidate.brandName || 'Brand found'}</h2><p>{candidate.summary}</p>{candidate.tone && <p className="field-help">Tone · {candidate.tone}</p>}
            <div className="identity-findings">{candidate.findings.map((finding, index) => <div key={index}><label className="identity-choice"><input type="checkbox" checked={findings.includes(index)} disabled={locked} onChange={event => setFindings(current => event.target.checked ? [...current, index] : current.filter(value => value !== index))} /><span><strong>{finding.topic}</strong><span>{finding.value}</span></span></label><details><summary>{finding.confidence === 'low' ? 'Uncertain · ' : ''}Sources</summary><ul>{finding.evidence.map(id => <li key={id}>{source.files.find(file => file.id === id)?.name ?? id}</li>)}</ul></details></div>)}</div>
            {logos.length > 0 && <fieldset className="identity-logos"><legend>Logos to keep</legend>{logos.map(asset => <label className="identity-logo" key={asset.id}><img src={asset.dataUri} alt={asset.name} /><span><input type="checkbox" checked={assets.includes(asset.id)} disabled={locked} onChange={event => setAssets(current => event.target.checked ? [...current, asset.id] : current.filter(id => id !== asset.id))} />{asset.name}</span></label>)}</fieldset>}
            {candidate.warnings.length > 0 && <ul className="identity-warnings">{candidate.warnings.map((warning, index) => <li key={index}>{warning}</li>)}</ul>}
            <button type="button" disabled={locked} onClick={() => { setAnalysis(null); setFindings([]); setAssets([]); }}>Change files</button>
          </div>}
        </>}
      </section>}
      {mode === 'identity' && candidate && !findings.length && !assets.length && <p className="field-help">Choose at least one finding or logo to keep.</p>}
      <div className="form-actions"><button type="button" disabled={locked} onClick={() => { setMode(null); setError(''); }}>Back</button><button className="primary" disabled={locked || !name.trim() || (mode === 'identity' && (!candidate || !findings.length && !assets.length))} type="submit">{busy ? 'Creating…' : 'Create project'}</button></div>
    </>}
    {error && <p className="message error" role="alert">{error}</p>}
  </form>;
}
