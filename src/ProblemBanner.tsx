import { useState, type ReactNode } from 'react';

export type ProblemAction = { label: string; onClick: () => void; disabled?: boolean };
export type ProblemBannerProps = {
  message: string;
  action?: ProblemAction;
  instruction?: string;
  children?: ReactNode;
  dismiss?: () => void;
  className?: string;
};

// Callers translate diagnostics before presentation; raw errors stay in state.
export function ProblemBanner({ message, action, instruction, children, dismiss, className = '' }: ProblemBannerProps) {
  const [dismissed, setDismissed] = useState<string | null>(null);
  if (!message || dismissed === message) return null;
  const [sentence, ...rest] = message.split(/(?<=[.!?])\s+/);
  return <div className={`problem-banner ${className}`.trim()} role="alert" onPointerDown={event => event.stopPropagation()}>
    <div className="problem-copy"><p>{sentence}</p>{!action && (instruction || rest.length > 0) && <p>{instruction || rest.join(' ')}</p>}</div>
    <div className="problem-actions">
      {action && <button type="button" className="primary" disabled={action.disabled} onClick={action.onClick}>{action.label}</button>}
      {children}
      <button type="button" onClick={() => { setDismissed(message); dismiss?.(); }}>Dismiss</button>
    </div>
  </div>;
}

export function needsAiSettings(error: string): boolean {
  return /connect|network|fetch|timed? ?out|auth|credential|API.?key|sign.?in|signed out|quota|usage limit|rate limit|too many requests|budget|spending|billing|credit|payment|model.*(?:unavailable|not available|not supported)|vault|keyring|secure storage/i.test(error);
}
