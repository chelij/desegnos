import { AppBar } from './AppBar';
import { CanvasWorkspace } from './CanvasWorkspace';
import { CommandPalette } from './CommandPalette';
import { EditorCrashBoundary } from './CrashReport';
import { EditorDialogs } from './EditorDialogs';
import { RecoveryList } from './editorShared';
import { errorValue } from './errorMessage.js';
import { NoticeProvider } from './ProblemBanner.js';
import { PanelSurfaces } from './PanelSurfaces';
import { ShellNotices } from './ShellNotices';
import { StartPage } from './StartPage';
import { StatusBar } from './StatusBar';
import { SurfaceHost, WorkspaceSurfaces } from './WorkspaceSurface';
import { isPresenting } from './projectCanvas/play';
import { useEditor } from './session/useEditor';
import type { WorkspaceView } from './types';
import './shell.css';

export default function App({ initialView }: { initialView?: WorkspaceView } = {}) {
  return <NoticeProvider><WorkspaceSurfaces><EditorCrashBoundary><Editor initialView={initialView} /></EditorCrashBoundary></WorkspaceSurfaces></NoticeProvider>;
}

function Editor({ initialView }: { initialView?: WorkspaceView } = {}) {
  const editor = useEditor({ initialView });
  const {
    project, view, onStartPage, loading, load, pending, setError, play, showPalette, setShowPalette, paletteCommands,
    newUntitled, open, openRecent, forgetRecent, pinRecent, recover, dismissRecoveries, appSettings, changeShowStartPage, recentProjects, run } = editor;
  const presenting = !onStartPage && !!project && isPresenting(project, play);
  return <div className={presenting ? 'application editor-shell presenting' : 'application editor-shell'}>
    {showPalette && <CommandPalette commands={paletteCommands} close={() => setShowPalette(false)} />}
    <AppBar editor={editor} />
    <SurfaceHost place="banner" />
    <ShellNotices editor={editor} />
    {loading ? <main className="startup"><p role="status">Opening your workspace…</p></main> : !view ? <main className="startup"><h1>Open the Desegnos desktop application</h1><button onClick={() => { void load(); }}>Retry workspace connection</button></main> : !project || onStartPage ? <>
      <SurfaceHost place="start" />
      <StartPage busy={pending > 0} create={template => run(() => newUntitled(template))}
        openProject={() => run(open)} showStartPageOnLaunch={appSettings.showStartPage} changeShowStartPage={value => run(() => changeShowStartPage(value))}
        recentProjects={recentProjects} openRecent={path => run(() => openRecent(path))}
        forgetRecent={path => run(() => forgetRecent(path))} pinRecent={(path, pinned) => run(() => pinRecent(path, pinned))}
        recovery={view.recoveries.length ? <RecoveryList items={view.recoveries} busy={pending > 0} recover={id => { void recover(id).catch(error => setError(errorValue(error))); }} dismiss={ids => dismissRecoveries(ids).catch(error => setError(errorValue(error)))} /> : null} />
    </> : <>
      <CanvasWorkspace editor={editor} project={project} />
      <StatusBar editor={editor} project={project} view={view} />
    </>}
    <PanelSurfaces editor={editor} />
    <EditorDialogs editor={editor} />
  </div>;
}
