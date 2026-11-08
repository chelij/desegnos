use crate::{
    ai_access::{Access, Connection, credential_key},
    chatgpt::ChatgptState,
    native_files::LockExt,
    native_files::{self, NativeResult, SMALL_RECORD_BYTES, message, private_directory},
};
use desegnos_ai::{
    ApiKey, AuthenticationMethod, CancellationToken, ChatgptView, CredentialInfo,
    CredentialManager, CredentialStorage, ErrorKind, ModelInfo, OpenAiClient, ProviderClient,
    ProviderDescriptor, ProviderError, ProviderId, ProviderSettings, SubmissionOutcome,
    chatgpt_model_catalogue, provider_catalogue,
};
use desegnos_core::{GenerationMode, RequirementsSnapshot, digest};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tauri::State;
use uuid::Uuid;

pub(crate) type SettingsState<'a> = State<'a, Mutex<AiSettingsStore>>;
const RECORD: &str = "AI settings";
fn read_json<T: for<'de> Deserialize<'de>>(path: &Path, limit: u64) -> NativeResult<T> {
    native_files::read_json(path, limit, RECORD)
}

fn write_json(path: &Path, value: &impl Serialize) -> NativeResult<()> {
    native_files::write_json(path, value, SMALL_RECORD_BYTES, RECORD)
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProviderPreference {
    version: u32,
    provider: ProviderId,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AuthenticationPreference {
    version: u32,
    authentication: AuthenticationMethod,
}

#[derive(ts_rs::TS, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiReadiness {
    ready: bool,
    api_key_ready: bool,
    blocked_reason: Option<String>,
    can_test: bool,
    can_refresh_models: bool,
    images_usable: bool,
    accounts: BTreeMap<Uuid, bool>,
}

#[derive(ts_rs::TS, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSettingsView {
    readiness: AiReadiness,
    image_credential: Option<String>,
    image_key: CredentialInfo,
    main_image_access: Option<desegnos_ai::ImageAccess>,
    image_key_access: Option<desegnos_ai::ImageAccess>,
    image_access: Option<desegnos_ai::ImageAccess>,
    image_access_checking: bool,
    provider: ProviderId,
    providers: Vec<ProviderDescriptor>,
    settings: ProviderSettings,
    credential: CredentialInfo,
    catalogue: Vec<ModelInfo>,
    models_error: Option<String>,
    models_provider_error: Option<ProviderError>,
    models_auth_required: bool,
    configuration_error: Option<String>,
    authentication: AuthenticationMethod,
    chatgpt: ChatgptView,
}

impl AiSettingsView {
    pub(super) fn require_ready(&self) -> Result<(), crate::native_error::NativeFailure> {
        match &self.readiness.blocked_reason {
            Some(reason) => Err(crate::native_error::NativeFailure::from_serializable(
                reason,
                crate::native_error::NativeErrorKind::AiSettings,
            )),
            None => Ok(()),
        }
    }

    pub(super) fn with_warning(mut self, warning: Option<String>) -> Self {
        if let Some(warning) = warning {
            self.readiness.ready = false;
            self.readiness.api_key_ready = false;
            self.readiness
                .accounts
                .values_mut()
                .for_each(|ready| *ready = false);
            self.readiness.can_test = false;
            self.readiness.blocked_reason = Some(warning.clone());
            self.configuration_error = Some(match self.configuration_error.take() {
                Some(existing) => format!("{existing} {warning}"),
                None => warning,
            });
        }
        self
    }
}

pub struct AiSettingsStore {
    pub(super) switch_guard: crate::ai_job::SwitchGuard,
    pub(super) api_models: BTreeMap<(ProviderId, String), desegnos_ai::Result<Vec<ModelInfo>>>,
    pub(super) image_credentials: Arc<CredentialManager>,
    pub(super) image_access: BTreeMap<String, desegnos_ai::ImageAccess>,
    pub(super) image_access_checking: BTreeMap<String, Uuid>,
    pub(super) provider: ProviderId,
    pub(super) provider_credentials: BTreeMap<ProviderId, Arc<CredentialManager>>,
    pub(super) directory: PathBuf,
    pub(super) settings: ProviderSettings,
    pub(super) configuration_error: Option<String>,
    pub(super) authentication: AuthenticationMethod,
    pub(super) authentication_error: Option<String>,
    pub(super) credentials: Arc<CredentialManager>,
    pub(super) client: Result<OpenAiClient, String>,
}

async fn api_catalogue_for_key(
    state: &Mutex<AiSettingsStore>,
    provider: ProviderId,
    settings: &ProviderSettings,
    key: &ApiKey,
    cancellation: &CancellationToken,
    force: bool,
) -> desegnos_ai::Result<Vec<ModelInfo>> {
    let credential_id = catalogue_identity(provider, settings, key)?;
    let client = {
        let service = state.locked();
        if !force
            && let Some(Ok(cached)) = service.api_models.get(&(provider, credential_id.clone()))
        {
            return Ok(cached.clone());
        }
        ProviderClient::with_settings(
            provider,
            service.client.clone().map_err(|_| {
                ProviderError::new(ErrorKind::InvalidSettings, SubmissionOutcome::NotSubmitted)
            })?,
            settings,
        )
        .map_err(|_| {
            ProviderError::new(ErrorKind::InvalidSettings, SubmissionOutcome::NotSubmitted)
        })?
    };
    let result = client.list_api_models(key, cancellation).await;
    if !cancellation.is_cancelled() {
        state
            .locked()
            .api_models
            .insert((provider, credential_id), result.clone());
    }
    result
}

