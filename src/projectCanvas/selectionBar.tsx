import type { CSSProperties } from 'react';
import { Icon } from '../icons';
import { withShortcut } from '../shortcuts';
import type { SelectionDistribution } from './arrange';
import type { ScreenRect } from './surface';

export type SelectionAlign = 'left' | 'centre' | 'right' | 'top' | 'middle' | 'bottom';

/** Whether an edit lands in this linked size only, or all sizes together. */
export type EditScope = 'this' | 'all';

const EDIT_SCOPE_EVENT = 'desegnos:edit-scope';

/** Components that cannot reach the canvas selection ask for the scope it should use. */
export function requestEditScope(scope: EditScope) {
  window.dispatchEvent(new CustomEvent<EditScope>(EDIT_SCOPE_EVENT, { detail: scope }));
}

export function onEditScope(handle: (scope: EditScope) => void): () => void {
  const listener = (event: Event) => handle((event as CustomEvent<EditScope>).detail);
  window.addEventListener(EDIT_SCOPE_EVENT, listener);
  return () => window.removeEventListener(EDIT_SCOPE_EVENT, listener);
}

/** The two editing scopes of a boundary with more than one linked size. */
export function EditScopeChoice({ scope, choose }: { scope: EditScope; choose: (scope: EditScope) => void }) {
  return <div className="segmented edit-scope" role="group" aria-label="Edit scope">
    <button type="button" aria-pressed={scope === 'this'} onClick={() => choose('this')}>This size</button>
    <button type="button" aria-pressed={scope === 'all'} onClick={() => choose('all')}>All sizes</button>
  </div>;
}

const ALIGN: { edge: SelectionAlign; label: string; icon: string }[] = [
  { edge: 'left', label: 'left', icon: 'M3 2.5v11M5 5h7M5 10h4' },
  { edge: 'centre', label: 'centre', icon: 'M8 2.5v11M4.5 5h7M6 10h4' },
  { edge: 'right', label: 'right', icon: 'M13 2.5v11M4 5h7M7 10h4' },
  { edge: 'top', label: 'top', icon: 'M2.5 3h11M5 5v7M10 5v4' },
  { edge: 'middle', label: 'middle', icon: 'M2.5 8h11M5 4.5v7M10 6v4' },
  { edge: 'bottom', label: 'bottom', icon: 'M2.5 13h11M5 4v7M10 7v4' },
];

const hex = (value: string) => {
  const trimmed = value.trim();
  const short = /^#([0-9a-f]{3})$/i.exec(trimmed);
  if (short) return `#${short[1].split('').map(part => part + part).join('')}`;
  return /^#[0-9a-f]{6}$/i.test(trimmed) ? trimmed : null;
};

/**
 * The contextual bar for the current layer selection. It lives in screen pixels above the
 * selection, so it never scales with the canvas and never lands inside a boundary.
 */
export function SelectionBar({ screen, viewWidth, fill, linked, linkable, hyperlink, scope, onFill, onAlign, onDistribute, count, onScope, onRequest, onLinkTo, onLink, onUnlink, onDuplicate, onDelete }: {
  screen: ScreenRect; viewWidth: number;
  fill: string; linked: boolean; linkable: boolean; hyperlink: boolean;
  scope: EditScope | null;
  onFill: (value: string) => void;
  onAlign: (edge: SelectionAlign) => void;
  onDistribute: (direction: SelectionDistribution) => void;
  count: number;
  onScope: (scope: EditScope) => void;
  onRequest: () => void;
  onLinkTo: () => void;
  onLink: () => void;
  onUnlink: () => void;
  onDuplicate: () => void;
  onDelete: () => void;
}) {
  // Enough room for the rounded bar above the layer; otherwise it drops underneath.
  const below = screen.top < 52;
  const half = Math.min(250, Math.max(0, viewWidth / 2 - 8));
  const centre = screen.left + screen.width / 2;
  const left = Math.min(Math.max(centre, half), Math.max(half, viewWidth - half));
  const position: CSSProperties = below
    ? { left, top: screen.top + screen.height + 8, transform: 'translateX(-50%)' }
    : { left, top: screen.top - 8, transform: 'translate(-50%, -100%)' };
  const colour = hex(fill) ?? '#d9d9d9';
  return <div className="floating-bar selection-bar" role="toolbar" aria-label="Selection actions" data-selection-bar
    style={position} onPointerDown={event => event.stopPropagation()} onDoubleClick={event => event.stopPropagation()}>
    <label className={`selection-fill${fill.trim() ? '' : ' is-empty'}`} title="Fill colour">
      <span className="selection-fill-swatch" style={fill.trim() ? { background: fill } : undefined} aria-hidden="true" />
      <input type="color" aria-label="Fill colour" value={colour} onChange={event => onFill(event.target.value)} />
    </label>
    <span className="floating-bar-divider" aria-hidden="true" />
    <div className="selection-align" role="group" aria-label="Align">
      {ALIGN.map(({ edge, label, icon }) => <button key={edge} type="button" className="selection-action" aria-label={`Align ${label}`} title={withShortcut(`Align ${label}`, ({ left: 'alignLeft', centre: 'alignCentre', right: 'alignRight', top: 'alignTop', middle: 'alignMiddle', bottom: 'alignBottom' } as const)[edge])}
        onClick={() => onAlign(edge)}>
        <svg viewBox="0 0 16 16" aria-hidden="true"><path d={icon} /></svg>
      </button>)}
    </div>
    {count >= 3 && <div className="selection-align" role="group" aria-label="Distribute">
      <button type="button" className="selection-action" aria-label="Distribute horizontally" title={withShortcut('Distribute horizontally', 'distributeHorizontal')} onClick={() => onDistribute('horizontal')}>
        <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M2 2v12M14 2v12M6 5v6M10 5v6M2 8h12" /></svg>
      </button>
      <button type="button" className="selection-action" aria-label="Distribute vertically" title={withShortcut('Distribute vertically', 'distributeVertical')} onClick={() => onDistribute('vertical')}>
        <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M2 2h12M2 14h12M5 6h6M5 10h6M8 2v12" /></svg>
      </button>
    </div>}
    {scope && <><span className="floating-bar-divider" aria-hidden="true" />
      <EditScopeChoice scope={scope} choose={onScope} /></>}
    <span className="floating-bar-divider" aria-hidden="true" />
    <button type="button" className="selection-action labeled" title={withShortcut('Request', 'askAI')} onClick={onRequest}>Request</button>
    <button type="button" className="selection-action" aria-label="Link to…" title="Link to…" disabled={!hyperlink} onClick={onLinkTo}><Icon name="link" /></button>
    <button type="button" className="selection-action" aria-label={linked ? 'Unlink layer' : 'Make linked'} title={withShortcut(linked ? 'Unlink' : 'Make linked', linked ? 'unlink' : 'link')}
      disabled={!linked && !linkable} onClick={linked ? onUnlink : onLink}><Icon name="linked-copies" /></button>
    <button type="button" className="selection-action" aria-label="Duplicate" title={withShortcut('Duplicate', 'duplicate')} onClick={onDuplicate}><Icon name="copy" /></button>
    <button type="button" className="selection-action" aria-label="Delete" title={withShortcut('Delete', 'delete')} onClick={onDelete}><Icon name="trash" /></button>
  </div>;
}
