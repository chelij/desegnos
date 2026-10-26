import { invoke } from '@tauri-apps/api/core';

// Leaving a text field ends the native history step, so the next edit is a new step and
// the History menu reads as whole changes instead of one entry per keystroke.
function isTextSurface(element: Element | null) {
  if (!element) return false;
  if (element.matches('textarea, input, [contenteditable]')) return true;
  return element instanceof HTMLElement && element.isContentEditable;
}

export function installHistorySteps() {
  document.addEventListener('focusout', event => {
    if (isTextSurface(event.target as Element | null)) void invoke('end_history_step').catch(() => {});
  }, true);
}
