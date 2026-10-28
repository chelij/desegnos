// One keymap for the canvas: tool strip tooltips, the menu and the keyboard help all read it,
// so a binding can never be added in one place and forgotten in another.
export type ShortcutAction =
  | 'selectTool' | 'requestTool' | 'requestFromShape' | 'boundaryTool' | 'pointerTool' | 'textTool' | 'panTool'
  | 'rectTool' | 'ellipseTool' | 'lineTool' | 'imageTool' | 'noteTool' | 'insert'
  | 'copy' | 'cut' | 'paste' | 'pasteChoice' | 'duplicate'
  | 'copyStyle' | 'pasteStyle' | 'paintFormat'
  | 'undo' | 'redo' | 'link' | 'unlink' | 'group' | 'ungroup' | 'delete' | 'renameLayer'
  | 'forward' | 'backward' | 'front' | 'back'
  | 'selectAll' | 'selectChild' | 'selectParent' | 'nudge' | 'nudgeFar'
  | 'zoomIn' | 'zoomOut' | 'zoomReset' | 'fitAll' | 'fitSelection' | 'hidePanels'
  | 'minimap' | 'saveView' | 'goToView'
  | 'play' | 'preview' | 'askAI' | 'export'
  | 'new' | 'open' | 'save' | 'saveAs' | 'closeProject'
  | 'help';

export type ShortcutGroup = 'File' | 'Edit' | 'Arrange' | 'Selection' | 'Tools' | 'View' | 'Run' | 'Help';
export type ShortcutInput = { key: string; code?: string; ctrlKey: boolean; metaKey: boolean; altKey: boolean; shiftKey: boolean; defaultPrevented?: boolean; isComposing?: boolean };

type KeyCombo = { key?: string; code?: string; mod?: boolean; shift?: boolean; alt?: boolean };
type Binding = {
  action: ShortcutAction;
  label: string;
  group: ShortcutGroup;
  /** The first entry is shown in menus and tooltips. */
  keys: KeyCombo[];
  display: string;
  /** Actions that still make sense while the caret is in a text field. */
  inText?: boolean;
};

const plain = (key: string): KeyCombo => ({ key, shift: false });
const shift = (key: string): KeyCombo => ({ key, shift: true });
const mod = (key: string): KeyCombo => ({ key, mod: true, shift: false });
const modShift = (key: string): KeyCombo => ({ key, mod: true, shift: true });
const altMod = (key: string): KeyCombo => ({ key, mod: true, shift: false, alt: true });
const altModShift = (key: string): KeyCombo => ({ key, mod: true, shift: true, alt: true });
const shiftCode = (code: string): KeyCombo => ({ code, shift: true });