fn catalogue_identity(
    provider: ProviderId,
    settings: &ProviderSettings,
    key: &ApiKey,
) -> desegnos_ai::Result<String> {
    let mut id = key.image_access_cache_key();
    if provider == ProviderId::Local {
        id.push_str(&digest(desegnos_ai::local_endpoint(settings)?.as_bytes()));
    }
    Ok(id)
}

pub(super) fn provider_key(
    provider: ProviderId,
    local_authentication: bool,
    credentials: &CredentialManager,
) -> desegnos_ai::Result<ApiKey> {
    if provider == ProviderId::Local && !local_authentication {
        return Ok(ApiKey::local_anonymous());
    }
    credentials.key()
}

pub(super) async fn api_model_for_key(
    state: &Mutex<AiSettingsStore>,
    provider: ProviderId,
    settings: &ProviderSettings,
    key: &ApiKey,
    cancellation: &CancellationToken,
) -> desegnos_ai::Result<ModelInfo> {
    let models = api_catalogue_for_key(state, provider, settings, key, cancellation, false).await?;
    let model = models
        .into_iter()
        .find(|model| model.id == settings.model)
        .ok_or_else(|| {
            ProviderError::new(ErrorKind::ModelUnavailable, SubmissionOutcome::NotSubmitted)
        })?;
    model.validate_api_settings(settings)?;
    Ok(model)
}

pub(super) async fn refresh_api_models(state: &Mutex<AiSettingsStore>) -> NativeResult<()> {
    let (provider, settings) = {
        let service = state.locked();
        if service.authentication != AuthenticationMethod::ApiKey {
            return Ok(());
        }
        (service.provider, service.settings.clone())
    };
    let key = credential_key(state, provider, &settings).await;
    if let Ok(key) = key {
        // Safe provider failures belong in the returned model status, not an API-key fallback.
        let _ = api_catalogue_for_key(
            state,
            provider,
            &settings,
            &key,
            &CancellationToken::new(),
            true,
        )
        .await;
    } else {
        state
            .locked()
            .api_models
            .retain(|(id, _), _| *id != provider);
    }
    Ok(())
}

impl AiSettingsStore {
    fn image_cache_id(&self, account: Option<Uuid>) -> Option<String> {
        if self.provider != ProviderId::Openai {
            return None;
        }
        match self.authentication {
            AuthenticationMethod::Chatgpt => account.map(|id| format!("chatgpt:{id}")),
            AuthenticationMethod::ApiKey => self
                .credentials
                .key()
                .ok()
                .map(|key| key.image_access_cache_key()),
        }
    }
    fn image_key_cache_id(&self) -> Option<String> {
        self.image_credentials
            .key()
            .ok()
            .map(|key| key.image_access_cache_key())
    }
    pub(super) fn selected_image_cache_id(&self, account: Option<Uuid>) -> Option<String> {
        let main = self.image_cache_id(account);
        if self.provider != ProviderId::Openai {
            return main;
        }
        let image = self.image_key_cache_id();
        if desegnos_ai::use_image_key(
            main.as_ref().and_then(|id| self.image_access.get(id)),
            image.as_ref().and_then(|id| self.image_access.get(id)),
        ) {
            image
        } else {
            main
        }
    }
    fn store_image_access(
        &mut self,
        id: String,
        access: desegnos_ai::ImageAccess,
    ) -> NativeResult<()> {
        self.image_access.insert(id, access);
        write_json(
            &self.directory.join("image-api-access.json"),
            &self.image_access,
        )
    }
    pub(super) fn invalidate_image_account(&mut self, account: Uuid) -> NativeResult<()> {
        let id = format!("chatgpt:{account}");
        self.image_access.remove(&id);
        // A reconnect invalidates any outstanding probe for the previous session.
        self.image_access_checking.remove(&id);
        write_json(
            &self.directory.join("image-api-access.json"),
            &self.image_access,
        )
    }
    pub fn initialize(data: &Path, switch_guard: crate::ai_job::SwitchGuard) -> NativeResult<Self> {
        Self::from_directory(data.join("generation"), switch_guard)
    }

