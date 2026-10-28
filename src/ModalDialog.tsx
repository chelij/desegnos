import { useLayoutEffect, useRef, type ReactNode } from 'react';

/**
 * A blocking question the user must answer before anything else, such as losing unsaved
 * work. Editing UI never uses this; it stays in the side panel or inline.
 */
export function ModalDialog({ title, close, children }: { title: string; close: () => void; children: ReactNode }) {
  const ref = useRef<HTMLDialogElement>(null);
  useLayoutEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.showModal();
    dialog.querySelector<HTMLElement>('.primary, button')?.focus({ preventScroll: true });
    return () => { dialog.close(); if (opener?.isConnected) opener.focus({ preventScroll: true }); };
  }, []);
  // Keys stay inside the dialog so canvas shortcuts such as Delete never act behind it.
  return <dialog ref={ref} className="modal-dialog" aria-label={title}
    onCancel={event => { event.preventDefault(); close(); }}
    onKeyDown={event => { event.stopPropagation(); if (event.key === 'Escape') { event.preventDefault(); close(); } }}>
    <h2 className="modal-title">{title}</h2>
    {children}
  </dialog>;
}
