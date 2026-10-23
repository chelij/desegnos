// Open page tabs are editor UI state: closing a tab only hides the page from the strip.
export type PageTabStorage = {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
};

export function pageTabsKey(projectId: string) {
  return `desegnos.page-tabs.${projectId}`;
}

/** Open tabs in page order, dropping ids for pages that no longer exist. */
export function visiblePageTabs(stored: string[] | undefined, pageIds: string[]): string[] {
  if (!stored?.length) return [...pageIds];
  const open = pageIds.filter(id => stored.includes(id));
  return open.length ? open : [...pageIds];
}

/** Open a page tab, keeping tabs in page order. Returns the same array when already open. */
export function showPageTab(openIds: string[], pageId: string, pageIds: string[]): string[] {
  if (!pageIds.includes(pageId) || openIds.includes(pageId)) return openIds;
  return pageIds.filter(id => id === pageId || openIds.includes(id));
}

/** Hide a page tab. The last open tab stays, and closing the active one selects its neighbour. */
export function closePageTab(openIds: string[], pageId: string, activeId: string | null): { open: string[]; active: string | null } {
  const index = openIds.indexOf(pageId);
  if (index < 0 || openIds.length <= 1) return { open: openIds, active: activeId };
  const open = openIds.filter(id => id !== pageId);
  if (activeId !== pageId) return { open, active: activeId };
  return { open, active: open[Math.min(index, open.length - 1)] };
}

/** All pages are open for a project with no saved tab state. */
export function loadPageTabs(projectId: string, pageIds: string[], storage?: PageTabStorage): string[] {
  try {
    const target = storage ?? globalThis.localStorage;
    const parsed: unknown = JSON.parse(target.getItem(pageTabsKey(projectId)) ?? 'null');
    const stored = Array.isArray(parsed) ? parsed.filter((id): id is string => typeof id === 'string') : undefined;
    return visiblePageTabs(stored, pageIds);
  } catch {
    return [...pageIds];
  }
}

export function savePageTabs(projectId: string, openIds: string[], storage?: PageTabStorage) {
  try {
    (storage ?? globalThis.localStorage).setItem(pageTabsKey(projectId), JSON.stringify(openIds));
  } catch {
    // Page tab state is a preference; losing it must not block editing.
  }
}
