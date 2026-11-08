import { errorMessage } from './errorMessage.js';
import type { CommandName } from './generated/commands';
import { Notice } from './ProblemBanner.js';
import { userError, providerProblem } from './userErrors.js';
import { useCallback, useEffect, useRef, useState, type FormEvent } from 'react';
import { native } from './api';
import { MenuButton, type MenuItem } from './Menus';
import { Icon } from './icons';
import { Disclosure } from './Disclosure';
import { StatusChip } from './StatusChip';
import './dialogs.css';
import type { AccessResult, AiSettingsView, ProviderSettings, ProviderError, ProviderId } from './types';
import { DEFAULT_LOCAL_ENDPOINT, MAX_HTTP_URL_BYTES } from './generated/limits';

// A model that offers only some levels keeps the request as strong as the user asked for,
// rather than dropping to its weakest one.
const reasoningOrder = ['minimal', 'low', 'medium', 'high', 'xhigh', 'max'];
function nearestReasoningEffort(offered: string[], current: string): string {
  if (!offered.length || offered.includes(current)) return current;
  // Custom levels from another provider have no place in this order; advertise their first.
  if (!reasoningOrder.includes(current) || offered.some(level => !reasoningOrder.includes(level))) return offered[0];
  const distance = (level: string) => Math.abs(reasoningOrder.indexOf(level) - reasoningOrder.indexOf(current));
  // On a tie the stronger effort wins, so raising a model's ceiling never weakens the request.
  return offered.reduce((best, level) => distance(level) < distance(best) || (distance(level) === distance(best) && reasoningOrder.indexOf(level) > reasoningOrder.indexOf(best)) ? level : best);
}
export type AiSettingsUpdate = (command: CommandName, args?: Record<string, unknown>) => Promise<AiSettingsView>;
// What the AI settings form is doing right now; it never does two things at once.
type Activity = 'loading' | 'idle' | 'saving' | 'checking' | 'checkingImages' | 'managingUsage';
// A single message slot, so a stale notice never outlives the operation that follows it.
type Feedback = { kind: 'none' } | { kind: 'status'; message: string } | { kind: 'error'; message: string; access?: boolean; providerError?: ProviderError; provider?: ProviderId };

