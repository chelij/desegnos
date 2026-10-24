import { Notice, ProblemBanner } from './ProblemBanner.js';
import { userError } from './userErrors.js';
import { useCallback, useEffect, useRef, useState, type FormEvent } from 'react';
import { native } from './api';
import { MenuButton, type MenuItem } from './Menus';
import './dialogs.css';
import type { AiSettingsView, ProviderSettings } from './types';

function message(error: unknown) { return error instanceof Error ? error.message : String(error); }
export type AiSettingsUpdate = (command: string, args?: Record<string, unknown>) => Promise<AiSettingsView>;

export function AiSettings({ current, refresh, update, manageUsage, pollingError }: {
  current: AiSettingsView | null; refresh: () => Promise<AiSettingsView>; update: AiSettingsUpdate; manageUsage: () => Promise<void>; pollingError: string;
}) {
  const accepted = useRef<AiSettingsView | null>(null);
  const [view, setView] = useState<AiSettingsView | null>(null);
  const [settings, setSettings] = useState<ProviderSettings | null>(null);
  const [key, setKey] = useState('');
  const [imageKey, setImageKey] = useState('');
  const [showImageKey, setShowImageKey] = useState(false);
  const [imageStorage, setImageStorage] = useState<'system' | 'session'>('system');
  const [storage, setStorage] = useState<'system' | 'session'>('system');
  const [chatgptStorage, setChatgptStorage] = useState<'system' | 'session'>('system');
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [checkingImages, setCheckingImages] = useState(false);
  const [error, setError] = useState('');
  const [status, setStatus] = useState('');
  const [advanced, setAdvanced] = useState(false);
  const [replacingKey, setReplacingKey] = useState(false);
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
      setReplacingKey(false);
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
      else { setReplacingKey(false); setStatus('Key connected.'); }
    }
  }
  async function connectImageKey() {
    const entered = imageKey; setImageKey('');
    const value = await action('set_ai_key', { image: true, key: entered, storage: imageStorage });
    if (value?.imageKey.configured) setShowImageKey(false);
  }
  async function removeKey() {
    setKey('');
    const value = await action('remove_ai_key');
    if (value) {
      if (value.credential.configured) setError(value.credential.vaultError ?? 'The key could not be removed.');
      else { setReplacingKey(false); setStatus('Key removed.'); }
    }
  }
  async function testAccess() {
    setCheckingImages(true);
    setBusy(true); setError(''); setStatus('');
    try {
      const result = await native<{ message: string }>('test_ai_access');
      receive(await refresh()); setStatus(result.message ? 'Connection checked.' : 'Access checked.');
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
  const connected = usesChatgpt ? !!account?.connected : view?.credential.configured;
  const identity = usesChatgpt ? account?.email ?? account?.label ?? 'ChatGPT' : view?.credential.maskedIdentifier ?? `${providerName} API key`;
  const imageStatus = view?.imageAccessChecking || checkingImages || pending ? 'Checking…' : !provider?.capabilities.imageGeneration || view?.imageAccess?.available === 'no' ? 'Not available' : view?.imageAccess?.available === 'yes' ? 'Available' : "Couldn't check";
  return <div className="ai-settings dialog-body">
    {error && <ProblemBanner message={userError(error)} dismiss={() => setError('')} />}
    {pollingError && <ProblemBanner message={userError(pollingError)} action={{ label: 'Try again', disabled: busy, onClick: () => { void load(); } }} />}
    {loading ? <p role="status">Reading AI settings…</p> : !view || !settings ? <button onClick={() => { void load(); }}>Retry AI settings</button> : <>
      <label className="ai-provider ai-inline-field">Provider<select disabled={busy || pending || dirty} value={usesChatgpt ? 'openai-chatgpt' : view.provider} onChange={event => { setKey(''); setReplacingKey(false); setStatus(''); void chooseProvider(event.target.value); }}>
        {(view.providers ?? []).flatMap(item => item.id === 'openai' ? [
          ...(item.signInSupported ? [<option key="openai-chatgpt" value="openai-chatgpt">ChatGPT plan</option>] : []),
          <option key="openai" value="openai">API key</option>,
        ] : [<option key={item.id} value={item.id} disabled={!item.available}>{item.name}{!item.available ? ' — coming soon' : ''}</option>])}
      </select></label>
      {view.configurationError && <ProblemBanner message={userError(view.configurationError)} />}
      <section className={usesChatgpt ? 'chatgpt-accounts' : 'api-key-connection'} aria-label={usesChatgpt ? 'ChatGPT account settings' : `${providerName} key connection`}>
        <div className="ai-account-card">
          <span className="ai-avatar" aria-hidden="true">{identity.charAt(0).toUpperCase()}</span>
          <div className="ai-account-copy"><span className="ai-account-identity">{identity}</span>
            <span className="chatgpt-account-status connection-status" role="status"><span className={`ai-status-dot${connected ? ' connected' : ''}`} aria-hidden="true" />{connected ? 'Connected' : 'Not connected'}</span>
          </div>
          {usesChatgpt && view.chatgpt.accounts.length > 1 && <select aria-label="ChatGPT account" className="ai-account-switcher" value={view.chatgpt.activeAccountId ?? ''} disabled={busy || pending} onChange={event => { if (event.target.value) void action('select_chatgpt_account', { accountId: event.target.value }); }}>
            {!account && <option value="" disabled>Choose account</option>}{view.chatgpt.accounts.map((item, index) => <option key={item.id} value={item.id}>{item.email ?? item.label}{view.chatgpt.accounts.filter(other => (other.email ?? other.label) === (item.email ?? item.label)).length > 1 ? ` (${index + 1})` : ''}{!item.connected ? ' · Not connected' : !item.planEnabled ? ' · Plan permission needed' : ''}</option>)}
          </select>}
          <MenuButton className="ai-account-options" align="end" reserveSpace label="⋯" ariaLabel="Account options" title={canTest ? 'Account options' : testHelp} showChevron={false} items={[
            ...(usesChatgpt ? [
              { id: 'add', label: 'Add account', disabled: busy || pending, action: () => { void action('begin_chatgpt_sign_in', { storage: chatgptStorage, accountId: null }); } },
              ...(account ? [{ id: 'reconnect', label: 'Reconnect', disabled: busy || pending, action: () => { void action('begin_chatgpt_sign_in', { storage: chatgptStorage, accountId: account.id }); } }] : []),
            ] : view.credential.configured ? [{ id: 'replace', label: 'Replace key', disabled: busy, action: () => setReplacingKey(true) }] : []),
            { id: 'test', label: 'Test connection', disabled: busy || !canTest, action: () => { void testAccess(); } },
            { id: 'storage', label: `Where to keep the ${usesChatgpt ? 'sign-in' : 'key'}`, disabled: busy || pending, children: (['system', 'session'] as const).map(value => ({
              id: value, label: value === 'system' ? 'Save securely on this device' : 'Current session only', role: 'menuitemradio' as const, checked: (usesChatgpt ? chatgptStorage : storage) === value,
              action: () => usesChatgpt ? setChatgptStorage(value) : setStorage(value),
            })) },
            ...((usesChatgpt ? account && (account.connected || account.storage !== 'none') : view.credential.configured || view.credential.vaultError) ? [
              { id: 'separator', label: '', separator: true },
              { id: 'remove', label: usesChatgpt ? 'Sign out' : 'Remove key', disabled: busy || pending, action: () => { if (usesChatgpt && account) void action('sign_out_chatgpt', { accountId: account.id }); else void removeKey(); } },
            ] : []),
          ] satisfies MenuItem[]} />
        </div>
        {usesChatgpt ? <>
          {view.chatgpt.storageError && <ProblemBanner message={userError(view.chatgpt.storageError)} />}
          {view.chatgpt.message && !['ChatGPT connected.', 'Signed out.', 'Secure storage repaired.'].includes(view.chatgpt.message) && <ProblemBanner message={userError(view.chatgpt.message)} />}
          {account?.connected && !account.planEnabled && <ProblemBanner message="Your ChatGPT plan is not enabled for Desegnos." />}
          {account?.storageRepairRequired && <ProblemBanner message="Your saved sign-in details need attention." action={{ label: 'Repair secure storage', disabled: busy || pending, onClick: () => { void action('retry_chatgpt_secure_storage', { accountId: account.id }); } }} />}
          {pending ? <button type="button" disabled={busy} onClick={() => { void action('cancel_chatgpt_sign_in'); }}>Cancel sign-in</button> : !connected && <button type="button" className="primary" disabled={busy} onClick={() => { void action('begin_chatgpt_sign_in', { storage: chatgptStorage, accountId: account?.id ?? null }); }}>Sign in with ChatGPT</button>}
          {view.chatgpt.signIn.message && view.chatgpt.signIn.status !== 'complete' && <>{view.chatgpt.signIn.status === 'failed' ? <ProblemBanner message={userError(view.chatgpt.signIn.message)} action={{ label: 'Sign in again', disabled: busy || pending, onClick: () => { void action('begin_chatgpt_sign_in', { storage: chatgptStorage, accountId: account?.id ?? null }); } }} /> : <Notice variant="info" message={view.chatgpt.signIn.message} />}</>}
        </> : <>
          {view.credential.vaultError && <ProblemBanner message={userError(view.credential.vaultError)} />}
          {(!view.credential.configured || replacingKey) && <form className="ai-key-form" onSubmit={saveKey}>
            <label>{view.credential.configured ? 'Replacement API key' : `${providerName} API key`}<input disabled={busy || pending} type="password" value={key} onChange={event => setKey(event.target.value)} autoComplete="new-password" spellCheck={false} required autoFocus={replacingKey} /></label>
            <button className="primary" type="submit" disabled={busy || pending || !key.trim()}>{busy ? 'Connecting…' : 'Connect'}</button>
            {replacingKey && <button type="button" disabled={busy} onClick={() => { setKey(''); setReplacingKey(false); }}>Cancel</button>}
          </form>}
        </>}
      </section>
      <form className="settings-form generation-settings-form" onSubmit={saveSettings}>
        <div className="ai-model-row"><label className="ai-inline-field">Model<select disabled={busy || pending || view.catalogue.length === 0} value={settings.model} onChange={event => {
          const chosen = view.catalogue.find(model => model.id === event.target.value);
          setSettings({ ...settings, model: event.target.value, reasoningEffort: chosen?.reasoningEfforts.includes(settings.reasoningEffort) ? settings.reasoningEffort : chosen?.reasoningEfforts[0] ?? '' });
        }}>{!model && <option value={settings.model} disabled>{settings.model} · unavailable</option>}{view.catalogue.map(model => <option key={model.id} value={model.id}>{model.name}</option>)}</select></label>
          {usesChatgpt && <button type="button" className="ai-usage-link" disabled={busy} onClick={() => { void usage(); }}>Usage ↗</button>}
        </div>
        {usesChatgpt && view.chatgpt.modelsError && <ProblemBanner message={userError(view.chatgpt.modelsError)} action={{ label: 'Test connection', disabled: busy || !canTest, onClick: () => { void testAccess(); } }} />}
        {!model && <ProblemBanner message="No available AI model is selected." />}
        {(!usesChatgpt || view.imageKey.configured) && <label className="ai-inline-field ai-budget">Budget per generation (USD)<input disabled={busy || pending} type="number" required min={0.01} step={0.01} aria-label="API budget per generation (USD)" aria-describedby="ai-budget-help" value={budget ?? String(settings.spendingLimitUsd)} onChange={event => { setBudget(event.target.value); if (event.target.value.trim() && Number.isFinite(event.target.valueAsNumber)) setSettings({ ...settings, spendingLimitUsd: event.target.valueAsNumber }); }} onBlur={() => setBudget(null)} /><span id="ai-budget-help" className="field-help">Blocks estimates over budget; not a guaranteed billing cap.</span></label>}
        <section className="ai-features" aria-label="Features">
          <div className="ai-feature-row image-generation-access"><div className="ai-feature-copy"><label htmlFor="ai-image-generation">Image generation</label><div className="ai-feature-meta"><span className="ai-feature-pill" id="ai-image-status" role="status">{imageStatus}</span><button type="button" className="ai-refresh" aria-label="Check again" title="Check again" disabled={busy || !canTest || view.imageAccessChecking} onClick={() => { void testAccess(); }}>↻</button></div></div>
            <input id="ai-image-generation" className="ai-switch" type="checkbox" role="switch" aria-describedby="ai-image-status" disabled={busy || pending || checkingImages || view.imageAccessChecking || view.imageAccess?.available !== 'yes' || !connected || (usesChatgpt && !account?.planEnabled) || !provider?.capabilities.imageGeneration} checked={settings.imageGeneration} onChange={event => setSettings({ ...settings, imageGeneration: event.target.checked })} />
          </div>
          {view.provider === 'openai' && <div className="ai-feature-meta">
            {view.imageCredential && <span>{view.imageCredential === 'chatgpt' ? 'Using your ChatGPT sign-in' : view.imageCredential === 'imageKey' ? 'Using image key' : 'Using your API key'}</span>}
            {(view.imageAccess?.available !== 'yes' || view.imageKey.configured) && <button type="button" disabled={busy || pending} onClick={() => setShowImageKey(value => !value)}>{view.imageKey.configured ? 'Replace image key' : 'Add image key'}</button>}
            {view.imageKey.configured && <button type="button" disabled={busy || pending} onClick={() => { setImageKey(''); void action('remove_ai_key', { image: true }); }}>Remove image key</button>}
          </div>}
          {view.provider === 'openai' && showImageKey && <div className="ai-key-form">
            <label>Image API key<input type="password" autoComplete="new-password" spellCheck={false} disabled={busy || pending} value={imageKey} onChange={event => setImageKey(event.target.value)} onKeyDown={event => { if (event.key === 'Enter') { event.preventDefault(); if (imageKey.trim()) void connectImageKey(); } }} /></label>
            <label>Store key<select value={imageStorage} disabled={busy || pending} onChange={event => setImageStorage(event.target.value as 'system' | 'session')}><option value="system">Secure storage</option><option value="session">This session</option></select></label>
            <button type="button" disabled={busy || pending || !imageKey.trim()} onClick={() => { void connectImageKey(); }}>Connect image key</button>
          </div>}
          {view.provider === 'openai' && view.imageKey.vaultError && <ProblemBanner message={userError(view.imageKey.vaultError)} />}
          {view.provider === 'openai' && view.imageAccess?.available !== 'yes' && [view.imageKeyAccess, view.mainImageAccess].find(access => access?.reason.includes('API Organization Verification')) && <ProblemBanner message="Verify your OpenAI account to create pictures." />}
          <div className="ai-feature-row"><span>Web search</span><span className="ai-feature-pill">{provider?.capabilities.webSearch ? 'Available' : 'Not available'}</span></div>
        </section>
        <details className="ai-advanced" open={advanced} onToggle={event => setAdvanced(event.currentTarget.open)}><summary>Advanced</summary><fieldset className="advanced-settings"><legend>Model details</legend>{usesChatgpt && account && <p className="small muted">Registration: <span className="source-url">{account.id}</span></p>}{model && <div className="model-details">{!usesChatgpt && <p className="small">Standard input: USD {model.inputUsdPerMillion} / million tokens · Standard output: USD {model.outputUsdPerMillion} / million tokens</p>}
          <p className="small muted">Catalogue verified {model.verifiedAt}.{!usesChatgpt && <> Pricing source: <span className="source-url">{model.sourceUrl}</span></>}</p></div>}
        <div className={usesChatgpt ? undefined : 'field-pair'}>{!usesChatgpt && <label>Maximum output tokens<input disabled={busy} type="number" required min={1} max={32768} step={1} value={settings.maxOutputTokens} onChange={event => setSettings({ ...settings, maxOutputTokens: Number(event.target.value) })} /></label>}
          <label>Reasoning effort<select disabled={busy || pending || !model} value={settings.reasoningEffort} onChange={event => setSettings({ ...settings, reasoningEffort: event.target.value })}>{model?.reasoningEfforts.map(level => <option key={level} value={level}>{level.charAt(0).toUpperCase() + level.slice(1)}</option>)}</select></label></div>
        </fieldset></details>
        <div className="form-actions ai-save-actions">{status && <Notice variant="success" message={status} />}{dirty && <button type="button" disabled={busy || pending} onClick={() => setSettings(view.settings)}>Reset changes</button>}{dirty && <button className="primary" disabled={busy || pending || (!model && !view.configurationError)}>Save AI settings</button>}</div>
      </form>
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
  return <section aria-label="You're using your ChatGPT plan" className="dialog-body"><strong>You're using your ChatGPT plan</strong>
    <p>Eligible AI requests in Desegnos use your ChatGPT plan or credits balance. Manage usage in your ChatGPT settings.</p>
    {error && <ProblemBanner message={userError(error)} dismiss={() => setError('')} />}
    <div className="form-actions"><button disabled={busy} onClick={() => { void manageUsage().catch(error => setError(message(error))); }}>Manage usage</button><button className="primary" disabled={busy} onClick={() => { void acknowledge(); }}>Got it</button>
    </div></section>;
}
