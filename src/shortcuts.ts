export type ShortcutAction =
  | 'selectTool' | 'requestTool' | 'requestFromShape' | 'boundaryTool' | 'pointerTool' | 'textTool' | 'panTool'
  | 'rectTool' | 'ellipseTool' | 'lineTool' | 'imageTool' | 'noteTool' | 'insert'
  | 'copy' | 'cut' | 'paste' | 'pasteChoice' | 'duplicate'
  | 'copyStyle' | 'pasteStyle' | 'paintFormat'
  | 'undo' | 'redo' | 'link' | 'unlink' | 'group' | 'ungroup' | 'delete' | 'renameLayer'
  | 'forward' | 'backward' | 'front' | 'back'
  | 'alignLeft' | 'alignCentre' | 'alignRight' | 'alignTop' | 'alignMiddle' | 'alignBottom'
  | 'distributeHorizontal' | 'distributeVertical' | 'toggleHidden' | 'toggleLocked' | 'stack'
  | 'flipHorizontal' | 'flipVertical' | 'bold' | 'italic' | 'fontSizeUp' | 'fontSizeDown'
  | 'opacity10' | 'opacity20' | 'opacity30' | 'opacity40' | 'opacity50' | 'opacity60' | 'opacity70' | 'opacity80' | 'opacity90' | 'opacity100'
  | 'selectAll' | 'selectChild' | 'selectParent' | 'nudge' | 'nudgeFar'
  | 'zoomIn' | 'zoomOut' | 'zoomReset' | 'fitAll' | 'fitSelection' | 'hidePanels'
  | 'minimap' | 'saveView' | 'goToView'
  | 'play' | 'preview' | 'askAI' | 'export'
  | 'new' | 'open' | 'save' | 'saveAs' | 'closeProject'
  | 'holdPan' | 'holdZoom' | 'nextSlide' | 'previousSlide' | 'exitPresentation'
  | 'adaptLayout' | 'commandPalette' | 'help';

export type ShortcutGroup = 'File' | 'Edit' | 'Arrange' | 'Selection' | 'Tools' | 'View' | 'Run' | 'Help';
export type ShortcutInput = { key: string; code?: string; ctrlKey: boolean; metaKey: boolean; altKey: boolean; shiftKey: boolean; defaultPrevented?: boolean; isComposing?: boolean; target?: EventTarget | null };

type KeyCombo = { key?: string; code?: string; mod?: boolean; shift?: boolean; alt?: boolean };
type Binding = {
  action: ShortcutAction;
  label: string;
  group: ShortcutGroup;
  keys: KeyCombo[];
  display: string;
  scope?: 'held' | 'presentation';
  palette?: boolean;
  canvasOnly?: boolean;
  toolOrder?: number;
  elementKind?: import("./types").ElementKind;
  tool?: import("./EditorTools").EditorTool;
  repeat?: boolean;
  withoutProject?: boolean;
  inText?: boolean;
  textOnly?: boolean;
};

const plain = (key: string): KeyCombo => ({ key, shift: false });
const shift = (key: string): KeyCombo => ({ key, shift: true });
const mod = (key: string): KeyCombo => ({ key, mod: true, shift: false });
const modShift = (key: string): KeyCombo => ({ key, mod: true, shift: true });
const altMod = (key: string): KeyCombo => ({ key, mod: true, shift: false, alt: true });
const altModShift = (key: string): KeyCombo => ({ key, mod: true, shift: true, alt: true });
const shiftCode = (code: string): KeyCombo => ({ code, shift: true });

