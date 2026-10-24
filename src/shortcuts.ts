export type EditorShortcut = 'new' | 'open' | 'save' | 'saveAs' | 'undo' | 'redo' | 'selectSimilar' | 'zoomIn' | 'zoomOut' | 'fit' | 'actualSize' | 'doubleSize' | 'nextPage' | 'previousPage' | 'closePageTab' | 'remove' | 'help' | 'selectTool' | 'requestTool' | 'rectangleTool' | 'circleTool' | 'freehandTool' | 'panTool' | 'askAI' | 'undoElement' | 'cutLayer' | 'copyLayer' | 'pasteLayer' | 'layerForward' | 'layerBackward' | 'layerFront' | 'layerBack';
export type ShortcutInput = { key: string; code?: string; ctrlKey: boolean; metaKey: boolean; altKey: boolean; shiftKey: boolean; defaultPrevented?: boolean; isComposing?: boolean };

export const SHORTCUT_HELP = [
  ['New project', 'Ctrl/Cmd+N'], ['Open project', 'Ctrl/Cmd+O'], ['Save', 'Ctrl/Cmd+S'], ['Save as', 'Ctrl/Cmd+Shift+S'],
  ['Copy / cut / paste selected layer', 'Ctrl/Cmd+C / Ctrl/Cmd+X / Ctrl/Cmd+V outside text'], ['Bring forward / send backward', 'Ctrl/Cmd+] / Ctrl/Cmd+['], ['Bring to front / send to back', 'Ctrl/Cmd+Shift+] / Ctrl/Cmd+Shift+['],
  ['Undo', 'Ctrl/Cmd+Z'], ['Undo changes to the selected element only', 'Alt+Z'], ['Redo', 'Ctrl/Cmd+Shift+Z · Ctrl+Y'],
  ['Zoom in / out', 'Ctrl/Cmd++ / Ctrl/Cmd+− · Ctrl+wheel'], ['Fit screen guide', 'Ctrl/Cmd+0'], ['Actual size', 'Ctrl/Cmd+1'], ['200% view', 'Ctrl/Cmd+2'],
  ['Next / previous page', 'Ctrl+Tab / Ctrl+Shift+Tab · Ctrl+Page Down / Page Up'], ['Close page tab', 'Ctrl/Cmd+W'],
  ['Select an element', 'Single-click · Shift/Ctrl/Cmd-click to add'], ['Select similar', 'Ctrl/Cmd+Shift+A'], ['Marquee select', 'Alt-drag on empty canvas · add Shift to keep'], ['Edit selected element', 'F2'], ['Move selected element', 'Arrow keys · Shift for 10 px'], ['Delete selected element', 'Delete / Backspace'],
  ['Move / resize element', 'Drag selection / its corners'], ['Element actions', 'Right-click'],
  ['Edit text in place', 'Double-click website text · Enter to save · Escape to cancel'],
  ['Select / Add request / Pan', 'V / R / H'], ['Ask AI about selection', 'Ctrl/Cmd+K'],
  ['Rectangle / Circle / Freehand request', 'Shift+R / C / L'], ['Perfect circle', 'Shift while drawing a circle'],
  ['Open a request', 'Double-click empty canvas · Add request tool'],
  ['Pan canvas', 'Drag empty space · Space+drag · Middle drag'], ['Draw an area', 'Shift+drag'],
  ['New request in view', 'Enter on blank canvas · Shift+Enter'], ['Next element / parent', 'Enter / Alt+Enter on generated canvas'],
  ['Move / resize request', 'Arrow keys on handles · Alt for 1 px'], ['Remove selected request', 'Delete / Backspace outside text'],
  ['Cancel gesture / close dialog', 'Escape'], ['Keyboard shortcuts', 'F1 · ? outside text'],
] as const;

