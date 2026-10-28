import { createContext, useCallback, useContext, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type ReactNode, type ReactElement } from 'react';
import { Icon } from './icons';

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
  details?: { summary: string; body: string };
  showDismiss?: boolean;
  /** A single affordance on a notice that closes itself, such as Undo after a delete. */
  undo?: ProblemAction;
};
type Entry = NoticeProps & { dismiss: () => void };
export function noticePriority(notice: NoticeProps): number {
  return notice.variant === 'error' ? 4 : notice.variant === 'warning' ? 3 : notice.priority === 'result' ? 2 : 1;
}
export function firstNotice<T extends NoticeProps>(entries: T[]): T | undefined {
  return entries.reduce<T | undefined>((best, entry) => !best || noticePriority(entry) > noticePriority(best) ? entry : best, undefined);
}
type NoticeKind = Pick<NoticeProps, 'variant' | 'action' | 'secondaryAction'>;
export function isToast(notice?: NoticeKind): boolean {
  return !!notice && (notice.variant === 'info' || notice.variant === 'success' || !notice.variant) && !notice.action && !notice.secondaryAction;
}
type NoticeStack = { rows: ReactElement[]; toast: boolean; focus?: NoticeProps['autoFocus']; focusKey?: string };
const NoticeDisplay = createContext<NoticeStack | null>(null);
export function NoticeHost() {
  const stack = useContext(NoticeDisplay);
  const host = useRef<HTMLDivElement>(null);
  const focus = stack?.focus;
  const focusKey = stack?.focusKey;
  useLayoutEffect(() => {
    if (focus) host.current?.querySelector<HTMLButtonElement>(focus === 'primary' ? '.primary' : '.notice-dismiss')?.focus({ preventScroll: true });
  }, [focus, focusKey]);
  // A toast closes itself; anything that waits for a choice stays in the middle of the canvas.
  return <div className={`notice-host${stack?.toast ? ' toast' : ''}`} ref={host}>{stack?.rows}</div>;
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
  const list = [...entries.values()];
  const active = firstNotice(list);
  // The same error may be reported by both a page and its editor. Dismiss it once.
  function dismissEntry(target: Entry) {
    for (const entry of entries.values()) if (entry.message === target.message && entry.title === target.title && entry.variant === target.variant) entry.dismiss();
  }
  // Toasts stack, newest last, up to three; a notice with a choice keeps the canvas to itself.
  const shown = !active ? [] : isToast(active) ? [active, ...list.filter(entry => entry !== active && isToast(entry)).slice(-2)] : [active];
  const rows = shown.map(entry => {
    const key = entry.dismissKey ?? `${entry.title ?? ''}:${entry.message}`;
    return isToast(entry)
      ? <ToastRow key={key} {...entry} dismiss={() => dismissEntry(entry)} />
      : <DismissibleNoticeRow key={key} {...entry} dismiss={() => dismissEntry(entry)} />;
  });
  const stack: NoticeStack = { rows, toast: isToast(active), focus: active?.autoFocus, focusKey: active?.dismissKey };
  return <Notices.Provider value={context}><NoticeDisplay.Provider value={stack}>{children}</NoticeDisplay.Provider></Notices.Provider>;
}

const ERROR_NOTICE_LIFETIME = 10_000;

/** Errors clear themselves after a while, but pointing at or focusing one keeps it open. */
function DismissibleNoticeRow(props: NoticeProps) {
  const [held, setHeld] = useState(false);
  const close = useRef(props.dismiss);
  close.current = props.dismiss;
  useEffect(() => {
    if (props.variant !== 'error' || held || !close.current) return;
    const timer = setTimeout(() => close.current?.(), ERROR_NOTICE_LIFETIME);
    return () => clearTimeout(timer);
  }, [props.variant, held]);
  return <NoticeRow {...props} onPointerEnter={() => setHeld(true)} onPointerLeave={() => setHeld(false)}
    onMouseEnter={() => setHeld(true)} onMouseLeave={() => setHeld(false)}
    onFocus={() => setHeld(true)} onBlur={() => setHeld(false)} />;
}

