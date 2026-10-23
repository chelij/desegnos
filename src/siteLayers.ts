import { reorderedLayers } from './layerOrder';
import type { ElementSelection, ElementStyleEdit, ElementTarget, LayerDirection, Rect } from './types';

export type SiteLayer = ElementSelection & { children: SiteLayer[]; canForward: boolean; canBackward: boolean };
const excluded = new Set(['script', 'style', 'link', 'meta', 'template', 'noscript', 'source', 'track']);
// A framed document has its own window; computed styles come from the node's own document.
const computedStyle = (node: Element, pseudo?: string) => {
  const view = node.ownerDocument.defaultView;
  return view ? view.getComputedStyle(node, pseudo) : getComputedStyle(node, pseudo);
};
const box = (node: Element): Rect => { const r = node.getBoundingClientRect(); return { x: r.x, y: r.y, width: r.width, height: r.height }; };
const visible = (node: Element) => { const r = box(node); return !excluded.has(node.localName) && Object.values(r).every(Number.isFinite) && r.width >= 1 && r.height >= 1 && computedStyle(node).visibility !== 'hidden'; };

function paintOrder(node: Element, order: boolean): { category: number; z: number; order: number } {
  const css = computedStyle(node), parent = node.parentElement && computedStyle(node.parentElement);
  const indexed = css.position !== 'static' || !!parent && /^(inline-)?(flex|grid)$/.test(parent.display);
  const z = indexed ? Number.parseInt(css.zIndex) || 0 : 0;
  const context = css.opacity !== '1' || css.transform !== 'none' || css.filter !== 'none' || css.perspective !== 'none' || css.isolation === 'isolate' || css.mixBlendMode !== 'normal' || /paint|layout|strict|content/.test(css.contain) || css.willChange.split(',').some(value => ['transform', 'opacity', 'filter'].includes(value.trim()));
  const category = z < 0 ? 0 : z > 0 ? 5 : css.position !== 'static' || context ? 4 : css.cssFloat !== 'none' ? 2 : css.display.startsWith('inline') || parent && /^(inline-)?(flex|grid)$/.test(parent.display) ? 3 : 1;
  return { category, z, order: order ? Number.parseInt(css.order) || 0 : 0 };
}
function siblings(parent: Element): Element[] {
  const order = /^(inline-)?(flex|grid)$/.test(computedStyle(parent).display);
  return [...parent.children].filter(visible).map((node, index) => ({ node, index, ...paintOrder(node, order) }))
    .sort((a, b) => a.category - b.category || a.z - b.z || a.order - b.order || a.index - b.index).map(item => item.node);
}

export function siteLayerTree(doc: Document, select: (node: Element) => ElementSelection | null): SiteLayer[] {
  const visit = (parent: Element): SiteLayer[] => {
    const peers = siblings(parent);
    const indexed = new Map(peers.map((node, index) => [node, index]));
    const descendants = [...parent.children].filter(node => !excluded.has(node.localName) && !indexed.has(node) && computedStyle(node).display !== 'none').flatMap(visit);
    return [...peers].reverse().flatMap(node => {
      const item = select(node);
      const children = node.localName === 'svg' ? [] : visit(node);
      if (!item) return children;
      const index = indexed.get(node)!;
      return [{ ...item, children, canForward: index < peers.length - 1, canBackward: index > 0 }];
    }).concat(descendants);
  };
  return visit(doc.body);
}

