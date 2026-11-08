import { AiSettings } from './AiSettings';
import { ModalDialog } from './ModalDialog';
import { SaveConflictDialog } from './SaveConflict';
import { UntitledSaveForm } from './dock/properties';
import { WorkspaceSurface } from './WorkspaceSurface';
import { errorValue } from './errorMessage.js';
import { native } from './api';
import type { Editor } from './session/useEditor';

/** Questions the editor puts to the user before it goes on: saving, closing, replacing, deleting. */
export function EditorDialogs({ editor }: { editor: Editor }) {
  const {
    project, view, pending, saving, closeUntitledSave, saveUntitledProject, projectLocation, closeRequest, setCloseRequest, chooseSaveConflict,
    replacement, setReplacement, save, confirmation, setConfirmation, setError, showAiSettings, setShowAiSettings, aiSettings, refreshAiSettings,
    updateAiSettings, manageChatgptUsage, aiPollingError } = editor;
  return <>
    {saving.kind !== 'idle' && saving.untitled && view?.untitled && project && <ModalDialog title="Save project" close={() => closeUntitledSave(null)}>
      <UntitledSaveForm key={project.id} project={project} location={projectLocation} busy={pending > 0}
        cancel={() => closeUntitledSave(null)}
        chooseLocation={name => native<string | null>('pick_project_save_path', { name })}
        save={saveUntitledProject} />
    </ModalDialog>}
    {closeRequest && <ModalDialog title="Close Desegnos?" close={() => { closeRequest.resolve(false); setCloseRequest(null); }}><p>{closeRequest.message}</p><div className="form-actions"><button className="primary" onClick={() => { closeRequest.resolve(false); setCloseRequest(null); }}>Keep editing</button><button className="danger" onClick={() => { closeRequest.resolve(true); setCloseRequest(null); }}>Close Desegnos</button></div></ModalDialog>}
    {saving.kind === 'conflict' && <SaveConflictDialog conflict={saving.conflict} choose={chooseSaveConflict} />}
    {replacement && <ModalDialog title="Save your changes?" close={() => setReplacement(null)}><p>Save “{project?.name}” before you {replacement.label}?</p><div className="form-actions">
      <button className="primary" disabled={pending > 0} onClick={() => { void (async () => { const saved = await save(); if (saved && !saved.dirty) { const run = replacement.run; setReplacement(null); await run(false); } })().catch(error => setError(errorValue(error))); }}>Save</button>
      <button className="danger" disabled={pending > 0} onClick={() => { const run = replacement.run; setReplacement(null); void run(true).catch(error => setError(errorValue(error))); }}>Discard</button><button onClick={() => setReplacement(null)}>Keep editing</button>
    </div></ModalDialog>}
    {confirmation && <WorkspaceSurface place="banner" title={confirmation.title} showHeader={false} close={() => setConfirmation(null)}><div className="inline-notice"><span>{confirmation.message}</span><button className={confirmation.confirmLabel ? undefined : 'danger'} disabled={pending > 0} onClick={() => { void confirmation.action().then(() => setConfirmation(null)).catch(error => setError(errorValue(error))); }}>{confirmation.confirmLabel ?? 'Delete'}</button><button onClick={() => setConfirmation(null)}>Cancel</button></div></WorkspaceSurface>}
    {showAiSettings && <ModalDialog title="AI settings" className="ai-settings-dialog" showClose close={() => setShowAiSettings(false)}><AiSettings current={aiSettings} refresh={refreshAiSettings} update={updateAiSettings} manageUsage={manageChatgptUsage} pollingError={aiPollingError} /></ModalDialog>}
  </>;
}