const bindings: Omit<Binding, "display">[] = [
  { action: 'new', repeat: false, withoutProject: true, label: 'New project', group: 'File', keys: [mod('n')] },
  { action: 'open', repeat: false, withoutProject: true, label: 'Open project…', group: 'File', keys: [mod('o')] },
  { action: 'save', label: 'Save', group: 'File', keys: [mod('s')], inText: true },
  { action: 'saveAs', repeat: false, label: 'Save as', group: 'File', keys: [modShift('s')], inText: true },
  { action: 'closeProject', label: 'Close project', group: 'File', keys: [mod('w')] },

  { action: 'undo', label: 'Undo', group: 'Edit', keys: [mod('z')] },
  { action: 'redo', label: 'Redo', group: 'Edit', keys: [modShift('z')] },
  { action: 'cut', label: 'Cut', group: 'Edit', keys: [mod('x')] },
  { action: 'copy', label: 'Copy', group: 'Edit', keys: [mod('c')] },
  { action: 'paste', label: 'Paste', group: 'Edit', keys: [mod('v')] },
  { action: 'pasteChoice', label: 'Paste with linked or unlinked choice', group: 'Edit', keys: [modShift('v')] },
  { action: 'duplicate', label: 'Duplicate', group: 'Edit', keys: [mod('d')] },
  { action: 'copyStyle', label: 'Copy style', group: 'Edit', keys: [altMod('c')] },
  { action: 'pasteStyle', label: 'Paste style', group: 'Edit', keys: [altMod('v')] },
  { action: 'paintFormat', label: 'Paint format', group: 'Edit', keys: [altModShift('c')] },
  { action: 'link', label: 'Link layers', group: 'Edit', keys: [altMod('k')] },
  { action: 'unlink', label: 'Unlink layers', group: 'Edit', keys: [altMod('b')] },
  { action: 'delete', label: 'Delete selection', group: 'Edit', keys: [{ key: 'Delete', shift: false }, { key: 'Backspace', shift: false }] },
  { action: 'renameLayer', label: 'Rename selected layer', group: 'Edit', keys: [{ key: 'F2', shift: false }] },

  { action: 'bold', label: 'Toggle bold', group: 'Edit', keys: [mod('b')], inText: true, textOnly: true },
  { action: 'italic', label: 'Toggle italic', group: 'Edit', keys: [mod('i')], inText: true, textOnly: true },
  { action: 'fontSizeUp', label: 'Increase font size', group: 'Edit', keys: [{ code: 'Period', mod: true, shift: true }, modShift('>')] },
  { action: 'fontSizeDown', label: 'Decrease font size', group: 'Edit', keys: [{ code: 'Comma', mod: true, shift: true }, modShift('<')] },
  { action: 'opacity100', canvasOnly: true, label: 'Opacity 100%', group: 'Edit', keys: [plain('0')] },
  { action: 'opacity10', canvasOnly: true, label: 'Opacity 10%', group: 'Edit', keys: [plain('1')] },
  { action: 'opacity20', canvasOnly: true, label: 'Opacity 20%', group: 'Edit', keys: [plain('2')] },
  { action: 'opacity30', canvasOnly: true, label: 'Opacity 30%', group: 'Edit', keys: [plain('3')] },
  { action: 'opacity40', canvasOnly: true, label: 'Opacity 40%', group: 'Edit', keys: [plain('4')] },
  { action: 'opacity50', canvasOnly: true, label: 'Opacity 50%', group: 'Edit', keys: [plain('5')] },
  { action: 'opacity60', canvasOnly: true, label: 'Opacity 60%', group: 'Edit', keys: [plain('6')] },
  { action: 'opacity70', canvasOnly: true, label: 'Opacity 70%', group: 'Edit', keys: [plain('7')] },
  { action: 'opacity80', canvasOnly: true, label: 'Opacity 80%', group: 'Edit', keys: [plain('8')] },
  { action: 'opacity90', canvasOnly: true, label: 'Opacity 90%', group: 'Edit', keys: [plain('9')] },

  { action: 'flipHorizontal', label: 'Flip horizontal', group: 'Arrange', keys: [shift('h')] },
  { action: 'flipVertical', label: 'Flip vertical', group: 'Arrange', keys: [shift('v')] },
  { action: 'adaptLayout', label: 'Adapt layout', group: 'Arrange', keys: [altModShift('a')] },
  { action: 'alignLeft', label: 'Align left', group: 'Arrange', keys: [{ key: 'a', code: 'KeyA', alt: true, shift: false }] },
  { action: 'alignCentre', label: 'Align horizontal centres', group: 'Arrange', keys: [{ key: 'h', code: 'KeyH', alt: true, shift: false }] },
  { action: 'alignRight', label: 'Align right', group: 'Arrange', keys: [{ key: 'd', code: 'KeyD', alt: true, shift: false }] },
  { action: 'alignTop', label: 'Align top', group: 'Arrange', keys: [{ key: 'w', code: 'KeyW', alt: true, shift: false }] },
  { action: 'alignMiddle', label: 'Align vertical centres', group: 'Arrange', keys: [{ key: 'v', code: 'KeyV', alt: true, shift: false }] },
  { action: 'alignBottom', label: 'Align bottom', group: 'Arrange', keys: [{ key: 's', code: 'KeyS', alt: true, shift: false }] },
  { action: 'distributeHorizontal', label: 'Distribute horizontally', group: 'Arrange', keys: [{ key: 'h', code: 'KeyH', alt: true, shift: true }] },
  { action: 'distributeVertical', label: 'Distribute vertically', group: 'Arrange', keys: [{ key: 'v', code: 'KeyV', alt: true, shift: true }] },
  { action: 'stack', label: 'Add Stack', group: 'Arrange', keys: [shift('a')] },
  { action: 'toggleHidden', label: 'Hide or show selection', group: 'Arrange', keys: [modShift('h')] },
  { action: 'toggleLocked', label: 'Lock or unlock selection', group: 'Arrange', keys: [modShift('l')] },
  { action: 'forward', label: 'Bring forward', group: 'Arrange', keys: [mod(']'), { code: 'BracketRight', mod: true, shift: false }] },
  { action: 'backward', label: 'Send backward', group: 'Arrange', keys: [mod('['), { code: 'BracketLeft', mod: true, shift: false }] },
  { action: 'front', label: 'Bring to front', group: 'Arrange', keys: [modShift(']'), { code: 'BracketRight', mod: true, shift: true }] },
  { action: 'back', label: 'Send to back', group: 'Arrange', keys: [modShift('['), { code: 'BracketLeft', mod: true, shift: true }] },
  { action: 'group', label: 'Group', group: 'Arrange', keys: [mod('g')] },
  { action: 'ungroup', label: 'Ungroup', group: 'Arrange', keys: [modShift('g')] },

  { action: 'selectAll', label: 'Select all', group: 'Selection', keys: [mod('a')] },
  { action: 'selectChild', label: 'Select child', group: 'Selection', keys: [{ key: 'Enter', shift: false }] },
  { action: 'selectParent', label: 'Select parent', group: 'Selection', keys: [{ key: 'Enter', shift: true }] },
  { action: 'nudge', label: 'Nudge', group: 'Selection', keys: [
    { key: 'ArrowLeft', shift: false }, { key: 'ArrowRight', shift: false }, { key: 'ArrowUp', shift: false }, { key: 'ArrowDown', shift: false },
  ] },
  { action: 'nudgeFar', label: 'Far nudge', group: 'Selection', keys: [
    { key: 'ArrowLeft', shift: true }, { key: 'ArrowRight', shift: true }, { key: 'ArrowUp', shift: true }, { key: 'ArrowDown', shift: true },
  ] },

  { action: 'selectTool', tool: 'select', toolOrder: 0, label: 'Select tool', group: 'Tools', keys: [plain('v')] },
  { action: 'boundaryTool', tool: 'boundary', toolOrder: 1, label: 'Boundary tool', group: 'Tools', keys: [plain('b')] },
  { action: 'requestTool', tool: 'request', toolOrder: 2, label: 'Request tool', group: 'Tools', keys: [plain('f')] },
  { action: 'requestFromShape', label: 'Request from a shape outline', group: 'Tools', keys: [{ key: 'f', shift: true }] },
  { action: 'textTool', tool: 'text', toolOrder: 3, label: 'Text tool', group: 'Tools', keys: [plain('t')] },
  { action: 'rectTool', tool: 'rect', elementKind: 'box', toolOrder: 4, label: 'Rectangle tool', group: 'Tools', keys: [plain('r')] },
  { action: 'ellipseTool', tool: 'ellipse', elementKind: 'ellipse', toolOrder: 5, label: 'Ellipse tool', group: 'Tools', keys: [plain('o')] },
  { action: 'lineTool', tool: 'line', elementKind: 'line', toolOrder: 6, label: 'Line tool', group: 'Tools', keys: [plain('l')] },
  { action: 'imageTool', tool: 'image', toolOrder: 7, label: 'Image tool', group: 'Tools', keys: [{ key: 'k', mod: true, shift: true, alt: false }] },
  { action: 'insert', tool: 'insert', toolOrder: 8, label: 'Insert any element', group: 'Tools', keys: [mod('i')] },
  { action: 'pointerTool', tool: 'pointer', toolOrder: 9, label: 'Pointer tool', group: 'Tools', keys: [plain('p')] },
  { action: 'noteTool', tool: 'note', toolOrder: 10, label: 'Note tool', group: 'Tools', keys: [plain('n')] },
  { action: 'panTool', tool: 'pan', toolOrder: 11, label: 'Pan tool', group: 'Tools', keys: [plain('h')] },

  { action: 'zoomIn', label: 'Zoom in', group: 'View', keys: [{ key: '=', mod: true }, { key: '+', mod: true }, { code: 'Equal', mod: true }, { code: 'NumpadAdd', mod: true }] },
  { action: 'zoomOut', label: 'Zoom out', group: 'View', keys: [{ key: '-', mod: true }, { key: '_', mod: true }, { code: 'Minus', mod: true }, { code: 'NumpadSubtract', mod: true }] },
  { action: 'zoomReset', label: 'Zoom to 100%', group: 'View', keys: [{ code: 'Digit0', mod: true }, { key: '0', mod: true }] },
  { action: 'fitAll', label: 'Fit all boundaries', group: 'View', keys: [shiftCode('Digit1')] },
  { action: 'fitSelection', label: 'Fit the selection', group: 'View', keys: [shiftCode('Digit2')] },
  { action: 'minimap', label: 'Show or hide the minimap', group: 'View', keys: [shift('m')] },
  { action: 'saveView', repeat: false, label: 'Save the current view', group: 'View', keys: ['1', '2', '3', '4', '5', '6', '7', '8', '9'].map(altMod) },
  { action: 'goToView', label: 'Go to a saved view', group: 'View', keys: ['1', '2', '3', '4', '5', '6', '7', '8', '9'].map(key => ({ key, alt: true, shift: false })) },
  { action: 'hidePanels', repeat: false, label: 'Hide or show panels', group: 'View', keys: [{ key: '\\', mod: true, shift: false }, { code: 'Backslash', mod: true, shift: false }] },

  { action: 'play', label: 'Play', group: 'Run', keys: [{ key: ' ', code: 'Space', shift: true }] },
  { action: 'preview', label: 'Preview export', group: 'Run', keys: [{ key: 'Enter', mod: true }], inText: true },
  { action: 'askAI', label: 'Request on selection', group: 'Run', keys: [mod('k')] },
  { action: 'export', label: 'Export…', group: 'Run', keys: [mod('e')] },

  { action: 'commandPalette', withoutProject: true, label: 'Command palette', group: 'Help', keys: [mod('/')] },
  { action: 'help', repeat: false, withoutProject: true, label: 'Keyboard shortcuts', group: 'Help', keys: [{ key: 'F1', shift: false }] },
  { action: 'holdPan', label: 'Hold to pan', group: 'Tools', keys: [plain(' ')], scope: 'held', palette: false },
  { action: 'holdZoom', label: 'Hold to zoom', group: 'Tools', keys: [plain('z')], scope: 'held', palette: false },
  { action: 'nextSlide', label: 'Next slide', group: 'Run', keys: ['ArrowRight', 'ArrowDown', ' ', 'PageDown'].map(plain), scope: 'presentation', palette: false },
  { action: 'previousSlide', label: 'Previous slide', group: 'Run', keys: ['ArrowLeft', 'ArrowUp', 'PageUp'].map(plain), scope: 'presentation', palette: false },
  { action: 'exitPresentation', label: 'Stop presentation', group: 'Run', keys: [plain('Escape')], scope: 'presentation', palette: false },
];

