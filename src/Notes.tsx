import { useId, useState, type FormEvent } from 'react';
import { DraftText } from './DraftText';
import type { Brief, Command, Decision, Project, Scope } from './types';

export function scopeLabel(project: Project, scope: Scope) {
  if (scope.kind === 'project') return 'Whole site';
  if (scope.kind === 'page') return project.pages.find(page => page.id === scope.id)?.name ?? 'Missing page';
  for (const page of project.pages) {
    const index = page.regions.findIndex(region => region.id === scope.id);
    if (index >= 0) return `${page.name} · Box ${index + 1}`;
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
    <option value="project">Whole site</option>
    {project.pages.map(page => <optgroup key={page.id} label={page.name}><option value={`page:${page.id}`}>Page: {page.name}</option>
      {page.regions.map((region, index) => <option key={region.id} value={`region:${region.id}`}>{page.name} · Box {index + 1}</option>)}
    </optgroup>)}
  </select></label>;
}

export type DossierSection = 'overview' | 'brief' | 'decisions' | 'exceptions';

export function Notes({ project, initialSection = 'overview', apply, changeBrief, reviewGeneration }: {
  project: Project; initialSection?: DossierSection; reviewGeneration?: () => void; apply: (command: Command) => Promise<void>; changeBrief: (patch: Partial<Brief>) => Promise<Brief>;
}) {
  const [tab, setTab] = useState<DossierSection>(initialSection);
  const tabId = useId();
  const sections = [{ id: 'overview', label: 'Overview' }, { id: 'brief', label: 'Brief' }, { id: 'decisions', label: 'Rules' }, { id: 'exceptions', label: 'Exceptions' }] as const;
  const latest = project.latestGeneration;
  const [editing, setEditing] = useState<Decision | null>(null);
  const [topic, setTopic] = useState('');
  const [value, setValue] = useState('');
  const [scope, setScope] = useState<Scope>({ kind: 'project' });
  const [protectedDecision, setProtectedDecision] = useState(false);
  const [rule, setRule] = useState('');
  const [reason, setReason] = useState('');
  const [exceptionScope, setExceptionScope] = useState<Scope>({ kind: 'project' });
  const [busy, setBusy] = useState(false);
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
    <div className="dossier-navigation"><div className="tabs" role="tablist" aria-label="Project brief sections">
      {sections.map((item, index) => <button key={item.id} id={`${tabId}-${item.id}`} role="tab" aria-selected={tab === item.id} aria-controls={`${tabId}-panel`} tabIndex={tab === item.id ? 0 : -1}
        onClick={() => setTab(item.id)} onKeyDown={event => {
          if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
          event.preventDefault();
          const next = event.key === 'Home' ? 0 : event.key === 'End' ? sections.length - 1 : (index + (event.key === 'ArrowRight' ? 1 : -1) + sections.length) % sections.length;
          setTab(sections[next].id); document.getElementById(`${tabId}-${sections[next].id}`)?.focus();
        }}>{item.label}</button>)}
    </div></div>
    <div id={`${tabId}-panel`} role="tabpanel" aria-labelledby={`${tabId}-${tab}`}>
    {tab === 'overview' && <div className="notes-section dossier-overview">
      {project.importedIdentity && <section aria-label="Imported brand"><h3>Imported brand</h3><p className="preserve-text">{project.importedIdentity.summary}</p><p className="field-help">From {project.importedIdentity.sourceName}</p>{project.importedIdentity.assets.length > 0 && <div className="dossier-identity-logos">{project.importedIdentity.assets.map(asset => <figure key={asset.id}><img src={asset.dataUri} alt={asset.name} /><figcaption>{asset.name}</figcaption></figure>)}</div>}</section>}
      {latest && reviewGeneration && <button className="conversation-link" onClick={reviewGeneration}>Latest generation · AI conversation →</button>}
      {!latest && !project.importedIdentity && <p className="empty-message">No brief yet.</p>}
    </div>}
    {tab === 'brief' && <div className="notes-section brief-form">
      <DraftText label="Purpose" value={project.brief.purpose} commit={value => changeBrief({ purpose: value }).then(brief => brief.purpose)} multiline />
      <DraftText label="Audience" value={project.brief.audience} commit={value => changeBrief({ audience: value }).then(brief => brief.audience)} multiline />
      <DraftText label="Brand name" value={project.brief.brandName} commit={value => changeBrief({ brandName: value }).then(brief => brief.brandName)} />
      <DraftText label="Voice and tone" value={project.brief.tone} commit={value => changeBrief({ tone: value }).then(brief => brief.tone)} multiline />
    </div>}
    {tab === 'decisions' && <div className="notes-section">
      <div className="record-list">{project.decisions.length === 0 ? <p className="empty-message">No rules yet.</p> : project.decisions.map(decision => <article className="note-record" key={decision.id}>
        <div className="record-heading"><h3>{decision.topic}</h3>{decision.protected && <span className="badge">Fixed</span>}</div>
        <p className="preserve-text">{decision.value}</p><p className="small muted">{scopeLabel(project, decision.scope)}</p>
        <div className="inline-actions"><button onClick={() => editDecision(decision)}>Edit rule</button><button className="subtle danger" onClick={() => { void apply({ type: 'remove_decision', decision_id: decision.id }).catch(() => {}); if (editing?.id === decision.id) resetDecision(); }}>Remove rule</button></div>
      </article>)}</div>
      <form className="notes-entry" onSubmit={saveDecision}><h3>{editing ? 'Edit choice' : 'Add a choice'}</h3>
        <label>Topic<input required maxLength={120} value={topic} onChange={event => setTopic(event.target.value)} disabled={!!editing} /></label>
        <label>Rule<textarea required rows={3} maxLength={16384} value={value} onChange={event => setValue(event.target.value)} /></label>
        <ScopeSelect project={project} value={scope} change={setScope} disabled={!!editing} />
        <label className="check-label"><input type="checkbox" checked={protectedDecision} onChange={event => setProtectedDecision(event.target.checked)} />Never change this rule</label>
        <div className="form-actions">{editing && <button type="button" disabled={busy} onClick={resetDecision}>Cancel editing</button>}<button className="primary" disabled={busy || !topic.trim() || !value.trim()}>{editing ? 'Save choice' : 'Add choice'}</button></div>
      </form>
    </div>}
    {tab === 'exceptions' && <div className="notes-section">
      <div className="record-list">{project.exceptions.length === 0 ? <p className="empty-message">No exceptions yet.</p> : project.exceptions.map(exception => <article className="note-record" key={exception.id}>
        <h3>{exception.rule}</h3><p className="preserve-text">{exception.reason}</p><p className="small muted">{scopeLabel(project, exception.scope)}</p>
        <button className="subtle danger" onClick={() => { void apply({ type: 'remove_exception', exception_id: exception.id }).catch(() => {}); }}>Remove exception</button>
      </article>)}</div>
      <form className="notes-entry" onSubmit={saveException}><h3>Add an exception</h3>
        <label>Which rule?<input required maxLength={120} value={rule} onChange={event => setRule(event.target.value)} /></label>
        <label>Why make an exception?<textarea required rows={3} maxLength={16384} value={reason} onChange={event => setReason(event.target.value)} /></label>
        <ScopeSelect project={project} value={exceptionScope} change={setExceptionScope} />
        <button className="primary" disabled={busy || !rule.trim() || !reason.trim()}>Add exception</button>
      </form>
    </div>}
    </div>
  </div>;
}
