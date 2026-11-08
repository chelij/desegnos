mod callback;
mod http;
mod lock;
mod models;
mod refresh;
mod storage;
mod types;

#[cfg(any(test, feature = "test-support"))]
pub use storage::ChatgptTestVault;
pub use types::{
    ChatgptAccount, ChatgptAuthorization, ChatgptError, ChatgptErrorKind, ChatgptResult,
    ChatgptSignIn, ChatgptSignInStatus, ChatgptView,
};

use crate::{
    ApiKey, CancellationToken, ChatgptModel, CredentialLocation, CredentialStorage, OpenAiClient,
};
use ChatgptErrorKind as Kind;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use http::{AuthHttp, Endpoints, SCOPES, TokenResponse, now};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs::File, path::Path, sync::Arc, time::Duration};
use storage::{Metadata, Registration, Store, SystemVault, TokenRecord, Vault, valid_client};
use tokio::{net::TcpListener, sync::Mutex};
use uuid::Uuid;
use zeroize::Zeroizing;

const AUTHORIZATION_DEADLINE: Duration = Duration::from_secs(300);
const ROTATION_STORAGE_MESSAGE: &str = "Refreshed ChatGPT credentials are retained privately in native memory because secure storage could not be updated. Repair secure storage before generating or closing Desegnos; the consumed refresh token will not be reused.";
const INTERRUPTED_REFRESH_MESSAGE: &str = "A ChatGPT refresh was interrupted before its replacement credentials were committed. Reconnect the saved account; Desegnos will not replay the previous refresh token.";

enum SignIn {
    Idle,
    SigningIn {
        id: Uuid,
        account_id: Option<Uuid>,
        cancellation: CancellationToken,
        completion: CancellationToken,
    },
    Complete,
    Cancelled {
        message: String,
    },
    Failed {
        error: ChatgptError,
    },
}

impl SignIn {
    fn view(&self) -> ChatgptSignIn {
        let (status, message) = match self {
            Self::Idle => (ChatgptSignInStatus::Idle, None),
            Self::SigningIn { .. } => (
                ChatgptSignInStatus::AwaitingBrowser,
                Some("Finish signing in through your browser.".into()),
            ),
            Self::Complete => (
                ChatgptSignInStatus::Complete,
                Some("ChatGPT sign-in complete.".into()),
            ),
            Self::Cancelled { message } => (ChatgptSignInStatus::Cancelled, Some(message.clone())),
            Self::Failed { error } => (ChatgptSignInStatus::Failed, Some(error.message().into())),
        };
        ChatgptSignIn { status, message }
    }

    fn start(
        &mut self,
        id: Uuid,
        account_id: Option<Uuid>,
        cancellation: CancellationToken,
        completion: CancellationToken,
    ) {
        *self = Self::SigningIn {
            id,
            account_id,
            cancellation,
            completion,
        };
    }

    fn is_attempt(&self, attempt_id: Uuid) -> bool {
        matches!(self, Self::SigningIn { id, .. } if *id == attempt_id)
    }

    fn cancel(&mut self, message: &str) -> Option<CancellationToken> {
        if let Self::SigningIn {
            cancellation,
            completion,
            ..
        } = self
        {
            cancellation.cancel();
            let completion = completion.clone();
            *self = Self::Cancelled {
                message: message.into(),
            };
            Some(completion)
        } else {
            None
        }
    }

    fn finish(&mut self, id: Uuid, result: &ChatgptResult<()>) -> bool {
        if !self.is_attempt(id) {
            return false;
        }
        *self = match result {
            Ok(()) => Self::Complete,
            Err(error) if error.kind == Kind::Cancelled => Self::Cancelled {
                message: error.message().into(),
            },
            Err(error) => Self::Failed {
                error: error.clone(),
            },
        };
        true
    }
}
#[derive(Clone)]
struct PendingRotation {
    id: Uuid,
    tokens: TokenRecord,
}
struct State {
    metadata: Metadata,
    sessions: BTreeMap<Uuid, TokenRecord>,
    models: BTreeMap<Uuid, Vec<ChatgptModel>>,
    model_errors: BTreeMap<Uuid, String>,
    model_provider_errors: BTreeMap<Uuid, crate::ProviderError>,
    epochs: BTreeMap<Uuid, Uuid>,
    pending_rotations: BTreeMap<Uuid, PendingRotation>,
    sign_in: SignIn,
    storage_error: Option<String>,
    message: Option<String>,
}
struct Inner {
    store: Store,
    state: Mutex<State>,
    gate: Mutex<()>,
    vault: Arc<dyn Vault>,
    http: AuthHttp,
    model_client: OpenAiClient,
}
#[derive(Clone)]
pub struct ChatgptManager {
    inner: Arc<Inner>,
}