export const KEYMAP: Binding[] = [
  // File
  { action: 'new', label: 'New project', group: 'File', keys: [mod('n')], display: 'Ctrl/Cmd+N' },
  { action: 'open', label: 'Open project', group: 'File', keys: [mod('o')], display: 'Ctrl/Cmd+O' },
  { action: 'save', label: 'Save', group: 'File', keys: [mod('s')], display: 'Ctrl/Cmd+S', inText: true },
  { action: 'saveAs', label: 'Save as', group: 'File', keys: [modShift('s')], display: 'Ctrl/Cmd+Shift+S', inText: true },
  { action: 'closeProject', label: 'Close project', group: 'File', keys: [mod('w')], display: 'Ctrl/Cmd+W' },

  // Edit
  { action: 'undo', label: 'Undo', group: 'Edit', keys: [mod('z')], display: 'Ctrl/Cmd+Z' },
  { action: 'redo', label: 'Redo', group: 'Edit', keys: [modShift('z')], display: 'Shift+Ctrl/Cmd+Z' },
  { action: 'cut', label: 'Cut', group: 'Edit', keys: [mod('x')], display: 'Ctrl/Cmd+X' },
  { action: 'copy', label: 'Copy', group: 'Edit', keys: [mod('c')], display: 'Ctrl/Cmd+C' },
  { action: 'paste', label: 'Paste', group: 'Edit', keys: [mod('v')], display: 'Ctrl/Cmd+V' },
  { action: 'pasteChoice', label: 'Paste with linked or unlinked choice', group: 'Edit', keys: [modShift('v')], display: 'Shift+Ctrl/Cmd+V' },
  { action: 'duplicate', label: 'Duplicate', group: 'Edit', keys: [mod('d')], display: 'Ctrl/Cmd+D' },
  { action: 'copyStyle', label: 'Copy style', group: 'Edit', keys: [altMod('c')], display: 'Ctrl/Cmd+Alt+C' },
  { action: 'pasteStyle', label: 'Paste style', group: 'Edit', keys: [altMod('v')], display: 'Ctrl/Cmd+Alt+V' },
  { action: 'paintFormat', label: 'Paint format', group: 'Edit', keys: [altModShift('c')], display: 'Shift+Ctrl/Cmd+Alt+C' },
  { action: 'link', label: 'Link layers', group: 'Edit', keys: [altMod('k')], display: 'Ctrl/Cmd+Alt+K' },
  { action: 'unlink', label: 'Unlink layers', group: 'Edit', keys: [altMod('b')], display: 'Ctrl/Cmd+Alt+B' },
  { action: 'delete', label: 'Delete selection', group: 'Edit', keys: [{ key: 'Delete', shift: false }, { key: 'Backspace', shift: false }], display: 'Delete or Backspace' },
  { action: 'renameLayer', label: 'Rename selected layer', group: 'Edit', keys: [{ key: 'F2', shift: false }], display: 'F2' },

  // Arrange
  { action: 'forward', label: 'Bring forward', group: 'Arrange', keys: [mod(']'), { code: 'BracketRight', mod: true, shift: false }], display: 'Ctrl/Cmd+]' },
  { action: 'backward', label: 'Send backward', group: 'Arrange', keys: [mod('['), { code: 'BracketLeft', mod: true, shift: false }], display: 'Ctrl/Cmd+[' },
  { action: 'front', label: 'Bring to front', group: 'Arrange', keys: [modShift(']'), { code: 'BracketRight', mod: true, shift: true }], display: 'Shift+Ctrl/Cmd+]' },
  { action: 'back', label: 'Send to back', group: 'Arrange', keys: [modShift('['), { code: 'BracketLeft', mod: true, shift: true }], display: 'Shift+Ctrl/Cmd+[' },
  { action: 'group', label: 'Group', group: 'Arrange', keys: [mod('g')], display: 'Ctrl/Cmd+G' },
  { action: 'ungroup', label: 'Ungroup', group: 'Arrange', keys: [modShift('g')], display: 'Shift+Ctrl/Cmd+G' },

  // Selection
  { action: 'selectAll', label: 'Select all', group: 'Selection', keys: [mod('a')], display: 'Ctrl/Cmd+A' },
  { action: 'selectChild', label: 'Select child', group: 'Selection', keys: [{ key: 'Enter', shift: false }], display: 'Enter' },
  { action: 'selectParent', label: 'Select parent', group: 'Selection', keys: [{ key: 'Enter', shift: true }], display: 'Shift+Enter' },
  { action: 'nudge', label: 'Nudge', group: 'Selection', keys: [
    { key: 'ArrowLeft', shift: false }, { key: 'ArrowRight', shift: false }, { key: 'ArrowUp', shift: false }, { key: 'ArrowDown', shift: false },
  ], display: 'Arrow keys' },
  { action: 'nudgeFar', label: 'Far nudge', group: 'Selection', keys: [
    { key: 'ArrowLeft', shift: true }, { key: 'ArrowRight', shift: true }, { key: 'ArrowUp', shift: true }, { key: 'ArrowDown', shift: true },
  ], display: 'Shift+Arrow keys' },

  // Tools
  { action: 'selectTool', label: 'Select tool', group: 'Tools', keys: [plain('v')], display: 'V' },
  { action: 'boundaryTool', label: 'Boundary tool', group: 'Tools', keys: [plain('b')], display: 'B' },
  { action: 'requestTool', label: 'Request tool', group: 'Tools', keys: [plain('f')], display: 'F' },
  { action: 'requestFromShape', label: 'Request from a shape outline', group: 'Tools', keys: [{ key: 'f', shift: true }], display: 'Shift+F' },
  { action: 'textTool', label: 'Text tool', group: 'Tools', keys: [plain('t')], display: 'T' },
  { action: 'rectTool', label: 'Rectangle tool', group: 'Tools', keys: [plain('r')], display: 'R' },
  { action: 'ellipseTool', label: 'Ellipse tool', group: 'Tools', keys: [plain('o')], display: 'O' },
  { action: 'lineTool', label: 'Line tool', group: 'Tools', keys: [plain('l')], display: 'L' },
  { action: 'imageTool', label: 'Image tool', group: 'Tools', keys: [{ key: 'k', mod: true, shift: true, alt: false }], display: 'Shift+Ctrl/Cmd+K' },
  { action: 'insert', label: 'Insert any element', group: 'Tools', keys: [mod('i')], display: 'Ctrl/Cmd+I' },
  { action: 'pointerTool', label: 'Pointer tool', group: 'Tools', keys: [plain('p')], display: 'P' },
  { action: 'noteTool', label: 'Note tool', group: 'Tools', keys: [plain('n')], display: 'N' },
  { action: 'panTool', label: 'Pan tool', group: 'Tools', keys: [plain('h')], display: 'H' },

  // View
  { action: 'zoomIn', label: 'Zoom in', group: 'View', keys: [{ key: '=', mod: true }, { key: '+', mod: true }, { code: 'Equal', mod: true }, { code: 'NumpadAdd', mod: true }], display: 'Ctrl/Cmd+=' },
  { action: 'zoomOut', label: 'Zoom out', group: 'View', keys: [{ key: '-', mod: true }, { key: '_', mod: true }, { code: 'Minus', mod: true }, { code: 'NumpadSubtract', mod: true }], display: 'Ctrl/Cmd+-' },
  { action: 'zoomReset', label: 'Zoom to 100%', group: 'View', keys: [{ code: 'Digit0', mod: true }, { key: '0', mod: true }], display: 'Ctrl/Cmd+0' },
  { action: 'fitAll', label: 'Fit all boundaries', group: 'View', keys: [shiftCode('Digit1')], display: 'Shift+1' },
  { action: 'fitSelection', label: 'Fit the selection', group: 'View', keys: [shiftCode('Digit2')], display: 'Shift+2' },
  { action: 'minimap', label: 'Show or hide the minimap', group: 'View', keys: [shift('m')], display: 'Shift+M' },
  { action: 'saveView', label: 'Save the current view', group: 'View', keys: ['1', '2', '3', '4', '5', '6', '7', '8', '9'].map(altMod), display: 'Ctrl/Cmd+Alt+1…9' },
  { action: 'goToView', label: 'Go to a saved view', group: 'View', keys: ['1', '2', '3', '4', '5', '6', '7', '8', '9'].map(key => ({ key, alt: true, shift: false })), display: 'Alt+1…9' },
  { action: 'hidePanels', label: 'Hide or show panels', group: 'View', keys: [plain('Tab')], display: 'Tab' },

  // Run
  { action: 'play', label: 'Play', group: 'Run', keys: [{ key: ' ', code: 'Space', shift: true }], display: 'Shift+Space' },
  { action: 'preview', label: 'Preview export', group: 'Run', keys: [{ key: 'Enter', mod: true }], display: 'Ctrl/Cmd+Enter', inText: true },
  { action: 'askAI', label: 'Ask AI on selection', group: 'Run', keys: [mod('k')], display: 'Ctrl/Cmd+K' },
  { action: 'export', label: 'Export', group: 'Run', keys: [mod('e')], display: 'Ctrl/Cmd+E' },

  // Help
  { action: 'help', label: 'Keyboard shortcuts', group: 'Help', keys: [{ key: 'F1', shift: false }], display: 'F1' },
];

