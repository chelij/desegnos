import type { ReactNode } from 'react';

export type StatusTone = 'neutral' | 'success' | 'warning' | 'danger' | 'info';

// One badge for every small state readout: connection, capability and check counts.
export function StatusChip({ tone = 'neutral', dot = false, onClick, title, children, className = '', id, role, ariaLabel }: {
  tone?: StatusTone; dot?: boolean; onClick?: () => void; title?: string; children: ReactNode; className?: string; id?: string; role?: string; ariaLabel?: string;
}) {
  const content = <>{dot && <span className="status-chip-dot" aria-hidden="true" />}<span>{children}</span></>;
  const classes = `status-chip status-chip-${tone}${className ? ` ${className}` : ''}`;
  return onClick
    ? <button type="button" className={classes} id={id} role={role} title={title} aria-label={ariaLabel} onClick={onClick}>{content}</button>
    : <span className={classes} id={id} role={role} title={title}>{content}</span>;
}
