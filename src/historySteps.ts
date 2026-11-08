import { invoke } from '@tauri-apps/api/core';
import { isTextField } from './textField';
import type { CommandName } from './generated/commands';

// Leaving a text field ends the native history step, so the next edit is a new step and
// the History menu reads as whole changes instead of one entry per keystroke.
export function installHistorySteps() {
  document.addEventListener('focusout', event => {
    if (isTextField(event.target)) void invoke('end_history_step' satisfies CommandName).catch(error => console.warn('Could not record editor activity', error));
  }, true);
}