struct Attempt {
    id: Uuid,
    registration: Option<Registration>,
    storage: CredentialStorage,
    verifier: Zeroizing<String>,
    nonce: Zeroizing<String>,
    state: Zeroizing<String>,
    redirect: String,
    cancellation: CancellationToken,
}

impl ChatgptManager {
    pub fn initialize(directory: impl AsRef<Path>) -> ChatgptResult<Self> {
        Self::construct(
            directory.as_ref(),
            Endpoints::production(),
            OpenAiClient::new().map_err(|_| Kind::Transport)?,
            Arc::new(SystemVault),
        )
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn for_loopback(
        directory: impl AsRef<Path>,
        oauth_base: &str,
        client: OpenAiClient,
        vault: ChatgptTestVault,
    ) -> ChatgptResult<Self> {
        Self::construct(
            directory.as_ref(),
            Endpoints::loopback(oauth_base)?,
            client,
            Arc::new(vault),
        )
    }
    fn construct(
        directory: &Path,
        endpoints: Endpoints,
        model_client: OpenAiClient,
        vault: Arc<dyn Vault>,
    ) -> ChatgptResult<Self> {
        let store = Store::initialize(directory)?;
        let metadata = store.read()?;
        let state = State {
            metadata,
            sessions: BTreeMap::new(),
            models: BTreeMap::new(),
            model_errors: BTreeMap::new(),
            model_provider_errors: BTreeMap::new(),
            epochs: BTreeMap::new(),
            pending_rotations: BTreeMap::new(),
            sign_in: SignIn::Idle,
            storage_error: None,
            message: None,
        };
        Ok(Self {
            inner: Arc::new(Inner {
                store,
                state: Mutex::new(state),
                gate: Mutex::new(()),
                vault,
                http: AuthHttp::new(endpoints)?,
                model_client,
            }),
        })
    }

    pub async fn view(&self) -> ChatgptView {
        let mut state = self.inner.state.lock().await;
        match self.inner.store.read() {
            Ok(metadata) => {
                discard_stale_rotations(&mut state, &metadata);
                state.metadata = metadata;
            }
            Err(error) => state.storage_error = Some(error.message().into()),
        }
        let metadata = state.metadata.clone();
        let mut accounts = vec![];
        for registration in &metadata.registrations {
            let retained_rotation = state.pending_rotations.contains_key(&registration.id);
            let interrupted_refresh = registration.refresh_pending.is_some() && !retained_rotation;
            let loaded = if let Some(tokens) = state
                .pending_rotations
                .get(&registration.id)
                .map(|rotation| &rotation.tokens)
                .or_else(|| state.sessions.get(&registration.id))
            {
                Ok(Some(tokens.clone()))
            } else if registration.system_stored {
                self.load_system(&metadata, registration).await
            } else {
                Ok(None)
            };
            let (connected, plan_enabled, storage) = match loaded {
                Ok(Some(tokens)) => (
                    !registration.reconnect_required && !interrupted_refresh,
                    !registration.reconnect_required
                        && !interrupted_refresh
                        && tokens.plan_enabled(),
                    if tokens.storage == CredentialStorage::System {
                        CredentialLocation::System
                    } else {
                        CredentialLocation::Session
                    },
                ),
                Ok(None) => (false, false, CredentialLocation::None),
                Err(error) => {
                    state.storage_error = Some(error.message().into());
                    (
                        false,
                        false,
                        if registration.system_stored {
                            CredentialLocation::System
                        } else {
                            CredentialLocation::None
                        },
                    )
                }
            };
            accounts.push(ChatgptAccount {
                id: registration.id,
                label: registration.label.clone(),
                email: registration.email.clone(),
                connected,
                plan_enabled,
                storage,
                storage_repair_required: !registration.reconnect_required && retained_rotation,
            });
        }
        let active = metadata.active;
        let selected = accounts.iter().find(|account| Some(account.id) == active);
        let show_welcome = selected
            .is_some_and(|account| account.connected && account.plan_enabled)
            && metadata
                .registrations
                .iter()
                .any(|registration| Some(registration.id) == active && !registration.welcomed);
        let can_use_plan = selected.is_some_and(ChatgptAccount::usable);
        ChatgptView {
            accounts,
            active_account_id: active,
            sign_in: state.sign_in.view(),
            models: active
                .filter(|_| can_use_plan)
                .and_then(|id| state.models.get(&id).cloned())
                .unwrap_or_default(),
            models_error: active.and_then(|id| state.model_errors.get(&id).cloned()),
            models_provider_error: active
                .and_then(|id| state.model_provider_errors.get(&id).cloned()),
            storage_error: if !state.pending_rotations.is_empty() {
                Some(ROTATION_STORAGE_MESSAGE.into())
            } else if metadata
                .registrations
                .iter()
                .any(|registration| registration.refresh_pending.is_some())
            {
                Some(INTERRUPTED_REFRESH_MESSAGE.into())
            } else {
                state.storage_error.clone()
            },
            show_welcome,
            message: state.message.clone(),
        }
    }

    pub async fn begin_sign_in(
        &self,
        storage: CredentialStorage,
        account_id: Option<Uuid>,
    ) -> ChatgptResult<ChatgptAuthorization> {
        let _gate = self.inner.gate.lock().await;
        let mut state = self.inner.state.lock().await;
        if matches!(&state.sign_in, SignIn::SigningIn { .. }) {
            return Err(Kind::Busy.into());
        }
        let metadata = self.inner.store.read()?;
        discard_stale_rotations(&mut state, &metadata);
        let registration = account_id
            .map(|id| metadata.registration(id).cloned())
            .transpose()?;
        if registration.is_none() && metadata.registrations.len() >= 64 {
            return Err(Kind::Storage.into());
        }
        let retained = match registration.as_ref() {
            Some(registration) => {
                if let Some(tokens) = state
                    .pending_rotations
                    .get(&registration.id)
                    .map(|rotation| &rotation.tokens)
                    .or_else(|| state.sessions.get(&registration.id))
                {
                    Some(tokens.clone())
                } else if registration.system_stored {
                    self.load_system(&metadata, registration).await?
                } else {
                    None
                }
            }
            None => None,
        };
        let consent = retained.as_ref().map_or_else(
            || {
                registration
                    .as_ref()
                    .is_some_and(|registration| registration.last_plan_enabled == Some(false))
            },
            |tokens| !tokens.plan_enabled(),
        );
        let hint = retained
            .as_ref()
            .map(|tokens| Zeroizing::new(tokens.id_token.clone()));
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| Kind::Callback)?;
        let redirect = format!(
            "http://127.0.0.1:{}/auth/callback",
            listener.local_addr().map_err(|_| Kind::Callback)?.port()
        );
        let verifier = random_value();
        let nonce = random_value();
        let anti_csrf = random_value();
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let mut url = reqwest::Url::parse(&self.inner.http.endpoints.authorize)
            .map_err(|_| Kind::Discovery)?;
        {
            let mut query = url.query_pairs_mut();
            query
                .append_pair(
                    "client_id",
                    registration
                        .as_ref()
                        .map_or("dynamic_agent_client", |value| value.client_id.as_str()),
                )
                .append_pair(
                    "ext_agent_host_id",
                    &format!("urn:uuid:{}", metadata.host_id),
                )
                .append_pair("response_type", "code")
                .append_pair("redirect_uri", &redirect)
                .append_pair("scope", SCOPES)
                .append_pair("resource", http::RESOURCE)
                .append_pair("state", &anti_csrf)
                .append_pair("nonce", &nonce)
                .append_pair("code_challenge_method", "S256")
                .append_pair("code_challenge", &challenge);
            if let Some(registration) = &registration {
                if let Some(hint) = &hint {
                    query.append_pair("id_token_hint", hint);
                }
                if let Some(email) = &registration.email {
                    query.append_pair("login_hint", email);
                }
                if consent {
                    query.append_pair("prompt", "consent");
                }
            } else {
                query.append_pair("agent_name_hint", "Desegnos");
            }
        }
        let id = Uuid::new_v4();
        let cancellation = CancellationToken::new();
        state.metadata = metadata;
        let completion = CancellationToken::new();
        state
            .sign_in
            .start(id, account_id, cancellation.clone(), completion.clone());
        let attempt = Attempt {
            id,
            registration,
            storage,
            verifier,
            nonce,
            state: anti_csrf,
            redirect,
            cancellation,
        };
        let manager = self.clone();
        tokio::spawn(async move {
            let result = tokio::time::timeout(
                AUTHORIZATION_DEADLINE,
                manager.authorize(listener, &attempt),
            )
            .await
            .unwrap_or_else(|_| Err(Kind::Deadline.into()));
            manager.finish_attempt(attempt.id, result).await;
            completion.cancel();
        });
        Ok(ChatgptAuthorization { url: url.into() })
    }

