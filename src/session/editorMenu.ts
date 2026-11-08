import type { PaletteCommand } from '../CommandPalette';
import type { EditorActions } from '../editorActions';
import type { Theme } from './useEditorTheme';
import type { MenuItem } from '../Menus';
import { KEYMAP } from '../shortcuts';
import type { AppSettingsView, GenerationMode, Project, RecentProject } from '../types';

type MenuState = {
  actions: EditorActions;
  onStartPage: boolean;
  project: Project | null;
  recentProjects: RecentProject[];
  appSettings: AppSettingsView;
  theme: Theme;
  working: boolean;
  run: (operation: () => Promise<unknown>) => void;
  openRecent: (path: string) => Promise<void>;
  forgetRecent: (path: string) => Promise<void>;
  setTheme: (theme: Theme) => void;
  changeShowStartPage: (value: boolean) => Promise<void>;
  setGenerationMode: (mode: GenerationMode) => Promise<unknown>;
};

export function editorMenu(state: MenuState): MenuItem[] {
  const { actions, onStartPage, project, recentProjects, appSettings, theme, working, run } = state;
  const { menuItem } = actions;
  const canEditProject = !!project && !onStartPage;
  return [
    ...(!onStartPage ? [menuItem('home')] : []),
    ...(onStartPage && project ? [menuItem('backToCanvas', 'back-to-canvas')] : []),
    menuItem('new'),
    menuItem('open'),
    ...(recentProjects.length ? [{ id: 'open-recent', label: 'Open recent', children: [
      ...recentProjects.map((entry, index) => ({ id: `open-recent-${index}`, label: entry.name, action: () => run(() => state.openRecent(entry.path)) })),
      { id: 'open-recent-separator', label: '', separator: true },
      { id: 'forget-recent', label: 'Forget recent project', children: recentProjects.map((entry, index) => ({ id: `forget-recent-${index}`, label: entry.name, action: () => run(() => state.forgetRecent(entry.path)) })) },
    ] }] : []),
    menuItem('openFolder', 'open-folder'),
    menuItem('save'),
    menuItem('closeProject', 'close-project'),
    menuItem('saveAs', 'save-as'),
    menuItem('saveCopy', 'save-copy'),
    menuItem('renameProject', 'rename-project'),
    ...(actions.can('recoverUnsaved') ? [menuItem('recoverUnsaved', 'recover')] : []),
    { id: 'file-divider', label: '', separator: true },
    menuItem('export'),
    ...(canEditProject && project.lastExport ? [menuItem('openInBrowser', 'open-browser')] : []),
    { id: 'ai-divider', label: '', separator: true },
    menuItem('aiSettings', 'settings'),
    { id: 'mode', label: 'AI style', disabled: !canEditProject || working, children: (['fuzzy', 'strict'] as GenerationMode[]).map(mode => ({ id: mode, label: mode === 'fuzzy' ? 'Flexible' : 'Precise', checked: project?.generationMode === mode, action: () => run(() => state.setGenerationMode(mode)) })) },
    menuItem('styleGuide', 'style-guide'),
    menuItem('importContext', 'import'),
    menuItem('projectBrief', 'dossier'),
    { id: 'help-divider', label: '', separator: true },
    { id: 'preferences', label: 'Preferences', children: [
      { id: 'show-start-page', label: 'Show start page on launch', checked: appSettings.showStartPage, action: () => run(() => state.changeShowStartPage(!appSettings.showStartPage)) },
      { id: 'preferences-divider', label: '', separator: true },
      { id: 'appearance', label: 'Editor theme', children: (['system', 'light', 'dark'] as Theme[]).map(value => ({ id: value, label: value === 'system' ? 'System' : value === 'light' ? 'Light' : 'Dark', checked: theme === value, action: () => state.setTheme(value) })) },
      menuItem('projectLocation', 'project-location'),
    ] },
    menuItem('checkUpdates', 'check-updates'),
    menuItem('help'),
  ];
}

export function paletteCommands(actions: EditorActions, menuItems: MenuItem[]): PaletteCommand[] {
  return [
    ...KEYMAP.filter(binding => binding.action !== 'commandPalette' && binding.palette !== false).map(binding => ({
      id: `action:${binding.action}`, label: binding.label, keys: binding.display, group: binding.group,
      disabled: !actions.can(binding.action),
      run: () => actions.perform(binding.action),
    })),
    ...menuPaletteCommands(menuItems),
  ];
}

export function menuPaletteCommands(items: MenuItem[], group = 'Menu'): PaletteCommand[] {
  return items.flatMap(item => {
    if (item.separator) return [];
    if (item.children?.length) return menuPaletteCommands(item.children, item.label);
    if (item.shortcut || item.disabled || !item.action) return [];
    return [{ id: `menu:${item.id}`, label: item.label, group, run: item.action }];
  });
}
