import type { ReactNode } from 'react';
import { Icon } from './icons';

// One disclosure: chevron from the icon set, the same row height and the same full width.
export function Disclosure({ summary, children, className = '', open, onToggle, ariaLabel, id }: {
  summary: ReactNode; children: ReactNode; className?: string; open?: boolean; onToggle?: (open: boolean) => void; ariaLabel?: string; id?: string;
}) {
  return <details className={`disclosure${className ? ` ${className}` : ''}`} open={open} onToggle={onToggle ? event => onToggle(event.currentTarget.open) : undefined} aria-label={ariaLabel} id={id}>
    <summary><Icon name="chevron-right" size={14} className="disclosure-chevron" /><span className="disclosure-summary">{summary}</span></summary>
    <div className="disclosure-body">{children}</div>
  </details>;
}