export function AiSettings({ current, refresh, update, manageUsage, pollingError }: {
  current: AiSettingsView | null; refresh: (refreshModels?: boolean) => Promise<AiSettingsView>; update: AiSettingsUpdate; manageUsage: () => Promise<void>; pollingError: string;
}) {
  const accepted = useRef<AiSettingsView | null>(null);
  const accountSettings = useRef<HTMLElement>(null);
  const showAccountSettings = () => { accountSettings.current?.scrollIntoView({ block: 'nearest' }); accountSettings.current?.focus(); };
  const [view, setView] = useState<AiSettingsView | null>(null);
  const [settings, setSettings] = useState<ProviderSettings | null>(null);
  const [key, setKey] = useState('');
  const [imageKey, setImageKey] = useState('');
  const [showImageKey, setShowImageKey] = useState(false);
  const [imageStorage, setImageStorage] = useState<'system' | 'session'>('system');
  const [storage, setStorage] = useState<'system' | 'session'>('system');
  const [chatgptStorage, setChatgptStorage] = useState<'system' | 'session'>('system');
  const [activity, setActivity] = useState<Activity>('loading');
  const [feedback, setFeedback] = useState<Feedback>({ kind: 'none' });
  const [advanced, setAdvanced] = useState(false);
  const [replacingKey, setReplacingKey] = useState(false);
  // Keep the typed text so a number input never shows stray leading zeros such as "010".
  const [budget, setBudget] = useState<string | null>(null);
  const [endpoint, setEndpoint] = useState<string | null>(null);
  const receive = useCallback((value: AiSettingsView, reset = false) => {
    const previous = accepted.current;
    accepted.current = value; setView(value);
    setSettings(draft => {
      const changedConnection = previous?.provider !== value.provider || previous?.authentication !== value.authentication || previous?.chatgpt.activeAccountId !== value.chatgpt.activeAccountId;
      let next = reset || changedConnection || !draft || JSON.stringify(draft) === JSON.stringify(previous?.settings) ? value.settings : draft;
      if (value.provider === 'local') next = { ...next, endpoint: value.settings.endpoint, localAuthentication: value.settings.localAuthentication };
      const model = value.catalogue.find(model => model.id === next.model);
      if (value.authentication === 'apiKey' && model?.defaultPricing) {
        const quoted = model.defaultPricing;
        const prior = next.pricing?.model === model.id ? next.pricing : undefined;
        next = { ...next, pricing: { model: model.id,
          inputUsdPerMillion: Math.max(prior?.inputUsdPerMillion ?? 0, quoted.inputUsdPerMillion ?? 0),
          outputUsdPerMillion: Math.max(prior?.outputUsdPerMillion ?? 0, quoted.outputUsdPerMillion ?? 0),
          requestUsd: Math.max(prior?.requestUsd ?? 0, quoted.requestUsd ?? 0),
          imageUsd: Math.max(prior?.imageUsd ?? 0, quoted.imageUsd ?? 0) } };
      }
      const reasoning = model && !model.reasoningEfforts.includes(next.reasoningEffort) && model.reasoningEfforts.length
        ? { ...next, reasoningEffort: nearestReasoningEffort(model.reasoningEfforts, next.reasoningEffort) }
        : next;
      return value.authentication === 'apiKey' && model?.maxOutputTokens && reasoning.maxOutputTokens > model.maxOutputTokens
        ? { ...reasoning, maxOutputTokens: model.maxOutputTokens } : reasoning;
    });
  }, []);
  useEffect(() => { if (current && current !== accepted.current) receive(current); }, [current, receive]);
  useEffect(() => { setEndpoint(null); }, [view?.provider]);
  // Every operation announces what it is doing, and reports its own failure.
  const perform = useCallback(async <T,>(next: Activity, task: () => Promise<T>): Promise<T | undefined> => {
    setActivity(next); setFeedback({ kind: 'none' });
    try { return await task(); }
    catch (error) { setFeedback({ kind: 'error', message: errorMessage(error) }); return undefined; }
    finally { setActivity('idle'); }
  }, []);
  const load = useCallback(() => perform('loading', async () => { receive(await refresh(true)); }), [perform, receive, refresh]);
  useEffect(() => { void load(); }, [load]);
  async function action(command: CommandName, args?: Record<string, unknown>) {
    // These commands re-probe image access, so the image status reads "Checking…" while they run.
    const probesImages = ['set_ai_key', 'select_chatgpt_account', 'set_ai_authentication', 'set_ai_provider'].includes(command);
    return perform(probesImages ? 'checkingImages' : 'saving', async () => {
      try { const value = await update(command, args); receive(value); return value; }
      catch (error) {
        try { receive(await refresh()); } catch (refreshError) { throw new Error(`${errorMessage(error)} Settings could not be refreshed: ${errorMessage(refreshError)}`); }
        throw error;
      }
    });
  }
  async function saveSettings(event: FormEvent) {
    event.preventDefault(); if (!settings) return;
    if (!view) return;
    if (view.authentication === 'apiKey' && key.trim()) {
      const entered = key; setKey('');
      const credential = await action('set_ai_key', { key: entered, storage });
      if (!credential?.credential.configured) { if (credential) setFeedback({ kind: 'error', message: credential.credential.vaultError ?? 'The key could not be connected.' }); return; }
      setReplacingKey(false);
    }
    const value = await action('save_ai_settings', { settings });
    if (value) { receive(value, true); if (!value.configurationError) setFeedback({ kind: 'status', message: 'AI settings saved.' }); }
  }
  async function connectServer(event: FormEvent) {
    event.preventDefault(); if (!view) return;
    const value = await action('save_ai_settings', { settings: { ...view.settings, endpoint: endpoint ?? view.settings.endpoint ?? DEFAULT_LOCAL_ENDPOINT }, connectionOnly: true });
    if (value) { receive(value, true); setEndpoint(null); }
  }
  function setPrice(field: 'inputUsdPerMillion' | 'outputUsdPerMillion' | 'requestUsd' | 'imageUsd', text: string) {
    setSettings(current => current ? { ...current, pricing: {
      model: current.model, inputUsdPerMillion: null, outputUsdPerMillion: null,
      ...current.pricing, [field]: text === '' ? null : Number(text) } as ProviderSettings['pricing'] } : current);
  }
  async function saveKey(event: FormEvent) {
    event.preventDefault();
    const entered = key; setKey('');
    const value = await action('set_ai_key', { key: entered, storage });
    if (value) {
      if (!value.credential.configured) setFeedback({ kind: 'error', message: value.credential.vaultError ?? 'The key was not configured.' });
      else { setReplacingKey(false); setFeedback({ kind: 'status', message: 'Key connected.' }); }
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
      if (value.credential.configured && (value.provider !== 'local' || value.settings.localAuthentication)) setFeedback({ kind: 'error', message: value.credential.vaultError ?? 'The key could not be removed.' });
      else { setReplacingKey(false); setFeedback({ kind: 'status', message: 'Key removed.' }); }
    }
  }
  async function testAccess() {
    await perform('checking', async () => {
      try {
        const result = await native<AccessResult>('test_ai_access');
        receive(await refresh());
        if (result.providerError) {
          setFeedback({ kind: 'error', access: true, message: result.message, providerError: result.providerError, provider: accepted.current?.provider });
          return;
        }
        setFeedback({ kind: 'status', message: result.message ? 'Connection checked.' : 'Access checked.' });
      } catch (error) {
        try { receive(await refresh()); } catch (refreshError) { throw new Error(`${errorMessage(error)} Settings could not be refreshed: ${errorMessage(refreshError)}`); }
        throw error;
      }
    });
  }
  function usage() { return perform('managingUsage', manageUsage); }
  // OpenAI offers a key or a ChatGPT plan sign-in; other providers use a key.
  async function chooseProvider(value: string) {
    const next = value === 'openai-chatgpt' ? 'openai' : value;
    if (next !== view?.provider && !await action('set_ai_provider', { provider: next })) return;
    if (next === 'openai') {
      const wanted = value === 'openai-chatgpt' ? 'chatgpt' : 'apiKey';
      const changed = accepted.current?.authentication !== wanted;
      if (changed && !await action('set_ai_authentication', { authentication: wanted })) return;
      if (changed || wanted === 'chatgpt') {
        try { receive(await refresh(true)); } catch (error) { setFeedback({ kind: 'error', message: errorMessage(error) }); }
      }
    }
  }
  const model = view?.catalogue.find(item => item.id === settings?.model);
  const dirty = !!view && !!settings && JSON.stringify(view.settings) !== JSON.stringify(settings);
  const usesChatgpt = view?.authentication === 'chatgpt' && view.provider === 'openai';
  const usesLocal = view?.provider === 'local';
  const endpointDirty = usesLocal && endpoint !== null && endpoint !== view?.settings.endpoint;
  const hasKey = !!view?.credential.configured && (!usesLocal || !!view.settings.localAuthentication);
  const provider = view?.providers?.find(item => item.id === view.provider);
  const providerName = provider?.name ?? 'OpenAI';
  const pending = view?.chatgpt.signIn.status === 'awaitingBrowser';
  const account = view?.chatgpt.accounts.find(item => item.id === view.chatgpt.activeAccountId);
  const canTest = !!view?.readiness.canTest && !dirty && !endpointDirty && !pending && !key.trim();
  const testHelp = pending ? 'Finish signing in through your browser first.' : key.trim() ? 'Connect the key above before testing access.' : dirty || endpointDirty ? 'Save your settings below before testing access.' : view?.readiness.blockedReason ?? 'Checks your connection without generating.';
  const connected = usesChatgpt ? !!account?.connected : view?.credential.configured;
  // A local server that rejected its anonymous key is not a working connection.
  const connectedHealthy = connected && !(usesLocal && view?.modelsAuthRequired);
  // Only one activity runs at a time; loading has its own placeholder instead of blocking controls.
  const busy = activity !== 'idle' && activity !== 'loading';
  const loading = activity === 'loading';
  // Testing access re-probes images too, so both activities show "Checking…".
  const checkingImages = activity === 'checking' || activity === 'checkingImages';
  const canRefreshModels = !!view?.readiness.canRefreshModels && !busy && !pending && !endpointDirty && !key.trim();
  const modelsError = usesChatgpt ? view?.chatgpt.modelsError : view?.modelsError;
  const modelsProviderError = usesChatgpt ? view?.chatgpt.modelsProviderError : view?.modelsProviderError;
  const identity = usesChatgpt ? account?.email ?? account?.label ?? 'ChatGPT' : view?.credential.maskedIdentifier ?? `${providerName} API key`;
  // An unusable switch reads as off; the saved preference returns once images are available.
  const imagesUsable = !!view?.readiness.imagesUsable;
  const imageStatus = view?.imageAccessChecking || checkingImages || pending ? 'Checking…' : !provider?.capabilities.imageGeneration || view?.imageAccess?.available === 'no' ? 'Not available' : !connectedHealthy ? 'Not connected' : view?.imageAccess?.available === 'yes' ? 'Available' : "Couldn't check";
  return <div className="ai-settings dialog-body">
    {feedback.kind === 'error' && <Notice variant="error" message={userError(feedback)} action={feedback.access ? { label: providerProblem(feedback.providerError)?.action ?? 'Try again', disabled: busy, onClick: () => { if (providerProblem(feedback.providerError)?.settings) showAccountSettings(); else void testAccess(); } } : undefined} dismiss={() => setFeedback({ kind: 'none' })} />}
    {pollingError && <Notice variant="error" message={userError(pollingError)} action={{ label: 'Try again', disabled: busy, onClick: () => { void load(); } }} />}
    {loading ? <p role="status">Reading AI settings…</p> : !view || !settings ? <button onClick={() => { void load(); }}>Retry AI settings</button> : <>
      <label className="ai-provider ai-inline-field">Provider<select disabled={busy || pending || dirty} value={usesChatgpt ? 'openai-chatgpt' : view.provider} onChange={event => { setKey(''); setReplacingKey(false); setFeedback({ kind: 'none' }); void chooseProvider(event.target.value); }}>
        {(view.providers ?? []).flatMap(item => item.id === 'openai' ? [
          ...(item.signInSupported ? [<option key="openai-chatgpt" value="openai-chatgpt">ChatGPT plan</option>] : []),
          <option key="openai" value="openai">API key</option>,
        ] : [<option key={item.id} value={item.id}>{item.name}</option>])}
      </select></label>
      {usesLocal && <form className="ai-key-form" onSubmit={connectServer}>
        <label>Server URL<input type="url" aria-label="Local server URL" required maxLength={MAX_HTTP_URL_BYTES} disabled={busy} value={endpoint ?? view.settings.endpoint ?? DEFAULT_LOCAL_ENDPOINT} onChange={event => setEndpoint(event.target.value)} /></label>
        <button type="submit" disabled={busy}>Connect</button>
      </form>}
      {view.configurationError && <Notice variant="error" message={userError(view.configurationError)} />}
      <section ref={accountSettings} tabIndex={-1} className={usesChatgpt ? 'chatgpt-accounts' : 'api-key-connection'} aria-label={usesChatgpt ? 'ChatGPT account settings' : `${providerName} key connection`}>
        <div className="ai-account-card">
          <span className="ai-avatar" aria-hidden="true">{identity.charAt(0).toUpperCase()}</span>
          <div className="ai-account-copy"><span className="ai-account-identity">{identity}</span>
            <StatusChip tone={connectedHealthy ? 'success' : 'neutral'} dot role="status" className="connection-status">{connectedHealthy ? 'Connected' : 'Not connected'}</StatusChip>
          </div>
          {usesChatgpt && view.chatgpt.accounts.length > 1 && <select aria-label="ChatGPT account" className="ai-account-switcher" value={view.chatgpt.activeAccountId ?? ''} disabled={busy || pending} onChange={event => { if (event.target.value) void action('select_chatgpt_account', { accountId: event.target.value }); }}>
            {!account && <option value="" disabled>Choose account</option>}{view.chatgpt.accounts.map((item, index) => <option key={item.id} value={item.id}>{item.email ?? item.label}{view.chatgpt.accounts.filter(other => (other.email ?? other.label) === (item.email ?? item.label)).length > 1 ? ` (${index + 1})` : ''}{!item.connected ? ' · Not connected' : !item.planEnabled ? ' · Plan permission needed' : ''}</option>)}
          </select>}
          <MenuButton className="ai-account-options" align="end" label={<Icon name="more" />} ariaLabel="Account options" title={canTest ? 'Account options' : testHelp} showChevron={false} items={[
            ...(usesChatgpt ? [
              { id: 'add', label: 'Add account', disabled: busy || pending, action: () => { void action('begin_chatgpt_sign_in', { storage: chatgptStorage, accountId: null }); } },
              ...(account ? [{ id: 'reconnect', label: 'Reconnect', disabled: busy || pending, action: () => { void action('begin_chatgpt_sign_in', { storage: chatgptStorage, accountId: account.id }); } }] : []),
            ] : view.credential.configured ? [{ id: 'replace', label: hasKey ? 'Replace key' : 'Add key', disabled: busy, action: () => setReplacingKey(true) }] : []),
            ...(usesLocal && view.settings.localAuthentication ? [{ id: 'anonymous', label: 'Use without a key', disabled: busy, action: () => { void action('save_ai_settings', { settings: { ...view.settings, localAuthentication: false }, connectionOnly: true }); } }] : []),
            { id: 'test', label: 'Test connection', disabled: busy || !canTest, action: () => { void testAccess(); } },
            { id: 'storage', label: `Where to keep the ${usesChatgpt ? 'sign-in' : 'key'}`, disabled: busy || pending, children: (['system', 'session'] as const).map(value => ({
              id: value, label: value === 'system' ? 'Save securely on this device' : 'Current session only', role: 'menuitemradio' as const, checked: (usesChatgpt ? chatgptStorage : storage) === value,
              action: () => usesChatgpt ? setChatgptStorage(value) : setStorage(value) })) },
            ...((usesChatgpt ? account && (account.connected || account.storage !== 'none') : hasKey || view.credential.vaultError) ? [
              { id: 'separator', label: '', separator: true },
              { id: 'remove', label: usesChatgpt ? 'Sign out' : 'Remove key', disabled: busy || pending, action: () => { if (usesChatgpt && account) void action('sign_out_chatgpt', { accountId: account.id }); else void removeKey(); } },
            ] : []),
          ] satisfies MenuItem[]} />
        </div>
        {usesChatgpt ? <>
          {view.chatgpt.storageError && <Notice variant="error" message={userError(view.chatgpt.storageError)} />}
          {view.chatgpt.message && !['ChatGPT connected.', 'Signed out. Remote access was revoked.', 'Secure storage repaired.'].includes(view.chatgpt.message) && <Notice variant="error" message={userError(view.chatgpt.message)} />}
          {account?.connected && !account.planEnabled && <Notice variant="error" message="Your ChatGPT plan is not enabled for Desegnos." />}
          {account?.storageRepairRequired && <Notice variant="error" message="Your saved sign-in details need attention." action={{ label: 'Repair secure storage', disabled: busy || pending, onClick: () => { void action('retry_chatgpt_secure_storage', { accountId: account.id }); } }} />}
          {pending ? <button type="button" disabled={busy} onClick={() => { void action('cancel_chatgpt_sign_in'); }}>Cancel sign-in</button> : !connected && <button type="button" className="primary" disabled={busy} onClick={() => { void action('begin_chatgpt_sign_in', { storage: chatgptStorage, accountId: account?.id ?? null }); }}>Sign in with ChatGPT</button>}
          {view.chatgpt.signIn.message && view.chatgpt.signIn.status !== 'complete' && <>{view.chatgpt.signIn.status === 'failed' ? <Notice variant="error" message={userError(view.chatgpt.signIn.message)} action={{ label: 'Sign in again', disabled: busy || pending, onClick: () => { void action('begin_chatgpt_sign_in', { storage: chatgptStorage, accountId: account?.id ?? null }); } }} /> : <Notice variant="info" message={view.chatgpt.signIn.message} />}</>}
        </> : <>
          {view.credential.configured && view.credential.vaultError && <Notice variant="error" message={userError(view.credential.vaultError)} />}
          {(!view.credential.configured || replacingKey) && <form className="ai-key-form" onSubmit={saveKey}>
            <label>{hasKey ? 'Replacement API key' : `${providerName} API key`}<input disabled={busy || pending} type="password" value={key} onChange={event => setKey(event.target.value)} autoComplete="new-password" spellCheck={false} required autoFocus={replacingKey} /></label>
            <button className="primary" type="submit" disabled={busy || pending || !key.trim()}>{busy ? 'Connecting…' : 'Connect'}</button>
            {replacingKey && <button type="button" disabled={busy} onClick={() => { setKey(''); setReplacingKey(false); }}>Cancel</button>}
          </form>}
        </>}
      </section>
      <form className="settings-form generation-settings-form" onSubmit={saveSettings}>
        <div className="ai-model-row"><label className="ai-inline-field">Model<select disabled={busy || pending || endpointDirty || view.catalogue.length === 0} value={settings.model} onChange={event => {
          const chosen = view.catalogue.find(model => model.id === event.target.value);
          setSettings({ ...settings, model: event.target.value, pricing: chosen?.defaultPricing, maxOutputTokens: usesChatgpt ? settings.maxOutputTokens : Math.min(settings.maxOutputTokens, chosen?.maxOutputTokens ?? 32768), reasoningEffort: nearestReasoningEffort(chosen?.reasoningEfforts ?? [], settings.reasoningEffort) });
        }}>{!model && <option value={settings.model} disabled>{settings.model} · unavailable</option>}{view.catalogue.map(model => <option key={model.id} value={model.id}>{model.name}</option>)}</select></label>
          <div className="ai-model-actions">
            <button type="button" className="icon-button sm" aria-label="Refresh models" title="Refresh models" disabled={!canRefreshModels} onClick={() => { void action('get_ai_settings', { refreshModels: true }); }}><Icon name="refresh" /></button>
            {usesChatgpt && <button type="button" className="ai-usage-link" disabled={busy} onClick={() => { void usage(); }}>Usage <Icon name="external" size={14} /></button>}
          </div>
        </div>
        {modelsError && <Notice variant="error" message={userError({ message: modelsError, providerError: modelsProviderError, provider: view.provider })} action={{ label: providerProblem(modelsProviderError)?.action ?? 'Refresh models', disabled: !canRefreshModels, onClick: () => { if (providerProblem(modelsProviderError)?.settings) showAccountSettings(); else void action('get_ai_settings', { refreshModels: true }); } }} />}
        {!model && connectedHealthy && <Notice variant="error" message="No available AI model is selected." />}
        {(!usesChatgpt || view.imageKey.configured) && <label className="ai-inline-field ai-budget">Budget per generation (USD)<input disabled={busy || pending} type="number" required min={0.01} step={0.01} aria-label="API budget per generation (USD)" value={budget ?? String(settings.spendingLimitUsd)} onChange={event => { setBudget(event.target.value); if (event.target.value.trim() && Number.isFinite(event.target.valueAsNumber)) setSettings({ ...settings, spendingLimitUsd: event.target.valueAsNumber }); }} onBlur={() => setBudget(null)} /></label>}
        {!usesChatgpt && model?.pricingRequired && <div className="field-pair ai-price-ceilings">
          <label>Input ceiling (USD / million)<input type="number" aria-label="Input price ceiling" required min={0} max={1000000} step="any" disabled={busy || pending} value={settings.pricing?.inputUsdPerMillion ?? ''} onChange={event => setPrice('inputUsdPerMillion', event.target.value)} /></label>
          <label>Output ceiling (USD / million)<input type="number" aria-label="Output price ceiling" required min={0} max={1000000} step="any" disabled={busy || pending} value={settings.pricing?.outputUsdPerMillion ?? ''} onChange={event => setPrice('outputUsdPerMillion', event.target.value)} /></label>
          {view.provider === 'openrouter' && <>
            <label>Request ceiling (USD)<input type="number" aria-label="Request price ceiling" required min={0} max={1000000} step="any" disabled={busy || pending} value={settings.pricing?.requestUsd ?? ''} onChange={event => setPrice('requestUsd', event.target.value)} /></label>
            <label>Image ceiling (USD)<input type="number" aria-label="Image price ceiling" required min={0} max={1000000} step="any" disabled={busy || pending} value={settings.pricing?.imageUsd ?? ''} onChange={event => setPrice('imageUsd', event.target.value)} /></label>
          </>}
        </div>}
        <section className="ai-features" aria-label="Features">
          <div className="ai-feature-row image-generation-access"><div className="ai-feature-copy"><label htmlFor="ai-image-generation">Image generation</label><div className="ai-feature-meta"><StatusChip tone={imageStatus === 'Checking…' ? 'info' : imageStatus === 'Available' || imageStatus === 'Not connected' ? 'neutral' : 'warning'} id="ai-image-status" role="status">{imageStatus}</StatusChip><button type="button" className="ai-refresh icon-button sm" aria-label="Check again" title="Check again" disabled={busy || !canTest || view.imageAccessChecking} onClick={() => { void testAccess(); }}><Icon name="refresh" /></button></div></div>
            <input id="ai-image-generation" className="ai-switch" type="checkbox" role="switch" aria-describedby="ai-image-status" disabled={busy || pending || view.imageAccessChecking || !imagesUsable} checked={settings.imageGeneration && imagesUsable} onChange={event => setSettings({ ...settings, imageGeneration: event.target.checked })} />
          </div>
          {view.provider === 'openai' && <div className="ai-feature-meta">
            
            {(view.imageAccess?.available !== 'yes' || view.imageKey.configured) && <button type="button" disabled={busy || pending} onClick={() => setShowImageKey(value => !value)}>{view.imageKey.configured ? 'Replace image key' : 'Add image key'}</button>}
            {view.imageKey.configured && <button type="button" disabled={busy || pending} onClick={() => { setImageKey(''); void action('remove_ai_key', { image: true }); }}>Remove image key</button>}
          </div>}
          {view.provider === 'openai' && showImageKey && <div className="ai-key-form">
            <label>Image API key<input type="password" autoComplete="new-password" spellCheck={false} disabled={busy || pending} value={imageKey} onChange={event => setImageKey(event.target.value)} onKeyDown={event => { if (event.key === 'Enter') { event.preventDefault(); if (imageKey.trim()) void connectImageKey(); } }} /></label>
            <label>Store key<select value={imageStorage} disabled={busy || pending} onChange={event => setImageStorage(event.target.value as 'system' | 'session')}><option value="system">Secure storage</option><option value="session">This session</option></select></label>
            <button type="button" disabled={busy || pending || !imageKey.trim()} onClick={() => { void connectImageKey(); }}>Connect image key</button>
          </div>}
          {view.provider === 'openai' && view.imageKey.configured && view.imageKey.vaultError && <Notice variant="error" message={userError(view.imageKey.vaultError)} />}
          {view.provider === 'openai' && view.imageAccess?.available !== 'yes' && [view.imageKeyAccess, view.mainImageAccess].find(access => access?.organizationVerification) && <Notice variant="error" message="Verify your OpenAI account to create pictures." />}
          <div className="ai-feature-row"><span>Web search</span><StatusChip tone={provider?.capabilities.webSearch ? 'neutral' : 'warning'}>{provider?.capabilities.webSearch ? 'Available' : 'Not available'}</StatusChip></div>
        </section>
        <Disclosure className="ai-advanced" summary="Advanced" open={advanced} onToggle={setAdvanced}><fieldset className="advanced-settings"><legend>Model details</legend>{usesChatgpt && account && <p className="small muted">Registration: <span className="source-url">{account.id}</span></p>}{model && !model.pricingRequired && <div className="model-details">{!usesChatgpt && <p className="small">Standard input: USD {model.inputUsdPerMillion} / million tokens · Standard output: USD {model.outputUsdPerMillion} / million tokens</p>}
          {!usesChatgpt && (model.verifiedAt || model.sourceUrl) && <p className="small muted">{model.verifiedAt && <>Catalogue verified {model.verifiedAt}. </>}Pricing source: <span className="source-url">{model.sourceUrl}</span></p>}</div>}
        <div className={usesChatgpt ? undefined : 'field-pair'}>{!usesChatgpt && <label>Maximum output tokens<input disabled={busy} type="number" required min={1} max={model?.maxOutputTokens ?? 32768} step={1} value={settings.maxOutputTokens} onChange={event => setSettings({ ...settings, maxOutputTokens: Number(event.target.value) })} /></label>}
          <label>Reasoning effort<select disabled={busy || pending || !model} value={settings.reasoningEffort} onChange={event => setSettings({ ...settings, reasoningEffort: event.target.value })}>{model?.reasoningEfforts.map(level => <option key={level} value={level}>{level === 'none' ? 'Default' : level.charAt(0).toUpperCase() + level.slice(1)}</option>)}</select></label></div>
        </fieldset></Disclosure>
        <div className="form-actions ai-save-actions">{feedback.kind === 'status' && <Notice variant="success" message={feedback.message} />}{dirty && <button type="button" disabled={busy || pending} onClick={() => setSettings(view.settings)}>Reset changes</button>}{dirty && <button className="primary" disabled={busy || pending || endpointDirty || (!model && !view.configurationError)}>Save AI settings</button>}</div>
      </form>
    </>}
  </div>;
}