    pub(super) fn from_directory(
        directory: PathBuf,
        switch_guard: crate::ai_job::SwitchGuard,
    ) -> NativeResult<Self> {
        let directory_error = private_directory(&directory, RECORD).err();
        let path = directory.join("settings.json");
        let loaded = if let Some(error) = directory_error {
            Err(error)
        } else if path.exists() {
            read_json::<ProviderSettings>(&path, SMALL_RECORD_BYTES).and_then(|settings| {
                settings
                    .validate_for_auth(AuthenticationMethod::Chatgpt)
                    .map_err(message)?;
                Ok(settings)
            })
        } else {
            Ok(ProviderSettings::default())
        };
        let (mut settings, mut configuration_error) = match loaded {
            Ok(settings) => (settings, None),
            Err(error) => (
                ProviderSettings::default(),
                Some(format!(
                    "Saved AI settings could not be loaded: {error} Review and explicitly save settings before generating. The original file remains on disk."
                )),
            ),
        };
        let preference_path = directory.join("authentication.json");
        let preference = if preference_path.exists() {
            read_json::<AuthenticationPreference>(&preference_path, 4096).and_then(|value| {
                if value.version == 1 {
                    Ok(value.authentication)
                } else {
                    Err("The authentication preference has an unsupported format.".into())
                }
            })
        } else {
            Ok(AuthenticationMethod::ApiKey)
        };
        let (authentication, authentication_error) = match preference {
            Ok(authentication) => (authentication, None),
            Err(error) => (
                AuthenticationMethod::ApiKey,
                Some(format!(
                    "Saved authentication settings could not be loaded: {error} Explicitly choose a generation method before sending. The original file remains on disk."
                )),
            ),
        };
        if configuration_error.is_none()
            && let Err(error) = settings.validate_for_auth(authentication)
        {
            configuration_error = Some(format!(
                "Saved AI settings could not be loaded: {error} Review and explicitly save settings before generating. The original file remains on disk."
            ));
            settings = ProviderSettings::default();
        }
        let credentials = Arc::new(CredentialManager::new());
        let client = OpenAiClient::new().map_err(message);
        let (credentials, client) = crate::testing::isolated_openai(credentials, client);
        let image_cache_path = directory.join("image-api-access.json");
        let image_access = if image_cache_path.exists() {
            read_json(&image_cache_path, 1024 * 1024)?
        } else {
            BTreeMap::new()
        };
        let mut service = Self {
            switch_guard,
            api_models: BTreeMap::new(),
            image_credentials: Arc::new(CredentialManager::for_images()),
            image_access,
            image_access_checking: BTreeMap::new(),
            provider: ProviderId::Openai,
            provider_credentials: BTreeMap::new(),
            directory,
            settings,
            configuration_error,
            authentication,
            authentication_error,
            credentials,
            client,
        };
        let preference = service.directory.join("provider.json");
        if preference.exists() {
            match read_json::<ProviderPreference>(&preference, 4096).and_then(|p| {
                if p.version != 1 {
                    return Err("The saved AI choice has an unsupported format.".into());
                }
                service.select_provider(p.provider, false)
            }) {
                Ok(()) => (),
                Err(error) => service.configuration_error = Some(error),
            }
        }
        Ok(service)
    }

    fn settings_path(&self, provider: ProviderId) -> PathBuf {
        if provider == ProviderId::Openai {
            self.directory.join("settings.json")
        } else {
            self.directory.join(format!(
                "settings-{}.json",
                serde_json::to_value(provider)
                    .expect("provider is serializable")
                    .as_str()
                    .expect("provider is a string")
            ))
        }
    }
    pub(super) fn adapter(&self, provider: ProviderId) -> NativeResult<ProviderClient> {
        ProviderClient::with_settings(provider, self.client.clone()?, &self.settings)
            .map_err(message)
    }
    fn set_local_authentication(&mut self, enabled: bool) -> NativeResult<()> {
        let path = self.settings_path(ProviderId::Local);
        let mut settings = if self.provider == ProviderId::Local {
            self.settings.clone()
        } else if path.exists() {
            read_json(&path, SMALL_RECORD_BYTES)?
        } else {
            ProviderId::Local.defaults()
        };
        settings.local_authentication = enabled;
        write_json(&path, &settings)?;
        if self.provider == ProviderId::Local {
            self.settings = settings;
        }
        Ok(())
    }
    pub(super) fn credentials_for(&mut self, provider: ProviderId) -> Arc<CredentialManager> {
        if provider == self.provider {
            return self.credentials.clone();
        }
        self.provider_credentials
            .entry(provider)
            .or_insert_with(|| Arc::new(CredentialManager::for_provider(provider)))
            .clone()
    }
    fn select_provider(&mut self, provider: ProviderId, persist: bool) -> NativeResult<()> {
        self.switch_guard.ensure_switch_allowed()?;

        let authentication = if provider == ProviderId::Openai {
            let path = self.directory.join("authentication.json");
            if path.exists() {
                let p: AuthenticationPreference = read_json(&path, 4096)?;
                if p.version != 1 {
                    return Err("The saved sign-in preference is unsupported.".into());
                }
                p.authentication
            } else {
                AuthenticationMethod::ApiKey
            }
        } else {
            AuthenticationMethod::ApiKey
        };
        let path = self.settings_path(provider);
        let settings = if path.exists() {
            read_json::<ProviderSettings>(&path, SMALL_RECORD_BYTES)?
        } else {
            provider.defaults()
        };
        provider
            .validate(&settings, authentication)
            .map_err(message)?;
        self.adapter(provider)?;
        if persist {
            write_json(
                &self.directory.join("provider.json"),
                &ProviderPreference {
                    version: 1,
                    provider,
                },
            )?;
        }
        self.provider_credentials
            .insert(self.provider, self.credentials.clone());
        self.credentials = self.credentials_for(provider);
        self.provider = provider;
        self.authentication = authentication;
        self.settings = settings;
        self.configuration_error = None;
        self.authentication_error = None;
        Ok(())
    }

