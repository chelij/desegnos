import { useEffect, useState } from 'react';

const KEY = 'desegnos.first-run-tips';
const TIPS = [
  { title: 'Place a box', body: 'Click Add request, then drag where something should go on the page.' },
  { title: 'Describe it', body: 'Type what belongs in the box: a heading, a photo grid, a contact form…' },
  { title: 'Generate', body: 'Click Generate. The AI builds real HTML and CSS in the places you chose.' },
];

function seen() { try { return localStorage.getItem(KEY) === 'done'; } catch { return false; } }

export function FirstRunTips({ hasBox, hasText, generationStarted, hidden }: { hasBox: boolean; hasText: boolean; generationStarted: boolean; hidden: boolean }) {
  const [step, setStep] = useState(() => seen() ? -1 : 0);
  useEffect(() => {
    if (generationStarted) {
      try { localStorage.setItem(KEY, 'done'); } catch {}
      setStep(-1);
    } else setStep(current => current < 0 ? current : Math.max(current, hasText ? 2 : hasBox ? 1 : 0));
  }, [hasBox, hasText, generationStarted]);
  if (step < 0 || generationStarted || hidden) return null;
  const finish = () => { try { localStorage.setItem(KEY, 'done'); } catch {} setStep(-1); };
  const tip = TIPS[step];
  return <aside className="first-run-tips" aria-label="Getting started">
    <div aria-live="polite" aria-atomic="true">
      <p className="tip-count">Tip {step + 1} of {TIPS.length}</p>
      <strong>{tip.title}</strong>
      <p>{tip.body}</p>
    </div>
    <div className="tip-actions">
      <button type="button" className="link-button" onClick={finish}>Skip tips</button>
      {step < TIPS.length - 1 ? <button type="button" onClick={() => setStep(step + 1)}>Next</button> : <button type="button" onClick={finish}>Got it</button>}
    </div>
  </aside>;
}
