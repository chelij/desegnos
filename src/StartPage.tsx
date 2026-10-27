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

type PreviewRect = { x: number; y: number; width: number; height: number };

// The previews are drawn on the same dot bed as the canvas. Each outline keeps the aspect
// ratio of the boundary it stands for, so the tiles read as small canvases rather than icons.
const PREVIEWS: Record<Exclude<CanvasTemplate, 'blank'>, PreviewRect[]> = {
  website: [
    { x: 6, y: 24, width: 88, height: 55 },
    { x: 100, y: 24, width: 50, height: 72 },
    { x: 156, y: 24, width: 22, height: 48 },
  ],
  appScreen: [{ x: 66, y: 8, width: 48, height: 104 }],
  slideDeck: [
    { x: 60, y: 12, width: 108, height: 61 },
    { x: 48, y: 26, width: 108, height: 61 },
    { x: 36, y: 40, width: 108, height: 61 },
  ],
  socialImage: [{ x: 42, y: 8, width: 96, height: 96 }],
};

function Preview({ kind }: { kind: CanvasTemplate }) {
  return <span className={`start-preview start-preview-${kind}`} aria-hidden="true">
    <svg viewBox="0 0 180 120" focusable="false">
      {kind === 'blank'
        ? <path className="start-preview-blank" d="M77 60h26M90 47v26" />
        : PREVIEWS[kind].map(rect => <rect key={`${rect.x}:${rect.y}:${rect.width}`} x={rect.x} y={rect.y} width={rect.width} height={rect.height}
          fill="none" stroke="currentColor" strokeWidth="1.5" vectorEffect="non-scaling-stroke" />)}
    </svg>
  </span>;
}

function folderOf(path: string) {
  const slash = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  if (slash < 0) return path;
  if (slash === 0) return '/';
  return path.slice(0, slash);
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

function openedTitle(openedAtMs: number) {
  return openedAtMs ? new Date(openedAtMs).toLocaleString() : '';
}

export function StartPage({ busy, create, openProject, recentProjects, openRecent, forgetRecent, pinRecent, recovery, showStartPageOnLaunch, changeShowStartPage }: {
  busy: boolean;
  create: (template: CanvasTemplate) => void;
  openProject: () => void;
  recentProjects: RecentProject[];
  openRecent: (path: string) => void;
  forgetRecent: (path: string) => void;
  pinRecent: (path: string, pinned: boolean) => void;
  recovery?: ReactNode;
  showStartPageOnLaunch: boolean;
  changeShowStartPage: (value: boolean) => void;
}) {
  // Pinned projects lead the table; within each group the most recently opened comes first.
  const orderedRecents = [...recentProjects].sort((a, b) => Number(b.pinned) - Number(a.pinned) || b.openedAtMs - a.openedAtMs);
  return <div className="start-page">
    <nav className="start-rail" aria-label="Start">
      <div className="start-rail-nav">
        <span className="start-rail-current" aria-current="page"><Icon name="home" /><span className="start-rail-label">Home</span></span>
        <button type="button" className="start-rail-action" disabled={busy} onClick={() => create('blank')}>
          <Icon name="plus" /><span className="start-rail-label">New</span>
        </button>
        <button type="button" className="start-rail-action" disabled={busy} onClick={openProject}>
          <Icon name="folder" /><span className="start-rail-label">Open</span>
        </button>
      </div>
      <details className="start-rail-preferences">
        <summary><Icon name="properties" /><span className="start-rail-label">Preferences</span><Icon name="chevron-right" size={14} className="start-rail-chevron" /></summary>
        <label className="check-label start-pref">
          <input type="checkbox" checked={showStartPageOnLaunch} onChange={event => changeShowStartPage(event.target.checked)} />
          <span>Show start page on launch</span>
        </label>
      </details>
    </nav>
    <main className="start-content">
      <div className="start-content-inner">
        <section className="start-section" aria-labelledby="start-new-title">
          <h2 className="start-section-title" id="start-new-title">New</h2>
          <div className="start-tiles">
            <button type="button" className="start-tile" disabled={busy} onClick={() => create('blank')}>
              <Preview kind="blank" />
              <span className="start-tile-name">Blank canvas</span>
            </button>
            {TEMPLATES.map(template => <button type="button" className="start-tile" key={template.id} disabled={busy} onClick={() => create(template.id)}>
              <Preview kind={template.id} />
              <span className="start-tile-name">{template.label}</span>
            </button>)}
          </div>
        </section>
        {recovery && <section className="start-section start-recovery" aria-label="Unsaved work">
          <h2 className="start-section-title">Unsaved work</h2>
          {recovery}
        </section>}
        <section className="start-section start-recents" aria-labelledby="start-recents-title">
          <h2 className="start-section-title" id="start-recents-title">Recent</h2>
          {orderedRecents.length > 0 && <div className="start-table-wrap">
            <table className="start-recents-table">
              <thead><tr>
                <th scope="col">Name</th>
                <th scope="col">Location</th>
                <th scope="col">Last opened</th>
                <th scope="col" className="start-recent-actions-head"><span className="sr-only">Pin</span></th>
              </tr></thead>
              <tbody>
                {orderedRecents.map(entry => <tr className={entry.pinned ? 'start-recent pinned' : 'start-recent'} key={entry.path} onDoubleClick={() => openRecent(entry.path)}>
                  <td className="start-recent-name-cell">
                    <button type="button" className="start-recent-open" title={entry.path} onClick={() => openRecent(entry.path)}>
                      <span className="start-recent-name">{entry.name}</span>
                    </button>
                  </td>
                  <td className="start-recent-location"><span title={folderOf(entry.path)}>{folderOf(entry.path)}</span></td>
                  <td className="start-recent-opened"><time dateTime={entry.openedAtMs ? new Date(entry.openedAtMs).toISOString() : undefined} title={openedTitle(entry.openedAtMs)}>{openedLabel(entry.openedAtMs)}</time></td>
                  <td className="start-recent-actions" onDoubleClick={event => event.stopPropagation()}>
                    <button type="button" className="start-recent-pin" aria-pressed={entry.pinned}
                      aria-label={`${entry.pinned ? 'Unpin' : 'Pin'} ${entry.name}`} title={entry.pinned ? 'Unpin' : 'Pin'}
                      onClick={() => pinRecent(entry.path, !entry.pinned)}><Icon name="pin" size={14} /></button>
                    <button type="button" className="start-recent-remove" aria-label={`Remove ${entry.name} from recent projects`} title="Remove from recent projects"
                      onClick={() => forgetRecent(entry.path)}><Icon name="close" size={14} /></button>
                  </td>
                </tr>)}
              </tbody>
            </table>
          </div>}
        </section>
      </div>
    </main>
  </div>;
}
