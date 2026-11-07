import { useCallback, useSyncExternalStore, type ReactNode } from 'react';
import { Icon } from '../icons';
import { readPreference as readString, writePreference as store } from '../preferences';

const SECTION_COLLAPSED_KEY = 'desegnos.dock-section-collapsed';

function readCollapsedSections(): Set<string> {
  try {
    const raw: unknown = JSON.parse(readString(SECTION_COLLAPSED_KEY) ?? '[]');
    return new Set(Array.isArray(raw) ? raw.filter((key): key is string => typeof key === 'string') : []);
  } catch { return new Set(); }
}
function storeCollapsedSections(collapsed: ReadonlySet<string>) {
  store(SECTION_COLLAPSED_KEY, JSON.stringify([...collapsed]));
}

// One shared snapshot keeps separately mounted sections in step and on disk;
// every toggle merges into the latest stored set instead of overwriting it.
let collapsedSections: ReadonlySet<string> | null = null;
const sectionSubscribers = new Set<() => void>();
function collapsedSnapshot(): ReadonlySet<string> {
  collapsedSections ??= readCollapsedSections();
  return collapsedSections;
}
function subscribeSections(listener: () => void) {
  sectionSubscribers.add(listener);
  return () => { sectionSubscribers.delete(listener); };
}
export function setSectionCollapsed(key: string, collapsed: boolean) {
  const next = new Set(collapsedSnapshot());
  if (collapsed) next.add(key); else next.delete(key);
  collapsedSections = next;
  storeCollapsedSections(next);
  for (const listener of sectionSubscribers) listener();
}
export function useCollapsedSections(): ReadonlySet<string> {
  return useSyncExternalStore(subscribeSections, collapsedSnapshot, collapsedSnapshot);
}

export function useSectionOpen(key: string): [boolean, () => void] {
  const collapsed = useCollapsedSections();
  const toggle = useCallback(() => setSectionCollapsed(key, !collapsedSnapshot().has(key)), [key]);
  return [!collapsed.has(key), toggle];
}

export function SectionHeader({ title, open, toggle, add, addLabel, addExpanded }: {
  title: string; open: boolean; toggle: () => void; add?: () => void; addLabel?: string; addExpanded?: boolean;
}) {
  return <div className="dock-section-head">
    <h3 className="dock-section-title">{title}</h3>
    {add && <button type="button" className="dock-section-add" aria-label={addLabel} title={addLabel} aria-expanded={addExpanded} onClick={add}><Icon name="plus" size={12} /></button>}
    <button type="button" className="dock-section-toggle" aria-expanded={open} aria-label={`${open ? 'Collapse' : 'Expand'} ${title.toLowerCase()}`}
      title={`${open ? 'Collapse' : 'Expand'} ${title}`} onClick={toggle}><Icon name="chevron-right" size={12} className={open ? 'expanded' : undefined} /></button>
  </div>;
}

export function PropsSection({ id, title, children, add, addLabel, addExpanded, group }: {
  id: string; title: string; children: ReactNode; add?: () => void; addLabel?: string; addExpanded?: boolean; group?: boolean;
}) {
  const [open, toggle] = useSectionOpen(id);
  return <div className={`props-section${open ? '' : ' is-collapsed'}`} {...(group ? { role: 'group', 'aria-label': title } : {})}>
    <SectionHeader title={title} open={open} toggle={toggle} add={add} addLabel={addLabel} addExpanded={addExpanded} />
    {open && <div className="props-section-body">{children}</div>}
  </div>;
}
