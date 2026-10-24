import { forwardRef, type ComponentPropsWithoutRef, type ReactNode } from 'react';

// One container for every floating editor bar. The class owns the radius, border,
// surface, shadow, padding and control height; bars only decide where they sit.
export const FloatingBar = forwardRef<HTMLDivElement, ComponentPropsWithoutRef<'div'>>(function FloatingBar({ className = '', children, ...rest }, ref) {
  return <div ref={ref} {...rest} className={`floating-bar${className ? ` ${className}` : ''}`}>{children}</div>;
});

export function FloatingBarDivider({ className = '' }: { className?: string }) {
  return <span className={`floating-bar-divider${className ? ` ${className}` : ''}`} aria-hidden="true" />;
}

export function FloatingBarLabel({ children, className = '' }: { children: ReactNode; className?: string }) {
  return <span className={`floating-bar-label${className ? ` ${className}` : ''}`}>{children}</span>;
}