    pub(super) fn connection(&self) -> Connection {
        Connection {
            provider: self.provider,
            authentication: self.authentication,
            settings: self.settings.clone(),
            account_id: None,
        }
    }

    pub(super) fn is_selected(&self, access: &Access) -> bool {
        self.authentication == access.authentication
            && self.provider == access.provider
            && self.settings == access.settings
    }

    pub(super) fn ready_connection(&self) -> NativeResult<Connection> {
        if let Some(error) = self
            .configuration_error
            .as_ref()
            .or(self.authentication_error.as_ref())
        {
            return Err(error.clone());
        }
        self.provider
            .validate(&self.settings, self.authentication)
            .map_err(message)?;
        Ok(self.connection())
    }

    /// The same complete request check the paid send performs, without credentials or network.
    pub(super) fn export_estimate(
        &self,
        plan: &desegnos_core::ExportPlan,
        snapshot: &RequirementsSnapshot,
        mode: GenerationMode,
    ) -> NativeResult<f64> {
        let client = self.adapter(self.provider)?;
        client
            .estimate(
                self.authentication,
                snapshot,
                mode,
                &plan.schema,
                &self.settings,
            )
            .map(|estimate| estimate.estimate_usd)
            .map_err(message)
    }

    pub(super) fn chatgpt_directory(&self) -> PathBuf {
        self.directory.join("chatgpt")
    }

    pub(super) fn client(&self) -> NativeResult<OpenAiClient> {
        self.client.clone()
    }

    pub(super) fn set_authentication(
        &mut self,
        authentication: AuthenticationMethod,
    ) -> NativeResult<()> {
        if self.provider != ProviderId::Openai {
            return Err("Choose OpenAI before changing its sign-in method.".into());
        }
        write_json(
            &self.directory.join("authentication.json"),
            &AuthenticationPreference {
                version: 1,
                authentication,
            },
        )?;
        self.authentication = authentication;
        self.authentication_error = None;
        Ok(())
    }
}

/// No lock is held across network I/O. The cache key binds results to the captured credential/account.
pub(super) async fn refresh_image_access(
    state: &Mutex<AiSettingsStore>,
    chatgpt: &ChatgptState,
    force: bool,
) -> NativeResult<()> {
    refresh_main_image_access(state, chatgpt, force).await?;
    let probe = {
        let mut service = state.locked();
        if service.provider != ProviderId::Openai {
            return Ok(());
        }
        let Ok(key) = service.image_credentials.key() else {
            return Ok(());
        };
        let id = key.image_access_cache_key();
        if service.image_access_checking.contains_key(&id)
            || (!force
                && service
                    .image_access
                    .get(&id)
                    .is_some_and(|a| !a.expired(desegnos_ai::image_access_now())))
        {
            return Ok(());
        }
        let client = service.client.clone()?;
        let probe_id = Uuid::new_v4();
        service.image_access_checking.insert(id.clone(), probe_id);
        let prior_access = service.image_access.get(&id).cloned();
        (id, key, client, probe_id, prior_access)
    };
    let (id, key, client, probe_id, prior_access) = probe;
    let access = client.probe_image_access(&key).await;
    let mut service = state.locked();
    if service.image_access_checking.get(&id) == Some(&probe_id) {
        service.image_access_checking.remove(&id);
        if service.image_key_cache_id().as_ref() == Some(&id)
            && service.image_access.get(&id) == prior_access.as_ref()
        {
            service.store_image_access(id, access)?;
        }
    }
    Ok(())
}