    pub async fn cancel_sign_in(&self) -> ChatgptView {
        let mut state = self.inner.state.lock().await;
        let completion = state
            .sign_in
            .cancel(ChatgptError::from(Kind::Cancelled).message());
        drop(state);
        if let Some(completion) = completion {
            completion.cancelled().await;
        }
        self.view().await
    }

    async fn authorize(&self, listener: TcpListener, attempt: &Attempt) -> ChatgptResult<()> {
        let callback = callback::receive(
            listener,
            &attempt.redirect,
            &attempt.state,
            &attempt.cancellation,
        )
        .await?;
        let registration = self
            .resolve_registration(attempt, callback.client_id.as_deref())
            .await?;
        let response = self
            .inner
            .http
            .exchange(
                &registration.client_id,
                &callback.code,
                &attempt.verifier,
                &attempt.redirect,
                &attempt.cancellation,
            )
            .await?;
        let identity = self
            .inner
            .http
            .identity(
                &response.id_token,
                &registration.client_id,
                Some(&attempt.nonce),
                &attempt.cancellation,
            )
            .await?;
        if registration
            .subject
            .as_ref()
            .is_some_and(|subject| subject != &identity.sub)
        {
            return Err(Kind::IdentityMismatch.into());
        }
        let metadata = self.inner.store.read()?;
        let tokens = make_record(
            &metadata,
            &registration,
            &response,
            &identity.sub,
            &attempt.nonce,
            attempt.storage,
        );
        let mut models_provider_error = None;
        let (models, models_error) = if tokens.plan_enabled() {
            let bearer = ApiKey::from_chatgpt_token(tokens.access_token.clone())
                .map_err(|_| Kind::InvalidToken)?;
            match self
                .inner
                .model_client
                .list_chatgpt_models(&bearer, &attempt.cancellation)
                .await
            {
                Ok(models) => (models, None),
                Err(_) if attempt.cancellation.is_cancelled() => return Err(Kind::Cancelled.into()),
                Err(error) => {
                    models_provider_error = Some(error.clone());
                    (vec![], Some(error.message()))
                }
            }
        } else {
            (
                vec![],
                Some(ChatgptError::from(Kind::PlanPermission).message().into()),
            )
        };
        let _gate = self.inner.gate.lock().await;
        let _lease = self.lease(&attempt.cancellation).await?;
        let mut state = self.inner.state.lock().await;
        if attempt.cancellation.is_cancelled() || !state.sign_in.is_attempt(attempt.id) {
            return Err(Kind::Cancelled.into());
        }
        let mut metadata = self.inner.store.read()?;
        let previous_metadata = metadata.clone();
        let updated = metadata
            .registrations
            .iter_mut()
            .find(|value| value.id == registration.id)
            .ok_or(Kind::Storage)?;
        if updated.client_id != registration.client_id
            || updated
                .subject
                .as_ref()
                .is_some_and(|subject| subject != &identity.sub)
        {
            return Err(Kind::IdentityMismatch.into());
        }
        updated.subject = Some(identity.sub);
        updated.email = identity.email;
        if attempt.storage == CredentialStorage::System {
            updated.system_stored = true;
        }
        if attempt.storage == CredentialStorage::System {
            self.inner.store.write(&metadata)?;
            self.save_system(&tokens).await?;
        }
        let updated = metadata
            .registrations
            .iter_mut()
            .find(|value| value.id == registration.id)
            .ok_or(Kind::Storage)?;
        updated.reconnect_required = false;
        updated.refresh_pending = None;
        updated.last_plan_enabled = Some(tokens.plan_enabled());
        updated.system_stored = attempt.storage == CredentialStorage::System;
        metadata.active = Some(registration.id);
        self.inner.store.write(&metadata)?;
        if attempt.storage == CredentialStorage::Session
            && previous_metadata
                .registration(registration.id)?
                .system_stored
        {
            let vault = self.inner.vault.clone();
            let id = registration.id;
            let removed = tokio::task::spawn_blocking(move || vault.remove(id))
                .await
                .map_err(|_| Kind::VaultUnavailable)?;
            if let Err(error) = removed {
                self.inner.store.write(&previous_metadata)?;
                return Err(error);
            }
        }
        if attempt.storage == CredentialStorage::Session {
            state.sessions.insert(registration.id, tokens);
        } else {
            state.sessions.remove(&registration.id);
        }
        state.pending_rotations.remove(&registration.id);
        state.metadata = metadata;
        state.models.insert(registration.id, models);
        state.model_errors.remove(&registration.id);
        state.model_provider_errors.remove(&registration.id);
        state.epochs.insert(registration.id, Uuid::new_v4());
        if let Some(error) = models_provider_error {
            state.model_provider_errors.insert(registration.id, error);
        }
        if let Some(error) = models_error {
            state.model_errors.insert(registration.id, error);
        }
        state.storage_error = None;
        state.message = Some("ChatGPT connected.".into());
        Ok(())
    }

