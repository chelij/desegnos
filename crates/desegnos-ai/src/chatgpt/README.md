# Native ChatGPT authentication

This module owns OpenAI OAuth registration, verified identity, account selection,
renewable credentials and the account-specific model catalogue. It has no project,
requirements, job or generated-site authority. Native callers must authorize the
trusted editor before invoking it and must bind generation to an explicit account
UUID. Account selection never redirects a bearer requested for another UUID.

## Public integration

`ChatgptManager` is cloneable. `initialize(private_directory)` reads or creates
private host/account metadata without HTTP, a browser or credential-vault access.
Use a Tokio runtime for its async methods:

| Method | Behavior |
| --- | --- |
| `view()` | Public cached status; reads private metadata and owned vault records, without HTTP. |
| `begin_sign_in(storage, account_id)` | Binds a literal-loopback callback, returns a native-only `ChatgptAuthorization`, and starts the bounded authorization task. `None` adds a registration; `Some(id)` reconnects that exact registration. |
| `cancel_sign_in()` | Cancels the current callback/exchange without replacing accepted access. |
| `select_account(id)` | Persists selection and cancels pending authorization. |
| `acknowledge_welcome()` | Persists the selected account's first-plan-use acknowledgment. |
| `bearer(id, cancellation)` | Returns the opaque native `ApiKey` wrapper; validates registration and plan scope, and serializes refresh when needed. |
| `discover_models(id, client, cancellation)` | Explicit authenticated discovery; updates the cache only while that registration still has access. |
| `retry_secure_storage(id)` | Commits a retained replacement bundle without HTTP or refresh replay. |
| `sign_out(id)` | Cancels a pending reconnect for that registration, attempts discovery-based revocation, disables local access and retains client/host mapping. |

Only native browser-opening code receives `ChatgptAuthorization::url()`. Do not
serialize or log it: returning authorization may contain an ID-token hint. Its
`Debug` representation is redacted and its URL is zeroized on drop. No public view
or error contains access, refresh or ID tokens, authorization codes, state, PKCE
verifiers or authorization URLs. Public fields use the contract's camelCase JSON,
including the per-account `storageRepairRequired` boolean. `ChatgptView::unavailable`
lets optional authentication damage stay visible without blocking authoring.

Model discovery runs once before a successful authorization becomes active.
Discovery failure is cached separately from verified identity; `view()` never
retries it. Models are hidden for disconnected, plan-disabled or repair-blocked
accounts. The native integration exposes all listed account models and validates saved
selections against the active account, without a model-name allowlist. Discovery also
retains advertised reasoning options and token limits.

## Credentials and interrupted operations

System tokens are one complete JSON bundle in the owned keyring service
`io.github.chelij.desegnos.chatgpt`, indexed by registration UUID. Session tokens
exist only in native memory after an explicit session choice. Reconnecting a system
registration with session storage disables the system mapping and retires its
owned vault entry; failure rolls back to the previously accepted mapping.

`accounts.json` contains host UUID, issued client IDs, labels, verified subject/email,
selection, permission/welcome status and a non-secret refresh fence. It contains no
tokens. Unix directory/file permissions are 0700/0600. Bounded reads, symlink
rejection, atomic replacement with synchronization and an OS file lock protect
metadata. Vault calls run on blocking workers rather than the async executor.

The local gate and OS file lock serialize refresh, revocation and metadata changes
across independent managers/processes. A refresh fence is committed **before**
submission. Successful replacement stores the access/refresh/ID tokens, scopes and
expiry together before clearing the fence. A known token rejection retains the
previous bundle; a terminal session rejection disables it. An uncertain response,
unverified replacement or interrupted process leaves the fence in place.

If a validated rotated bundle cannot be stored, the submitting process retains it
privately and reports `storageRepairRequired` for that account. Generation is
blocked until explicit `retry_secure_storage(id)` succeeds or the account is
reconnected/signed out. Another process, or a fresh process without that retained
bundle, requires reconnecting the saved registration and never replays the old
refresh token. Closing the only process holding an unstored replacement loses that
replacement; there is deliberately no plaintext recovery file. If a process stops
after the vault write but before the fence clears, reconnect is conservatively
required even though the replacement may already be in the vault.

Sign-out uses a retained newest bundle when available. Local disconnection is
durable before local tokens are removed. Vault-removal failure leaves access
disabled and surfaces a storage error. Remote network/5xx revocation receives at
most three bounded attempts; unconfirmed remote revocation is stated explicitly.
An interrupted refresh whose newest bundle is unavailable cannot claim remote
revocation confirmation based only on its possibly consumed old token.

## Protocol boundaries

Production uses only fixed HTTPS OpenAI authorize/token/JWKS/discovery endpoints
and the fixed public inference resource. TLS validation stays enabled. HTTP has
no redirects, proxy inheritance, automatic retry, credential logging or TLS key
logging. Discovery and revocation targets are checked against the trusted origin.

Every attempt has fresh state, nonce and S256 PKCE. The listener binds literal
127.0.0.1 before browser opening, verifies the exact host/path/state, bounds headers
and connections, and ends after five minutes or cancellation. Dynamic registration
must return an issued client ID; returning authorization must retain its client
and verified subject. Distinct registrations remain distinct even with equal email.

ID-token activation requires an allowed asymmetric signature, trusted JWKS key,
issuer, audience/authorized party, expiry, issued-at and initial nonce. Unknown
key IDs refresh the bounded JWKS cache once. Grant scope, rather than email or an
unverified token claim, determines ChatGPT-plan permission. A valid identity without
that permission cannot discover inference models or supply a generation bearer.

## Tests and sources

`cargo test -p desegnos-ai --features test-support` drives real localhost HTTP with
synthetic signed JWTs and an isolated memory/unavailable vault. It covers identity
rejection, state/PKCE/client mapping, cancellation/deadline, account isolation,
rotation and cross-process fencing, storage transitions, revocation and late
responses after sign-out. `fixtures/test-key.der` is an intentional locally generated
RSA test key; it is never an account credential.

Debug/test-only `ChatgptManager::for_loopback(directory, oauth_base, model_client,
vault)` accepts only an explicit literal 127.0.0.1 HTTP port. `ChatgptTestVault`
offers `memory()`, `unavailable()`, a task-owned memory write-failure flag and `system()`. Use `system()` only after an
external harness has verified a private credential daemon/session bus and private
storage. Ordinary tests never invoke it. The crate rejects release builds with
`test-support`. The native UI/private OS-vault gates are separate integration checks;
no test here authorizes a live account or makes a paid generation request.

Protocol documentation was fetched from OpenAI on 2026-10-18:

- [Sign-in and registration](https://developers.openai.com/siwc/token-sharing-open-source/sign-in)
- [Profiles, refresh and revocation](https://developers.openai.com/siwc/token-sharing-open-source/profiles-and-sessions)
- [Token reference](https://developers.openai.com/siwc/token-sharing-open-source/token-reference)
- [Errors and recovery](https://developers.openai.com/siwc/token-sharing-open-source/errors-and-recovery)
- [Models and inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference)

Pinned dependency/source/license/advisory review is in `dependency-review.json`.
Recheck the actual enabled dependency graph against current advisories before each
release; this dated review does not substitute for release auditing.
