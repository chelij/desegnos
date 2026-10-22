import { useCallback, useEffect, useRef, useState, type FormEvent } from 'react';
import { native } from './api';
import { Modal } from './Modal';
import './dialogs.css';
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
  const [checkingImages, setCheckingImages] = useState(false);
  const [error, setError] = useState('');
  const [status, setStatus] = useState('');
  const [advanced, setAdvanced] = useState(false);
  const [more, setMore] = useState(false);
  // Keep the typed text so a number input never shows stray leading zeros such as "010".
  const [budget, setBudget] = useState<string | null>(null);
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
    const probesImages = ['set_ai_key', 'select_chatgpt_account', 'set_ai_authentication', 'set_ai_provider'].includes(command);
    if (probesImages) setCheckingImages(true);
    try {
      const value = await update(command, args); receive(value);
      return value;
    } catch (error) {
      try { receive(await refresh()); } catch { /* Preserve the original operation error. */ }
      setError(message(error)); return null;
    }
    finally { setBusy(false); if (probesImages) setCheckingImages(false); }
  }
  async function saveSettings(event: FormEvent) {
    event.preventDefault(); if (!settings) return;
    if (!view) return;
    if (view.authentication === 'apiKey' && key.trim()) {
      const entered = key; setKey('');
      const credential = await action('set_ai_key', { key: entered, storage });
      if (!credential?.credential.configured) { if (credential) setError(credential.credential.vaultError ?? 'The key could not be connected.'); return; }
    }
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
    setCheckingImages(true);
    setBusy(true); setError(''); setStatus('');
    try {
      const result = await native<{ message: string }>('test_ai_access');
      receive(await refresh()); setStatus(result.message ? 'Connected.' : 'Access checked.');
    } catch (error) {
      const original = message(error);
      try { receive(await refresh()); } catch {}
      setError(original);
    }
    finally { setBusy(false); setCheckingImages(false); }
  }
  async function usage() {
    setBusy(true); setError('');
    try { await manageUsage(); } catch (error) { setError(message(error)); }
    finally { setBusy(false); }
  }
  // OpenAI offers a key or a ChatGPT plan sign-in; other providers use a key.
  async function chooseProvider(value: string) {
    const next = value === 'openai-chatgpt' ? 'openai' : value;
    if (next !== view?.provider && !await action('set_ai_provider', { provider: next })) return;
    if (next === 'openai') { const wanted = value === 'openai-chatgpt' ? 'chatgpt' : 'apiKey'; if (accepted.current?.authentication !== wanted) await action('set_ai_authentication', { authentication: wanted }); }
  }
  const model = view?.catalogue.find(item => item.id === settings?.model);
  const dirty = !!view && !!settings && JSON.stringify(view.settings) !== JSON.stringify(settings);
  const usesChatgpt = view?.authentication === 'chatgpt' && view.provider === 'openai';
  const provider = view?.providers?.find(item => item.id === view.provider);
  const providerName = provider?.name ?? 'OpenAI';
  const pending = view?.chatgpt.signIn.status === 'awaitingBrowser';
  const account = view?.chatgpt.accounts.find(item => item.id === view.chatgpt.activeAccountId);
  const canTest = !!view && !dirty && !view.configurationError && !pending && (usesChatgpt ? !!account?.connected && account.planEnabled && !account.storageRepairRequired : !!model && view.credential.configured && !key.trim());
  const testHelp = pending ? 'Finish signing in through your browser first.' : key.trim() ? 'Connect the key above before testing access.' : dirty || view?.configurationError ? 'Save your settings below before testing access.' : usesChatgpt ? !account?.connected ? 'Sign in above to test your connection.' : !account.planEnabled ? 'Allow ChatGPT plan usage when signing in.' : account.storageRepairRequired ? 'Repair secure storage above before testing access.' : 'Checks your connection without generating.' : !view?.credential.configured ? `Add and connect a ${providerName} API key above first.` : 'Checks your connection without generating.';
  return <div className="ai-settings dialog-body">
    {error && <p className="message error" role="alert">{error}</p>}
    {pollingError && <p className="message error" role="alert">{pollingError}</p>}
    {status && <p className="operation-status" role="status">{status}</p>}
    {loading ? <p role="status">Reading AI settings…</p> : !view || !settings ? <button onClick={() => { void load(); }}>Retry AI settings</button> : <>
      <label>AI provider<select disabled={busy || pending || dirty} value={view.provider === 'openai' && usesChatgpt ? 'openai-chatgpt' : view.provider} onChange={event => { setKey(''); void chooseProvider(event.target.value); }}>
        {(view.providers ?? []).flatMap(item => item.id === 'openai' ? [
          <option key="openai" value="openai">OpenAI (API key)</option>,
          ...(item.signInSupported ? [<option key="openai-chatgpt" value="openai-chatgpt">ChatGPT plan (sign in)</option>] : []),
        ] : [<option key={item.id} value={item.id} disabled={!item.available}>{item.available ? `${item.name} (API key)` : `${item.name} — coming soon`}</option>])}
      </select></label>
      {provider && !provider.capabilities.webSearch && <p className="field-help">Web search unavailable.</p>}
      <div className="image-generation-access"><label><input type="checkbox" disabled={busy || pending || view.imageAccessChecking || view.imageAccess?.available !== 'yes' || (usesChatgpt ? !account?.connected || !account.planEnabled : !view.credential.configured) || !provider?.capabilities.imageGeneration} checked={settings.imageGeneration} onChange={event => setSettings({ ...settings, imageGeneration: event.target.checked })} /> Image generation</label>
        <span role="status">{view.imageAccessChecking || checkingImages || pending ? 'Checking…' : view.imageAccess?.available === 'yes' ? 'Available' : view.imageAccess?.available === 'no' || !provider?.capabilities.imageGeneration ? 'Not available for this account' : "Couldn't check"}</span>
        <button type="button" disabled={busy || !canTest || view.imageAccessChecking} onClick={() => { void testAccess(); }}>Check again</button>
      </div>
      {dirty && <p className="field-help">Save or reset your changes below before changing the generation method.</p>}
      {view.configurationError && <p className="message error" role="alert">{view.configurationError} Review and save your generation settings before sending a request.</p>}
      {usesChatgpt && <section className="chatgpt-accounts" aria-label="ChatGPT account settings">
        <h3>Use your ChatGPT plan</h3><p className="small muted">Generation counts toward your plan or credits.</p>
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
        </> : <p>No ChatGPT account connected.</p>}
        <details className="key-storage"><summary>Where to keep the sign-in</summary><fieldset className="chatgpt-storage" disabled={busy || pending}><legend>Sign-in storage</legend><label className="check-label"><input type="radio" name="chatgptStorage" checked={chatgptStorage === 'system'} onChange={() => setChatgptStorage('system')} />Save securely on this device</label>
          <label className="check-label"><input type="radio" name="chatgptStorage" checked={chatgptStorage === 'session'} onChange={() => setChatgptStorage('session')} />Current session only</label></fieldset>
        </details>
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
      {!usesChatgpt && <section className="api-key-connection" aria-label={`${providerName} key connection`}>
        <h3>Your {providerName} key</h3>
        <p className="connection-status">{view.credential.configured ? `Connected · ${view.credential.storage === 'system' ? 'Saved securely on this device' : 'Current session only'}` : 'Not connected'}{advanced && view.credential.maskedIdentifier && <span className="masked-key"> {view.credential.maskedIdentifier}</span>}</p>
        {view.credential.vaultError && <p className="message warning" role="alert">{view.credential.vaultError} You can choose current-session storage below.</p>}
        <form className="settings-form" onSubmit={saveKey}>
          <label>{view.credential.configured ? 'Replacement API key' : `${providerName} API key`}<input disabled={busy || pending} type="password" value={key} onChange={event => setKey(event.target.value)} autoComplete="new-password" spellCheck={false} required /></label>
          <details className="key-storage"><summary>Where to keep the key</summary><fieldset disabled={busy || pending}><legend>Keep this key</legend><label className="check-label"><input type="radio" name="keyStorage" checked={storage === 'system'} onChange={() => setStorage('system')} />Save securely on this device</label>
            <label className="check-label"><input type="radio" name="keyStorage" checked={storage === 'session'} onChange={() => setStorage('session')} />Current session only</label></fieldset>
          </details>
          <div className="inline-actions"><button type="submit" disabled={busy || pending || !key.trim()}>{busy ? 'Connecting…' : view.credential.configured ? 'Update key' : 'Connect key'}</button>
            {(view.credential.configured || view.credential.vaultError) && <button type="button" className="danger" disabled={busy || pending} onClick={() => { void removeKey(); }}>Remove key</button>}</div>
        </form>
      </section>}
      {(usesChatgpt ? !!account?.connected : view.credential.configured) && <section className="ai-access-test" aria-label="Check AI connection"><button type="button" disabled={busy || !canTest} onClick={() => { void testAccess(); }}>Test access</button>{!canTest && <p className="field-help">{testHelp}</p>}</section>}
      <details className="ai-more" open={more || dirty || !!view.configurationError} onToggle={event => setMore(event.currentTarget.open)}><summary>Model and spending</summary>
      <form className="settings-form generation-settings-form" onSubmit={saveSettings}>
        {usesChatgpt && account?.connected && account.planEnabled && <div className="plan-usage"><span>Using ChatGPT plan</span><button type="button" disabled={busy} onClick={() => { void usage(); }}>Manage usage</button></div>}
        <label>Model<select disabled={busy || pending || view.catalogue.length === 0} value={settings.model} onChange={event => {
          const chosen = view.catalogue.find(model => model.id === event.target.value);
          setSettings({ ...settings, model: event.target.value, reasoningEffort: chosen?.reasoningEfforts.includes(settings.reasoningEffort) ? settings.reasoningEffort : chosen?.reasoningEfforts[0] ?? '' });
        }}>{!model && <option value={settings.model} disabled>{settings.model} · unavailable</option>}{view.catalogue.map(model => <option key={model.id} value={model.id}>{model.name}</option>)}</select></label>
        {usesChatgpt && view.chatgpt.modelsError && <p className="message error" role="alert">{view.chatgpt.modelsError}</p>}
        {!model && <p className="field-error" role="alert">{usesChatgpt ? !account?.connected ? 'Sign in to see available models.' : !account.planEnabled ? 'Allow ChatGPT plan usage to continue.' : 'No supported models are available. Test access to refresh.' : 'Choose an available model.'}</p>}
        {!usesChatgpt && <><label>API budget per generation (USD)<input disabled={busy || pending} type="number" required min={0.01} step={0.01} value={budget ?? String(settings.spendingLimitUsd)} onChange={event => { setBudget(event.target.value); if (event.target.value.trim() && Number.isFinite(event.target.valueAsNumber)) setSettings({ ...settings, spendingLimitUsd: event.target.valueAsNumber }); }} onBlur={() => setBudget(null)} /></label>
          <p className="field-help">Generation is blocked if its estimate exceeds this budget; this is not a guaranteed billing cap.</p></>}
        <details className="ai-advanced" open={advanced} onToggle={event => setAdvanced(event.currentTarget.open)}><summary>Advanced settings</summary><fieldset className="advanced-settings"><legend>Model details</legend>{model && <div className="model-details">{!usesChatgpt && <p className="small">Standard input: USD {model.inputUsdPerMillion} / million tokens · Standard output: USD {model.outputUsdPerMillion} / million tokens</p>}
          <p className="small muted">Catalogue verified {model.verifiedAt}.{!usesChatgpt && <> Pricing source: <span className="source-url">{model.sourceUrl}</span></>}</p></div>}
        <div className={usesChatgpt ? undefined : 'field-pair'}>{!usesChatgpt && <label>Maximum output tokens<input disabled={busy} type="number" required min={1} max={32768} step={1} value={settings.maxOutputTokens} onChange={event => setSettings({ ...settings, maxOutputTokens: Number(event.target.value) })} /></label>}
          <label>Reasoning effort<select disabled={busy || pending || !model} value={settings.reasoningEffort} onChange={event => setSettings({ ...settings, reasoningEffort: event.target.value })}>{model?.reasoningEfforts.map(level => <option key={level} value={level}>{level.charAt(0).toUpperCase() + level.slice(1)}</option>)}</select></label></div>
        </fieldset></details>
        {advanced && !usesChatgpt && <p className="field-help">Remove key clears saved and current-session keys.</p>}
        <div className="form-actions ai-save-actions">{dirty && <button type="button" disabled={busy || pending} onClick={() => setSettings(view.settings)}>Reset changes</button>}<button className="primary" disabled={busy || pending || (!model && !view.configurationError)}>Save AI settings</button></div>
      </form></details>
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
    <div className="form-actions"><button disabled={busy} onClick={() => { void manageUsage().catch(error => setError(message(error))); }}>Manage usage</button><button className="primary" disabled={busy} onClick={() => { void acknowledge(); }}>Got it</button>
    </div></div></Modal>;
}