    async fn resolve_registration(
        &self,
        attempt: &Attempt,
        returned_client: Option<&str>,
    ) -> ChatgptResult<Registration> {
        if let Some(registration) = &attempt.registration {
            if returned_client.is_some_and(|client| client != registration.client_id) {
                return Err(Kind::ClientMismatch.into());
            }
            return Ok(registration.clone());
        }
        let client = returned_client
            .filter(|client| valid_client(client))
            .ok_or(Kind::RegistrationIncomplete)?;
        let _gate = self.inner.gate.lock().await;
        let _lease = self.lease(&attempt.cancellation).await?;
        let mut metadata = self.inner.store.read()?;
        if metadata.registrations.iter().any(|registration| {
            registration.client_id == client
                && registration.issuer == self.inner.http.endpoints.issuer
        }) {
            return Err(Kind::ClientMismatch.into());
        }
        let registration = Registration {
            id: Uuid::new_v4(),
            label: format!("ChatGPT account {}", metadata.registrations.len() + 1),
            email: None,
            subject: None,
            client_id: client.into(),
            issuer: self.inner.http.endpoints.issuer.clone(),
            system_stored: false,
            reconnect_required: true,
            refresh_pending: None,
            last_plan_enabled: None,
            welcomed: false,
        };
        metadata.registrations.push(registration.clone());
        self.inner.store.write(&metadata)?;
        Ok(registration)
    }

