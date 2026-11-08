import { errorMessage, errorValue, type ErrorValue } from './errorMessage.js';
import { MAX_IMPORT_PAGES } from './generated/limits';
import { usePoll } from './usePoll';
import { ProblemBanner, Notice } from './ProblemBanner.js';
import { userError, needsAiSettings } from './userErrors.js';
import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent, type ReactNode } from 'react';
import { native } from './api';
import type { AiSettingsView, IdentityAnalysis, IdentityCandidate, IdentitySelection, IdentitySource, WebsiteImportResultView, WebsiteImportSourceView } from './types';
import { groupIdentityPages, selectedPageContent, findingSection, parseColours, parseFonts, parseTone, type IdentityFinding } from './identityReview';
import { useWebsiteImport, WebsiteImportStatus } from './WebsiteImport';
import { Icon } from './icons';
import './importContext.css';

// One source supplies the page import and the optional brand review.
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
type Props = { initialSource?: IdentitySource | null; busy: boolean; settings: AiSettingsView | null; openSettings: () => void; importInto: (analysisId: string, findings: number[], assets: string[], photos: string[], items: number[], pages: string[], attachMatching: boolean) => Promise<void>; onPagesImported: (result: WebsiteImportResultView) => void; close: () => void };

/** The files a source is read from: the automatic pick, or the user's own. */
type Choice = { files: string[]; automatic: boolean; pickVersion: number; selection: IdentitySelection | null };
/** A finished analysis and what the user chose to keep from it. */
type Brand = { analysis: IdentityAnalysis; candidate: IdentityCandidate; findings: number[]; logos: string[]; photos: string[]; items: number[]; pages: string[] };
/** A chosen source and the files it is read from; present from the first pick onwards. */
type Picked = { source: IdentitySource; choice: Choice };
/** The brand import runs one step at a time, and each step owns the data that only exists there. */
type Step =
  | { kind: 'source' }
  | ({ kind: 'choose' } & Picked)
  | ({ kind: 'reading'; jobId: string } & Picked)
  | ({ kind: 'unread'; analysis: IdentityAnalysis } & Picked)
  | ({ kind: 'review'; brand: Brand } & Picked)
  | ({ kind: 'kept'; brand: Brand } & Picked);
type BrandChoice = 'default' | 'included' | 'skipped';
/** The brand an earlier session left behind is read in the background, apart from the import step. */
type PreviousBrand =
  | { status: 'reading'; retrying: string }
  | { status: 'ready'; analysis: IdentityAnalysis | null }
  | { status: 'failed'; error: string };

type Change =
  | { type: 'source'; source: IdentitySource | null }
  | { type: 'resume'; source: IdentitySource; analysis: IdentityAnalysis }
  | { type: 'files'; update: (current: string[]) => string[] }
  | { type: 'pickForMe' }
  | { type: 'selection'; selection: IdentitySelection | null; files: string[] | null }
  | { type: 'accepted'; analysis: IdentityAnalysis }
  | { type: 'findings'; update: (current: number[]) => number[] }
  | { type: 'logos'; update: (current: string[]) => string[] }
  | { type: 'photos'; update: (current: string[]) => string[] }
  | { type: 'items'; update: (current: number[]) => number[] }
  | { type: 'pages'; update: (current: string[]) => string[] }
  | { type: 'changeFiles' }
  | { type: 'keep' };

const NO_STRINGS: string[] = [];
const NO_NUMBERS: number[] = [];
function startChoice(): Choice { return { files: [], automatic: true, pickVersion: 0, selection: null }; }

