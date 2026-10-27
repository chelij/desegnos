import { needsAiSettings, ProblemBanner, Notice } from './ProblemBanner.js';
import { userError } from './userErrors.js';
import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent, type ReactNode } from 'react';
import { native } from './api';
import type { AiSettingsView, IdentityAnalysis, IdentitySelection, IdentitySource } from './types';
import { groupIdentityPages, selectedPageContent, findingSection, parseColours, parseFonts, parseTone, type IdentityFinding } from './identityReview';
import { Icon } from './icons';

// One source supplies the page import and the optional brand review.
function message(error: unknown) { return error instanceof Error ? error.message : String(error); }
function skippedSummary(skipped: string[]) {
  const counts = { build: 0, code: 0, other: 0 };
  for (const item of skipped) {
    if (/(^|\/|\b)(node_modules|dist|build|out|\.astro|\.next|\.git|vendor|cache|coverage)(\/|:|\b)|lockfile|\.lock|lock\.(json|yaml)/i.test(item)) counts.build += 1;
    else if (/\.(js|jsx|ts|tsx|mjs|cjs)(:|\b)/i.test(item)) counts.code += 1;
    else counts.other += 1;
  }
  return Object.entries(counts).filter(([, count]) => count > 0).map(([kind, count]) => `${count} ${kind} file${count === 1 ? '' : 's'}`).join(', ');
}
function skippedFile(value: string) {
  const separator = value.indexOf(': ');
  if (separator < 0) return /additional excluded entries/.test(value) ? value.replace('excluded entries were omitted from this report.', 'more files were left out.') : value;
  const name = value.slice(0, separator);
  const reason = value.slice(separator + 2);
  const explanation = /credential|private key/.test(reason) ? 'May contain private information'
    : /limit|budget/.test(reason) ? 'Left out to make room for the most useful files'
    : /unsupported file type|not needed|private, generated|app or game code/.test(reason) ? 'Not needed to read your brand'
    : /not selected/.test(reason) ? 'We picked other logos'
    : /UTF-8|binary content/.test(reason) ? "Couldn't read this as text"
    : /symbolic link/.test(reason) ? 'A shortcut to another file or folder'
    : reason;
  return `${name}: ${explanation}`;
}
type Props = { initialSource?: IdentitySource | null; busy: boolean; settings: AiSettingsView | null; openSettings: () => void; importInto: (analysisId: string, findings: number[], assets: string[], photos: string[], items: number[], pages: string[], attachMatching: boolean) => Promise<void>; importWebsite?: () => void; close: () => void };

function ReviewCard({ name, checked, disabled, toggle, children, className = '', finding, source, linked }: {
  name: string; checked: boolean; disabled: boolean; toggle: () => void; children: ReactNode; className?: string;
  finding?: IdentityFinding; source?: IdentitySource | null; linked?: boolean;
}) {
  return <div className={`identity-card-wrap ${className}`}>
    <label className="identity-card" title={finding ? finding.value : name}>
      <input className="sr-only" type="checkbox" aria-label={`Keep ${name}`} checked={checked} disabled={disabled} onChange={toggle} />
      <span className="identity-check" aria-hidden="true"><Icon name="check" size={12} /></span>
      {children}
      {finding?.confidence === 'low' && <span className="identity-unsure">Not sure</span>}
      {linked && <span className="sr-only">Choices from this finding are kept together.</span>}
    </label>
    {finding && <details className="identity-card-info"><summary aria-label={`Details: ${name}`} title="Details and source"><Icon name="info" size={12} /></summary><div>
      <strong>{finding.topic}</strong><p>{finding.value}</p>
      {finding.evidence.length > 0 && <><strong>Source</strong><ul>{finding.evidence.map(id => <li key={id}>{source?.files.find(file => file.id === id)?.name ?? id}</li>)}</ul></>}
    </div></details>}
  </div>;
}

function ReviewGroup({ name, className = '', disabled, choose, children }: { name: string; className?: string; disabled: boolean; choose: (all: boolean) => void; children: ReactNode }) {
  return <section className={`identity-review-section ${className}`} aria-label={name}>
    <div className="identity-section-heading"><h3>{name}</h3><div className="identity-section-actions"><button type="button" disabled={disabled} aria-label={`Keep all ${name.toLowerCase()}`} onClick={() => choose(true)}>All</button><span aria-hidden="true">/</span><button type="button" disabled={disabled} aria-label={`Keep no ${name.toLowerCase()}`} onClick={() => choose(false)}>None</button></div></div>
    <div className="identity-review-grid">{children}</div>
  </section>;
}