    async fn finish_attempt(&self, id: Uuid, result: ChatgptResult<()>) {
        let mut state = self.inner.state.lock().await;
        if !state.sign_in.finish(id, &result) {
            return;
        }
        if let Err(error) = result
            && matches!(error.kind, Kind::Storage | Kind::VaultUnavailable)
        {
            state.storage_error = Some(error.message().into());
        }
    }

    pub async fn select_account(&self, account_id: Uuid) -> ChatgptResult<ChatgptView> {
        let _gate = self.inner.gate.lock().await;
        let cancellation = CancellationToken::new();
        let _lease = self.lease(&cancellation).await?;
        let mut metadata = self.inner.store.read()?;
        metadata.registration(account_id)?;
        metadata.active = Some(account_id);
        self.inner.store.write(&metadata)?;
        let mut state = self.inner.state.lock().await;
        state
            .sign_in
            .cancel("Pending sign-in was cancelled when you selected a saved account.");
        state.metadata = metadata;
        drop(state);
        drop(_lease);
        drop(_gate);
        Ok(self.view().await)
    }
    pub async fn acknowledge_welcome(&self) -> ChatgptResult<ChatgptView> {
        let _gate = self.inner.gate.lock().await;
        let cancellation = CancellationToken::new();
        let _lease = self.lease(&cancellation).await?;
        let mut metadata = self.inner.store.read()?;
        if let Some(id) = metadata.active {
            let registration = metadata.registration(id)?;
            let session = self.inner.state.lock().await.sessions.get(&id).cloned();
            let tokens = match session {
                Some(tokens) => tokens,
                None if registration.system_stored => self
                    .load_system(&metadata, registration)
                    .await?
                    .ok_or(Kind::Reconnect)?,
                None => return Err(Kind::Reconnect.into()),
            };
            if registration.reconnect_required || !tokens.plan_enabled() {
                return Err(Kind::PlanPermission.into());
            }
            let registration = metadata
                .registrations
                .iter_mut()
                .find(|value| value.id == id)
                .ok_or(Kind::Storage)?;
            registration.welcomed = true;
            self.inner.store.write(&metadata)?;
        }
        self.inner.state.lock().await.metadata = metadata;
        drop(_lease);
        drop(_gate);
        Ok(self.view().await)
    }