const BINDING = new Map(KEYMAP.map(binding => [binding.action, binding]));

/** The compact label a menu or tooltip shows for an action. */
export function shortcutLabel(action: ShortcutAction): string {
  return BINDING.get(action)?.display ?? '';
}

/** A tooltip with its shortcut, e.g. "Ask AI · Ctrl/Cmd+K". */
export function withShortcut(label: string, action: ShortcutAction) {
  return `${label} · ${shortcutLabel(action)}`;
}

export const TOOL_SHORTCUTS: Record<string, string> = {
  newProject: shortcutLabel('new'), openProject: shortcutLabel('open'), save: shortcutLabel('save'), saveAs: shortcutLabel('saveAs'),
  copy: shortcutLabel('copy'), cut: shortcutLabel('cut'), paste: shortcutLabel('paste'), pasteChoice: shortcutLabel('pasteChoice'), duplicate: shortcutLabel('duplicate'),
  copyStyle: shortcutLabel('copyStyle'), pasteStyle: shortcutLabel('pasteStyle'), paintFormat: shortcutLabel('paintFormat'),
  forward: shortcutLabel('forward'), backward: shortcutLabel('backward'), front: shortcutLabel('front'), back: shortcutLabel('back'),
  undo: shortcutLabel('undo'), redo: shortcutLabel('redo'), link: shortcutLabel('link'), unlink: shortcutLabel('unlink'),
  group: shortcutLabel('group'), ungroup: shortcutLabel('ungroup'),
  zoomIn: shortcutLabel('zoomIn'), zoomOut: shortcutLabel('zoomOut'), zoomReset: shortcutLabel('zoomReset'), fit: shortcutLabel('fitAll'), fitSelection: shortcutLabel('fitSelection'),
  closeProject: shortcutLabel('closeProject'), closePage: shortcutLabel('closeProject'),
  editElement: shortcutLabel('renameLayer'), remove: shortcutLabel('delete'), askAI: shortcutLabel('askAI'),
  selectTool: shortcutLabel('selectTool'), requestTool: shortcutLabel('requestTool'), requestFromShape: shortcutLabel('requestFromShape'), boundaryTool: shortcutLabel('boundaryTool'), pointerTool: shortcutLabel('pointerTool'),
  textTool: shortcutLabel('textTool'), panTool: shortcutLabel('panTool'), rectTool: shortcutLabel('rectTool'), rectangleTool: shortcutLabel('rectTool'),
  ellipseTool: shortcutLabel('ellipseTool'), lineTool: shortcutLabel('lineTool'), imageTool: shortcutLabel('imageTool'), noteTool: shortcutLabel('noteTool'),
  insertTool: shortcutLabel('insert'), play: shortcutLabel('play'), preview: shortcutLabel('preview'), export: shortcutLabel('export'), help: shortcutLabel('help'),
  selectAll: shortcutLabel('selectAll'), selectChild: shortcutLabel('selectChild'), selectParent: shortcutLabel('selectParent'),
};

