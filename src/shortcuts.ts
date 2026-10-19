export type EditorShortcut = 'new' | 'open' | 'save' | 'saveAs' | 'undo' | 'redo' | 'zoomIn' | 'zoomOut' | 'fit' | 'actualSize' | 'doubleSize' | 'nextPage' | 'previousPage' | 'remove' | 'help';
export type ShortcutInput = { key: string; code?: string; ctrlKey: boolean; metaKey: boolean; altKey: boolean; shiftKey: boolean; defaultPrevented?: boolean; isComposing?: boolean };

export const SHORTCUT_HELP = [
  ['New project', 'Ctrl/Cmd+N'], ['Open project', 'Ctrl/Cmd+O'], ['Save', 'Ctrl/Cmd+S'], ['Save as', 'Ctrl/Cmd+Shift+S'],
  ['Undo', 'Ctrl/Cmd+Z'], ['Redo', 'Ctrl/Cmd+Shift+Z · Ctrl+Y'],
  ['Zoom in / out', 'Ctrl/Cmd++ / − · Ctrl+wheel'], ['Fit screen guide', 'Ctrl/Cmd+0'], ['Actual size', 'Ctrl/Cmd+1'], ['200% view', 'Ctrl/Cmd+2'],
  ['Next / previous page', 'Ctrl+Tab / Ctrl+Shift+Tab · Ctrl+Page Down / Page Up'],
  ['Select an element', 'Single-click'], ['Edit selected element', 'F2'], ['Move selected element', 'Arrow keys · Shift for 10 px'], ['Delete selected element', 'Delete / Backspace'],
  ['Move / resize element', 'Drag selection / its corners'], ['Element actions', 'Right-click'],
  ['Open a request', 'Double-click canvas or website element'],
  ['Pan canvas', 'Drag empty space · Space+drag · Middle drag'], ['Draw an area', 'Shift+drag'],
  ['New request in view', 'Enter on blank canvas · Shift+Enter'], ['Next element / parent', 'Enter / Alt+Enter on generated canvas'],
  ['Move / resize request', 'Arrow keys on handles · Alt for 1 px'], ['Remove selected request', 'Delete / Backspace outside text'],
  ['Cancel gesture / close dialog', 'Escape'], ['Keyboard shortcuts', 'F1 · ? outside text'],
] as const;

export function editorShortcut(event: ShortcutInput, editingText: boolean): EditorShortcut | null {
  if (event.defaultPrevented || event.isComposing || event.altKey) return null;
  // WebKitGTK reports Shift+Tab as Unidentified with the physical Tab code.
  const key = (event.key === 'Unidentified' && event.code === 'Tab' ? 'Tab' : event.key).toLowerCase();
  const command = event.ctrlKey || event.metaKey;
  if (command) {
    if (key === 's') return event.shiftKey ? 'saveAs' : 'save';
    if (!event.shiftKey && key === 'n') return 'new';
    if (!event.shiftKey && key === 'o') return 'open';
    if (key === '+' || key === '=') return 'zoomIn';
    if (key === '-' || key === '_') return 'zoomOut';
    if (!event.shiftKey && key === '0') return 'fit';
    if (!event.shiftKey && key === '1') return 'actualSize';
    if (!event.shiftKey && key === '2') return 'doubleSize';
    if (event.ctrlKey && key === 'tab') return event.shiftKey ? 'previousPage' : 'nextPage';
    if (event.ctrlKey && !event.shiftKey && key === 'pagedown') return 'nextPage';
    if (event.ctrlKey && !event.shiftKey && key === 'pageup') return 'previousPage';
    if (!editingText && key === 'z') return event.shiftKey ? 'redo' : 'undo';
    if (!editingText && !event.shiftKey && key === 'y') return 'redo';
    return null;
  }
  if (!editingText && (key === 'f1' || key === '?')) return 'help';
  if (!editingText && !event.shiftKey && (key === 'delete' || key === 'backspace')) return 'remove';
  return null;
}
