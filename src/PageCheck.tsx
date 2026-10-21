import { useMemo } from 'react';
import type { Project } from './types';

export type CheckKind = 'placeholder' | 'issue' | 'found';
export type CheckItem = { id: string; kind: CheckKind; pageId: string; pageName: string; title: string; detail: string; fix: string; elementId: string | null; selector: string | null; tag: string | null };

const PLACEHOLDER_TEXT = /\b(placeholder|lorem ipsum|sample (text|content|copy|image|photo)|add your [a-z]+|your (business|company|brand|shop) name|tbd|coming soon)\b/i;

// Same structural selector the canvas uses, so an item can be selected there.
function selectorFor(node: Element): string | null {
  const parts: string[] = [];
  for (let current: Element | null = node; current && current.localName !== 'body'; current = current.parentElement) {
    if (!current.parentElement || parts.length >= 63) return null;
    const siblings = Array.from(current.parentElement.children).filter(sibling => sibling.localName === current!.localName);
    parts.unshift(`${current.localName}:nth-of-type(${siblings.indexOf(current) + 1})`);
  }
  return ['body', ...parts].join(' > ');
}

function snippet(node: Element) {
  const text = (node.getAttribute('aria-label') || node.textContent || node.getAttribute('alt') || '').replace(/\s+/g, ' ').trim();
  return text.length > 70 ? `${text.slice(0, 70)}…` : text;
}

// Static checks on the saved page source. DOMParser never runs scripts or loads resources.
export function checkProject(project: Project): CheckItem[] {
  const items: CheckItem[] = [];
  for (const page of project.pages) {
    if (!page.content?.html) continue;
    const doc = new DOMParser().parseFromString(page.content.html, 'text/html');
    const seen = new Set<Element>();
    const add = (kind: CheckKind, node: Element | null, title: string, detail: string, fix: string) => {
      if (node) { if (seen.has(node)) return; seen.add(node); }
      items.push({ id: `${page.id}:${items.length}`, kind, pageId: page.id, pageName: page.name, title, detail, fix, elementId: node?.getAttribute('data-desegnos-id') ?? null, selector: node ? selectorFor(node) : null, tag: node?.localName ?? null });
    };
    for (const node of doc.body.querySelectorAll('[data-desegnos-placeholder], img[alt^="Image placeholder" i]'))
      add('placeholder', node, 'Image placeholder', snippet(node).replace(/^Image placeholder:?\s*/i, '') || 'No picture yet', 'Find a real, suitable photo for this on the web and use it here, with a visible credit.');
    for (const node of doc.body.querySelectorAll('[data-desegnos-found]')) {
      let host = ''; try { host = new URL(node.getAttribute('data-desegnos-found') ?? '').hostname.replace(/^www\./, ''); } catch {}
      items.push({ id: `${page.id}:${items.length}`, kind: 'found', pageId: page.id, pageName: page.name, title: node.localName === 'img' ? 'Picture found online' : 'Text found online', detail: [host, snippet(node)].filter(Boolean).join(' · '), fix: '', elementId: node.getAttribute('data-desegnos-id'), selector: selectorFor(node), tag: node.localName });
    }
    const walker = doc.createTreeWalker(doc.body, NodeFilter.SHOW_TEXT);
    for (let text = walker.nextNode(); text; text = walker.nextNode()) {
      const parent = text.parentElement;
      if (!parent || ['SCRIPT', 'STYLE'].includes(parent.tagName) || !PLACEHOLDER_TEXT.test(text.nodeValue ?? '')) continue;
      add('placeholder', parent, 'Placeholder text', snippet(parent), 'Replace this placeholder with real content. Search the web for facts if needed; ask me only for private details.');
    }
    for (const node of doc.body.querySelectorAll('img:not([alt])'))
      add('issue', node, 'Image has no description', 'Screen readers and search engines can’t tell what it shows.', 'Add a short, accurate description (alt text) for this image.');
    for (const node of doc.body.querySelectorAll('a')) {
      const href = (node.getAttribute('href') ?? '').trim();
      if (!href || href === '#' || /^javascript:/i.test(href)) add('issue', node, 'Link goes nowhere', snippet(node) || 'Unnamed link', 'Make this link go to a real destination, or turn it into plain text.');
    }
    for (const node of doc.body.querySelectorAll('a, button'))
      if (!snippet(node) && !node.querySelector('img[alt]:not([alt=""])')) add('issue', node, `${node.localName === 'a' ? 'Link' : 'Button'} has no label`, 'People using screen readers won’t know what it does.', 'Give this a clear, short visible label.');
    const headings = Array.from(doc.body.querySelectorAll('h1, h2, h3, h4, h5, h6'));
    const h1s = headings.filter(node => node.localName === 'h1');
    if (h1s.length === 0) add('issue', null, 'No main heading', 'Each page should have one main (H1) heading.', 'Add one clear main heading (H1) for this page.');
    else if (h1s.length > 1) add('issue', h1s[1], 'More than one main heading', snippet(h1s[1]), 'Make this a smaller heading so the page has a single main heading.');
    let level = 1;
    for (const node of headings) {
      const next = Number(node.localName[1]);
      if (next > level + 1) add('issue', node, 'Heading skips a level', `${snippet(node)} (H${level} → H${next})`, `Change this heading to H${level + 1} so the outline doesn’t skip a level.`);
      level = next;
    }
    if (doc.querySelector('title') && !doc.title.trim()) add('issue', null, 'Page has no title', 'The browser tab and search results will show no name.', 'Add a short, descriptive page title.');
  }
  return items;
}

export function PageCheckPanel({ items, busy, close, show, fix, source }: {
  items: CheckItem[]; busy: boolean; close: () => void; show: (item: CheckItem) => void; fix: (item: CheckItem) => void; source: (item: CheckItem) => void;
}) {
  const groups = useMemo(() => [['placeholder', 'Still placeholder'], ['issue', 'Things to fix'], ['found', 'Found online']] as const, []);
  return <section className="page-check" role="dialog" aria-label="Page check">
    <header><h2>Page check</h2><button type="button" className="page-check-close" aria-label="Close page check" onClick={close}>×</button></header>
    {items.filter(item => item.kind !== 'found').length === 0 && <p className="page-check-empty">No issues found.</p>}{groups.map(([kind, label]) => {
      const list = items.filter(item => item.kind === kind);
      return list.length > 0 && <section key={kind}><h3>{label} ({list.length})</h3><ul>{list.map(item => <li key={item.id}>
        <div className="page-check-text"><strong>{item.title}</strong><span>{item.pageName}{item.detail ? ` · ${item.detail}` : ''}</span></div>
        <div className="page-check-actions">{item.selector && <button type="button" onClick={() => show(item)}>Show</button>}{item.kind === 'found' ? <button type="button" onClick={() => source(item)}>Source</button> : <button type="button" disabled={busy} onClick={() => fix(item)}>Ask AI to fix</button>}</div>
      </li>)}</ul></section>;
    })}
  </section>;
}
