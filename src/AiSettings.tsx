import { useCallback, useEffect, useRef, useState, type FormEvent } from 'react';
import { native } from './api';
import { Modal } from './Modal';
import { MenuButton } from './Menus';
import type { AiSettingsView, AuthenticationMethod, ProviderSettings } from './types';

function message(error: unknown) { return error instanceof Error ? error.message : String(error); }
export type AiSettingsUpdate = (command: string, args?: Record<string, unknown>) => Promise<AiSettingsView>;

export function AiSettings({ current, refresh, update, manageUsage, pollingError }: {
  current: AiSettingsView | null; refresh: () => Promise<AiSettingsView>; update: AiSettingsUpdate; manageUsage: () => Promise<void>; pollingError: string;
}) {
  const accepted = useRef<AiSettingsView | null>(null);
  const [view, setView] = useState<AiSettingsView | null>(null);
  const [settings, setSettings] = useState<ProviderSettings | null>(null);
  const [key, setKey] = useState('');
  const [storage, setStorage] = useState<'system' | 'session'>('system');
  const [chatgptStorage, setChatgptStorage] = useState<'system' | 'session'>('system');
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [status, setStatus] = useState('');
  const [advanced, setAdvanced] = useState(false);
  const receive = useCallback((value: AiSettingsView, reset = false) => {
    const previous = accepted.current;
    accepted.current = value; setView(value);
    setSettings(draft => reset || !draft || JSON.stringify(draft) === JSON.stringify(previous?.settings) ? value.settings : draft);
  }, []);
  useEffect(() => { if (current && current !== accepted.current) receive(current); }, [current, receive]);
  const load = useCallback(async () => {
    setLoading(true); setError('');
    try { receive(await refresh()); }
    catch (error) { setError(message(error)); }
    finally { setLoading(false); }
  }, [receive, refresh]);
  useEffect(() => { void load(); }, [load]);
  async function action(command: string, args?: Record<string, unknown>) {
    setBusy(true); setError(''); setStatus('');
    try {
      const value = await update(command, args); receive(value);
      return value;
    } catch (error) {
      try { receive(await refresh()); } catch { /* Preserve the original operation error. */ }
      setError(message(error)); return null;
    }
    finally { setBusy(false); }
  }
  async function saveSettings(event: FormEvent) {
    event.preventDefault(); if (!settings) return;
    const value = await action('save_ai_settings', { settings });
    if (value) { receive(value, true); if (!value.configurationError) setStatus('AI settings saved.'); }
  }
  async function saveKey(event: FormEvent) {
    event.preventDefault();
    const entered = key; setKey('');
    const value = await action('set_ai_key', { key: entered, storage });
    if (value) {
      if (!value.credential.configured) setError(value.credential.vaultError ?? 'The key was not configured.');
      else setStatus(value.credential.storage === 'system' ? 'Key saved on this device.' : 'Key saved for this session.');
    }
  }
  async function removeKey() {
    setKey('');
    const value = await action('remove_ai_key');
    if (value) {
      if (value.credential.configured) setError(value.credential.vaultError ?? 'The key could not be removed.');
      else setStatus('Key removed from Desegnos.');
    }
  }
  async function testAccess() {
    setBusy(true); setError(''); setStatus('');
    try {
      const result = await native<{ message: string }>('test_ai_access');
      receive(await refresh()); setStatus(result.message ? 'Connected. Ready to generate.' : 'Access checked.');
    } catch (error) {
      const original = message(error);
      try { receive(await refresh()); } catch {}
      setError(original);
    }
    finally { setBusy(false); }
  }
  async function usage() {
    setBusy(true); setError('');
    try { await manageUsage(); } catch (error) { setError(message(error)); }
    finally { setBusy(false); }
  }
  const model = view?.catalogue.find(item => item.id === settings?.model);
  const dirty = !!view && !!settings && JSON.stringify(view.settings) !== JSON.stringify(settings);
  const usesChatgpt = view?.authentication === 'chatgpt';
  const pending = view?.chatgpt.signIn.status === 'awaitingBrowser';
  const account = view?.chatgpt.accounts.find(item => item.id === view.chatgpt.activeAccountId);
  const canTest = !!view && !dirty && !!model && !view.configurationError && (usesChatgpt ? !!account?.connected && account.planEnabled && !pending : view.credential.configured && !key);
  return <div className="ai-settings dialog-body">
    <div className="settings-heading"><p>Connect OpenAI to create your site.</p><MenuButton label="More" ariaLabel="AI settings options" items={[{ id: 'advanced', label: 'Advanced settings', checked: advanced, action: () => setAdvanced(value => !value) }]} /></div>
    {error && <p className="message error" role="alert">{error}</p>}
    {pollingError && <p className="message error" role="alert">{pollingError}</p>}
    {status && <p className="operation-status" role="status">{status}</p>}
    {loading ? <p role="status">Reading AI settings…</p> : !view || !settings ? <button onClick={() => { void load(); }}>Retry AI settings</button> : <>
      <fieldset className="authentication-method" disabled={busy || pending || dirty}><legend>Generation method</legend>
        <div className="inline-actions">{(['apiKey', 'chatgpt'] as AuthenticationMethod[]).map(method => <label className="check-label" key={method}>
          <input type="radio" name="authentication" value={method} checked={view.authentication === method} onChange={() => { setKey(''); void action('set_ai_authentication', { authentication: method }); }} />{method === 'apiKey' ? 'API key' : 'ChatGPT plan'}
        </label>)}</div>
      </fieldset>
      {dirty && <p className="field-help">Save or reset your settings before changing the generation method.</p>}
      {view.configurationError && <p className="message error" role="alert">{view.configurationError} Review and save your generation settings before sending a request.</p>}
      {usesChatgpt && <section className="chatgpt-accounts" aria-label="ChatGPT account settings">
        <h3>Use your ChatGPT plan</h3><p className="small muted">Sign in through your browser. Generation counts toward your plan or credits.</p>
        {view.chatgpt.storageError && <p className="message error" role="alert">{view.chatgpt.storageError}</p>}
        {view.chatgpt.message && <p className="small" role="status">{view.chatgpt.message}</p>}
        {view.chatgpt.accounts.length > 0 ? <>
          <label>ChatGPT account<select value={view.chatgpt.activeAccountId ?? ''} disabled={busy || pending} onChange={event => { if (event.target.value) void action('select_chatgpt_account', { accountId: event.target.value }); }}>
            {!account && <option value="" disabled>Choose a saved account</option>}{view.chatgpt.accounts.map((item, index) => <option key={item.id} value={item.id}>{item.email ?? item.label}{view.chatgpt.accounts.filter(other => (other.email ?? other.label) === (item.email ?? item.label)).length > 1 ? ` (${index + 1})` : ''}{!item.connected ? ' · Disconnected' : !item.planEnabled ? ' · Plan permission needed' : ''}</option>)}
          </select></label>
          {account && <div className="chatgpt-account-status"><p>{account.connected ? account.planEnabled ? 'Connected · ChatGPT plan enabled' : 'Connected · Plan usage not authorized' : 'Disconnected'} · {account.storage === 'system' ? 'Saved on this device' : account.storage === 'session' ? 'Current session only' : 'Sign in to reconnect'}</p>
            {advanced && <p className="small muted">Registration: <span className="source-url">{account.id}</span></p>}
            {account.storageRepairRequired && <><p className="message warning" role="alert">Keep Desegnos open and repair secure storage to continue.</p>
              <div className="inline-actions"><button className="primary" disabled={busy || pending} onClick={() => { void action('retry_chatgpt_secure_storage', { accountId: account.id }); }}>Repair secure storage</button></div></>}
          </div>}
        </> : <p>No ChatGPT account is connected. Continue with ChatGPT to add one.</p>}
        <fieldset className="chatgpt-storage" disabled={busy || pending}><legend>Sign-in storage</legend><label className="check-label"><input type="radio" name="chatgptStorage" checked={chatgptStorage === 'system'} onChange={() => setChatgptStorage('system')} />System credential manager</label>
          <label className="check-label"><input type="radio" name="chatgptStorage" checked={chatgptStorage === 'session'} onChange={() => setChatgptStorage('session')} />Current session only</label></fieldset>
        <p className="field-help">Current-session sign-in ends when Desegnos closes.</p>
        <div className="inline-actions">
          {pending ? <button disabled={busy} onClick={() => { void action('cancel_chatgpt_sign_in'); }}>Cancel sign-in</button> : <>
            {!account && <button className="primary" disabled={busy} onClick={() => { void action('begin_chatgpt_sign_in', { storage: chatgptStorage, accountId: null }); }}>Continue with ChatGPT</button>}
            {account && <><button disabled={busy} onClick={() => { void action('begin_chatgpt_sign_in', { storage: chatgptStorage, accountId: account.id }); }}>Reconnect with ChatGPT</button>
              <button disabled={busy} onClick={() => { void action('begin_chatgpt_sign_in', { storage: chatgptStorage, accountId: null }); }}>Add account</button>
              {(account.connected || account.storage !== 'none') && <button className="danger" disabled={busy} onClick={() => { void action('sign_out_chatgpt', { accountId: account.id }); }}>Sign out</button>}</>}
          </>}
          {(!account?.connected || !account.planEnabled) && <button disabled={busy} onClick={() => { void usage(); }}>Manage usage</button>}
        </div>
        {view.chatgpt.signIn.message && <p className={view.chatgpt.signIn.status === 'failed' ? 'message error' : 'operation-status'} role={view.chatgpt.signIn.status === 'failed' ? 'alert' : 'status'}>{view.chatgpt.signIn.message}</p>}
        {pending && <p className="field-help" role="status">Finish signing in through your browser.</p>}
      </section>}
      <form className="settings-form" onSubmit={saveSettings}><h3>Generation settings</h3>
        {usesChatgpt && account?.connected && account.planEnabled && <div className="plan-usage"><span>Using ChatGPT plan</span><button type="button" disabled={busy} onClick={() => { void usage(); }}>Manage usage</button></div>}
        <label>Model<select disabled={busy || pending || view.catalogue.length === 0} value={settings.model} onChange={event => {
          const chosen = view.catalogue.find(model => model.id === event.target.value);
          setSettings({ ...settings, model: event.target.value, reasoningEffort: chosen?.reasoningEfforts.includes(settings.reasoningEffort) ? settings.reasoningEffort : chosen?.reasoningEfforts[0] ?? '' });
        }}>{!model && <option value={settings.model} disabled>{settings.model} · unavailable</option>}{view.catalogue.map(model => <option key={model.id} value={model.id}>{model.name}</option>)}</select></label>
        {usesChatgpt && view.chatgpt.modelsError && <p className="message error" role="alert">{view.chatgpt.modelsError}</p>}
        {!model && <p className="field-error" role="alert">{usesChatgpt ? !account?.connected ? 'Sign in to see available models.' : !account.planEnabled ? 'Allow ChatGPT plan usage to continue.' : 'No supported models are available. Test access to refresh.' : 'Choose an available model.'}</p>}
        {advanced && <fieldset className="advanced-settings"><legend>Advanced settings</legend>{model && <div className="model-details">{!usesChatgpt && <p className="small">Standard input: USD {model.inputUsdPerMillion} / million tokens · Standard output: USD {model.outputUsdPerMillion} / million tokens</p>}
          <p className="small muted">Catalogue verified {model.verifiedAt}.{!usesChatgpt && <> Pricing source: <span className="source-url">{model.sourceUrl}</span></>}</p></div>}
        <div className={usesChatgpt ? undefined : 'field-pair'}>{!usesChatgpt && <label>Maximum output tokens<input disabled={busy} type="number" required min={1} max={32768} step={1} value={settings.maxOutputTokens} onChange={event => setSettings({ ...settings, maxOutputTokens: Number(event.target.value) })} /></label>}
          <label>Reasoning effort<select disabled={busy || pending || !model} value={settings.reasoningEffort} onChange={event => setSettings({ ...settings, reasoningEffort: event.target.value })}>{model?.reasoningEfforts.map(level => <option key={level} value={level}>{level.charAt(0).toUpperCase() + level.slice(1)}</option>)}</select></label></div>
        </fieldset>}
        {!usesChatgpt && <><label>Estimated request allowance (USD)<input disabled={busy} type="number" required min={0.01} step={0.01} value={settings.spendingLimitUsd} onChange={event => setSettings({ ...settings, spendingLimitUsd: Number(event.target.value) })} /></label>
          <p className="field-help">Requests above this estimate need a higher allowance. Final charges may vary.</p></>}
        <div className="form-actions"><button className="primary" disabled={busy || pending || (!model && !view.configurationError) || (!dirty && !view.configurationError)}>Save AI settings</button>
          {dirty && <button type="button" disabled={busy} onClick={() => setSettings(view.settings)}>Reset unsaved settings</button>}</div>
      </form>
      {usesChatgpt ? <section className="credential-status"><h3>ChatGPT access</h3><button disabled={busy || pending || !account?.connected || !account.planEnabled || account.storageRepairRequired || dirty || !!view.configurationError} onClick={() => { void testAccess(); }}>Test access</button><p className="field-help">Checks your connection without generating.{dirty ? ' Save your settings first.' : ''}</p></section> : <>
        <section className="credential-status" aria-label="Saved key status"><h3>Your key</h3>
          <p>{view.credential.configured ? `Connected · ${view.credential.storage === 'system' ? 'Saved on this device' : 'Current session'}` : 'Add a key to connect.'}{advanced && view.credential.maskedIdentifier && <span className="masked-key"> {view.credential.maskedIdentifier}</span>}</p>
          {view.credential.vaultError && <p className="message warning">{view.credential.vaultError} You can choose session storage below.</p>}
          <div className="inline-actions"><button disabled={busy || !canTest} onClick={() => { void testAccess(); }}>Test access</button>
            {(view.credential.configured || view.credential.vaultError) && <button className="danger" disabled={busy} onClick={() => { void removeKey(); }}>Remove key</button>}</div>
          <p className="field-help">Checks your connection without generating.{dirty ? ' Save your settings first.' : ''}</p>
        </section>
        <form className="settings-form" onSubmit={saveKey}><label>{view.credential.configured ? 'Replacement API key' : 'OpenAI API key'}<input disabled={busy} type="password" value={key} onChange={event => setKey(event.target.value)} autoComplete="new-password" spellCheck={false} required /></label>
          <fieldset disabled={busy}><legend>Key storage</legend><label className="check-label"><input type="radio" name="keyStorage" checked={storage === 'system'} onChange={() => setStorage('system')} />System credential manager</label>
            <label className="check-label"><input type="radio" name="keyStorage" checked={storage === 'session'} onChange={() => setStorage('session')} />Current session only</label></fieldset>
          <p className="field-help">Your key stays private in Desegnos.</p>
          {advanced && <p className="field-help">A session key overrides a stored key until Desegnos closes. Remove key clears both.</p>}
          <button className="primary" disabled={busy || !key.trim()}>{busy ? 'Saving…' : view.credential.configured ? 'Replace key' : 'Save key'}</button>
        </form>
      </>}
    </>}
  </div>;
}

export function ChatgptWelcome({ update, manageUsage, dismissed }: { update: AiSettingsUpdate; manageUsage: () => Promise<void>; dismissed: () => void }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  async function acknowledge() {
    if (busy) return;
    setBusy(true); setError('');
    try { await update('acknowledge_chatgpt_welcome'); dismissed(); }
    catch (error) { setError(message(error)); }
    finally { setBusy(false); }
  }
  return <Modal title="You're using your ChatGPT plan" close={dismissed}><div className="dialog-body">
    <p>Eligible AI requests in Desegnos use your ChatGPT plan or credits balance. Manage usage in your ChatGPT settings.</p>
    {error && <p className="message error" role="alert">{error}</p>}
    <div className="form-actions"><button className="primary" disabled={busy} onClick={() => { void acknowledge(); }}>Got it</button>
      <button disabled={busy} onClick={() => { void manageUsage().catch(error => setError(message(error))); }}>Manage usage</button>
    </div></div></Modal>;
}