/// A toast closes itself, so it carries no dismiss button; a notice that waits for the user keeps one.
export function showsDismissButton(notice: NoticeKind): boolean {
  return !isToast(notice);
}
type HoverHold = { onPointerEnter?: () => void; onPointerLeave?: () => void; onMouseEnter?: () => void; onMouseLeave?: () => void; onFocus?: () => void; onBlur?: () => void };
export function NoticeRow({ variant = 'info', title, message, action, secondaryAction, dismiss, className = '', details, showDismiss = true, onPointerEnter, onPointerLeave, onMouseEnter, onMouseLeave, onFocus, onBlur }: NoticeProps & HoverHold) {
  const choose = (run: () => void, label: string) => {
    run();
    // The settings this notice points to replace it at once when opened from the notice.
    if (label === 'AI settings') dismiss?.();
  };
  return <div className={`notice floating-bar notice-${variant} ${className}`.trim()} role={variant === 'error' || variant === 'warning' ? 'alert' : 'status'}
    onPointerDown={event => event.stopPropagation()} onPointerEnter={onPointerEnter} onPointerLeave={onPointerLeave}
    onMouseEnter={onMouseEnter} onMouseLeave={onMouseLeave} onFocus={onFocus} onBlur={onBlur}>
    <div className="notice-copy" title={[title, message].filter(Boolean).join(' · ')}>{title && <strong>{title}</strong>}{message && <span>{message}</span>}</div>
    <div className="notice-actions">
      {action && <button type="button" className="primary" disabled={action.disabled} onClick={() => choose(action.onClick, action.label)}>{action.label}</button>}
      {secondaryAction && <button type="button" disabled={secondaryAction.disabled} onClick={() => choose(secondaryAction.onClick, secondaryAction.label)}>{secondaryAction.label}</button>}
      {showDismiss && showsDismissButton({ variant, action, secondaryAction }) && <button type="button" className="close-button notice-dismiss icon-button sm" aria-label="Dismiss notice" title="Dismiss" onClick={dismiss}><Icon name="close" /></button>}
    </div>
    {details && <details className="notice-details"><summary>{details.summary}</summary><pre>{details.body}</pre></details>}
  </div>;
}

const TOAST_LIFETIME = 4000;
const TOAST_FADE = 200;

/** A toast disappears on its own; pointing at it, or tabbing to its Undo, holds it open. */
export function ToastRow({ variant = 'info', title, message, undo, dismiss, className = '' }: NoticeProps) {
  const [held, setHeld] = useState(false);
  const [leaving, setLeaving] = useState(false);
  const close = useRef(dismiss); close.current = dismiss;
  const deadline = useRef(Date.now() + TOAST_LIFETIME);
  useEffect(() => {
    if (held || leaving) return;
    const timer = setTimeout(() => setLeaving(true), Math.max(0, deadline.current - Date.now()));
    return () => clearTimeout(timer);
  }, [held, leaving]);
  useEffect(() => {
    if (!leaving) return;
    const timer = setTimeout(() => close.current?.(), TOAST_FADE);
    return () => clearTimeout(timer);
  }, [leaving]);
  return <div className={`notice floating-bar notice-${variant} toast${leaving ? ' leaving' : ''}${className ? ` ${className}` : ''}`}
    role={variant === 'error' || variant === 'warning' ? 'alert' : 'status'}
    onPointerEnter={() => setHeld(true)} onPointerLeave={() => setHeld(false)}
    onFocus={() => setHeld(true)} onBlur={() => setHeld(false)}>
    <div className="notice-copy" title={[title, message].filter(Boolean).join(' · ')}>{title && <strong>{title}</strong>}{message && <span>{message}</span>}</div>
    {undo && <div className="notice-actions"><button type="button" className="primary" onClick={() => { close.current?.(); undo.onClick(); }}>{undo.label}</button></div>}
  </div>;
}

export function Notice(props: NoticeProps) {
  const context = useContext(Notices);
  const id = useId();
  const [dismissed, setDismissed] = useState<string | null>(null);
  const { variant = 'info', title, message, action, secondaryAction, undo, dismiss, priority, dismissKey, className, autoFocus, details, showDismiss } = props;
  const identity = dismissKey ?? `${title ?? ''}:${message}`;
  const visible = !!(message || title) && dismissed !== identity;
  const close = useCallback(() => { setDismissed(identity); dismiss?.(); }, [identity, dismiss]);
  useLayoutEffect(() => {
    context?.update(id, visible ? { variant, title, message, action, secondaryAction, undo, priority, className, autoFocus, dismissKey, details, showDismiss, dismiss: close } : null);
  }, [context, id, visible, variant, title, message, action, secondaryAction, undo, priority, className, autoFocus, dismissKey, details, showDismiss, close]);
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