async fn refresh_main_image_access(
    state: &Mutex<AiSettingsStore>,
    chatgpt: &ChatgptState,
    force: bool,
) -> NativeResult<()> {
    let view = chatgpt.view().await;
    if view.sign_in.status == desegnos_ai::ChatgptSignInStatus::AwaitingBrowser {
        return Ok(());
    }
    let (id, auth, client, credentials, prior_access) = {
        let service = state.locked();
        let Some(id) = service.image_cache_id(view.active_account_id) else {
            return Ok(());
        };
        if service.image_access_checking.contains_key(&id)
            || (!force
                && service
                    .image_access
                    .get(&id)
                    .is_some_and(|access| !access.expired(desegnos_ai::image_access_now())))
        {
            return Ok(());
        }
        if service.authentication == AuthenticationMethod::Chatgpt
            && !view
                .accounts
                .iter()
                .any(|account| Some(account.id) == view.active_account_id && account.usable())
        {
            return Ok(());
        }
        (
            id.clone(),
            service.authentication,
            service.client.clone()?,
            service.credentials.clone(),
            service.image_access.get(&id).cloned(),
        )
    };
    let key = if auth == AuthenticationMethod::Chatgpt {
        let Some(account) = view.active_account_id else {
            return Ok(());
        };
        match chatgpt
            .manager()?
            .bearer(account, &CancellationToken::new())
            .await
        {
            Ok(key) => key,
            Err(_) => return Ok(()),
        }
    } else {
        match tauri::async_runtime::spawn_blocking(move || credentials.key())
            .await
            .map_err(message)?
        {
            Ok(key) => key,
            Err(_) => return Ok(()),
        }
    };
    if auth == AuthenticationMethod::ApiKey && key.image_access_cache_key() != id {
        return Ok(());
    }
    let probe_id = Uuid::new_v4();
    {
        let mut service = state.locked();
        if service.image_access_checking.contains_key(&id) {
            return Ok(());
        }
        service.image_access_checking.insert(id.clone(), probe_id);
    }
    let access = client.probe_image_access(&key).await;
    let mut service = state.locked();
    if service.image_access_checking.get(&id) != Some(&probe_id) {
        return Ok(());
    }
    service.image_access_checking.remove(&id);
    // A runtime rejection or reconnect during this probe takes precedence over its late answer.
    if service.image_access.get(&id) != prior_access.as_ref() {
        return Ok(());
    }
    service.store_image_access(id, access)
}

pub(super) async fn settings_view(
    state: &SettingsState<'_>,
    chatgpt_state: &ChatgptState,
) -> NativeResult<AiSettingsView> {
    refresh_image_access(state, chatgpt_state, false).await?;
    let (settings, credentials, configuration_error, authentication, provider) = {
        let service = state.locked();
        (
            service.settings.clone(),
            service.credentials.clone(),
            [
                service.configuration_error.clone(),
                service.authentication_error.clone(),
                service.client.as_ref().err().cloned(),
            ]
            .into_iter()
            .flatten()
            .reduce(|first, next| format!("{first} {next}")),
            service.authentication,
            service.provider,
        )
    };
    let connection_settings = settings.clone();
    let local_authentication = settings.local_authentication;
    let (credential, credential_id) = tauri::async_runtime::spawn_blocking(move || {
        let credential =
            if provider == ProviderId::Local && !connection_settings.local_authentication {
                CredentialInfo {
                    configured: true,
                    storage: desegnos_ai::CredentialLocation::None,
                    masked_identifier: Some("Local server".into()),
                    vault_error: None,
                }
            } else {
                credentials.status()
            };
        let identity = provider_key(
            provider,
            connection_settings.local_authentication,
            &credentials,
        )
        .ok()
        .and_then(|key| catalogue_identity(provider, &connection_settings, &key).ok());
        (credential, identity)
    })
    .await
    .map_err(message)?;
    let (api_catalogue, models_error, models_auth_required, models_provider_error) = {
        let service = state.locked();
        match credential_id
            .as_ref()
            .and_then(|id| service.api_models.get(&(provider, id.clone())))
        {
            Some(cache) => match cache {
                Ok(models) => (models.clone(), None, false, None),
                Err(error) => (
                    vec![],
                    Some(desegnos_ai::models_error(
                        provider,
                        local_authentication,
                        error,
                    )),
                    desegnos_ai::models_need_key(provider, local_authentication, error),
                    Some(error.clone()),
                ),
            },
            None => (vec![], None, false, None),
        }
    };
    let chatgpt = chatgpt_state.view().await;
    let catalogue = if authentication == AuthenticationMethod::Chatgpt {
        if chatgpt
            .accounts
            .iter()
            .any(|account| Some(account.id) == chatgpt.active_account_id && account.usable())
        {
            chatgpt_model_catalogue(&chatgpt.models)
        } else {
            vec![]
        }
    } else {
        api_catalogue
    };
    let (
        image_access,
        image_access_checking,
        image_credential,
        main_image_access,
        image_key_access,
        image_key,
    ) = {
        let service = state.locked();
        let main = service.image_cache_id(chatgpt.active_account_id);
        let image = service.image_key_cache_id();
        let id = service.selected_image_cache_id(chatgpt.active_account_id);
        let credential = id
            .as_ref()
            .filter(|id| {
                service.image_access.get(*id).is_some_and(|a| {
                    a.available == desegnos_ai::ImageAvailability::Yes
                        && !a.expired(desegnos_ai::image_access_now())
                })
            })
            .map(|id| {
                if main.as_ref() == Some(id) {
                    if authentication == AuthenticationMethod::Chatgpt {
                        "chatgpt"
                    } else {
                        "connection"
                    }
                } else {
                    "imageKey"
                }
                .to_owned()
            });
        (
            id.as_ref()
                .and_then(|id| service.image_access.get(id))
                .cloned(),
            id.as_ref()
                .is_some_and(|id| service.image_access_checking.contains_key(id))
                || image
                    .as_ref()
                    .is_some_and(|id| service.image_access_checking.contains_key(id)),
            credential,
            main.as_ref()
                .and_then(|id| service.image_access.get(id))
                .cloned(),
            image
                .as_ref()
                .and_then(|id| service.image_access.get(id))
                .cloned(),
            service.image_credentials.status(),
        )
    };
    let accounts: BTreeMap<_, _> = chatgpt
        .accounts
        .iter()
        .map(|account| (account.id, account.usable()))
        .collect();
    let connection_ready = if authentication == AuthenticationMethod::Chatgpt {
        chatgpt
            .active_account_id
            .is_some_and(|id| accounts.get(&id) == Some(&true))
    } else {
        credential.configured
    };
    let blocked_reason = configuration_error.clone().or_else(|| {
        if !connection_ready {
            Some("Connect AI and repair any secure storage problems in AI settings.".into())
        } else if authentication == AuthenticationMethod::Chatgpt
            && !chatgpt.models.is_empty()
            && !catalogue.iter().any(|model| model.id == settings.model)
        {
            Some("Choose and save a supported model in AI settings.".into())
        } else {
            None
        }
    });
    let ready = blocked_reason.is_none();
    let readiness = AiReadiness {
        ready,
        api_key_ready: configuration_error.is_none() && credential.configured,
        blocked_reason,
        can_test: ready && !settings.model.is_empty(),
        can_refresh_models: connection_ready,
        images_usable: connection_ready
            && !models_auth_required
            && provider_catalogue()
                .iter()
                .any(|item| item.id == provider && item.capabilities.image_generation.is_some())
            && image_access
                .as_ref()
                .is_some_and(|access| access.available == desegnos_ai::ImageAvailability::Yes),
        accounts: accounts
            .into_iter()
            .map(|(id, ready)| (id, ready && configuration_error.is_none()))
            .collect(),
    };
    Ok(AiSettingsView {
        readiness,
        image_credential,
        image_key,
        main_image_access,
        image_key_access,
        image_access,
        image_access_checking,
        provider,
        providers: provider_catalogue(),
        settings,
        credential,
        catalogue,
        models_error,
        models_provider_error,
        models_auth_required,
        configuration_error,
        authentication,
        chatgpt,
    })
}

