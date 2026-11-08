# ChatGPT-plan generation contract

Desegnos offers Continue with ChatGPT through OpenAI's documented open-source sign-in flow alongside its existing API-key mode. This is a provider/authentication capability. Core requirements, validation, source isolation and transactional application retain their existing authority.

## Result and proof

Users authorize Desegnos in their system browser, choose a saved account registration or add one, and explicitly grant ChatGPT-plan usage. Native Rust validates identity and owns credentials. Generate sends the same exact REQUIREMENTS.md file, job identity and global Fuzzy/Strict setting to the public Responses endpoint with the selected authentication method frozen for the job. It cannot silently switch billing methods.

The verification gate covers real loopback OAuth HTTP, signed-token validation, state/nonce/PKCE, token rotation/revocation, profile isolation, model discovery, exact requirements delivery, plan-limit failures, native settings/review and credential non-disclosure. A real OpenAI consent and generation remain a distinct human-authorized check. Test endpoints, browser interception and vault adapters are explicit debug-only facilities, never production fallbacks.

## Authentication and storage

Production endpoints are OpenAI's public authorize/token/JWKS/discovery endpoints and `https://api.openai.com/v1`. The browser listener binds literal `127.0.0.1` before opening authorization. The callback path stays `/auth/callback`; only the port varies. Every attempt owns fresh state, nonce and an S256 PKCE verifier. Code exchange uses the exact callback URI and issued client ID. First registration uses `dynamic_agent_client`, app name Desegnos and a persisted `urn:uuid:` host ID. Returning authorization reuses the registration/client/host mapping and never substitutes another account's credentials.

Verify ID-token signatures against the issuer's JWKS, issuer, audience, expiry and nonce before activation; returning identity must match the selected registration. Multiple registrations retain distinct stable IDs/labels even when email matches. Granted scopes, including `chatgpt.tokens.use.direct`, determine plan permission. A valid identity without plan permission does not authorize inference.

Tokens stay in system credential storage or explicitly selected session memory. Public native views contain account identity/status, never tokens, verifiers, codes or authorization URLs. A stable host/registration metadata file is private and atomically written; tokens never enter it, projects, requirements, jobs, diagnostics or browser storage. Serialize rotating refreshes across processes. Refresh replaces access/refresh/ID tokens, scopes and expiry together. If secure storage fails after rotation, retain the newest validated bundle privately in memory and block generation until Repair secure storage commits it; repair makes no HTTP request. Keep Desegnos open during repair because this pending bundle cannot survive process exit. A successful session-only reconnect retires that registration's older system credential and cannot revive it at restart. Sign-out attempts discovery-based session revocation with the newest bundle, clears local tokens, retains the client/host mapping, and states when remote revocation was not confirmed. Preserve accepted credentials on local storage failure.

## Provider interfaces

Keep existing `ProviderSettings` fields compatible; optional `pricing` adds model-bound API price ceilings and is omitted when absent. Add a separately serializable `AuthenticationMethod` with camelCase values `apiKey` and `chatgpt`, defaulting to `apiKey` for existing records. Native preferences choose it; each job freezes it and an optional opaque ChatGPT account UUID. Account switching or sign-out cannot redirect an already prepared job to another account.

The existing transport accepts the native opaque `ApiKey` bearer wrapper for either credential, with a separate crate-only constructor for bounded OAuth tokens. API-key validation keeps its existing bound. New public provider entry points:

- `estimate_for_auth(authentication, snapshot, mode, schema, settings) -> CostEstimate`.
- `OpenAiClient::generate_for_auth(authentication, snapshot, mode, schema, settings, key, cancel, progress) -> ProviderResponse`.
- `OpenAiClient::list_chatgpt_models(key, cancel) -> Vec<ChatgptModel>` using authenticated `GET /models`; models contain `id` and `name` from server `slug`/`display_name`, filtered by `visibility == "list"`, preserving server order, plus advertised reasoning levels and optional context/output limits.

Existing API-key methods keep their behavior. ChatGPT inference omits unsupported `max_output_tokens` and other unsupported fields, including service-tier overrides. It keeps `store:false`, `stream:true`, strict candidate schema, exact Base64 Markdown, separate input-text job/mode/hash, no tools, existing byte/deadline/cancellation limits and completion validation. Its USD estimate field is unused by UI; show plan/credit use instead of API prices or a promised monetary cap. Check complete input/context bounds without treating an unsupported token parameter as an enforced generation cap. Never truncate.