function comboLabel(combo: KeyCombo): string {
  const key = combo.key ?? combo.code?.replace(/^(Digit|Key|Numpad)/, '') ?? '';
  const label = key === ' ' ? 'Space' : key.startsWith('Arrow') ? key.replace('Arrow', '') : key.toUpperCase() === key && key.length > 1 ? key : key.length === 1 ? key.toUpperCase() : key;
  return [combo.mod && 'Ctrl/Cmd', combo.alt && 'Alt', combo.shift && 'Shift', label].filter(Boolean).join('+');
}

export const KEYMAP: Binding[] = bindings.map(binding => ({ ...binding,
  display: binding.action === 'delete' ? [...new Set(binding.keys.map(comboLabel))].join(' or ')
    : binding.scope === 'presentation' ? binding.keys.map(comboLabel).join(' / ')
    : binding.action === 'nudge' || binding.action === 'nudgeFar'
    ? `${binding.keys[0].shift ? 'Shift+' : ''}Arrow keys`
    : binding.action === 'saveView' || binding.action === 'goToView'
      ? `${comboLabel(binding.keys[0]).slice(0, -1)}1…9` : comboLabel(binding.keys[0]),
}));

export const TOOL_ACTIONS = KEYMAP.filter((binding): binding is Binding & { tool: import('./EditorTools').EditorTool } => !!binding.tool).sort((a, b) => a.toolOrder! - b.toolOrder!);
export const actionBinding = (action: ShortcutAction) => BINDING.get(action)!;