#[tauri::command]
pub async fn get_ai_settings(
    state: SettingsState<'_>,
    chatgpt: State<'_, ChatgptState>,
    refresh_models: Option<bool>,
) -> Result<AiSettingsView, crate::native_error::NativeFailure> {
    let result: NativeResult<AiSettingsView> = async {
        if refresh_models.unwrap_or(false) {
            let (authentication, client) = {
                let service = state.locked();
                (service.authentication, service.client.clone())
            };
            let view = chatgpt.view().await;
            if authentication == AuthenticationMethod::ApiKey {
                refresh_api_models(&state).await?;
            }
            if authentication == AuthenticationMethod::Chatgpt
                && let Some(account) = view
                    .accounts
                    .iter()
                    .find(|account| Some(account.id) == view.active_account_id && account.usable())
                && let Ok(client) = client
            {
                // Discovery errors remain visible in models_error; retain the settings view.
                let _ = chatgpt
                    .manager()?
                    .discover_models(account.id, &client, &CancellationToken::new())
                    .await;
            }
        }
        settings_view(&state, &chatgpt).await
    }
    .await;
    result.map_err(crate::native_error::NativeErrorKind::AiSettings.failure())
}

#[tauri::command]
pub async fn set_ai_provider(
    state: SettingsState<'_>,
    chatgpt: State<'_, ChatgptState>,
    provider: ProviderId,
) -> Result<AiSettingsView, crate::native_error::NativeFailure> {
    let result: NativeResult<AiSettingsView> = async {
        state.locked().select_provider(provider, true)?;
        refresh_api_models(&state).await?;
        settings_view(&state, &chatgpt).await
    }
    .await;
    result.map_err(crate::native_error::NativeErrorKind::AiSettings.failure())
}