    pub async fn sign_out(&self, account_id: Uuid) -> ChatgptResult<ChatgptView> {
        let completion = {
            let mut state = self.inner.state.lock().await;
            if matches!(
                &state.sign_in,
                SignIn::SigningIn { account_id: pending_account, .. }
                    if *pending_account == Some(account_id)
            ) {
                state.sign_in.cancel(
                    "Pending reconnect was cancelled when you signed out of this registration.",
                )
            } else {
                None
            }
        };
        if let Some(completion) = completion {
            completion.cancelled().await;
        }
        let _gate = self.inner.gate.lock().await;
        let cancellation = CancellationToken::new();
        let _lease = self.lease(&cancellation).await?;
        let metadata = self.inner.store.read()?;
        let registration = metadata.registration(account_id)?.clone();
        let (session, retained_rotation) = {
            let mut state = self.inner.state.lock().await;
            discard_stale_rotations(&mut state, &metadata);
            let rotation = state
                .pending_rotations
                .get(&account_id)
                .filter(|rotation| Some(rotation.id) == registration.refresh_pending);
            (
                rotation
                    .map(|rotation| &rotation.tokens)
                    .or_else(|| {
                        (!registration.system_stored)
                            .then(|| state.sessions.get(&account_id))
                            .flatten()
                    })
                    .cloned(),
                rotation.is_some(),
            )
        };
        let tokens = match session {
            Some(tokens) => Some(tokens),
            None if registration.system_stored => {
                self.load_system(&metadata, &registration).await?
            }
            None => None,
        };
        let confirmed = if let Some(tokens) = tokens {
            self.inner
                .http
                .revoke(
                    &registration.client_id,
                    &tokens.refresh_token,
                    &cancellation,
                )
                .await
                .is_ok()
                && (registration.refresh_pending.is_none() || retained_rotation)
        } else {
            false
        };
        if let Err(error) = self
            .disconnect_locked(&metadata, &registration, false)
            .await
        {
            self.inner.state.lock().await.storage_error = Some(error.message().into());
            return Err(error);
        }
        let message = if confirmed {
            "Signed out. Remote access was revoked."
        } else {
            "Signed out locally. Remote session revocation was not confirmed. Disconnect this app in ChatGPT Settings to end remote access."
        };
        self.inner.state.lock().await.message = Some(message.into());
        drop(_lease);
        drop(_gate);
        Ok(self.view().await)
    }
    async fn disconnect_locked(
        &self,
        metadata: &Metadata,
        registration: &Registration,
        invalid: bool,
    ) -> ChatgptResult<()> {
        let mut next = metadata.clone();
        let record = next
            .registrations
            .iter_mut()
            .find(|value| value.id == registration.id)
            .ok_or(Kind::Storage)?;
        record.reconnect_required = true;
        record.refresh_pending = None;
        self.inner.store.write(&next)?;
        {
            let mut state = self.inner.state.lock().await;
            state.sessions.remove(&registration.id);
            state.pending_rotations.remove(&registration.id);
            state.models.remove(&registration.id);
            state.model_errors.remove(&registration.id);
            state.model_provider_errors.remove(&registration.id);
            state.metadata = next.clone();
            state.epochs.insert(registration.id, Uuid::new_v4());
        }
        if registration.system_stored {
            let vault = self.inner.vault.clone();
            let id = registration.id;
            tokio::task::spawn_blocking(move || vault.remove(id))
                .await
                .map_err(|_| Kind::VaultUnavailable)??;
            next.registrations
                .iter_mut()
                .find(|value| value.id == id)
                .ok_or(Kind::Storage)?
                .system_stored = false;
            self.inner.store.write(&next)?;
        }
        let mut state = self.inner.state.lock().await;
        state.metadata = next;
        if invalid {
            state.message = Some(ChatgptError::from(Kind::Reconnect).message().into());
        }
        Ok(())
    }

