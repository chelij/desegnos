import { useEffect, useId, useState, type FormEvent } from 'react';
import { DraftText } from './DraftText';
import { MenuButton } from './Menus';
import type { Brief, Command, Decision, Project, Scope } from './types';

export function scopeLabel(project: Project, scope: Scope) {
  if (scope.kind === 'project') return 'Whole project';
  if (scope.kind === 'page') return project.pages.find(page => page.id === scope.id)?.name ?? 'Missing page';
  for (const page of project.pages) {
    const index = page.regions.findIndex(region => region.id === scope.id);
    if (index >= 0) return `${page.name} · Request ${index + 1}`;
  }
  return 'Missing request';
}

function encodeScope(scope: Scope) { return scope.kind === 'project' ? 'project' : `${scope.kind}:${scope.id}`; }
function decodeScope(value: string): Scope {
  if (value === 'project') return { kind: 'project' };
  const [kind, id] = value.split(':');
  return { kind: kind === 'page' ? 'page' : 'region', id };
}

function ScopeSelect({ project, value, change, disabled = false }: { project: Project; value: Scope; change: (scope: Scope) => void; disabled?: boolean }) {
  return <label>Applies to<select value={encodeScope(value)} onChange={event => change(decodeScope(event.target.value))} disabled={disabled}>
    <option value="project">Whole project</option>
    {project.pages.map(page => <optgroup key={page.id} label={page.name}><option value={`page:${page.id}`}>Page: {page.name}</option>
      {page.regions.map((region, index) => <option key={region.id} value={`region:${region.id}`}>{page.name} · Request {index + 1}</option>)}
    </optgroup>)}
  </select></label>;
}

export type DossierSection = 'overview' | 'brief' | 'decisions' | 'exceptions' | 'markdown';