function editChoice(step: Step, update: (choice: Choice) => Choice): Step {
  return step.kind === 'source' ? step : { ...step, choice: update(step.choice) };
}
function editBrand(step: Step, update: (brand: Brand) => Brand): Step {
  return step.kind === 'review' || step.kind === 'kept' ? { ...step, brand: update(step.brand) } : step;
}
function accepted(step: Step, analysis: IdentityAnalysis): Step {
  if (step.kind === 'source') return step;
  const picked = { source: step.source, choice: step.choice };
  if (analysis.status === 'analyzing') return { kind: 'reading', jobId: analysis.jobId, ...picked };
  if (analysis.status !== 'complete' || !analysis.candidate) return { kind: 'unread', analysis, ...picked };
  const { candidate } = analysis;
  return {
    kind: 'review',
    ...picked,
    brand: {
      analysis,
      candidate,
      findings: candidate.findings.flatMap((finding, index) => finding.confidence === 'low' ? [] : [index]),
      logos: candidate.logoAssetIds,
      photos: (analysis.source.photos ?? []).map(photo => photo.id),
      items: (candidate.content ?? []).map((_, index) => index),
      pages: groupIdentityPages(analysis.source, candidate.content).map(group => group.page.path) } };
}
function reduce(step: Step, change: Change): Step {
  switch (change.type) {
    // A new source, or none, starts the flow over.
    case 'source':
      return change.source ? { kind: 'choose', source: change.source, choice: startChoice() } : { kind: 'source' };
    case 'resume':
      return accepted({ kind: 'choose', source: change.source, choice: { ...startChoice(), files: change.analysis.fileIds, automatic: false } }, change.analysis);
    // A manual pick replaces the automatic one, so the selection is prepared again.
    case 'files':
      return step.kind === 'choose' || step.kind === 'unread'
        ? { kind: 'choose', source: step.source, choice: { ...step.choice, files: change.update(step.choice.files), automatic: false, selection: null } }
        : step;
    case 'pickForMe':
      return editChoice(step, choice => ({ ...choice, automatic: true, selection: null, pickVersion: choice.pickVersion + 1 }));
    case 'selection':
      return editChoice(step, choice => ({ ...choice, selection: change.selection, files: change.files ?? choice.files }));
    case 'accepted':
      return accepted(step, change.analysis);
    case 'changeFiles':
      return step.kind === 'review' ? { kind: 'choose', source: step.source, choice: step.choice } : step;
    case 'findings':
      return editBrand(step, brand => ({ ...brand, findings: change.update(brand.findings) }));
    case 'logos':
      return editBrand(step, brand => ({ ...brand, logos: change.update(brand.logos) }));
    case 'photos':
      return editBrand(step, brand => ({ ...brand, photos: change.update(brand.photos) }));
    case 'items':
      return editBrand(step, brand => ({ ...brand, items: change.update(brand.items) }));
    case 'pages':
      return editBrand(step, brand => ({ ...brand, pages: change.update(brand.pages) }));
    case 'keep':
      return step.kind === 'review' ? { ...step, kind: 'kept' } : step;
  }
}

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