    async fn load_system(
        &self,
        metadata: &Metadata,
        registration: &Registration,
    ) -> ChatgptResult<Option<TokenRecord>> {
        let vault = self.inner.vault.clone();
        let id = registration.id;
        let value = tokio::task::spawn_blocking(move || vault.get(id))
            .await
            .map_err(|_| Kind::VaultUnavailable)??;
        let Some(value) = value else {
            return Ok(None);
        };
        if value.len() > storage::MAX_RECORD {
            return Err(Kind::Reconnect.into());
        }
        let tokens: TokenRecord = serde_json::from_str(&value).map_err(|_| Kind::Reconnect)?;
        tokens.validate_mapping(metadata, registration)?;
        Ok(Some(tokens))
    }
    fn set_refresh_pending(
        &self,
        metadata: &Metadata,
        account_id: Uuid,
        pending: Option<Uuid>,
    ) -> ChatgptResult<Metadata> {
        let mut next = metadata.clone();
        next.registrations
            .iter_mut()
            .find(|registration| registration.id == account_id)
            .ok_or(Kind::Reconnect)?
            .refresh_pending = pending;
        self.inner.store.write(&next)?;
        Ok(next)
    }
    async fn save_system(&self, tokens: &TokenRecord) -> ChatgptResult<()> {
        let serialized = Zeroizing::new(serde_json::to_string(tokens).map_err(|_| Kind::Storage)?);
        if serialized.len() > storage::MAX_RECORD {
            return Err(Kind::Storage.into());
        }
        let vault = self.inner.vault.clone();
        let id = tokens.registration_id;
        tokio::task::spawn_blocking(move || vault.set(id, &serialized))
            .await
            .map_err(|_| Kind::VaultUnavailable)??;
        Ok(())
    }
    async fn lease(&self, cancellation: &CancellationToken) -> ChatgptResult<File> {
        lock::lease(self.inner.store.open_lock()?, cancellation).await
    }
}

fn discard_stale_rotations(state: &mut State, metadata: &Metadata) {
    let stale: Vec<_> = state
        .pending_rotations
        .iter()
        .filter_map(|(id, rotation)| {
            (!metadata.registrations.iter().any(|registration| {
                registration.id == *id
                    && !registration.reconnect_required
                    && registration.refresh_pending == Some(rotation.id)
            }))
            .then_some(*id)
        })
        .collect();
    for id in stale {
        state.pending_rotations.remove(&id);
        state.sessions.remove(&id);
        state.models.remove(&id);
        state.model_errors.remove(&id);
        state.model_provider_errors.remove(&id);
        state.epochs.insert(id, Uuid::new_v4());
    }
    if state.pending_rotations.is_empty()
        && state.storage_error.as_deref() == Some(ROTATION_STORAGE_MESSAGE)
    {
        state.storage_error = None;
    }
}

fn random_value() -> Zeroizing<String> {
    Zeroizing::new(format!(
        "{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    ))
}
fn make_record(
    metadata: &Metadata,
    registration: &Registration,
    response: &TokenResponse,
    subject: &str,
    nonce: &str,
    storage: CredentialStorage,
) -> TokenRecord {
    TokenRecord {
        version: 1,
        registration_id: registration.id,
        host_id: metadata.host_id,
        client_id: registration.client_id.clone(),
        subject: subject.into(),
        issuer: registration.issuer.clone(),
        access_token: response.access_token.clone(),
        refresh_token: response.refresh_token.clone(),
        id_token: response.id_token.clone(),
        nonce: nonce.into(),
        scopes: response.scopes(),
        expires_at: now().saturating_add(response.expires_in),
        earliest_refresh_at: response.earliest_refresh_at,
        storage,
    }
}

#[cfg(test)]
mod tests;
