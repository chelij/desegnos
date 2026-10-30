import { useLayoutEffect, useRef, type ReactNode } from 'react';
import { Icon } from './icons';
import { NoticeHost, NoticeProvider } from './ProblemBanner';

/**
 * A blocking question or account settings popup. Canvas editing stays in panels or inline.
 */
export function ModalDialog({ title, close, children, className = '', showClose = false }: { title: string; close: () => void; children: ReactNode; className?: string; showClose?: boolean }) {
  const ref = useRef<HTMLDialogElement>(null);
  useLayoutEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.showModal();
    // A field to type in comes first, then the primary action; a selector list would take whichever is first in the DOM.
    (dialog.querySelector<HTMLElement>('input:not([type="hidden"]):not(:disabled), textarea:not(:disabled)')
      ?? dialog.querySelector<HTMLElement>('.primary') ?? dialog.querySelector<HTMLElement>('button'))?.focus({ preventScroll: true });
    return () => { dialog.close(); if (opener?.isConnected) opener.focus({ preventScroll: true }); };
  }, []);
  // Keys stay inside the dialog so canvas shortcuts such as Delete never act behind it.
  return <dialog ref={ref} className={`modal-dialog ${className}`.trim()} aria-label={title}
    onCancel={event => { event.preventDefault(); close(); }}
    onKeyDown={event => { event.stopPropagation(); if (event.key === 'Escape') { event.preventDefault(); close(); } }}>
    <header className="modal-header"><h2 className="modal-title">{title}</h2>{showClose && <button type="button" className="close-button icon-button sm" aria-label={`Close ${title}`} title="Close" onClick={close}><Icon name="close" /></button>}</header>
    <NoticeProvider><NoticeHost />{children}</NoticeProvider>
  </dialog>;
}
