import { createContext, useCallback, useContext, useId, useLayoutEffect, useMemo, useRef, useState, type ReactNode, type ReactElement } from 'react';

export type ProblemAction = { label: string; onClick: () => void; disabled?: boolean };
export type NoticeProps = {
  variant?: 'success' | 'info' | 'warning' | 'error';
  title?: string;
  message: string;
  action?: ProblemAction;
  secondaryAction?: ProblemAction;
  dismiss?: () => void;
  priority?: 'result';
  dismissKey?: string;
  className?: string;
  autoFocus?: 'primary' | 'dismiss';
};
type Entry = NoticeProps & { dismiss: () => void };
export function noticePriority(notice: NoticeProps): number {
  return notice.variant === 'error' ? 4 : notice.variant === 'warning' ? 3 : notice.priority === 'result' ? 2 : 1;
}
export function firstNotice<T extends NoticeProps>(entries: T[]): T | undefined {
  return entries.reduce<T | undefined>((best, entry) => !best || noticePriority(entry) > noticePriority(best) ? entry : best, undefined);
}
const NoticeDisplay = createContext<ReactElement<NoticeProps> | null>(null);
export function NoticeHost() {
  const notice = useContext(NoticeDisplay);
  const host = useRef<HTMLDivElement>(null);
  const focus = notice?.props.autoFocus;
  const focusKey = notice?.props.dismissKey;
  useLayoutEffect(() => {
    if (focus) host.current?.querySelector<HTMLButtonElement>(focus === 'primary' ? '.primary' : '.notice-dismiss')?.focus({ preventScroll: true });
  }, [focus, focusKey]);
  return <div className="notice-host" ref={host}>{notice}</div>;
}
const Notices = createContext<{ update: (id: string, entry: Entry | null) => void } | null>(null);

// Mounted sources stay queued in insertion order, including while a higher priority source is visible.
export function NoticeProvider({ children }: { children: ReactNode }) {
  const [entries, setEntries] = useState<Map<string, Entry>>(new Map());
  const update = useCallback((id: string, entry: Entry | null) => setEntries(previous => {
    if (!entry && !previous.has(id)) return previous;
    const next = new Map(previous);
    if (entry) next.set(id, entry); else next.delete(id);
    return next;
  }), []);
  const context = useMemo(() => ({ update }), [update]);
  const active = firstNotice([...entries.values()]);
  return <Notices.Provider value={context}><NoticeDisplay.Provider value={active ? <NoticeRow {...active} dismiss={() => {
      // The same error may be reported by both a page and its editor. Dismiss it once.
      for (const entry of entries.values()) if (entry.message === active.message && entry.title === active.title && entry.variant === active.variant) entry.dismiss();
    }} /> : null}>{children}</NoticeDisplay.Provider></Notices.Provider>;
}

export function NoticeRow({ variant = 'info', title, message, action, secondaryAction, dismiss, className = '' }: NoticeProps) {
  return <div className={`notice notice-${variant} ${className}`.trim()} role={variant === 'error' || variant === 'warning' ? 'alert' : 'status'} onPointerDown={event => event.stopPropagation()}>
    <div className="notice-copy" title={[title, message].filter(Boolean).join(' · ')}>{title && <strong>{title}</strong>}{message && <span>{message}</span>}</div>
    <div className="notice-actions">
      {action && <button type="button" className="primary" disabled={action.disabled} onClick={action.onClick}>{action.label}</button>}
      {secondaryAction && <button type="button" disabled={secondaryAction.disabled} onClick={secondaryAction.onClick}>{secondaryAction.label}</button>}
      <button type="button" className="notice-dismiss" aria-label="Dismiss notice" title="Dismiss" onClick={dismiss}>×</button>
    </div>
  </div>;
}

export function Notice(props: NoticeProps) {
  const context = useContext(Notices);
  const id = useId();
  const [dismissed, setDismissed] = useState<string | null>(null);
  const { variant = 'info', title, message, action, secondaryAction, dismiss, priority, dismissKey, className, autoFocus } = props;
  const identity = dismissKey ?? `${title ?? ''}:${message}`;
  const visible = !!(message || title) && dismissed !== identity;
  const close = useCallback(() => { setDismissed(identity); dismiss?.(); }, [identity, dismiss]);
  useLayoutEffect(() => {
    context?.update(id, visible ? { variant, title, message, action, secondaryAction, priority, className, autoFocus, dismissKey, dismiss: close } : null);
  }, [context, id, visible, variant, title, message, action, secondaryAction, priority, className, autoFocus, dismissKey, close]);
  useLayoutEffect(() => () => context?.update(id, null), [context, id]);
  // Standalone rendering also supports previews and server-rendered component tests.
  return !context && visible ? <NoticeRow {...props} dismiss={close} /> : null;
}

export type ProblemBannerProps = NoticeProps;
// Callers translate diagnostics before presentation; raw errors stay in state.
export function ProblemBanner({ variant = 'error', ...props }: ProblemBannerProps) {
  return <Notice {...props} variant={variant} />;
}

export function needsAiSettings(error: string): boolean {
  return /connect|network|fetch|timed? ?out|auth|credential|API.?key|sign.?in|signed out|quota|usage limit|rate limit|too many requests|budget|spending|billing|credit|payment|model.*(?:unavailable|not available|not supported)|vault|keyring|secure storage/i.test(error);
}