Every listed model in the current account's discovered catalogue is selectable without a local model-name allowlist. Settings and each submission validate model membership and reasoning against the active account. Native generation, identity analysis and export check advertised context/output limits alongside application limits; absent metadata retains the existing application bounds and low/medium/high reasoning subset. An explicitly empty reasoning list omits that parameter. New effort identifiers use the same bounded string validation as discovery. API-key discovery also uses the connected account catalogue. API pricing remains separate: known rates retain their verification, while unknown API models require explicit model-bound price ceilings. Plan models never inherit API prices. No guessed entitlement or fallback billing. Classify structured plan permission/eligibility/usage-limit/availability/capability errors with safe actionable messages; a failed terminal SSE event after partial output is failure, never a candidate. No automatic inference retry.

## Native authentication manager

`ChatgptManager` lives in `desegnos-ai`, owns no project authority and receives only its private authentication directory. Root integration adapts its concrete method signatures while preserving these responsibilities:

- Start/cancel browser authorization, returning the URL only to native code that opens the fixed trusted destination.
- Read public status, choose/add/reauthorize saved accounts, and sign out.
- Obtain an opaque bearer for a specified account UUID, refreshing safely when required.
- Discover the active account's models and record explicit access/transport failures.

Public `ChatgptView` is camelCase: `accounts`, `activeAccountId`, `signIn`, `models`, `modelsError`, `storageError`, `showWelcome`, `message`. Each account is `{id, label, email, connected, planEnabled, storage, storageRepairRequired}`. `signIn` is `{status, message}`, with status `idle | awaitingBrowser | complete | cancelled | failed`. Models are `{id,name}`. IDs are native UUIDs; optional values are null. Credential storage uses the existing `system | session | none` vocabulary. Native scope verifies the trusted editor before account operations. Optional authentication-state damage stays visible without preventing ordinary project authoring.

Native commands expose `set_ai_authentication`, `begin_chatgpt_sign_in`, `cancel_chatgpt_sign_in`, `select_chatgpt_account`, `retry_chatgpt_secure_storage`, `sign_out_chatgpt` and `open_chatgpt_usage`, alongside extended `get_ai_settings`/`test_ai_access`. Begin takes `{storage, accountId: UUID | null}`; null adds a registration. Select/repair/sign-out take `{accountId}`. Selecting a connected account refreshes its model discovery; the account catalogue preserves server order, display names and advertised capabilities. Opening AI settings or using Refresh models requests `get_ai_settings` with `{refreshModels: true}`; ordinary reads and sign-in polling omit it or use false. Settings gains `authentication` and `chatgpt`. Jobs gain default-compatible `authentication`, `chatgptAccountId` and a safe optional failure kind. Root owns native command registration, durable job binding and transaction integration.

## Editor

No sidebars. AI settings offers ChatGPT and API key as explicit generation methods. Continue with ChatGPT opens the real system browser; Cancel stops the attempt. A saved-account picker, Add account, reconnect, sign-out, storage selection and Test access have native operations. Poll status only while authorization is pending. Do not replace accepted identity with an unfinished attempt.

Show the first successful plan-use confirmation once, with Got it. Show Using ChatGPT plan beside generation/model controls and Manage usage linking to `https://chatgpt.com/settings/usage`. Keep plan/credit disclosure in request review and retry confirmation. The API-key cost/output-cap UI remains for API mode; it is not presented as a ChatGPT billing or output guarantee. Usage-limit states make Manage usage primary. Prompts, global Fuzzy/Strict and candidate/identity review keep their existing interaction.

## Sources

- [Registration and sign-in](https://developers.openai.com/siwc/token-sharing-open-source/sign-in).
- [Profiles, refresh and revocation](https://developers.openai.com/siwc/token-sharing-open-source/profiles-and-sessions).
- [Models and inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference).
- [Preview limitations](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations).
- [Errors and recovery](https://developers.openai.com/siwc/token-sharing-open-source/errors-and-recovery).
- [UI/UX guidelines](https://developers.openai.com/siwc/ui-ux-guidelines).
