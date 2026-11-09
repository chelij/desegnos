import { CrashReportNotice } from './CrashReport';
import { errorValue } from './errorMessage.js';
import { Notice, NoticeHost } from './ProblemBanner.js';
import { native } from './api';
import type { Editor } from './session/useEditor';
import { userError } from './userErrors.js';

/** Everything that reports to the user above the workspace: errors, update results, toasts and recovery warnings. */
export function ShellNotices({ editor }: { editor: Editor }) {
  const {
    onStartPage, error, setError, problemAction, openFailure, setOpenFailure, setNotice, updateState, dismissUpdate, installUpdate, openRelease,
    notices, dismissNotice, view, dismissedRecovery, setDismissedRecovery } = editor;
  return <>
    {onStartPage && <NoticeHost />}
    {error && <Notice variant="error" message={userError(error)} dismiss={() => setError('')} action={problemAction} />}
    {openFailure && <Notice variant="warning" title="Couldn't open your browser" message={openFailure.message} action={{ label: 'Copy link', onClick: () => { const url = openFailure.url; void native('copy_text', { text: url }).then(() => setNotice('Link copied.')).catch(error => setError(errorValue(error))); } }} dismiss={() => setOpenFailure(null)} />}
    {updateState.kind === 'done' && <Notice variant={updateState.result.variant} message={updateState.result.message} action={updateState.result.release ? { label: updateState.result.installSupported ? 'Install and restart' : 'Download', onClick: () => { if (updateState.result.installSupported) void installUpdate(); else if (updateState.result.release) void openRelease(updateState.result.release.url); } } : undefined} dismiss={dismissUpdate} />}
    {updateState.kind === 'installing' && <Notice variant="info" message={updateState.message} action={{ label: 'Install and restart', onClick: () => {}, disabled: true }} showDismiss={false} />}
    {updateState.kind === 'failed' && <Notice variant="error" message={userError(updateState.message)} action={updateState.download ? { label: 'Download', onClick: () => { if (updateState.download) void openRelease(updateState.download); } } : undefined} dismiss={dismissUpdate} />}
    {notices.map(item => <Notice key={item.id} variant="info" message={item.message} undo={item.undo ? { label: 'Undo', onClick: item.undo } : undefined} dismiss={() => dismissNotice(item.id)} />)}
    <CrashReportNotice />
    {view?.recoveryError && !dismissedRecovery.includes(`error:${view.recoveryError}`) && <Notice variant="error" message={userError(view.recoveryError)} dismiss={() => setDismissedRecovery(items => [...items, `error:${view.recoveryError}`])} />}
  </>;
}
