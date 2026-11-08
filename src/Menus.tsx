import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode } from 'react';
import { Icon } from './icons';

export type MenuItem = {
  id: string;
  label: string;
  action?: () => void;
  disabled?: boolean;
  checked?: boolean;
  role?: 'menuitemradio';
  shortcut?: string;
  children?: MenuItem[];
  separator?: boolean;
};

export function MenuButton({ label, items, ariaLabel, title, className = '', showChevron = true, align = 'start', reserveSpace = false, openSignal }: {
  label: ReactNode; items: MenuItem[]; ariaLabel?: string; title?: string; className?: string; showChevron?: boolean; align?: 'start' | 'end'; reserveSpace?: boolean; openSignal?: number;
}) {
  const [open, setOpen] = useState(false);
  const [menuHeight, setMenuHeight] = useState(0);
  const [startAtEnd, setStartAtEnd] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const container = useRef<HTMLDivElement>(null);
  const id = useId();
  function close(restore = true) {
    setOpen(false);
    if (restore) trigger.current?.focus({ preventScroll: true });
  }
  useEffect(() => {
    if (!openSignal) return;
    setStartAtEnd(false);
    setOpen(true);
  }, [openSignal]);
  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent | FocusEvent) => {
      if (event.target instanceof Node && !container.current?.contains(event.target)) setOpen(false);
    };
    const resize = () => setOpen(false);
    document.addEventListener('pointerdown', outside, true);
    document.addEventListener('focusin', outside);
    window.addEventListener('resize', resize);
    return () => {
      document.removeEventListener('pointerdown', outside, true);
      document.removeEventListener('focusin', outside);
      window.removeEventListener('resize', resize);
    };
  }, [open]);
  return <div className={`menu-anchor ${className}`} ref={container} style={reserveSpace && open ? { marginBottom: menuHeight + 4 } : undefined}>
    <button ref={trigger} type="button" aria-label={ariaLabel} title={title} aria-haspopup="menu" aria-expanded={open} aria-controls={open ? id : undefined}
      onClick={() => { setStartAtEnd(false); setOpen(value => !value); }}
      onKeyDown={event => {
        if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
          event.preventDefault(); setStartAtEnd(event.key === 'ArrowUp'); setOpen(true);
        } else if (event.key === 'Escape' && open) { event.preventDefault(); close(); }
      }}>{label}{showChevron && <Icon name="chevron-down" size={12} className="menu-chevron" />}</button>
    {open && <MenuLevel id={id} label={ariaLabel ?? (typeof label === 'string' ? label : 'Menu')} items={items} anchor={trigger.current} align={align} measured={reserveSpace ? setMenuHeight : undefined} last={startAtEnd} rootClose={close} />}
  </div>;
}

