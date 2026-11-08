/** Whether a key event's target is somewhere the user is typing. */
export function isTextField(target: EventTarget | null) {
  return target instanceof HTMLElement && !!target.closest('input:not([type=checkbox], [type=radio], [type=button], [type=submit], [type=reset], [type=range], [type=color], [type=file]), textarea, select, [contenteditable]:not([contenteditable="false"]), .cm-editor, [role="textbox"]');
}