export function ImportContext({ busy, settings, openSettings, importInto, importWebsite, close, initialSource }: Props) {
  const [source, setSource] = useState<IdentitySource | null>(initialSource ?? null);
  const [previousBrand, setPreviousBrand] = useState(false);
  const [files, setFiles] = useState<string[]>([]);
  const [automatic, setAutomatic] = useState(true);
  const [pickVersion, setPickVersion] = useState(0);
  const [selection, setSelection] = useState<IdentitySelection | null>(null);
  const [analysis, setAnalysis] = useState<IdentityAnalysis | null>(null);
  const [previous, setPrevious] = useState<IdentityAnalysis | null>(null);
  const [findings, setFindings] = useState<number[]>([]);
  const [assets, setAssets] = useState<string[]>([]);
  const [photoIds, setPhotoIds] = useState<string[]>([]);
  const [itemIndices, setItemIndices] = useState<number[]>([]);
  const [pagePaths, setPagePaths] = useState<string[]>([]);
  const [working, setWorking] = useState(false);
  const [kept, setKept] = useState(false);
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
  const requestedFiles = automatic ? null : files;
  const review = useMemo(() => candidate?.findings.map((finding, index) => ({ finding, index, section: findingSection(finding), colours: parseColours(finding.value, finding.topic), fonts: parseFonts(finding.value, finding.topic) })) ?? [], [candidate]);
  const fontFamilies = useMemo(() => review.filter(item => item.section === 'Fonts').flatMap(item => item.fonts.map(font => font.family)), [review]);
  useEffect(() => {
    // Imports currently supply images; use embedded font assets too when supplied.
    const fontAssets = source?.files.flatMap(file => file.asset && (/^(?:font\/|application\/(?:font|x-font|vnd\.ms-fontobject))/.test(file.asset.mime) || /\.(?:woff2?|ttf|otf)$/i.test(file.asset.name)) ? [file.asset] : []) ?? [];
    const loaded: FontFace[] = [];
    let stopped = false;
    const normalize = (value: string) => value.toLowerCase().replace(/[^a-z0-9]/g, '');
    for (const family of new Set(fontFamilies)) {
      const asset = fontAssets.find(asset => normalize(asset.name.split('/').at(-1)?.replace(/\.[^.]+$/, '') ?? '').startsWith(normalize(family)));
      if (!asset || !/^data:(?:font\/|application\/)[^,]*;base64,/.test(asset.dataUri)) continue;
      try {
        const bytes = Uint8Array.from(atob(asset.dataUri.slice(asset.dataUri.indexOf(',') + 1)), char => char.charCodeAt(0));
        const face = new FontFace(family, bytes);
        void face.load().then(font => { if (!stopped) { document.fonts.add(font); loaded.push(font); } }).catch(() => {});
      } catch { /* Invalid font data keeps the CSS family fallback. */ }
    }
    return () => { stopped = true; loaded.forEach(font => document.fonts.delete(font)); };
  }, [source, fontFamilies]);
  useEffect(() => {
    if (!source) return;
    let stopped = false;
    setSelection(null);
    void native<IdentitySelection>('prepare_identity_selection', { sourceId: source.id, fileIds: requestedFiles }).then(value => {
      if (stopped) return;
      setSelection(value);
      if (requestedFiles === null) setFiles(value.fileIds);
    }).catch(error => { if (!stopped) setSelection({ fileIds: requestedFiles ?? [], fits: false, message: message(error), needsSettings: false }); });
    return () => { stopped = true; };
  }, [source, requestedFiles, pickVersion, settings?.provider, settings?.authentication, settings?.settings.model, settings?.settings.reasoningEffort, settings?.settings.maxOutputTokens, settings?.settings.spendingLimitUsd]);

  const readPrevious = useCallback(async () => {
    const serial = ++previousRead.current;
    setReadingPrevious(true);
    try {
      const value = await native<IdentityAnalysis | null>('get_identity_analysis');
      if (mounted.current && serial === previousRead.current) { setPrevious(value); setRecoveryError(''); }
    } catch (error) { if (mounted.current && serial === previousRead.current) setRecoveryError(message(error)); }
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
      setPhotoIds((value.source.photos ?? []).map(photo => photo.id));
      setItemIndices((value.candidate.content ?? []).map((_, index) => index));
      setPagePaths(groupIdentityPages(value.source, value.candidate.content).map(group => group.page.path));
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
      } catch (error) { if (!stopped) setError(message(error)); }
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
      acceptSource(value);
    });
  }
  const [address, setAddress] = useState('');
  async function fetchSite(event?: FormEvent) {
    event?.preventDefault();
    const serial = ++picker.current;
    await run(async () => {
      const value = await native<IdentitySource>('fetch_identity_source', { url: address.trim() });
      if (!mounted.current || serial !== picker.current) return;
      acceptSource(value);
    });
  }
  function acceptSource(value: IdentitySource) {
    setSource(value); setPreviousBrand(false); setKept(false); setFiles([]); setAutomatic(true); setSelection(null); setAnalysis(null); setFindings([]); setAssets([]); setPhotoIds([]); setItemIndices([]);
  }
  async function analyze() {
    if (!source || !selection?.fits || !connected || locked) return;
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
    if (locked || kept || !analysis || !candidate || !keepable) return;
    void run(async () => { const kept = selectedPageContent(pageGroups, pagePaths, photoIds, itemIndices); await importInto(analysis.jobId, findings, assets, kept.photos, kept.items, pagePaths, true); setKept(true); close(); });
  }
  const pageGroups = useMemo(() => groupIdentityPages(source, candidate?.content), [source, candidate]);
  const keepable = findings.length > 0 || assets.length > 0 || pagePaths.length > 0;
  const logos = source?.files.flatMap(file => file.asset && candidate?.logoAssetIds.includes(file.asset.id) ? [file.asset] : []) ?? [];
  function toggleFinding(index: number) { setFindings(current => current.includes(index) ? current.filter(value => value !== index) : [...current, index]); }
  function chooseFindings(indices: number[], all: boolean) { setFindings(current => all ? [...new Set([...current, ...indices])] : current.filter(index => !indices.includes(index))); }
  function findingCards(items: typeof review) {
    return items.flatMap(({ finding, index, section, colours, fonts }) => {
      const shared = { checked: findings.includes(index), disabled: !!locked, toggle: () => toggleFinding(index), finding, source };
      if (section === 'Colours' && colours.length) return colours.map((colour, part) => <ReviewCard key={`${index}-${part}`} {...shared} name={`${colour.label} ${colour.hex}`} className="identity-swatch-card" linked={colours.length > 1}>
        <span className="identity-swatch" style={{ backgroundColor: colour.hex }} /><strong>{colour.label}</strong><small>{colour.hex}</small>
      </ReviewCard>);
      if (section === 'Fonts' && fonts.length) return fonts.map((font, part) => <ReviewCard key={`${index}-${part}`} {...shared} name={`${font.role}: ${font.family}`} className="identity-font-card" linked={fonts.length > 1}>
        <small>{font.role}</small><span className="identity-font-specimen" style={{ fontFamily: /^(?:serif|sans-serif|monospace|cursive|fantasy|system-ui|ui-[a-z-]+)$/i.test(font.family) ? font.family : `${JSON.stringify(font.family)}, system-ui, sans-serif` }}><strong>Aa {font.family}</strong><span>The quick brown fox.</span></span>
      </ReviewCard>);
      return [<ReviewCard key={index} {...shared} name={finding.topic} className="identity-pill-card">
        <span className="identity-pills">{(section === 'Style & tone' ? parseTone(finding.value) : [finding.value.length > 60 ? finding.value.slice(0, 57) + '…' : finding.value]).map((text, part) => <span className="identity-pill" key={part}>{text}</span>)}</span>
      </ReviewCard>];
    });
  }
  const aiNeedsFixing = !connected || selection?.needsSettings || (analysis?.status === 'failed' && /settings|sign in|connect|account|spending|key|model access|permission/i.test(analysis.error ?? ''));
  return <form className="new-project import-context dialog-body" onSubmit={submit}>
    {error && <ProblemBanner message={userError(error)} dismiss={() => setError('')} action={needsAiSettings(error) ? { label: 'AI settings', onClick: openSettings } : { label: 'Choose another location', disabled: !!locked, onClick: () => { void pick('file'); } }} />}
      <section className="identity-import" aria-label="Brand evidence">
        <>
        <h2>Source</h2>
        <div className="identity-address"><label>Website address<input type="text" inputMode="url" placeholder="example.com" value={address} disabled={locked} onChange={event => setAddress(event.target.value)} onKeyDown={event => { if (event.key === 'Enter') { event.preventDefault(); if (address.trim()) void fetchSite(); } }} /></label>
          <button type="button" disabled={locked || !address.trim()} onClick={() => { void fetchSite(); }}>{working && address.trim() ? 'Opening…' : 'Open website'}</button></div>
        <div className="identity-source-actions"><span className="identity-or">or</span><button type="button" disabled={locked} onClick={() => { void pick('folder'); }}>Choose folder…</button><button type="button" disabled={locked} onClick={() => { void pick('file'); }}>Choose ZIP or HTML files…</button></div>
        {importWebsite && <div className="identity-source-actions"><button type="button" disabled={locked} onClick={importWebsite}>Import a website as pages…</button></div>}
        {source && <h3 className="identity-source-name">{source.name}</h3>}
        {!source && recoveryError && <ProblemBanner message={userError(recoveryError)} action={{ label: 'Try loading again', disabled: !!locked || readingPrevious, onClick: () => { void readPrevious(); } }} />}
        {!source && previous && <button type="button" disabled={locked} onClick={() => { setSource(previous.source); setPreviousBrand(true); setFiles(previous.fileIds); setAutomatic(false); setSelection(null); acceptAnalysis(previous); setPrevious(null); }}>Continue with my previous brand</button>}
        </>
        {source && <h2>Brand and content</h2>}
        {source && !connected && <button type="button" onClick={openSettings}>Connect AI</button>}
        {source && connected && !kept && <>
          {!candidate && <>
          <details className="identity-file-review" key={source.id}><summary>Choose files yourself</summary>
            <p className="field-help">{files.length} file{files.length === 1 ? '' : 's'} picked.</p>
            {!automatic && <button type="button" disabled={locked || !!candidate} onClick={() => { setAutomatic(true); setSelection(null); setPickVersion(current => current + 1); }}>Pick for me</button>}
            <div className="identity-files">{source.files.map(file => <label className="identity-choice" key={file.id}><input type="checkbox" disabled={locked || !!candidate} checked={files.includes(file.id)} onChange={event => { setAutomatic(false); setSelection(null); setFiles(current => event.target.checked ? [...current, file.id] : current.filter(id => id !== file.id)); setAnalysis(null); }} /><span>{file.name}<small>{file.asset ? 'Logo' : `${Math.ceil(file.textBytes / 1024)} KB`}</small></span></label>)}</div>
            {source.skipped.length > 0 && <><p className="field-help">Left out: {skippedSummary(source.skipped)}.</p><details><summary>See all skipped files</summary><ul className="identity-skipped">{source.skipped.map((reason, index) => <li key={index}>{skippedFile(reason)}</li>)}</ul></details></>}
          </details>
          {!candidate && <p className="field-help">Sent to {analysisMethod === 'chatgpt' ? `ChatGPT (${(analysisAccount ?? account)?.label ?? 'current account'})` : `${settings?.providers.find(provider => provider.id === settings.provider)?.name ?? 'OpenAI'} (API key)`}</p>}
          {!candidate && selection?.message && <ProblemBanner message={userError(selection.message)} />}
          </>}
          {analyzing ? <div className="identity-progress" role="status"><span>Reading your brand…</span><button type="button" disabled={working} onClick={() => { void cancel(); }}>Stop reading</button></div> : !candidate ? <>
            {analysis && (analysis.status === 'cancelled' ? <Notice message="Reading stopped." /> : <ProblemBanner message={userError(analysis.error || 'The AI could not read your brand. Try again.')} action={needsAiSettings(analysis.error || '') ? { label: 'AI settings', onClick: openSettings } : undefined} />)}
            <div className="identity-source-actions"><button type="button" className="primary" disabled={locked || !selection?.fits || !connected} onClick={() => { void analyze(); }}>Read my brand</button>{aiNeedsFixing && <button type="button" disabled={locked} onClick={openSettings}>{connected ? 'Check AI settings' : 'Connect AI'}</button>}</div>
          </> : <div className="identity-result" aria-label="Brand review">
            <div className="identity-review-intro"><h3>Choose what to keep</h3></div>
            {(['Colours', 'Fonts', 'Logos', 'Style & tone', 'Pages', 'Other'] as const).map(section => {
              const items = review.filter(item => item.section === section);
              if (!items.length && !(section === 'Logos' && logos.length) && !(section === 'Pages' && pageGroups.length)) return null;
              return <ReviewGroup key={section} name={section} className={section === 'Logos' ? 'identity-logos' : section === 'Pages' ? 'identity-pages' : ''} disabled={!!locked} choose={all => {
                chooseFindings(items.map(item => item.index), all);
                if (section === 'Logos') setAssets(all ? logos.map(asset => asset.id) : []);
                if (section === 'Pages') setPagePaths(all ? pageGroups.map(group => group.page.path) : []);
              }}>
                {findingCards(items)}
                {section === 'Logos' && logos.map(asset => <ReviewCard key={asset.id} name={`logo: ${asset.name}`} className="identity-logo" checked={assets.includes(asset.id)} disabled={!!locked} toggle={() => setAssets(current => current.includes(asset.id) ? current.filter(id => id !== asset.id) : [...current, asset.id])}><img src={asset.dataUri} alt={asset.name} /><span>{asset.name}</span></ReviewCard>)}
                {section === 'Pages' && pageGroups.map(({ page, photos, items }) => <article key={page.path} className={`identity-page-card${pagePaths.includes(page.path) ? ' selected' : ''}`}>
                  <label className="identity-page-heading"><input type="checkbox" aria-label={`Keep page: ${page.title}`} checked={pagePaths.includes(page.path)} disabled={!!locked} onChange={() => setPagePaths(current => current.includes(page.path) ? current.filter(path => path !== page.path) : [...current, page.path])} /><strong>{page.title}</strong><span className="identity-kind">{page.kind}</span></label>
                  <small className="identity-page-path">{page.path}</small>
                  {photos.length > 0 && <div className="identity-page-strip">{photos.map(photo => <img key={photo.id} src={photo.thumbnail} alt={candidate.photos?.find(item => item.id === photo.id)?.description || photo.name} className={!photoIds.includes(photo.id) ? 'excluded' : ''} />)}</div>}
                  <div className="identity-page-work">{items.map(({ item, index }) => <div key={index} className={!itemIndices.includes(index) ? 'excluded' : ''}><span className="identity-kind">{item.kind}</span><span title={item.summary}>{item.title}</span></div>)}</div>
                  <details className="identity-page-details"><summary>Choose content · {photos.length} {photos.length === 1 ? 'photo' : 'photos'} · {items.length} {items.length === 1 ? 'item' : 'items'}</summary><fieldset disabled={!!locked || !pagePaths.includes(page.path)}><legend className="sr-only">Content for {page.title}</legend>
                    {photos.map(photo => <label key={photo.id}><input type="checkbox" aria-label={`Keep photo: ${photo.name} on ${page.title}`} checked={photoIds.includes(photo.id)} onChange={() => setPhotoIds(current => current.includes(photo.id) ? current.filter(id => id !== photo.id) : [...current, photo.id])} /><img src={photo.thumbnail} alt="" /><span>{photo.name.split('/').at(-1)}</span></label>)}
                    {items.map(({ item, index }) => <label key={index}><input type="checkbox" aria-label={`Keep work: ${item.title} on ${page.title}`} checked={itemIndices.includes(index)} onChange={() => setItemIndices(current => current.includes(index) ? current.filter(value => value !== index) : [...current, index])} /><span className="identity-kind">{item.kind}</span><span>{item.title}</span></label>)}
                  </fieldset></details>
                </article>)}
              </ReviewGroup>;
            })}
            {candidate.warnings.length > 0 && <details className="identity-warnings"><summary>Things to check</summary><ul>{candidate.warnings.map((warning, index) => <li key={index}>{userError(warning)}</li>)}</ul></details>}
            <button type="button" disabled={locked} onClick={() => { setAnalysis(null); setFindings([]); setAssets([]); }}>Change files</button>
          </div>}
        </>}
      </section>
    {candidate && !keepable && <p className="field-help">Choose at least one detail, logo or page to keep.</p>}
    {kept && <Notice message="Brand choices kept." />}
    <div className="form-actions"><button type="button" disabled={locked} onClick={close}>Cancel</button>{candidate && connected && !kept && <button className="primary" type="submit" disabled={locked || !keepable}>{busy || working ? 'Working…' : 'Keep brand choices'}</button>}</div>
  </form>;
}
