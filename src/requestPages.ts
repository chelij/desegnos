// The pages a request may change: its own page by default, specific pages, a path group or all.
import type { Page, PageSelection } from './types';

export type PageGroup = { path: string; pages: Page[] };

const pathOf = (path: string) => path.replace(/^\/+|\/+$/g, '');

/** The directory a page path belongs to, for example `/projects` for `/projects/atlas`. */
export function pageGroupPath(path: string): string {
  const trimmed = path.replace(/\/+$/, '');
  const index = trimmed.lastIndexOf('/');
  return index > 0 ? trimmed.slice(0, index) : '/';
}

/** Pages grouped by their directory, in page order. */
export function pageGroups(pages: Page[]): PageGroup[] {
  const groups = new Map<string, Page[]>();
  for (const page of pages) {
    const path = pageGroupPath(page.path);
    const group = groups.get(path);
    if (group) group.push(page);
    else groups.set(path, [page]);
  }
  return [...groups].map(([path, pages]) => ({ path, pages }));
}

export function coversPage(selection: PageSelection, page: Page): boolean {
  if (selection.kind === 'all') return true;
  if (selection.kind === 'page') return selection.pageId === page.id;
  const location = pathOf(page.path);
  const prefix = pathOf(selection.path);
  return !!prefix && (location === prefix || location.startsWith(`${prefix}/`));
}

export function coversSelection(selection: PageSelection[], page: Page): boolean {
  return selection.some(item => coversPage(item, page));
}

export function coversGroup(selection: PageSelection[], group: PageGroup): boolean {
  return group.pages.length > 0 && group.pages.every(page => coversSelection(selection, page));
}

export function selectionKey(selection: PageSelection): string {
  return selection.kind === 'page' ? `page:${selection.pageId}` : selection.kind === 'prefix' ? `prefix:${pathOf(selection.path)}` : 'all';
}

/** Drop duplicates and pages already covered by a group or by All pages. */
export function normalizeSelections(selection: PageSelection[], pages: Page[]): PageSelection[] {
  if (selection.some(item => item.kind === 'all')) return [{ kind: 'all' }];
  const prefixes = selection
    .flatMap(item => item.kind === 'prefix' ? [{ kind: 'prefix' as const, path: pathOf(item.path) }] : [])
    .filter(item => item.path)
    .filter((item, index, all) => all.findIndex(other => other.path === item.path) === index);
  const pageIds = [...new Set(selection.flatMap(item => item.kind === 'page' ? [item.pageId] : []))];
  const chosen = pageIds
    .filter(id => !prefixes.some(prefix => {
      const page = pages.find(item => item.id === id);
      return page ? coversPage(prefix, page) : false;
    }))
    .map(pageId => ({ kind: 'page' as const, pageId }));
  return [...prefixes, ...chosen];
}

/** The compact label beside the Pages control. */
export function pageSelectionLabel(selection: PageSelection[], pages: Page[]): string {
  if (selection.length === 0) return 'This page';
  if (selection.some(item => item.kind === 'all')) return 'All pages';
  const covered = pages.filter(page => coversSelection(selection, page));
  if (selection.length === 1 && selection[0].kind === 'prefix') return `${pathOf(selection[0].path)}/*`;
  if (covered.length === 1) return covered[0].name;
  return `${covered.length} pages`;
}

export function toggleAll(selection: PageSelection[]): PageSelection[] {
  return selection.some(item => item.kind === 'all') ? [] : [{ kind: 'all' }];
}

/** Check or uncheck a whole path group, keeping other choices. */
export function toggleGroup(selection: PageSelection[], group: PageGroup, pages: Page[]): PageSelection[] {
  const path = pathOf(group.path);
  if (!path) return selection;
  if (!coversGroup(selection, group)) {
    return normalizeSelections([...selection, { kind: 'prefix', path }], pages);
  }
  return group.pages.reduce((current, page) => togglePage(current, page, pages), selection);
}

/** Check or uncheck one page, replacing a covering group with its remaining pages. */
export function togglePage(selection: PageSelection[], page: Page, pages: Page[]): PageSelection[] {
  if (!coversSelection(selection, page)) {
    return normalizeSelections([...selection, { kind: 'page', pageId: page.id }], pages);
  }
  const next: PageSelection[] = [];
  for (const item of selection) {
    if (item.kind === 'page') {
      if (item.pageId !== page.id) next.push(item);
    } else if (item.kind === 'all' || coversPage(item, page)) {
      for (const other of pages) {
        if (other.id !== page.id && (item.kind === 'all' || coversPage(item, other))) {
          next.push({ kind: 'page', pageId: other.id });
        }
      }
    } else {
      next.push(item);
    }
  }
  return normalizeSelections(next, pages);
}
