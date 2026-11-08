import brandLight from './assets/brand/desegnos-wordmark-light.svg';
import brandDark from './assets/brand/desegnos-wordmark-dark.svg';
import { Icon } from './icons';
import { MenuButton } from './Menus';
import { playIsActive } from './projectCanvas/play';
import type { Editor } from './session/useEditor';
import { withShortcut } from './shortcuts';
import { projectHasPreviewPages, showsOpener } from './visibleActions';

const PROJECT_COLOURS = ['#2f6fde', '#c2410c', '#267a36', '#ae3ec9', '#0e7490', '#b35c00'];

export function AppBar({ editor }: { editor: Editor }) {
  const {
    actions, run, project, view, onStartPage, menuItems, resolvedTheme, setTheme, panel, openPanel, togglePanel, panels, panelActions,
    switchProjectTab, closeProjectTab, play, aiConnected, showAiSettings, setShowAiSettings, generation, working, nothingToGenerate, generate } = editor;
  const previewAvailable = !onStartPage && !!project && projectHasPreviewPages(project);
  const playing = playIsActive(play);
  return <header className="app-bar"><div className="wordmark" aria-label="Desegnos"><img className="brand-light" src={brandLight} alt="" /><img className="brand-dark" src={brandDark} alt="" /><span className="sr-only">Desegnos</span></div>
    {!onStartPage && project && !!view?.openProjects?.length && <nav className="project-tabs" aria-label="Open projects">
      {view.openProjects.map((entry, index) => <div className={entry.active ? 'project-tab active' : 'project-tab'} key={entry.id}>
        <button type="button" className="project-tab-name" title={entry.name} aria-current={entry.active ? 'page' : undefined} onClick={() => run(() => switchProjectTab(entry.id))}>
          <span className="project-dot" style={{ background: PROJECT_COLOURS[index % PROJECT_COLOURS.length] }} aria-hidden="true" />
          <span className="project-tab-label">{entry.name}</span>
          {entry.dirty && <span className="dirty-dot" aria-label="Unsaved changes" />}
        </button>
        <button type="button" className="project-tab-close icon-button sm" aria-label={`Close ${entry.name}`} title={`Close ${entry.name}`} onClick={() => run(() => closeProjectTab(entry.id))}><Icon name="close" size={12} /></button>
      </div>)}
    </nav>}
    <MenuButton label="Menu" ariaLabel="Editor menu" items={menuItems} />
    <div className="shell-primary-actions">
      {(onStartPage || !project) && <button type="button" className="theme-toggle icon-button" aria-label={resolvedTheme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'} title={resolvedTheme === 'dark' ? 'Light mode' : 'Dark mode'} onClick={() => setTheme(resolvedTheme === 'dark' ? 'light' : 'dark')}>{resolvedTheme === 'dark' ? <Icon name="sun" /> : <Icon name="moon" />}</button>}
      {project && <button type="button" className={`icon-button labeled${panel.kind === 'import' ? ' active' : ''}`} aria-pressed={panel.kind === 'import'} aria-label="Import" title="Import" onClick={() => togglePanel({ kind: 'import', source: null })}><Icon name="folder" /><span className="button-label">Import</span></button>}
      {!onStartPage && project && <><button aria-label="Undo last change" type="button" className="icon-button" disabled={!actions.can('undo')} title={withShortcut('Undo', 'undo')} onClick={() => actions.perform('undo')}><Icon name="undo" /></button><button aria-label="Redo last change" type="button" className="icon-button" disabled={!actions.can('redo')} title={withShortcut('Redo', 'redo')} onClick={() => actions.perform('redo')}><Icon name="redo" /></button>
        <button type="button" className={panel.kind === 'history' ? 'icon-button active' : 'icon-button'} aria-pressed={panel.kind === 'history'} aria-label="History and alternatives" title="History and alternatives" onClick={() => { togglePanel({ kind: 'history' }); if (panels.hidden) panelActions.toggleHidden(); if (panels.collapsed) panelActions.setCollapsed(false); }}><Icon name="history" /></button></>}
      {!onStartPage && project && <button type="button" className={`icon-button labeled${playing ? ' active' : ''}`} aria-pressed={playing} aria-label={playing ? 'Stop Play' : 'Play'} title={withShortcut(playing ? 'Stop Play' : 'Play', 'play')} onClick={() => actions.perform('play')}><Icon name={playing ? 'stop' : 'play'} /><span className="button-label">{playing ? 'Stop' : 'Play'}</span></button>}
      {previewAvailable && <button type="button" className="icon-button labeled" aria-label="Preview export" title={withShortcut('Preview export', 'preview')} onClick={() => actions.perform('preview')}><Icon name="monitor" /><span className="button-label">Preview</span></button>}
      {!onStartPage && project && <button type="button" className={`icon-button labeled${panel.kind === 'export' ? ' active' : ''}`} aria-pressed={panel.kind === 'export'} aria-label="Export" title={withShortcut('Export', 'export')} onClick={() => actions.perform('export')}><Icon name="download" /><span className="button-label">Export</span></button>}
      {showsOpener({ available: !aiConnected, open: showAiSettings }) && <button className="ai-shortcut needs-connection" onClick={() => actions.perform('aiSettings')} aria-label="AI settings" title="Connect AI to generate">Connect AI</button>}
      {showsOpener({ available: !onStartPage && !!project && !!(generation.job || generation.operation), open: panel.kind === 'log' }) && <button type="button" className="icon-button" aria-label="AI log" title="AI conversation" onClick={() => openPanel({ kind: 'log', focus: 0 })}><Icon name="note" /></button>}
      {!onStartPage && project && <button className={generation.canStop ? undefined : 'primary'} title={generation.canStop ? 'Stop generation (Esc)' : nothingToGenerate ? 'Describe at least one request first.' : working ? 'Wait for the current generation to finish.' : !aiConnected ? 'Connect AI to generate' : 'Generate the described changes'} disabled={!generation.canStop && (nothingToGenerate || working)} onClick={generation.canStop ? () => { void generation.cancel(); } : !aiConnected && !nothingToGenerate ? () => setShowAiSettings(true) : () => generate()}>{generation.canStop ? 'Stop' : 'Generate'}</button>}
    </div>
  </header>;
}