// Compact hints for icon buttons. Values mirror SHORTCUT_HELP so a tooltip names the
// same key the editor listens for; keep them in step when a binding changes.
export const TOOL_SHORTCUTS = {
  newProject: 'Ctrl/Cmd+N', openProject: 'Ctrl/Cmd+O', save: 'Ctrl/Cmd+S', saveAs: 'Ctrl/Cmd+Shift+S',
  copy: 'Ctrl/Cmd+C', cut: 'Ctrl/Cmd+X', paste: 'Ctrl/Cmd+V',
  forward: 'Ctrl/Cmd+]', backward: 'Ctrl/Cmd+[', front: 'Ctrl/Cmd+Shift+]', back: 'Ctrl/Cmd+Shift+[',
  undo: 'Ctrl/Cmd+Z', undoElement: 'Alt+Z', redo: 'Ctrl/Cmd+Shift+Z',
  zoomIn: 'Ctrl/Cmd++', zoomOut: 'Ctrl/Cmd+−', fit: 'Ctrl/Cmd+0', actualSize: 'Ctrl/Cmd+1',
  nextPage: 'Ctrl+Tab', previousPage: 'Ctrl+Shift+Tab', closePage: 'Ctrl/Cmd+W',
  selectSimilar: 'Ctrl/Cmd+Shift+A', editElement: 'F2', remove: 'Delete', askAI: 'Ctrl/Cmd+K',
  selectTool: 'V', requestTool: 'R', panTool: 'H',
  rectangleTool: 'Shift+R', circleTool: 'C', freehandTool: 'L', help: 'F1',
} as const;

export type ToolShortcut = keyof typeof TOOL_SHORTCUTS;

/** A tooltip with its shortcut, e.g. "Ask AI · Ctrl/Cmd+K". */
export function withShortcut(label: string, action?: ToolShortcut) {
  return action ? `${label} · ${TOOL_SHORTCUTS[action]}` : label;
}

export function editorShortcut(event: ShortcutInput, editingText: boolean): EditorShortcut | null {
  if (event.defaultPrevented || event.isComposing) return null;
  if (event.altKey) return !editingText && !event.ctrlKey && !event.metaKey && !event.shiftKey && (event.key.toLowerCase() === 'z' || event.code === 'KeyZ') ? 'undoElement' : null;
  // WebKitGTK reports Shift+Tab as Unidentified with the physical Tab code.
  const key = (event.key === 'Unidentified' && event.code === 'Tab' ? 'Tab' : event.key).toLowerCase();
  const command = event.ctrlKey || event.metaKey;
  if (command) {
    if (!editingText && !event.shiftKey && key === 'x') return 'cutLayer';
    if (!editingText && !event.shiftKey && key === 'c') return 'copyLayer';
    if (!editingText && !event.shiftKey && key === 'v') return 'pasteLayer';
    if (!editingText && (key === '[' || key === '{' || event.code === 'BracketLeft')) return event.shiftKey ? 'layerBack' : 'layerBackward';
    if (!editingText && (key === ']' || key === '}' || event.code === 'BracketRight')) return event.shiftKey ? 'layerFront' : 'layerForward';
    if (!editingText && !event.shiftKey && key === 'k') return 'askAI';
    if (!editingText && event.shiftKey && key === 'a') return 'selectSimilar';
    if (key === 's') return event.shiftKey ? 'saveAs' : 'save';
    if (!event.shiftKey && key === 'w') return 'closePageTab';
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
  if (!editingText && !event.shiftKey && key === 'v') return 'selectTool';
  if (!editingText && !event.shiftKey && key === 'r') return 'requestTool';
  if (!editingText && event.shiftKey && key === 'r') return 'rectangleTool';
  if (!editingText && !event.shiftKey && key === 'c') return 'circleTool';
  if (!editingText && !event.shiftKey && key === 'l') return 'freehandTool';
  if (!editingText && !event.shiftKey && key === 'h') return 'panTool';
  if (!editingText && (key === 'f1' || key === '?')) return 'help';
  if (!editingText && !event.shiftKey && (key === 'delete' || key === 'backspace')) return 'remove';
  return null;
}