export function ImportContext({ busy, settings, openSettings, importInto, onPagesImported, close, initialSource }: Props) {
  const [step, setStep] = useState<Step>(initialSource ? { kind: 'choose', source: initialSource, choice: startChoice() } : { kind: 'source' });
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<ErrorValue>('');
  const [brandChoice, setBrandChoice] = useState<BrandChoice>('default');
  const [previousBrand, setPreviousBrand] = useState<PreviousBrand>({ status: 'reading', retrying: '' });
  const mounted = useRef(false);
  const previousRead = useRef(0);
  // Closing the panel mid-read cancels the job, so the cleanup needs the newest step.
  const live = useRef(step);
  useEffect(() => { live.current = step; }, [step]);
  const pages = useWebsiteImport(busy, onPagesImported);
  const source = step.kind === 'source' ? null : step.source;
  const choice = step.kind === 'source' ? null : step.choice;
  const files = choice?.files ?? NO_STRINGS;
  const automatic = choice?.automatic ?? true;
  const selection = choice?.selection ?? null;
  const pickVersion = choice?.pickVersion ?? 0;
  const analyzing = step.kind === 'reading';
  const kept = step.kind === 'kept';
  const jobId = step.kind === 'reading' ? step.jobId : null;
  const lastAttempt = step.kind === 'unread' ? step.analysis : null;
  const brand = step.kind === 'review' || step.kind === 'kept' ? step.brand : null;
  const candidate = brand?.candidate ?? null;
  const findings = brand?.findings ?? NO_NUMBERS;
  const assets = brand?.logos ?? NO_STRINGS;
  const photoIds = brand?.photos ?? NO_STRINGS;
  const itemIndices = brand?.items ?? NO_NUMBERS;
  const pagePaths = brand?.pages ?? NO_STRINGS;
  const locked = busy || working || analyzing || pages.working;
  const analysisMethod = lastAttempt?.authentication ?? settings?.authentication;
  const analysisAccount = settings?.chatgpt.accounts.find(item => item.id === lastAttempt?.chatgptAccountId);
  const account = settings?.chatgpt.accounts.find(item => item.id === settings.chatgpt.activeAccountId);
  const connected = settings?.readiness.ready ?? false;
  const alsoBrand = connected && brandChoice !== 'skipped';
  const hasPages = !!pages.source?.pages.length && pages.phase.kind !== 'imported';
  const requestedFiles = automatic ? null : files;
  const previousReadError = previousBrand.status === 'ready' ? '' : previousBrand.status === 'failed' ? previousBrand.error : previousBrand.retrying;
  function change(next: Change) { setStep(current => reduce(current, next)); }
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
        void face.load().then(font => { if (!stopped) { document.fonts.add(font); loaded.push(font); } }).catch(error => console.warn('Could not load imported font; using CSS fallback', error));
      } catch { /* Invalid font data keeps the CSS family fallback. */ }
    }
    return () => { stopped = true; loaded.forEach(font => document.fonts.delete(font)); };
  }, [source, fontFamilies]);
  useEffect(() => {
    if (!source) return;
    let stopped = false;
    change({ type: 'selection', selection: null, files: null });
    void native<IdentitySelection>('prepare_identity_selection', { sourceId: source.id, fileIds: requestedFiles }).then(value => {
      if (stopped) return;
      change({ type: 'selection', selection: value, files: requestedFiles === null ? value.fileIds : null });
    }).catch(error => { if (!stopped) change({ type: 'selection', selection: { fileIds: requestedFiles ?? NO_STRINGS, fits: false, message: errorMessage(error), needsSettings: false }, files: null }); });
    return () => { stopped = true; };
  }, [source, requestedFiles, pickVersion, settings?.provider, settings?.authentication, settings?.settings.model, settings?.settings.reasoningEffort, settings?.settings.maxOutputTokens, settings?.settings.spendingLimitUsd]);

  const readPrevious = useCallback(async (retrying = '') => {
    const serial = ++previousRead.current;
    const current = () => mounted.current && serial === previousRead.current;
    setPreviousBrand({ status: 'reading', retrying });
    try {
      const analysis = await native<IdentityAnalysis | null>('get_identity_analysis');
      if (current()) setPreviousBrand({ status: 'ready', analysis });
    } catch (error) { if (current()) setPreviousBrand({ status: 'failed', error: errorMessage(error) }); }
  }, []);
  useEffect(() => {
    mounted.current = true;
    void readPrevious();
    return () => {
      mounted.current = false;
      previousRead.current += 1;
      if (live.current.kind === 'reading') void native('cancel_identity_analysis', { jobId: live.current.jobId }).catch(error => console.warn('Could not cancel identity analysis on unmount', error));
    };
  }, [readPrevious]);
  usePoll(!!jobId, jobId ?? '', 900, async active => {
    const value = await native<IdentityAnalysis | null>('get_identity_analysis');
    if (active() && value?.jobId === jobId) { change({ type: 'accepted', analysis: value }); setError(''); }
  }, error => setError(errorValue(error)), 900, 500);
  async function run(operation: () => Promise<void>) {
    setWorking(true); setError('');
    try { await operation(); } catch (error) { if (mounted.current) setError(errorValue(error)); }
    finally { if (mounted.current) setWorking(false); }
  }
  function acceptRead(value: WebsiteImportSourceView | null) {
    if (!value || !mounted.current) return;
    setError('');
    setBrandChoice('default');
    change({ type: 'source', source: value.identitySource });
    if (value.identityError) setError(value.identityError);
  }
  async function pick(kind: 'file' | 'folder' | 'zip') {
    acceptRead(await pages.pick(kind));
  }
  const [address, setAddress] = useState('');
  async function fetchSite(event?: FormEvent) {
    event?.preventDefault();
    acceptRead(await pages.readUrl(address.trim()));
  }
  async function importSource() {
    if (locked) return;
    const leftOut = hasPages ? await pages.runImport() : [];
    if (!leftOut) return;
    // The brand reading continues in this panel; a clean pages-only import is done, while
    // one that left something out stays open so its warnings can be read.
    if (alsoBrand) await analyze(); else if (!leftOut.length) close();
  }
  function resume(previous: IdentityAnalysis) {
    change({ type: 'resume', source: previous.source, analysis: previous });
    setPreviousBrand({ status: 'ready', analysis: null });
  }
  async function analyze() {
    if (!mounted.current || !source || !selection?.fits || !connected || locked) return;
    await run(async () => {
      const value = await native<IdentityAnalysis>('analyze_identity', { sourceId: source.id, fileIds: files });
      if (!mounted.current) { if (value.status === 'analyzing') await native('cancel_identity_analysis', { jobId: value.jobId }); return; }
      change({ type: 'accepted', analysis: value });
    });
  }
  async function cancel() {
    if (!jobId) return;
    await run(async () => change({ type: 'accepted', analysis: await native<IdentityAnalysis>('cancel_identity_analysis', { jobId }) }));
  }
  function submit(event: FormEvent) {
    event.preventDefault();
    if (step.kind !== 'review' || locked || !keepable) return;
    const keeping = step.brand;
    void run(async () => { const kept = selectedPageContent(pageGroups, pagePaths, photoIds, itemIndices); await importInto(keeping.analysis.jobId, findings, assets, kept.photos, kept.items, pagePaths, true); change({ type: 'keep' }); close(); });
  }
  const pageGroups = useMemo(() => groupIdentityPages(source, candidate?.content), [source, candidate]);
  const keepable = findings.length > 0 || assets.length > 0 || pagePaths.length > 0;
  const logos = source?.files.flatMap(file => file.asset && candidate?.logoAssetIds.includes(file.asset.id) ? [file.asset] : []) ?? [];
  function toggleFinding(index: number) { change({ type: 'findings', update: current => current.includes(index) ? current.filter(value => value !== index) : [...current, index] }); }
  function chooseFindings(indices: number[], all: boolean) { change({ type: 'findings', update: current => all ? [...new Set([...current, ...indices])] : current.filter(index => !indices.includes(index)) }); }
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
  const aiNeedsFixing = !connected || selection?.needsSettings || (lastAttempt?.status === 'failed' && needsAiSettings(lastAttempt.error ?? '', lastAttempt.providerError));
  return <form className="new-project import-context dialog-body" onSubmit={submit}>
    {error && <ProblemBanner message={userError(error)} dismiss={() => setError('')} action={needsAiSettings(error) ? { label: 'AI settings', onClick: openSettings } : { label: 'Choose another location', disabled: !!locked, onClick: () => { void pick('file'); } }} />}
      <section className="identity-import" aria-label="Brand evidence">
        <>
        <h2>Source</h2>
        <div className="identity-address"><label>Website address<input type="text" inputMode="url" placeholder="example.com" value={address} disabled={locked} onChange={event => setAddress(event.target.value)} onKeyDown={event => { if (event.key === 'Enter') { event.preventDefault(); if (address.trim()) void fetchSite(); } }} /></label>
          <button type="button" disabled={locked || !address.trim()} onClick={() => { void fetchSite(); }}>{pages.phase.kind === 'reading' ? 'Reading…' : 'Read'}</button></div>
        <div className="identity-source-actions"><span className="identity-or">or</span><button type="button" disabled={locked} onClick={() => { void pick('folder'); }}>Choose folder…</button><button type="button" disabled={locked} onClick={() => { void pick('zip'); }}>Choose ZIP…</button><button type="button" disabled={locked} onClick={() => { void pick('file'); }}>Choose HTML files…</button></div>

        {source && <h3 className="identity-source-name">{source.name}</h3>}
        {!source && previousReadError && <ProblemBanner message={userError(previousReadError)} action={{ label: 'Try loading again', disabled: !!locked || previousBrand.status === 'reading', onClick: () => { void readPrevious(previousReadError); } }} />}
        {!source && previousBrand.status === 'ready' && previousBrand.analysis && <button type="button" disabled={locked} onClick={() => { const previous = previousBrand.analysis; if (previous) resume(previous); }}>Continue with my previous brand</button>}
        </>
        {source && source.skipped.length > 0 && <details><summary>See all skipped files · {skippedSummary(source.skipped)}</summary><ul className="identity-skipped">{source.skipped.map((reason, index) => <li key={index}>{skippedFile(reason)}</li>)}</ul></details>}
        <WebsiteImportStatus flow={pages} busy={busy} />
        {pages.source && pages.source.pages.length > 0 && <section className="website-import-pages" aria-label="Pages to import">
          <p>{pages.source.totalPages} {pages.source.totalPages === 1 ? 'page' : 'pages'} found.{pages.source.totalPages > MAX_IMPORT_PAGES ? ` The ${MAX_IMPORT_PAGES} closest to the home page are imported.` : ''}</p>
          <ul className="website-import-list">{pages.source.pages.map(page => <li key={page.route}><span>{page.title || page.route}</span>{page.title && <span className="muted">{page.route}</span>}</li>)}</ul>
          {pages.phase.kind === 'imported' && <Notice message="Pages imported." />}
        </section>}
        {hasPages && source && connected && !candidate && <label className="identity-choice"><input type="checkbox" checked={!!alsoBrand} disabled={!!locked} onChange={event => setBrandChoice(event.target.checked ? 'included' : 'skipped')} />Also read brand and content</label>}
        {hasPages && <button type="button" className="primary" disabled={locked || (!!alsoBrand && !!source && !selection?.fits)} onClick={() => { void importSource(); }}>Import {pages.source!.pages.length > 1 ? `${pages.source!.pages.length} pages` : 'page'}</button>}
        {source && (!hasPages || alsoBrand) && <h2>Brand and content</h2>}
        {(source || pages.source) && !connected && <button type="button" onClick={openSettings}>Connect AI</button>}
        {source && connected && (!hasPages || alsoBrand) && !kept && <>
          {!candidate && <>
          <details className="identity-file-review" key={source.id}><summary>Choose files yourself</summary>
            <p className="field-help">{files.length} file{files.length === 1 ? '' : 's'} picked.</p>
            {!automatic && <button type="button" disabled={locked || !!candidate} onClick={() => change({ type: 'pickForMe' })}>Pick for me</button>}
            <div className="identity-files">{source.files.map(file => <label className="identity-choice" key={file.id}><input type="checkbox" disabled={locked || !!candidate} checked={files.includes(file.id)} onChange={event => change({ type: 'files', update: current => event.target.checked ? [...current, file.id] : current.filter(id => id !== file.id) })} /><span>{file.name}<small>{file.asset ? 'Logo' : `${Math.ceil(file.textBytes / 1024)} KB`}</small></span></label>)}</div>
          </details>
          {!candidate && <p className="field-help">Sent to {analysisMethod === 'chatgpt' ? `ChatGPT (${(analysisAccount ?? account)?.label ?? 'current account'})` : `${settings?.providers.find(provider => provider.id === settings.provider)?.name ?? 'OpenAI'} (API key)`}</p>}
          {!candidate && selection?.message && <ProblemBanner message={userError(selection.message)} />}
          </>}
          {analyzing ? <div className="identity-progress" role="status"><span>Reading your brand…</span><button type="button" disabled={working} onClick={() => { void cancel(); }}>Stop reading</button></div> : !candidate ? <>
            {lastAttempt && (lastAttempt.status === 'cancelled' ? <Notice message="Reading stopped." /> : <ProblemBanner message={userError({ ...lastAttempt, message: lastAttempt.error || 'The AI could not read your brand. Try again.' })} action={needsAiSettings(lastAttempt.error || '', lastAttempt.providerError) ? { label: 'AI settings', onClick: openSettings } : { label: 'Try again', disabled: !!locked, onClick: () => { void analyze(); } }} />)}
            <div className="identity-source-actions">{!hasPages && <button type="button" className="primary" disabled={locked || !selection?.fits || !connected} onClick={() => { void analyze(); }}>Read my brand</button>}{aiNeedsFixing && <button type="button" disabled={locked} onClick={openSettings}>{connected ? 'Check AI settings' : 'Connect AI'}</button>}</div>
          </> : <div className="identity-result" aria-label="Brand review">
            <div className="identity-review-intro"><h3>Choose what to keep</h3></div>
            {(['Colours', 'Fonts', 'Logos', 'Style & tone', 'Pages', 'Other'] as const).map(section => {
              const items = review.filter(item => item.section === section);
              if (!items.length && !(section === 'Logos' && logos.length) && !(section === 'Pages' && pageGroups.length)) return null;
              return <ReviewGroup key={section} name={section} className={section === 'Logos' ? 'identity-logos' : section === 'Pages' ? 'identity-pages' : ''} disabled={!!locked} choose={all => {
                chooseFindings(items.map(item => item.index), all);
                if (section === 'Logos') change({ type: 'logos', update: () => all ? logos.map(asset => asset.id) : [] });
                if (section === 'Pages') change({ type: 'pages', update: () => all ? pageGroups.map(group => group.page.path) : [] });
              }}>
                {findingCards(items)}
                {section === 'Logos' && logos.map(asset => <ReviewCard key={asset.id} name={`logo: ${asset.name}`} className="identity-logo" checked={assets.includes(asset.id)} disabled={!!locked} toggle={() => change({ type: 'logos', update: current => current.includes(asset.id) ? current.filter(id => id !== asset.id) : [...current, asset.id] })}><img src={asset.dataUri} alt={asset.name} /><span>{asset.name}</span></ReviewCard>)}
                {section === 'Pages' && pageGroups.map(({ page, photos, items }) => <article key={page.path} className={`identity-page-card${pagePaths.includes(page.path) ? ' selected' : ''}`}>
                  <label className="identity-page-heading"><input type="checkbox" aria-label={`Keep page: ${page.title}`} checked={pagePaths.includes(page.path)} disabled={!!locked} onChange={() => change({ type: 'pages', update: current => current.includes(page.path) ? current.filter(path => path !== page.path) : [...current, page.path] })} /><strong>{page.title}</strong><span className="identity-kind">{page.kind}</span></label>
                  <small className="identity-page-path">{page.path}</small>
                  {photos.length > 0 && <div className="identity-page-strip">{photos.map(photo => <img key={photo.id} src={photo.thumbnail} alt={candidate.photos?.find(item => item.id === photo.id)?.description || photo.name} className={!photoIds.includes(photo.id) ? 'excluded' : ''} />)}</div>}
                  <div className="identity-page-work">{items.map(({ item, index }) => <div key={index} className={!itemIndices.includes(index) ? 'excluded' : ''}><span className="identity-kind">{item.kind}</span><span title={item.summary}>{item.title}</span></div>)}</div>
                  <details className="identity-page-details"><summary>Choose content · {photos.length} {photos.length === 1 ? 'photo' : 'photos'} · {items.length} {items.length === 1 ? 'item' : 'items'}</summary><fieldset disabled={!!locked || !pagePaths.includes(page.path)}><legend className="sr-only">Content for {page.title}</legend>
                    {photos.map(photo => <label key={photo.id}><input type="checkbox" aria-label={`Keep photo: ${photo.name} on ${page.title}`} checked={photoIds.includes(photo.id)} onChange={() => change({ type: 'photos', update: current => current.includes(photo.id) ? current.filter(id => id !== photo.id) : [...current, photo.id] })} /><img src={photo.thumbnail} alt="" /><span>{photo.name.split('/').at(-1)}</span></label>)}
                    {items.map(({ item, index }) => <label key={index}><input type="checkbox" aria-label={`Keep work: ${item.title} on ${page.title}`} checked={itemIndices.includes(index)} onChange={() => change({ type: 'items', update: current => current.includes(index) ? current.filter(value => value !== index) : [...current, index] })} /><span className="identity-kind">{item.kind}</span><span>{item.title}</span></label>)}
                  </fieldset></details>
                </article>)}
              </ReviewGroup>;
            })}
            {candidate.warnings.length > 0 && <details className="identity-warnings"><summary>Things to check</summary><ul>{candidate.warnings.map((warning, index) => <li key={index}>{userError(warning)}</li>)}</ul></details>}
            <button type="button" disabled={locked} onClick={() => change({ type: 'changeFiles' })}>Change files</button>
          </div>}
        </>}
      </section>
    {candidate && !keepable && <p className="field-help">Choose at least one detail, logo or page to keep.</p>}
    {kept && <Notice message="Brand choices kept." />}
    <div className="form-actions"><button type="button" disabled={locked} onClick={close}>Cancel</button>{candidate && connected && !kept && <button className="primary" type="submit" disabled={locked || !keepable}>{busy || working ? 'Working…' : 'Keep brand choices'}</button>}</div>
  </form>;
}
