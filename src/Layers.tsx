import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { SiteLayer } from './siteLayers';
import { documentKey, targetKey } from './elementTargets';
import type { LayerDirection, Page, Region } from './types';
import { RequestShapeIcon } from './RegionOutline';
import { Icon, type IconName } from './icons';
import { withShortcut } from './shortcuts';

export function layerName(tag: string, text: string): string {
  return ({ main: 'Main content', header: 'Header', footer: 'Footer', nav: 'Navigation' }[tag]) || text.trim().slice(0, 80) || ({ div: 'Group', section: 'Section', img: 'Image', svg: 'Graphic', button: 'Button', a: 'Link', p: 'Paragraph', form: 'Form', input: 'Input', h1: 'Main heading', h2: 'Heading' }[tag] ?? 'Page item');
}
export function LayersPopover({ open, page, siteLayers, selectedElement, selectedRequest, busy, ready, anchor, contextElement, contextRequest, chooseElement, chooseRequest, arrange, copy, paste, close }: {
  open: boolean; page: Page; siteLayers: SiteLayer[]; selectedElement: string | null; selectedRequest: string | null; busy: boolean; ready: boolean;
  chooseElement: (item: SiteLayer) => void; chooseRequest: (region: Region) => void;
  arrange: (direction: LayerDirection) => void; copy: () => void; paste: () => void; close: () => void;
  anchor: { x: number; y: number; height: number } | null;
  contextElement: (item: SiteLayer, point: { x: number; y: number }) => void;
  contextRequest: (id: string, point: { x: number; y: number }) => void;
}) {
  const panel = useRef<HTMLElement>(null);
  const [position, setPosition] = useState({ left: 0, top: 0 });
  const opener = useRef<HTMLButtonElement | null>(null);
  const [collapsed, setCollapsed] = useState(new Set<string>());
  const pending = page.regions.map((region, index) => ({ region, index })).filter(item => !item.region.completed)
    .sort((a, b) => b.region.zIndex - a.region.zIndex || b.index - a.index).map(item => item.region);
  const find = (items: SiteLayer[]): SiteLayer | undefined => { for (const item of items) { if (targetKey(item.element) === selectedElement) return item; const match = find(item.children); if (match) return match; } };
  const selectedSite = find(siteLayers), requestIndex = pending.findIndex(region => region.id === selectedRequest);
  const canForward = selectedSite?.canForward ?? requestIndex > 0;
  const canBackward = selectedSite?.canBackward ?? (requestIndex >= 0 && requestIndex < pending.length - 1);
  const selected = !!selectedSite || requestIndex >= 0;
  useEffect(() => { setCollapsed(new Set()); }, [page.id]);
  useLayoutEffect(() => {
    if (!open || !panel.current) return;
    const parent = panel.current.offsetParent?.getBoundingClientRect(); if (!parent) return;
    const trigger = document.querySelector<HTMLButtonElement>('.element-toolbar [data-layers-trigger]') ?? document.querySelector<HTMLButtonElement>('.canvas-tools [data-layers-trigger]');
    const rect = trigger?.getBoundingClientRect(), bounds = panel.current.getBoundingClientRect();
    const at = anchor ?? (rect ? { x: rect.left - parent.left, y: rect.top - parent.top, height: rect.height } : { x: 8, y: 8, height: 0 });
    const left = Math.max(8, Math.min(at.x, parent.width - bounds.width - 8));
    const below = at.y + at.height + 6;
    const top = Math.max(8, Math.min(below + bounds.height <= parent.height - 8 ? below : at.y - bounds.height - 6, parent.height - bounds.height - 8));
    setPosition(previous => previous.left === left && previous.top === top ? previous : { left, top });
  });
  useEffect(() => {
    if (!open) return;
    opener.current = document.activeElement instanceof HTMLButtonElement && document.activeElement.hasAttribute('data-layers-trigger') ? document.activeElement : document.querySelector<HTMLButtonElement>('.element-toolbar [data-layers-trigger]') ?? document.querySelector<HTMLButtonElement>('.canvas-tools [data-layers-trigger]');
    const outside = (event: PointerEvent) => {
      const target = event.target;
      if (target instanceof Element && !panel.current?.contains(target) && !target.closest('[data-layers-trigger]')) close();
    };
    document.addEventListener('pointerdown', outside, true);
    const focus = requestAnimationFrame(() => (panel.current?.querySelector<HTMLButtonElement>('[data-selected="true"] button.layer-select') ?? panel.current?.querySelector<HTMLButtonElement>('button.layer-select') ?? panel.current?.querySelector<HTMLButtonElement>('.layers-close'))?.focus({ preventScroll: true }));
    return () => { cancelAnimationFrame(focus); document.removeEventListener('pointerdown', outside, true); };
  }, [open]);
  if (!open) return null;
  const rows = (items: SiteLayer[], depth = 0) => items.map(item => {
    const key = targetKey(item.element);
    return <div key={key} className="layer-branch" role="group">
    <div className="layer-row" data-selector={item.element.selector} data-document={item.element.document ? documentKey(item.element.document) : undefined} data-selected={selectedElement === key} style={{ paddingInlineStart: 6 + depth * 14 }} onContextMenu={event => { event.preventDefault(); event.stopPropagation(); contextElement(item, { x: event.clientX, y: event.clientY }); }}>
      {item.children.length ? <button type="button" className="layer-disclosure" aria-label={`${collapsed.has(key) ? 'Expand' : 'Collapse'} ${layerName(item.element.tag, item.element.label)}`} title={collapsed.has(key) ? 'Expand' : 'Collapse'} aria-expanded={!collapsed.has(key)} onClick={() => setCollapsed(current => { const next = new Set(current); if (next.has(key)) next.delete(key); else next.add(key); return next; })}><Icon name="chevron-right" size={12} className={collapsed.has(key) ? undefined : 'expanded'} /></button> : <span className="layer-spacer" />}
      <button type="button" className="layer-select" data-layer-selector={item.element.selector} aria-pressed={selectedElement === key} title={item.element.label || layerName(item.element.tag, '')} onClick={() => chooseElement(item)} disabled={busy}><LayerIcon /><span>{layerName(item.element.tag, item.element.label)}</span>{item.element.tag === 'iframe' && item.children.length ? <span className="layer-frame-tag" aria-hidden="true">frame</span> : null}</button>
    </div>
    {!collapsed.has(key) && rows(item.children, depth + 1)}
  </div>;
  });
  return <section ref={panel} className="layers-popover" style={position} role="dialog" aria-modal="false" aria-label="Layers" onKeyDown={event => {
    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); close(); (opener.current?.isConnected ? opener.current : document.querySelector<HTMLButtonElement>('.canvas-tools [data-layers-trigger]'))?.focus({ preventScroll: true }); }
    if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
      const buttons = [...panel.current!.querySelectorAll<HTMLButtonElement>('button.layer-select:not(:disabled)')];
      const index = buttons.indexOf(document.activeElement as HTMLButtonElement); if (index < 0) return;
      event.preventDefault(); event.stopPropagation(); buttons[event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length - 1 : Math.max(0, Math.min(buttons.length - 1, index + (event.key === 'ArrowDown' ? 1 : -1)))]?.focus({ preventScroll: true });
    }
  }}>
    <header><h2>Layers</h2><button type="button" className="layers-close icon-button sm" aria-label="Close Layers" title="Close" onClick={close}><Icon name="close" /></button></header>
    <div className="layer-actions" role="toolbar" aria-label="Layer actions">
      <button type="button" title={withShortcut('Bring to front', 'front')} aria-label="Bring to front" disabled={busy || !canForward} onClick={() => arrange('front')}><LayerIcon direction="front" /></button>
      <button type="button" title={withShortcut('Bring forward', 'forward')} aria-label="Bring forward" disabled={busy || !canForward} onClick={() => arrange('forward')}><LayerIcon direction="forward" /></button>
      <button type="button" title={withShortcut('Send backward', 'backward')} aria-label="Send backward" disabled={busy || !canBackward} onClick={() => arrange('backward')}><LayerIcon direction="backward" /></button>
      <button type="button" title={withShortcut('Send to back', 'back')} aria-label="Send to back" disabled={busy || !canBackward} onClick={() => arrange('back')}><LayerIcon direction="back" /></button>
      <span className="tool-divider" />
      <button type="button" title={withShortcut('Copy layer', 'copy')} aria-label="Copy layer" disabled={busy || !selected} onClick={copy}><Icon name="copy" /></button>
      <button type="button" title={withShortcut('Paste layer', 'paste')} aria-label="Paste layer" disabled={busy} onClick={paste}><Icon name="paste" /></button>
    </div>
    <div className="layer-list">
      <h3>Website</h3>
      {page.content && !ready ? <p role="status">Loading layers…</p> : siteLayers.length ? rows(siteLayers) : <p className="muted">No elements.</p>}
      <h3>Requests</h3>
      {pending.length ? pending.map((region, index) => <div key={region.id} className="layer-row" data-selected={selectedRequest === region.id} data-region-id={region.id} onContextMenu={event => { event.preventDefault(); event.stopPropagation(); contextRequest(region.id, { x: event.clientX, y: event.clientY }); }}>
        <span className="layer-spacer" /><button type="button" className="layer-select" aria-pressed={selectedRequest === region.id} disabled={busy} onClick={() => chooseRequest(region)} title={region.prompt || 'Empty request'}><span className="request-layer-mark" aria-hidden="true">{region.shape && region.shape.kind !== 'rect' && <RequestShapeIcon kind={region.shape.kind} />}</span><span>{region.prompt.trim().slice(0, 80) || `Request ${page.regions.indexOf(region) + 1}`}</span><span className="sr-only">, layer {index + 1} from front</span></button>
      </div>) : <p className="muted">No pending requests.</p>}
    </div>
  </section>;
}

export function LayerIcon({ direction }: { direction?: LayerDirection }) {
  const name: IconName = direction ? (`arrange-${direction}` as IconName) : 'layers';
  return <Icon name={name} />;
}