#[tauri::command]
pub async fn save_ai_settings(
    state: SettingsState<'_>,
    chatgpt: State<'_, ChatgptState>,
    settings: ProviderSettings,
    connection_only: Option<bool>,
) -> Result<AiSettingsView, crate::native_error::NativeFailure> {
    let result: NativeResult<AiSettingsView> = async {
        if connection_only.unwrap_or(false) {
            {
                let mut service = state.locked();
                if service.provider != ProviderId::Local {
                    return Err("Choose Local before changing its server address.".into());
                }
                service.switch_guard.ensure_switch_allowed()?;
                let endpoint = desegnos_ai::local_endpoint(&settings).map_err(|_| {
                    format!(
                        "Use a local server address ending in /v1, such as {}.",
                        desegnos_ai::DEFAULT_LOCAL_ENDPOINT
                    )
                })?;
                let mut next = service.settings.clone();
                next.endpoint = Some(endpoint);
                next.local_authentication = settings.local_authentication;
                write_json(&service.settings_path(ProviderId::Local), &next)?;
                service.settings = next;
                service.configuration_error = None;
            }
            refresh_api_models(&state).await?;
            return settings_view(&state, &chatgpt).await;
        }
        let account = chatgpt.view().await;
        let (provider, authentication) = {
            let service = state.locked();
            (service.provider, service.authentication)
        };
        if authentication == AuthenticationMethod::ApiKey {
            let key = credential_key(&state, provider, &settings).await?;
            api_model_for_key(&state, provider, &settings, &key, &CancellationToken::new())
                .await
                .map_err(message)?;
        }
        {
            let mut service = state.locked();
            if service.provider != provider || service.authentication != authentication {
                return Err(
                    "The AI connection changed. Review the current settings and save again.".into(),
                );
            }
            service
                .provider
                .validate(&settings, service.authentication)
                .map_err(message)?;
            if service.authentication == AuthenticationMethod::Chatgpt {
                account.selected_model(&settings).map_err(message)?;
            }
            write_json(&service.settings_path(service.provider), &settings)?;
            service.settings = settings;
            service.configuration_error = None;
            if service.client.is_err() {
                service.client = OpenAiClient::new().map_err(message);
            }
        }
        settings_view(&state, &chatgpt).await
    }
    .await;
    result.map_err(crate::native_error::NativeErrorKind::AiSettings.failure())
}

#[tauri::command]
pub async fn set_ai_key(
    state: SettingsState<'_>,
    chatgpt: State<'_, ChatgptState>,
    image: Option<bool>,
    key: String,
    storage: CredentialStorage,
) -> Result<AiSettingsView, crate::native_error::NativeFailure> {
    let result: NativeResult<AiSettingsView> = async {
        let (credentials, provider) = {
            let service = state.locked();
            if image.unwrap_or(false) {
                (service.image_credentials.clone(), service.provider)
            } else {
                (service.credentials.clone(), service.provider)
            }
        };
        tauri::async_runtime::spawn_blocking(move || credentials.set_key(key, storage))
            .await
            .map_err(message)?
            .map_err(message)?;
        if provider == ProviderId::Local && !image.unwrap_or(false) {
            state.locked().set_local_authentication(true)?;
        }
        if !image.unwrap_or(false) {
            refresh_api_models(&state).await?;
        }
        refresh_image_access(&state, &chatgpt, true).await?;
        settings_view(&state, &chatgpt).await
    }
    .await;
    result.map_err(crate::native_error::NativeErrorKind::AiSettings.failure())
}

#[tauri::command]
pub async fn remove_ai_key(
    state: SettingsState<'_>,
    chatgpt: State<'_, ChatgptState>,
    image: Option<bool>,
) -> Result<AiSettingsView, crate::native_error::NativeFailure> {
    let result: NativeResult<AiSettingsView> = async {
        let (credentials, provider) = {
            let service = state.locked();
            if image.unwrap_or(false) {
                (service.image_credentials.clone(), service.provider)
            } else {
                (service.credentials.clone(), service.provider)
            }
        };
        tauri::async_runtime::spawn_blocking(move || credentials.remove_key())
            .await
            .map_err(message)?
            .map_err(message)?;
        if provider == ProviderId::Local && !image.unwrap_or(false) {
            state.locked().set_local_authentication(false)?;
            refresh_api_models(&state).await?;
        }
        settings_view(&state, &chatgpt).await
    }
    .await;
    result.map_err(crate::native_error::NativeErrorKind::AiSettings.failure())
}

#[derive(ts_rs::TS, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessResult {
    provider_error: Option<ProviderError>,
    message: String,
}

#[tauri::command]
pub async fn test_ai_access(
    state: SettingsState<'_>,
    chatgpt: State<'_, ChatgptState>,
) -> Result<AccessResult, crate::native_error::NativeFailure> {
    let result: NativeResult<AccessResult> = async {
    refresh_image_access(&state, &chatgpt, true).await?;
    let (client, settings, authentication, provider) = {
        let service = state.locked();
        (
            service.client.clone()?,
            service.settings.clone(),
            service.authentication,
            service.provider,
        )
    };
    if authentication == AuthenticationMethod::Chatgpt {
        let manager = chatgpt.manager()?;
        let account_id = manager
            .view()
            .await
            .active_account_id
            .ok_or("Sign in to ChatGPT before checking access.")?;
        let view = manager
            .discover_models(account_id, &client, &CancellationToken::new())
            .await
            .map_err(message)?;
        if view.active_account_id != Some(account_id) {
            return Err("The selected ChatGPT account changed while checking access. Test the currently selected account again.".into());
        }
        if let Some(error) = view.models_provider_error {
            return Ok(AccessResult {
                message: error.message(),
                provider_error: Some(error),
            });
        }
        view.selected_model(&settings).map_err(message)?;
        return Ok(AccessResult {
            provider_error: None,
            message: "Connected · ChatGPT plan and model access confirmed.".into(),
        });
    }
    let key = credential_key(&state, provider, &settings).await?;
    let result = async {
        api_model_for_key(&state, provider, &settings, &key, &CancellationToken::new()).await?;
        ProviderClient::with_settings(provider, client, &settings)?
            .test_model_access(&settings, &key, &CancellationToken::new())
            .await
    }
    .await;
    Ok(match result {
        Ok(_) => AccessResult {
            provider_error: None,
            message: "Connected · Model access confirmed.".into(),
        },
        Err(error) => AccessResult {
            message: error.message(),
            provider_error: Some(error),
        },
    })

    }.await;
    result.map_err(crate::native_error::NativeErrorKind::AiSettings.failure())
}

