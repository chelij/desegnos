import { useCallback, useEffect, useRef, useState, type FormEvent } from 'react';
import { native } from './api';
import type { AiSettingsView, IdentityAnalysis, IdentitySource } from './types';
import './newProject.css';

// Import context: read a website, folder, project or ZIP, let the AI find the brand's colours,
// fonts, tone and logos, then add the chosen findings to the open project's brief.
function message(error: unknown) { return error instanceof Error ? error.message : String(error); }
function initialFiles(source: IdentitySource) {
  let total = 0;
  return source.files.filter(file => { if (total + file.textBytes > 480 * 1024) return false; total += file.textBytes; return true; }).map(file => file.id);
}
type Props = { busy: boolean; settings: AiSettingsView | null; openSettings: () => void; importInto: (analysisId: string, findings: number[], assets: string[]) => Promise<void>; close: () => void };

export function ImportContext({ busy, settings, openSettings, importInto, close }: Props) {
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
    if (locked || !analysis || !candidate || !keepable) return;
    void run(async () => { await importInto(analysis.jobId, findings, assets); close(); });
  }
  const selectedBytes = source?.files.filter(file => files.includes(file.id)).reduce((total, file) => total + file.textBytes, 0) ?? 0;
  const photos = source?.photos ?? [];
  const work = candidate?.content ?? [];
  // Photos and existing work always come along; findings and logos are chosen.
  const keepable = findings.length > 0 || assets.length > 0 || photos.length > 0 || work.length > 0;
  const logos = source?.files.flatMap(file => file.asset && candidate?.logoAssetIds.includes(file.asset.id) ? [file.asset] : []) ?? [];
  return <form className="new-project import-context dialog-body" onSubmit={submit}>
    <p className="field-help">Add your brand to this project: the AI reads a website, folder, project or ZIP, and you choose which colours, fonts, tone and logos to keep. They become rules the AI follows when it builds your pages.</p>
      <section className="identity-import" aria-label="Use my existing brand">
        <div className="identity-address"><label>Website address<input type="url" inputMode="url" placeholder="example.com" value={address} disabled={locked} onChange={event => setAddress(event.target.value)} onKeyDown={event => { if (event.key === 'Enter') { event.preventDefault(); if (address.trim()) void fetchSite(); } }} /></label>
          <button type="button" disabled={locked || !address.trim()} onClick={() => { void fetchSite(); }}>{working && address.trim() ? 'Reading…' : 'Read website'}</button></div>
        <p className="field-help">Reads the home page, its styles and logo images over the public web. Nothing on the website is changed.</p>
        <div className="identity-source-actions"><span className="identity-or">or</span><button type="button" disabled={locked} onClick={() => { void pick('file'); }}>Choose project or ZIP</button><button type="button" disabled={locked} onClick={() => { void pick('folder'); }}>Choose website folder</button></div>
        {!source && recoveryError && <div className="message error" role="alert"><span>{recoveryError}</span><button type="button" disabled={locked || readingPrevious} onClick={() => { void readPrevious(); }}>Retry loading brand review</button></div>}
        {!source && previous && <button type="button" disabled={locked} onClick={() => { setSource(previous.source); setFiles(previous.fileIds); acceptAnalysis(previous); setPrevious(null); }}>Continue previous brand review</button>}
        {source && <>
          <strong className="identity-source-name">{source.name}</strong>
          <details className="identity-file-review"><summary>Files to read · {files.length} selected</summary><div className="identity-files">{source.files.map(file => <label className="identity-choice" key={file.id}><input type="checkbox" disabled={locked || !!candidate} checked={files.includes(file.id)} onChange={event => { setFiles(current => event.target.checked ? [...current, file.id] : current.filter(id => id !== file.id)); setAnalysis(null); }} /><span>{file.name}<small>{file.textBytes ? `${Math.ceil(file.textBytes / 1024)} KB` : 'Image'}{file.asset ? ' · possible logo' : ''}</small></span></label>)}</div><p className="field-help">Selected files and logo images go to {settings?.authentication === 'chatgpt' ? 'ChatGPT' : 'OpenAI'} for analysis.</p></details>
          {photos.length > 0 && !candidate && <p className="field-help">{photos.length} photo{photos.length === 1 ? '' : 's'} found. Small previews go to the AI so it can describe them; the photos are kept with your project for the AI to place on pages.</p>}
          {source.skipped.length > 0 && <details><summary>Skipped files · {source.skipped.length}</summary><ul className="identity-skipped">{source.skipped.map((reason, index) => <li key={index}>{reason}</li>)}</ul></details>}
          {selectedBytes >= 512 * 1024 && <p role="alert">Choose fewer files to keep this analysis under 512 KB.</p>}
          {!candidate && <p className="field-help">Analysis sends the selected files and logos to {analysisMethod === 'chatgpt' ? `ChatGPT using ${analysisAccount?.label ? `${analysisAccount.label}’s` : 'your'} plan` : 'OpenAI using your API key'}.</p>}
          {analyzing ? <div className="identity-progress" role="status"><span>{analysis.message || 'Reading your brand…'}</span><button type="button" disabled={working} onClick={() => { void cancel(); }}>Cancel analysis</button></div> : !candidate ? <>
            {analysis && <p role={analysis.status === 'failed' || analysis.status === 'unknown' ? 'alert' : 'status'}>{analysis.error || analysis.message}</p>}
            <div className="identity-source-actions"><button type="button" className="primary" disabled={locked || (!files.length && !photos.length) || selectedBytes >= 512 * 1024 || !connected} onClick={() => { void analyze(); }}>Analyze brand</button><button type="button" disabled={locked} onClick={openSettings}>{connected ? 'AI settings' : 'Connect AI'}</button></div>
          </> : <div className="identity-result" aria-label="Brand review">
            <h2>{candidate.brandName || 'Brand found'}</h2><p>{candidate.summary}</p>{candidate.tone && <p className="field-help">Tone · {candidate.tone}</p>}
            <div className="identity-findings">{candidate.findings.map((finding, index) => <div key={index}><label className="identity-choice"><input type="checkbox" checked={findings.includes(index)} disabled={locked} onChange={event => setFindings(current => event.target.checked ? [...current, index] : current.filter(value => value !== index))} /><span><strong>{finding.topic}</strong><span>{finding.value}</span></span></label><details><summary>{finding.confidence === 'low' ? 'Uncertain · ' : ''}Sources</summary><ul>{finding.evidence.map(id => <li key={id}>{source.files.find(file => file.id === id)?.name ?? id}</li>)}</ul></details></div>)}</div>
            {logos.length > 0 && <fieldset className="identity-logos"><legend>Logos to keep</legend>{logos.map(asset => <label className="identity-logo" key={asset.id}><img src={asset.dataUri} alt={asset.name} /><span><input type="checkbox" checked={assets.includes(asset.id)} disabled={locked} onChange={event => setAssets(current => event.target.checked ? [...current, asset.id] : current.filter(id => id !== asset.id))} />{asset.name}</span></label>)}</fieldset>}
            {work.length > 0 && <details className="identity-work" open><summary>Existing work · {work.length}</summary><p className="field-help">The AI will know about these and link to them when a page mentions them.</p><ul>{work.map((item, index) => <li key={index}><strong>{item.title}</strong> <small>{item.kind}{item.url ? ` · ${item.url}` : ''}</small>{item.summary && <span>{item.summary}</span>}</li>)}</ul></details>}
            {photos.length > 0 && <details className="identity-photos" open><summary>Photos · {photos.length}</summary><p className="field-help">Kept with your project so the AI can use them on pages.</p><div className="identity-photo-grid">{photos.map(photo => <figure key={photo.id}><img src={photo.thumbnail} alt="" /><figcaption>{candidate.photos?.find(item => item.id === photo.id)?.description || photo.name}</figcaption></figure>)}</div></details>}
            {candidate.warnings.length > 0 && <ul className="identity-warnings">{candidate.warnings.map((warning, index) => <li key={index}>{warning}</li>)}</ul>}
            <button type="button" disabled={locked} onClick={() => { setAnalysis(null); setFindings([]); setAssets([]); }}>Change files</button>
          </div>}
        </>}
      </section>
    {candidate && !keepable && <p className="field-help">Choose at least one finding or logo to keep.</p>}
    <div className="form-actions"><button type="button" disabled={locked} onClick={close}>Cancel</button><button className="primary" type="submit" disabled={locked || !candidate || !keepable}>{busy || working ? 'Importing…' : 'Add to project'}</button></div>
    {error && <p className="message error" role="alert">{error}</p>}
  </form>;
}