export type ToolShortcut = keyof typeof TOOL_SHORTCUTS;

function comboMatches(combo: KeyCombo, event: ShortcutInput) {
  const command = event.ctrlKey || event.metaKey;
  if (!!combo.mod !== command) return false;
  if (!!combo.alt !== event.altKey) return false;
  if (combo.shift !== undefined && combo.shift !== event.shiftKey) return false;
  if (combo.code && event.code === combo.code) return true;
  if (combo.key !== undefined) {
    // WebKitGTK reports Shift+Tab as Unidentified with the physical Tab code.
    const key = event.key === 'Unidentified' && event.code === 'Tab' ? 'Tab' : event.key;
    if (key.toLowerCase() === combo.key.toLowerCase()) return true;
  }
  return false;
}

/** The action a key event performs, or null. Text fields keep their own editing keys. */
export function shortcutFor(event: ShortcutInput, editingText: boolean): ShortcutAction | null {
  if (event.defaultPrevented || event.isComposing) return null;
  for (const binding of KEYMAP) {
    if (editingText && !binding.inText) continue;
    if (binding.keys.some(combo => comboMatches(combo, event))) return binding.action;
  }
  return null;
}

export type ShortcutHelpGroup = { group: ShortcutGroup; items: { label: string; keys: string }[] };

/** The numbered slot (1–9) a save-view or go-to-view chord refers to. */
export function viewSlot(event: { key: string; code?: string }): number | null {
  const key = /^[1-9]$/.exec(event.key);
  if (key) return Number(key[0]);
  const code = /^(?:Digit|Numpad)([1-9])$/.exec(event.code ?? '');
  return code ? Number(code[1]) : null;
}

/** The keyboard help is generated from the keymap, grouped in reading order. */
export const SHORTCUT_GROUPS: ShortcutHelpGroup[] = (() => {
  const order: ShortcutGroup[] = ['Tools', 'Edit', 'Arrange', 'Selection', 'View', 'Run', 'File', 'Help'];
  return order.map(group => ({
    group,
    items: KEYMAP.filter(binding => binding.group === group).map(binding => ({ label: binding.label, keys: binding.display })),
  })).filter(entry => entry.items.length > 0);
})();

export const SHORTCUT_HELP = SHORTCUT_GROUPS.flatMap(entry => entry.items.map(item => [item.label, item.keys] as const));