#[cfg(test)]
mod api_model_cache_tests {
    use super::*;
    use std::fs;

    #[tokio::test]
    async fn local_catalogues_are_bound_to_server_and_key_mode() {
        let directory =
            std::env::temp_dir().join(format!("desegnos-local-model-cache-{}", Uuid::new_v4()));
        let mut service =
            AiSettingsStore::from_directory(directory.clone(), Default::default()).unwrap();
        let anonymous = ApiKey::local_anonymous();
        let settings = ProviderId::Local.defaults();
        let other_server = ProviderSettings {
            endpoint: Some("http://127.0.0.1:8080/v1".into()),
            ..settings.clone()
        };
        let authenticated = ApiKey::new("synthetic-local-server-key".into()).unwrap();
        let identity = catalogue_identity(ProviderId::Local, &settings, &anonymous).unwrap();
        assert_ne!(
            identity,
            catalogue_identity(ProviderId::Local, &other_server, &anonymous).unwrap()
        );
        assert_ne!(
            identity,
            catalogue_identity(ProviderId::Local, &settings, &authenticated).unwrap()
        );
        service
            .api_models
            .insert((ProviderId::Local, identity), Ok(Vec::new()));
        // Anonymous mode must not consult the credential store, even when no key exists.
        assert!(
            provider_key(
                ProviderId::Local,
                false,
                &service.credentials_for(ProviderId::Local)
            )
            .is_ok()
        );
        let state = Mutex::new(service);
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(
            api_catalogue_for_key(
                &state,
                ProviderId::Local,
                &settings,
                &anonymous,
                &cancel,
                false
            )
            .await
            .unwrap()
            .is_empty()
        );
        assert_eq!(
            api_catalogue_for_key(
                &state,
                ProviderId::Local,
                &other_server,
                &anonymous,
                &cancel,
                false
            )
            .await
            .unwrap_err()
            .kind,
            ErrorKind::Cancelled
        );
        drop(state);
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn api_model_cache_isolates_credentials_and_providers_and_survives_cancelled_refresh() {
        let directory =
            std::env::temp_dir().join(format!("desegnos-api-model-cache-{}", Uuid::new_v4()));
        let mut service =
            AiSettingsStore::from_directory(directory.clone(), Default::default()).unwrap();
        let first = ApiKey::new("synthetic-first-provider-key".into()).unwrap();
        let second = ApiKey::new("synthetic-second-provider-key".into()).unwrap();
        let openai = ProviderId::Openai.catalogue();
        let anthropic = ProviderId::Anthropic.catalogue();
        service.api_models.insert(
            (ProviderId::Openai, first.image_access_cache_key()),
            Ok(openai.clone()),
        );
        service.api_models.insert(
            (ProviderId::Anthropic, first.image_access_cache_key()),
            Ok(anthropic.clone()),
        );
        service.api_models.insert(
            (ProviderId::Openai, second.image_access_cache_key()),
            Err(ProviderError::new(
                ErrorKind::Authentication,
                SubmissionOutcome::NotSubmitted,
            )),
        );
        let state = Mutex::new(service);
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert_eq!(
            api_catalogue_for_key(
                &state,
                ProviderId::Openai,
                &ProviderSettings::default(),
                &first,
                &cancelled,
                false
            )
            .await
            .unwrap(),
            openai
        );
        assert_eq!(
            api_catalogue_for_key(
                &state,
                ProviderId::Anthropic,
                &ProviderSettings::default(),
                &first,
                &cancelled,
                false
            )
            .await
            .unwrap(),
            anthropic
        );
        for (provider, key) in [(ProviderId::Openai, &second), (ProviderId::Google, &first)] {
            assert_eq!(
                api_catalogue_for_key(
                    &state,
                    provider,
                    &ProviderSettings::default(),
                    key,
                    &cancelled,
                    false
                )
                .await
                .unwrap_err()
                .kind,
                ErrorKind::Cancelled
            );
        }
        assert_eq!(
            api_catalogue_for_key(
                &state,
                ProviderId::Openai,
                &ProviderSettings::default(),
                &first,
                &cancelled,
                true
            )
            .await
            .unwrap_err()
            .kind,
            ErrorKind::Cancelled
        );
        assert_eq!(
            api_catalogue_for_key(
                &state,
                ProviderId::Openai,
                &ProviderSettings::default(),
                &first,
                &cancelled,
                false
            )
            .await
            .unwrap(),
            openai
        );
        drop(state);
        fs::remove_dir_all(directory).unwrap();
    }
}
