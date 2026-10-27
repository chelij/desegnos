import type { ReactNode } from 'react';
import { Icon } from './icons';
import type { CanvasTemplate, RecentProject } from './types';
import './startPage.css';

type Template = { id: Exclude<CanvasTemplate, 'blank'>; label: string };

const TEMPLATES: Template[] = [
  { id: 'website', label: 'Website' },
  { id: 'appScreen', label: 'App screen' },
  { id: 'slideDeck', label: 'Slide deck' },
  { id: 'socialImage', label: 'Social image' },
];

function Preview({ kind }: { kind: CanvasTemplate }) {
  const outline = { fill: 'none', stroke: 'currentColor', strokeWidth: 1.5, strokeLinejoin: 'round' as const };
  return <span className={`start-preview start-preview-${kind}`} aria-hidden="true">
    <svg viewBox="0 0 120 76" focusable="false">
      {kind === 'blank' && <rect x="24" y="8" width="72" height="60" rx="2" {...outline} />}
      {kind === 'website' && <>
        <rect x="8" y="12" width="64" height="46" rx="2" {...outline} />
        <rect x="78" y="18" width="22" height="34" rx="2" {...outline} />
        <rect x="105" y="24" width="9" height="28" rx="2" {...outline} />
      </>}
      {kind === 'appScreen' && <rect x="42" y="6" width="36" height="64" rx="4" {...outline} />}
      {kind === 'slideDeck' && <>
        <rect x="6" y="18" width="32" height="40" rx="2" {...outline} />
        <rect x="44" y="18" width="32" height="40" rx="2" {...outline} />
        <rect x="82" y="18" width="32" height="40" rx="2" {...outline} />
      </>}
      {kind === 'socialImage' && <rect x="37" y="8" width="60" height="60" rx="2" {...outline} />}
    </svg>
  </span>;
}

function folderOf(path: string) {
  const slash = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  return slash > 0 ? path.slice(0, slash) : path;
}

function openedLabel(openedAtMs: number) {
  if (!openedAtMs) return 'Earlier';
  const elapsed = Math.max(0, Date.now() - openedAtMs);
  const minutes = Math.floor(elapsed / 60_000);
  if (minutes < 1) return 'Just now';
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} hr ago`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days} ${days === 1 ? 'day' : 'days'} ago`;
  return new Date(openedAtMs).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
}

export function StartPage({ busy, create, recentProjects, openRecent, forgetRecent, pinRecent, recovery }: {
  busy: boolean;
  create: (template: CanvasTemplate) => void;
  recentProjects: RecentProject[];
  openRecent: (path: string) => void;
  forgetRecent: (path: string) => void;
  pinRecent: (path: string, pinned: boolean) => void;
  recovery?: ReactNode;
}) {
  return <main className="start-page">
    <div className="start-page-inner">
      <section className="start-blank" aria-label="New project">
        <button type="button" className="start-tile" disabled={busy} onClick={() => create('blank')}>
          <Preview kind="blank" />
          <span className="start-tile-name">Blank canvas</span>
        </button>
      </section>
      <section className="start-templates" aria-label="Templates">
        {TEMPLATES.map(template => <button type="button" className="start-tile" key={template.id} disabled={busy} onClick={() => create(template.id)}>
          <Preview kind={template.id} />
          <span className="start-tile-name">{template.label}</span>
        </button>)}
      </section>
      {recovery && <section className="start-recovery" aria-label="Unsaved work"><h2>Unsaved work</h2>{recovery}</section>}
      <section className="start-recents" aria-labelledby="start-recents-title">
        <h2 id="start-recents-title">Recent projects</h2>
        {recentProjects.length
          ? <ul>
            {recentProjects.map(entry => <li className={entry.pinned ? 'start-recent pinned' : 'start-recent'} key={entry.path}>
              <button type="button" className="start-recent-open" title={entry.path} onClick={() => openRecent(entry.path)}>
                <span className="start-recent-name">{entry.name}</span>
                <span className="start-recent-path">{folderOf(entry.path)}</span>
                <span className="start-recent-opened">{openedLabel(entry.openedAtMs)}</span>
              </button>
              <button type="button" className="icon-button sm start-recent-pin" aria-pressed={entry.pinned}
                aria-label={`${entry.pinned ? 'Unpin' : 'Pin'} ${entry.name}`} title={entry.pinned ? 'Unpin' : 'Pin'}
                onClick={() => pinRecent(entry.path, !entry.pinned)}><Icon name="pin" size={14} /></button>
              <button type="button" className="icon-button sm" aria-label={`Remove ${entry.name} from recent projects`} title="Remove from recent projects"
                onClick={() => forgetRecent(entry.path)}><Icon name="close" size={14} /></button>
            </li>)}
          </ul>
          : <p className="start-empty" role="status">No recent projects</p>}
      </section>
    </div>
  </main>;
}
