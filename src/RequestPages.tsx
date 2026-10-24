// Compact inline page picker for a request: this page, specific pages, path groups or all pages.
import { useEffect, useRef, useState } from 'react';
import { coversGroup, coversSelection, normalizeSelections, pageGroups, pageSelectionLabel, toggleAll, toggleGroup, togglePage } from './requestPages';
import type { Page, PageSelection } from './types';

export function RequestPages({ pages, hostPageId, selection, change, label = 'Pages' }: {
  pages: Page[]; hostPageId: string; selection: PageSelection[]; change: (selection: PageSelection[]) => void; label?: string;
}) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (event: PointerEvent) => { if (!(event.target instanceof Node) || !root.current?.contains(event.target)) setOpen(false); };
    window.addEventListener('pointerdown', close);
    return () => window.removeEventListener('pointerdown', close);
  }, [open]);
  const groups = pageGroups(pages).filter(group => group.path !== '/' && group.pages.length > 1);
  const chosen = normalizeSelections(selection, pages);
  return <div className="request-pages" ref={root} onPointerDown={event => event.stopPropagation()}>
    <button type="button" className="request-pages-summary" aria-expanded={open} title="Pages this request can change" onClick={() => setOpen(value => !value)}>{label} · {pageSelectionLabel(chosen, pages)}</button>
    {open && <div className="request-pages-panel" role="group" aria-label="Pages this request can change">
      <label className="check-label"><input type="checkbox" checked={chosen.length === 0} onChange={() => change([])} />This page</label>
      <label className="check-label"><input type="checkbox" checked={chosen.some(item => item.kind === 'all')} onChange={() => change(toggleAll(chosen))} />All pages</label>
      {groups.map(group => <label key={group.path} className="check-label request-pages-group"><input type="checkbox" checked={coversGroup(chosen, group)} onChange={() => change(toggleGroup(chosen, group, pages))} />{group.path.replace(/^\//, '')}/*</label>)}
      <ul className="request-pages-list">{pages.map(page => <li key={page.id}><label className={`check-label${page.id === hostPageId ? ' host-page' : ''}`}><input type="checkbox" checked={coversSelection(chosen, page)} onChange={() => change(togglePage(chosen, page, pages))} />{page.name}</label></li>)}</ul>
    </div>}
  </div>;
}