// Stacking belongs to a parent group. Keep DOM reading order and every measured
// position intact; a layer edit must not quietly turn into a layout edit.
export function siteLayerStyles(node: Element, direction: LayerDirection, target: (node: Element) => ElementTarget | null): ElementStyleEdit[] | null {
  const parent = node.parentElement;
  if (!parent || node.localName === 'body') throw new Error('Select an item inside the page.');
  const peers = siblings(parent), next = reorderedLayers(peers, node, direction);
  if (!next) return null;
  const measured = [parent, ...parent.querySelectorAll('*')].filter(value => !excluded.has(value.localName));
  if (measured.length > 20000) throw new Error('Select a smaller group to arrange its layers.');
  const before = measured.map(box);
  const doc = node.ownerDocument;
  const points = peers.flatMap(peer => { const rect = box(peer); return [.25, .5, .75].map(part => ({ x: rect.x + rect.width * part, y: rect.y + rect.height * part })); })
    .filter(point => point.x >= 0 && point.y >= 0 && point.x < doc.documentElement.clientWidth && point.y < doc.documentElement.clientHeight);
  const unaffected = (point: { x: number; y: number }) => doc.elementsFromPoint(point.x, point.y).filter(value => !node.contains(value));
  const paint = points.map(unaffected);
  const crossed = peers.filter(peer => peer !== node && (peers.indexOf(peer) < peers.indexOf(node)) !== (next.indexOf(peer) < next.indexOf(node)));
  const overlaps = crossed.flatMap(peer => {
    const a = box(node), b = box(peer), left = Math.max(a.x, b.x, 0), top = Math.max(a.y, b.y, 0);
    const right = Math.min(a.x + a.width, b.x + b.width, doc.documentElement.clientWidth), bottom = Math.min(a.y + a.height, b.y + b.height, doc.documentElement.clientHeight);
    if (right <= left || bottom <= top) return [];
    return [.25, .5, .75].map(part => ({ peer, x: left + (right - left) * part, y: top + (bottom - top) * part }));
  });
  const identity = target(node);
  if (!identity) throw new Error('The page changed. Select this layer again.');
  const original = node.getAttribute('style'), css = doc.createElement('div').style;
  css.cssText = original ?? '';
  const layout = computedStyle(parent).display;
  if (computedStyle(node).position === 'static' && !/^(inline-)?(flex|grid)$/.test(layout)) {
    css.setProperty('position', 'relative', 'important'); css.setProperty('inset', 'auto', 'important');
  }
  const zValues = peers.map(peer => Number.parseInt(computedStyle(peer).zIndex) || 0);
  const destination = next.indexOf(node), neighbor = next[destination + (direction === 'forward' || direction === 'front' ? -1 : 1)];
  const neighborZ = Number.parseInt(computedStyle(neighbor).zIndex) || 0;
  const candidates = [...new Set([neighborZ, neighborZ + 1, neighborZ - 1, ...zValues.flatMap(z => [z, z + 1, z - 1])])]
    .filter(z => Number.isSafeInteger(z) && Math.abs(z) <= 2147483647);
  try {
    for (const z of candidates) {
      css.setProperty('z-index', String(z), 'important'); node.setAttribute('style', css.cssText);
      const actual = siblings(parent);
      if (actual.some((value, index) => value !== next[index])) continue;
      if (measured.some((value, index) => Object.entries(box(value)).some(([key, number]) => Math.abs(number - before[index][key as keyof Rect]) > .5))) {
        throw new Error('Arranging this item would move part of its layout. Select its containing group to move that group instead.');
      }
      if (points.some((point, index) => { const after = unaffected(point); return after.length !== paint[index].length || after.some((value, at) => value !== paint[index][at]); })) {
        throw new Error('This item shares its stacking with another group. Select its containing group to arrange it without changing other items.');
      }
      if (overlaps.some(point => {
        const stack = doc.elementsFromPoint(point.x, point.y);
        const selected = stack.findIndex(value => node.contains(value)), other = stack.findIndex(value => point.peer.contains(value));
        return selected >= 0 && other >= 0 && (selected < other) !== (next.indexOf(node) > next.indexOf(point.peer));
      })) throw new Error('This item has its own painting rules. Select its containing group to arrange it.');
      return [{ target: identity, style: css.cssText }];
    }
    throw new Error('These items share a stacking level that cannot be split without changing another item. Select their containing group to arrange it.');
  } finally {
    if (original === null) node.removeAttribute('style'); else node.setAttribute('style', original);
  }
}

export function siteCopyStyles(node: Element, identify: (node: Element) => ElementTarget | null): ElementStyleEdit[] {
  const nodes = [node, ...node.querySelectorAll('*')];
  if (nodes.length > 512) throw new Error('This layer is too large to copy at once. Copy a smaller group.');
  const doc = node.ownerDocument, pause = doc.head.querySelector('style[data-desegnos-editor-paused]'), next = pause?.nextSibling;
  pause?.remove();
  try {
    const snapshot = (computed: CSSStyleDeclaration) => {
      const style = doc.createElement('div').style;
      for (const name of computed) {
        const css = computed.getPropertyValue(name).replace(/url\((['"]?)(.*?)\1\)/g, (original, _quote: string, address: string) => {
          try { const url = new URL(address, doc.baseURI); const base = url.href.split('#')[0]; return url.hash && [doc.URL.split('#')[0], doc.baseURI.split('#')[0]].includes(base) ? `url("${url.hash}")` : original; } catch { return original; }
        });
        style.setProperty(name, css, 'important');
      }
      return style.cssText;
    };
    return nodes.map(value => {
      const target = identify(value); if (!target) throw new Error('The page changed. Select this layer again.');
      const computed = computedStyle(value);
      const pseudoStyles = (['before', 'after', 'marker'] as const).flatMap(pseudo => {
        const decoration = computedStyle(value, `::${pseudo}`);
        const marker = pseudo === 'marker' && computed.display === 'list-item' && decoration.content !== 'none';
        return marker || decoration.content && !['none', 'normal'].includes(decoration.content) ? [{ pseudo, style: snapshot(decoration) }] : [];
      });
      return { target, style: snapshot(computed), ...(pseudoStyles.length ? { pseudoStyles } : {}) };
    });
  } finally { if (pause) doc.head.insertBefore(pause, next ?? null); }
}
