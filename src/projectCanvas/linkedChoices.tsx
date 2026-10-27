import { useEffect, useRef } from 'react';
import type { ScreenRect } from './surface';
import type { PasteMode } from '../pastePreference';

const place = (screen: ScreenRect) => ({
  left: Math.max(8, Math.min(screen.left, window.innerWidth - 300)),
  top: Math.max(8, screen.top),
});

/** The small inline choice at the paste point; Enter takes the highlighted option. */
export function PasteChoiceRow({ screen, value, useDefault, change, toggleDefault, confirm, cancel }: {
  screen: ScreenRect; value: PasteMode; useDefault: boolean;
  change: (mode: PasteMode) => void; toggleDefault: (value: boolean) => void;
  confirm: () => void; cancel: () => void;
}) {
  const row = useRef<HTMLDivElement>(null);
  useEffect(() => { row.current?.focus({ preventScroll: true }); }, []);
  return <div ref={row} className="canvas-choice paste-choice" role="group" aria-label="Paste linked or unlinked" tabIndex={-1}
    style={place(screen)}
    onPointerDown={event => event.stopPropagation()}
    onDoubleClick={event => event.stopPropagation()}
    onKeyDown={event => {
      if (event.key === 'Enter') { event.preventDefault(); confirm(); }
      else if (event.key === 'Escape') { event.preventDefault(); cancel(); }
      else if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') { event.preventDefault(); change(value === 'linked' ? 'unlinked' : 'linked'); }
    }}>
    <span className="canvas-choice-label">Paste</span>
    {(['linked', 'unlinked'] as PasteMode[]).map(mode => <button key={mode} type="button" className={value === mode ? 'canvas-choice-option active' : 'canvas-choice-option'}
      aria-pressed={value === mode} onClick={() => change(mode)}>{mode === 'linked' ? 'Linked' : 'Unlinked'}</button>)}
    <label className="canvas-choice-check"><input type="checkbox" checked={useDefault} onChange={event => toggleDefault(event.target.checked)} />Use as default</label>
    <button type="button" className="canvas-choice-option primary" onClick={confirm}>Paste</button>
  </div>;
}

export type LinkCopy = { id: string; label: string; boundary: string };

/** The inline choice of the copy whose content wins when linking, or why linking failed. */
export function LinkChoiceRow({ screen, copies, error, pick, cancel }: {
  screen: ScreenRect; copies: LinkCopy[]; error: string | null;
  pick: (id: string) => void; cancel: () => void;
}) {
  const row = useRef<HTMLDivElement>(null);
  useEffect(() => { row.current?.focus({ preventScroll: true }); }, []);
  return <div ref={row} className="canvas-choice link-choice" role="group" aria-label={error ? 'Cannot link these elements' : 'Choose the copy to keep'} tabIndex={-1}
    style={place(screen)}
    onPointerDown={event => event.stopPropagation()}
    onDoubleClick={event => event.stopPropagation()}
    onKeyDown={event => { if (event.key === 'Escape') { event.preventDefault(); cancel(); } }}>
    {error
      ? <><span className="canvas-choice-error" role="alert">{error}</span><button type="button" className="canvas-choice-option" onClick={cancel}>Close</button></>
      : <>
        <span className="canvas-choice-label">Link these copies</span>
        {copies.map(copy => <button key={copy.id} type="button" className="canvas-choice-copy" onClick={() => pick(copy.id)}>
          <span className="canvas-choice-copy-name">{copy.label}</span>
          <span className="canvas-choice-copy-boundary">{copy.boundary}</span>
        </button>)}
        <button type="button" className="canvas-choice-option" onClick={cancel}>Cancel</button>
      </>}
  </div>;
}

/** The inline choice a placeholder offers: include the layer in this size, or dismiss it. */
export function PlaceholderChoiceRow({ screen, include, dismiss, close }: {
  screen: ScreenRect; include: () => void; dismiss: () => void; close: () => void;
}) {
  const row = useRef<HTMLDivElement>(null);
  useEffect(() => { row.current?.focus({ preventScroll: true }); }, []);
  return <div ref={row} className="canvas-choice placeholder-choice" role="group" aria-label="Include this layer in this size" tabIndex={-1}
    style={place(screen)}
    onPointerDown={event => event.stopPropagation()}
    onDoubleClick={event => event.stopPropagation()}
    onKeyDown={event => {
      if (event.key === 'Enter') { event.preventDefault(); include(); }
      else if (event.key === 'Escape') { event.preventDefault(); close(); }
    }}>
    <span className="canvas-choice-label">This layer is in another size</span>
    <button type="button" className="canvas-choice-option primary" onClick={include}>Include</button>
    <button type="button" className="canvas-choice-option" onClick={dismiss}>Dismiss</button>
    <button type="button" className="canvas-choice-option" onClick={close}>Close</button>
  </div>;
}