const BINDING = new Map(KEYMAP.map(binding => [binding.action, binding]));

export function shortcutLabel(action: ShortcutAction): string {
  return BINDING.get(action)?.display ?? '';
}

export function withShortcut(label: string, action: ShortcutAction) {
  return `${label} · ${shortcutLabel(action)}`;
}

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

export function shortcutFor(event: ShortcutInput, editingText: boolean, canvasText = false, scope?: 'held' | 'presentation', canvasFocused = true): ShortcutAction | null {
  if (event.defaultPrevented || event.isComposing) return null;
  for (const binding of KEYMAP) {
    if (binding.scope !== scope || binding.canvasOnly && !canvasFocused) continue;
    if (editingText && !binding.inText && !(binding.textOnly && canvasText)) continue;
    if (binding.textOnly && !canvasText) continue;
    if (binding.keys.some(combo => comboMatches(combo, event))) return binding.action;
  }
  return null;
}

export type ShortcutHelpGroup = { group: ShortcutGroup; items: { label: string; keys: string }[] };

export function viewSlot(event: { key: string; code?: string }): number | null {
  const key = /^[1-9]$/.exec(event.key);
  if (key) return Number(key[0]);
  const code = /^(?:Digit|Numpad)([1-9])$/.exec(event.code ?? '');
  return code ? Number(code[1]) : null;
}

export function viewNameForSlot(slot: number): string {
  return `View ${slot}`;
}

export const SHORTCUT_GROUPS: ShortcutHelpGroup[] = (() => {
  const order: ShortcutGroup[] = ['Tools', 'Edit', 'Arrange', 'Selection', 'View', 'Run', 'File', 'Help'];
  return order.map(group => ({
    group,
    items: KEYMAP.filter(binding => binding.group === group).map(binding => ({ label: binding.label, keys: binding.display })),
  })).filter(entry => entry.items.length > 0);
})();