function MenuLevel({ id, label, items, anchor, last = false, nested = false, back, rootClose, align = 'start', measured }: {
  id: string; label: string; items: MenuItem[]; anchor: Pick<HTMLElement, 'getBoundingClientRect'> | null; last?: boolean; nested?: boolean; align?: 'start' | 'end'; measured?: (height: number) => void;
  back?: () => void; rootClose: (restore?: boolean) => void;
}) {
  const panel = useRef<HTMLUListElement>(null);
  const buttons = useRef(new Map<number, HTMLButtonElement>());
  const [submenu, setSubmenu] = useState<number | null>(null);
  const [position, setPosition] = useState({ left: 0, top: 0, ready: false });
  const search = useRef({ text: '', at: 0 });
  const enabled = items.flatMap((item, index) => !item.separator && !item.disabled ? [index] : []);
  useLayoutEffect(() => {
    if (!anchor || !panel.current) return;
    const menu = panel.current;
    menu.showPopover();
    const rect = anchor.getBoundingClientRect();
    const bounds = panel.current.getBoundingClientRect();
    measured?.(bounds.height);
    const margin = 8;
    const preferredLeft = nested ? rect.right + 2 : align === 'end' ? rect.right - bounds.width : rect.left;
    const left = nested && preferredLeft + bounds.width > window.innerWidth - margin ? rect.left - bounds.width - 2 : preferredLeft;
    const preferredTop = nested ? rect.top : rect.bottom + 4;
    setPosition({
      left: Math.max(margin, Math.min(left, window.innerWidth - bounds.width - margin)),
      top: Math.max(margin, Math.min(preferredTop, window.innerHeight - bounds.height - margin)), ready: true,
    });
    return () => { if (menu.matches(':popover-open')) menu.hidePopover(); };
  }, []);
  useLayoutEffect(() => {
    if (position.ready) buttons.current.get(last ? enabled.at(-1)! : enabled[0])?.focus({ preventScroll: true });
  }, [position.ready]);
  function returnTo(index: number) { setSubmenu(null); buttons.current.get(index)?.focus({ preventScroll: true }); }
  function activate(index: number) {
    const item = items[index];
    if (!item || item.disabled || item.separator) return;
    if (item.children?.length) { setSubmenu(index); return; }
    rootClose();
    item.action?.();
  }
  function keyboard(event: KeyboardEvent<HTMLUListElement>) {
    if (!(event.target instanceof HTMLElement) || event.target.closest('[role="menu"]') !== panel.current) return;
    const focused = [...buttons.current].find(([, button]) => button === document.activeElement)?.[0] ?? enabled[0];
    const index = enabled.indexOf(focused);
    if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
      event.preventDefault();
      const next = event.key === 'Home' ? enabled[0] : event.key === 'End' ? enabled.at(-1)! : enabled[(index + (event.key === 'ArrowDown' ? 1 : -1) + enabled.length) % enabled.length];
      setSubmenu(null); buttons.current.get(next)?.focus({ preventScroll: true }); buttons.current.get(next)?.scrollIntoView({ block: 'nearest' });
    } else if (event.key === 'ArrowRight' && items[focused]?.children?.length) { event.preventDefault(); activate(focused); }
    else if ((event.key === 'ArrowLeft' && nested) || event.key === 'Escape') {
      event.preventDefault(); event.stopPropagation(); if (nested) back?.(); else rootClose();
    } else if (event.key === 'Tab') {
      rootClose();
    } else if (event.key.length === 1 && !event.ctrlKey && !event.metaKey && !event.altKey && event.key !== ' ') {
      const now = Date.now();
      search.current = { text: (now - search.current.at < 700 ? search.current.text : '') + event.key.toLocaleLowerCase(), at: now };
      const ordered = [...enabled.slice(index + 1), ...enabled.slice(0, index + 1)];
      const next = ordered.find(candidate => items[candidate].label.toLocaleLowerCase().startsWith(search.current.text));
      if (next !== undefined) { event.preventDefault(); setSubmenu(null); buttons.current.get(next)?.focus({ preventScroll: true }); }
    }
  }
  return <ul ref={panel} id={id} popover="manual" role="menu" aria-label={label} className="editor-menu" onKeyDown={keyboard}
    style={{ left: position.left, top: position.top, visibility: position.ready ? 'visible' : 'hidden' }}>
    {items.map((item, index) => item.separator ? <li key={item.id} role="separator" className="menu-separator" /> : <li key={item.id} role="none">
      <button type="button" ref={element => { if (element) buttons.current.set(index, element); else buttons.current.delete(index); }}
        role={item.role ?? (item.checked === undefined ? 'menuitem' : 'menuitemcheckbox')} aria-label={item.label} aria-checked={item.checked} disabled={item.disabled} tabIndex={-1}
        aria-haspopup={item.children?.length ? 'menu' : undefined} aria-expanded={item.children?.length ? submenu === index : undefined}
        aria-controls={submenu === index ? `${id}-${item.id}` : undefined} onClick={() => activate(index)}>
        <span className="menu-check" aria-hidden="true">{item.checked && <Icon name="check" size={14} />}</span><span className="menu-label">{item.label}</span>
        {item.shortcut && <span className="menu-shortcut">{item.shortcut}</span>}{item.children?.length && <Icon name="chevron-right" size={14} className="menu-arrow" />}
      </button>
      {submenu === index && item.children && <MenuLevel id={`${id}-${item.id}`} label={item.label} items={item.children} anchor={buttons.current.get(index) ?? null} nested back={() => returnTo(index)} rootClose={rootClose} />}
    </li>)}
  </ul>;
}
