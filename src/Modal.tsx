import { useLayoutEffect, useRef, type ReactNode } from 'react';
import './dialogs.css';

type ModalRecord = { dialog: HTMLDialogElement; opener: HTMLElement | null; parent: ModalRecord | null };
const openModals: ModalRecord[] = [];

export function Modal({ title, children, close, wide = false, headerContent, className = '' }: { title: string; children: ReactNode; close: () => void; wide?: boolean; headerContent?: ReactNode; className?: string }) {
  const ref = useRef<HTMLDialogElement>(null);
  useLayoutEffect(() => {
    const dialog = ref.current!;
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const record: ModalRecord = { dialog, opener, parent: [...openModals].reverse().find(item => !!opener && item.dialog.contains(opener)) ?? null };
    dialog.showModal();
    openModals.push(record);
    return () => {
      const index = openModals.indexOf(record);
      if (index >= 0) openModals.splice(index, 1);
      if (dialog.open) dialog.close();
      queueMicrotask(() => {
        const top = openModals.at(-1)?.dialog;
        const activeDialog = document.activeElement instanceof Element ? document.activeElement.closest('dialog[open]') : null;
        for (let previous: ModalRecord | null = record; previous; previous = previous.parent) {
          const target = previous.opener;
          if (!target?.isConnected || target.closest('dialog:not([open]), [inert]') || target.matches(':disabled')) continue;
          if (top?.open && !top.contains(target)) return;
          if (activeDialog && !activeDialog.contains(target)) return;
          target.focus({ preventScroll: true });
          return;
        }
      });
    };
  }, []);
  return <dialog ref={ref} className={`modal${wide ? ' wide' : ''} ${className}`} aria-label={title} onCancel={event => { event.preventDefault(); close(); }}>
    <header className="modal-header"><div className="modal-titlebar"><h2>{title}</h2><button type="button" className="modal-close" onClick={close} aria-label={`Close ${title}`} title="Close"><span aria-hidden="true">×</span></button></div>{headerContent}</header>
    {children}
  </dialog>;
}
