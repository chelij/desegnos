import { useState } from 'react';

const KEY = 'desegnos.first-run-tips';
const TIPS = [
  { title: 'Place a box', text: 'Choose Add request, or double-click the page, where something should go.' },
  { title: 'Describe it', text: 'Type what belongs there in plain words, like “a big photo of our bakery with opening hours”.' },
  { title: 'Press Generate', text: 'The AI builds your page. Undo or Try again any time; nothing is lost.' },
];

function seen() { try { return localStorage.getItem(KEY) === 'done'; } catch { return false; } }

export function FirstRunTips() {
  const [step, setStep] = useState(() => seen() ? -1 : 0);
  if (step < 0) return null;
  const finish = () => { try { localStorage.setItem(KEY, 'done'); } catch {} setStep(-1); };
  const tip = TIPS[step];
  return <aside className="first-run-tips" aria-label="Getting started">
    <p className="tip-count">Tip {step + 1} of {TIPS.length}</p>
    <strong>{tip.title}</strong>
    <p>{tip.text}</p>
    <div className="tip-actions">
      <button type="button" className="link-button" onClick={finish}>Skip tips</button>
      {step < TIPS.length - 1 ? <button type="button" className="primary" onClick={() => setStep(step + 1)}>Next</button> : <button type="button" className="primary" onClick={finish}>Got it</button>}
    </div>
  </aside>;
}