export function Notes({ project, initialSection = 'overview', apply, changeBrief, markdown, reviewGeneration }: {
  project: Project; initialSection?: DossierSection; reviewGeneration?: () => void; apply: (command: Command) => Promise<void>; changeBrief: (patch: Partial<Brief>) => Promise<Brief>; markdown: () => Promise<string>;
}) {
  const [tab, setTab] = useState<DossierSection>(initialSection);
  const tabId = useId();
  const sections = [{ id: 'overview', label: 'Overview' }, { id: 'brief', label: 'Brief' }, { id: 'decisions', label: 'Choices' }, { id: 'exceptions', label: 'Exceptions' }] as const;
  const latest = project.latestGeneration;
  const [source, setSource] = useState('');
  const [sourceError, setSourceError] = useState('');
  const [sourceLoading, setSourceLoading] = useState(false);
  const [editing, setEditing] = useState<Decision | null>(null);
  const [topic, setTopic] = useState('');
  const [value, setValue] = useState('');
  const [scope, setScope] = useState<Scope>({ kind: 'project' });
  const [protectedDecision, setProtectedDecision] = useState(false);
  const [rule, setRule] = useState('');
  const [reason, setReason] = useState('');
  const [exceptionScope, setExceptionScope] = useState<Scope>({ kind: 'project' });
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (tab !== 'markdown') return;
    let active = true;
    setSourceLoading(true); setSourceError('');
    void markdown().then(text => { if (active) setSource(text); }, error => { if (active) setSourceError(String(error)); })
      .finally(() => { if (active) setSourceLoading(false); });
    return () => { active = false; };
  }, [tab, project.revision, markdown]);
  function resetDecision() { setEditing(null); setTopic(''); setValue(''); setScope({ kind: 'project' }); setProtectedDecision(false); }
  function editDecision(decision: Decision) {
    setEditing(decision); setTopic(decision.topic); setValue(decision.value); setScope(decision.scope); setProtectedDecision(decision.protected);
  }
  async function saveDecision(event: FormEvent) {
    event.preventDefault(); setBusy(true);
    try { await apply({ type: 'set_decision', topic: topic.trim(), value: value.trim(), scope, protected: protectedDecision }); resetDecision(); }
    catch {}
    finally { setBusy(false); }
  }
  async function saveException(event: FormEvent) {
    event.preventDefault(); setBusy(true);
    try { await apply({ type: 'add_exception', rule: rule.trim(), reason: reason.trim(), scope: exceptionScope }); setRule(''); setReason(''); }
    catch {}
    finally { setBusy(false); }
  }
  return <div className="notes">
    <div className="dossier-navigation"><div className="tabs" role="tablist" aria-label="Project dossier sections">
      {sections.map((item, index) => <button key={item.id} id={`${tabId}-${item.id}`} role="tab" aria-selected={tab === item.id} aria-controls={`${tabId}-panel`} tabIndex={tab === item.id || (tab === 'markdown' && index === 0) ? 0 : -1}
        onClick={() => setTab(item.id)} onKeyDown={event => {
          if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
          event.preventDefault();
          const next = event.key === 'Home' ? 0 : event.key === 'End' ? sections.length - 1 : (index + (event.key === 'ArrowRight' ? 1 : -1) + sections.length) % sections.length;
          setTab(sections[next].id); document.getElementById(`${tabId}-${sections[next].id}`)?.focus();
        }}>{item.label}</button>)}
    </div><MenuButton label="More" ariaLabel="Dossier options" items={[{ id: 'markdown', label: 'PROJECT.md', action: () => setTab('markdown') }]} /></div>
    <div id={`${tabId}-panel`} role="tabpanel" aria-labelledby={tab === 'markdown' ? undefined : `${tabId}-${tab}`} aria-label={tab === 'markdown' ? 'PROJECT.md' : undefined}>
    {tab === 'overview' && <div className="notes-section dossier-overview">
      <p className="muted">Updated automatically after generation.</p>
      {project.importedIdentity && <section aria-label="Imported identity"><h3>Imported identity</h3><p className="preserve-text">{project.importedIdentity.summary}</p><p className="field-help">From {project.importedIdentity.sourceName}</p>{project.importedIdentity.assets.length > 0 && <div className="dossier-identity-logos">{project.importedIdentity.assets.map(asset => <figure key={asset.id}><img src={asset.dataUri} alt={asset.name} /><figcaption>{asset.name}</figcaption></figure>)}</div>}</section>}
      {latest ? <>
        <div className="record-heading"><h3>Latest generation</h3><span className="badge">{latest.applicationStatus === 'applied' ? 'Applied' : latest.applicationStatus === 'undone' ? 'Undone' : 'Draft'}</span></div>
        <p className="preserve-text">{latest.summary}</p>
        {latest.applicationStatus === 'draft' && <p className="field-help">Draft suggestions become saved choices when you accept them.</p>}
        {reviewGeneration && <button onClick={reviewGeneration}>Review generation</button>}
        {latest.decisionProposals.length > 0 && <section className="dossier-generated"><h3>{latest.applicationStatus === 'draft' ? 'Suggested choices' : 'Generated choices'}</h3>
          <div className="record-list">{latest.decisionProposals.map((proposal, index) => <article className="note-record" key={`${proposal.topic}:${index}`}>
            <div className="record-heading"><h4>{proposal.topic}</h4>{latest.applicationStatus === 'applied' && latest.acceptedDecisionIndices.includes(index) && <span className="badge">Accepted</span>}</div>
            <p className="preserve-text">{proposal.value}</p><p className="small muted">{scopeLabel(project, proposal.scope)}</p>
            {proposal.rationale && <p className="field-help preserve-text">{proposal.rationale}</p>}
          </article>)}</div>
        </section>}
        {latest.criticism.length > 0 && <section className="dossier-generated"><h3>Design feedback</h3><p className="field-help">Suggestions, with room for your creative choices.</p>
          <div className="record-list">{latest.criticism.map((item, index) => <article className="note-record" key={index}><p className="preserve-text">{item.message}</p>{item.suggestion && <p className="preserve-text">{item.suggestion}</p>}</article>)}</div>
        </section>}
      </> : !project.importedIdentity && <p className="empty-message">Generate your first design to start the dossier.</p>}
    </div>}
    {tab === 'brief' && <div className="notes-section brief-form">
      <DraftText label="Purpose" value={project.brief.purpose} commit={value => changeBrief({ purpose: value }).then(brief => brief.purpose)} multiline />
      <DraftText label="Audience" value={project.brief.audience} commit={value => changeBrief({ audience: value }).then(brief => brief.audience)} multiline />
      <DraftText label="Brand name" value={project.brief.brandName} commit={value => changeBrief({ brandName: value }).then(brief => brief.brandName)} />
      <DraftText label="Voice and tone" value={project.brief.tone} commit={value => changeBrief({ tone: value }).then(brief => brief.tone)} multiline />
      <p className="field-help">Saved locally as you type.</p>
    </div>}
    {tab === 'decisions' && <div className="notes-section">
      <p className="muted">Keep colors, type, names and facts consistent.</p>
      <div className="record-list">{project.decisions.length === 0 ? <p className="empty-message">No saved choices yet.</p> : project.decisions.map(decision => <article className="note-record" key={decision.id}>
        <div className="record-heading"><h3>{decision.topic}</h3>{decision.protected && <span className="badge">Fixed</span>}</div>
        <p className="preserve-text">{decision.value}</p><p className="small muted">{scopeLabel(project, decision.scope)}</p>
        <div className="inline-actions"><button onClick={() => editDecision(decision)}>Edit choice</button><button className="subtle danger" onClick={() => { void apply({ type: 'remove_decision', decision_id: decision.id }).catch(() => {}); if (editing?.id === decision.id) resetDecision(); }}>Remove choice</button></div>
      </article>)}</div>
      <form className="notes-entry" onSubmit={saveDecision}><h3>{editing ? 'Edit choice' : 'Add a choice'}</h3>
        <label>Topic<input required maxLength={120} value={topic} onChange={event => setTopic(event.target.value)} disabled={!!editing} /></label>
        <label>Choice<textarea required rows={3} maxLength={16384} value={value} onChange={event => setValue(event.target.value)} /></label>
        <ScopeSelect project={project} value={scope} change={setScope} disabled={!!editing} />
        <label className="check-label"><input type="checkbox" checked={protectedDecision} onChange={event => setProtectedDecision(event.target.checked)} />Keep this choice fixed</label>
        <div className="form-actions"><button className="primary" disabled={busy || !topic.trim() || !value.trim()}>{editing ? 'Save choice' : 'Add choice'}</button>{editing && <button type="button" onClick={resetDecision}>Cancel editing</button>}</div>
      </form>
    </div>}
    {tab === 'exceptions' && <div className="notes-section">
      <p className="muted">Allow a deliberate exception to a saved choice.</p>
      <div className="record-list">{project.exceptions.length === 0 ? <p className="empty-message">No exceptions yet.</p> : project.exceptions.map(exception => <article className="note-record" key={exception.id}>
        <h3>{exception.rule}</h3><p className="preserve-text">{exception.reason}</p><p className="small muted">{scopeLabel(project, exception.scope)}</p>
        <button className="subtle danger" onClick={() => { void apply({ type: 'remove_exception', exception_id: exception.id }).catch(() => {}); }}>Remove exception</button>
      </article>)}</div>
      <form className="notes-entry" onSubmit={saveException}><h3>Add an exception</h3>
        <label>Choice or design concern<input required maxLength={120} value={rule} onChange={event => setRule(event.target.value)} /></label>
        <label>Why make an exception?<textarea required rows={3} maxLength={16384} value={reason} onChange={event => setReason(event.target.value)} /></label>
        <ScopeSelect project={project} value={exceptionScope} change={setExceptionScope} />
        <button className="primary" disabled={busy || !rule.trim() || !reason.trim()}>Add exception</button>
      </form>
    </div>}
    {tab === 'markdown' && <div className="notes-section"><p className="muted">Saved with your project.</p>
      {sourceLoading ? <p role="status">Loading project document…</p> : sourceError ? <p role="alert" className="error-text">{sourceError}</p> : <pre className="markdown-source" tabIndex={0} aria-label="PROJECT.md source">{source}</pre>}
    </div>}
    </div>
  </div>;
}
