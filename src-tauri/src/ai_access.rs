//! The one step that turns "this job uses the selected AI connection" into a ready `Access`:
//! the ChatGPT account or API key, the model it will use, and the same input check the paid
//! send performs. Generation, export and identity analysis all go through it, so they cannot
//! drift apart on repair checks, sign-out epochs or model validation.
use crate::{
    ai_settings::{AiSettingsStore, api_model_for_key, provider_key},
    chatgpt::ChatgptState,
    native_files::{LockExt, NativeResult, message},
};
use desegnos_ai::{
    ApiKey, AuthenticationMethod, CancellationToken, ChatgptAccount, ChatgptModel, CostEstimate, ErrorKind,
    ModelInfo, ProviderClient, ProviderError, ProviderId, ProviderSettings, SubmissionOutcome,
};
use desegnos_core::{GenerationMode, RequirementsSnapshot};
use serde_json::Value;
use std::sync::Mutex;
use uuid::Uuid;

/// Which AI connection a job uses, and the ChatGPT account it is bound to.
#[derive(Clone)]
pub struct Connection {
    pub provider: ProviderId,
    pub authentication: AuthenticationMethod,
    pub settings: ProviderSettings,
    pub account_id: Option<Uuid>,
}

/// A connection ready to send: credentials resolved and the model confirmed. Credentials
/// never enter an IPC view, project, source snapshot, or persisted record.
pub struct Access {
    pub provider: ProviderId,
    pub authentication: AuthenticationMethod,
    pub settings: ProviderSettings,
    pub client: ProviderClient,
    pub key: ApiKey,
    /// The ChatGPT account and its sign-out epoch when the access was resolved.
    pub account: Option<(Uuid, u64)>,
    account_model: Option<ChatgptModel>,
    api_model: Option<ModelInfo>,
}

pub struct AccessFailure {
    pub message: String,
    pub provider_error: Option<Box<ProviderError>>,
}

impl From<String> for AccessFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            provider_error: None,
        }
    }
}

impl From<&str> for AccessFailure {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}

impl From<ProviderError> for AccessFailure {
    fn from(error: ProviderError) -> Self {
        Self {
            message: error.message(),
            provider_error: Some(Box::new(error)),
        }
    }
}

impl AccessFailure {
    /// The failure as a provider error, for a job that records one.
    pub fn into_provider_error(self) -> ProviderError {
        self.provider_error.map(|error| *error).unwrap_or_else(|| {
            ProviderError::new(
                ErrorKind::MissingCredential,
                SubmissionOutcome::NotSubmitted,
            )
        })
    }
}

/// The signed-in ChatGPT account a new job binds to. `action` completes "before ...".
pub async fn active_account(chatgpt: &ChatgptState, action: &str) -> NativeResult<Uuid> {
    let view = chatgpt.view().await;
    let account = view
        .accounts
        .iter()
        .find(|account| Some(account.id) == view.active_account_id)
        .ok_or_else(|| format!("Sign in to ChatGPT before {action}."))?;
    check_account(account, action)?;
    Ok(account.id)
}

fn check_account(account: &ChatgptAccount, action: &str) -> NativeResult<()> {
    if !account.connected || !account.plan_enabled {
        return Err(format!(
            "Sign in to this ChatGPT account and grant plan usage before {action}."
        ));
    }
    if account.storage_repair_required {
        return Err(format!(
            "Keep Desegnos open and repair this account's secure storage in AI settings before {action}."
        ));
    }
    Ok(())
}

/// Resolve the credentials and model for a job. Cancelling stops it with a cancelled error.
pub async fn resolve(
    service: &Mutex<AiSettingsStore>,
    chatgpt: &ChatgptState,
    connection: &Connection,
    cancellation: &CancellationToken,
) -> Result<Access, AccessFailure> {
    cancellation
        .run_until_cancelled(resolve_credentials(
            service,
            chatgpt,
            connection,
            cancellation,
        ))
        .await
        .unwrap_or_else(|| {
            Err(ProviderError::new(ErrorKind::Cancelled, SubmissionOutcome::NotSubmitted).into())
        })
}

async fn resolve_credentials(
    service: &Mutex<AiSettingsStore>,
    chatgpt: &ChatgptState,
    connection: &Connection,
    cancellation: &CancellationToken,
) -> Result<Access, AccessFailure> {
    let Connection {
        provider,
        authentication,
        settings,
        account_id,
    } = connection;
    let openai = service.locked().client()?;
    let client =
        ProviderClient::with_settings(*provider, openai.clone(), settings).map_err(message)?;
    let (key, account, account_model) = if *authentication == AuthenticationMethod::Chatgpt {
        let account_id = account_id
            .ok_or("This ChatGPT request has no bound account. Prepare a new request.")?;
        let epoch = chatgpt.epoch(account_id)?;
        let manager = chatgpt.manager()?;
        let mut view = chatgpt.view().await;
        let account = view
            .accounts
            .iter()
            .find(|account| account.id == account_id)
            .ok_or("This ChatGPT account is no longer connected.")?;
        check_account(account, "sending")?;
        if view.models.is_empty() {
            view = manager
                .discover_models(account_id, &openai, cancellation)
                .await
                .map_err(message)?;
        }
        if view.active_account_id != Some(account_id) {
            return Err("The selected ChatGPT account changed. Prepare the request again.".into());
        }
        if let Some(error) = view.models_provider_error {
            return Err(error.into());
        }
        let model = view.selected_model(settings)?.clone();
        let key = manager
            .bearer(account_id, cancellation)
            .await
            .map_err(message)?;
        (key, Some((account_id, epoch)), Some(model))
    } else {
        (
            credential_key(service, *provider, settings).await?,
            None,
            None,
        )
    };
    let api_model = if *authentication == AuthenticationMethod::ApiKey {
        Some(api_model_for_key(service, *provider, settings, &key, cancellation).await?)
    } else {
        None
    };
    Ok(Access {
        provider: *provider,
        authentication: *authentication,
        settings: settings.clone(),
        client,
        key,
        account,
        account_model,
        api_model,
    })
}

/// The API key for a provider, read off the async runtime.
pub async fn credential_key(
    service: &Mutex<AiSettingsStore>,
    provider: ProviderId,
    settings: &ProviderSettings,
) -> NativeResult<ApiKey> {
    let credentials = service.locked().credentials_for(provider);
    let local_authentication = settings.local_authentication;
    tauri::async_runtime::spawn_blocking(move || {
        provider_key(provider, local_authentication, &credentials)
    })
    .await
    .map_err(message)?
    .map_err(message)
}

impl Access {
    /// Reject an input the selected model cannot take.
    pub fn validate_input(&self, input_tokens: u64) -> desegnos_ai::Result<()> {
        if let Some(model) = &self.account_model {
            model.validate_input(input_tokens)?;
        }
        if let Some(model) = &self.api_model {
            model.validate_api_input(&self.settings, input_tokens)?;
        }
        Ok(())
    }

    /// The cost estimate for a request, after checking it fits the model.
    pub fn check(
        &self,
        snapshot: &RequirementsSnapshot,
        mode: GenerationMode,
        schema: &Value,
    ) -> desegnos_ai::Result<CostEstimate> {
        let estimate =
            self.client
                .estimate(self.authentication, snapshot, mode, schema, &self.settings)?;
        self.validate_input(estimate.input_token_upper_bound)?;
        Ok(estimate)
    }

    pub fn account_id(&self) -> Option<Uuid> {
        self.account.map(|(id, _)| id)
    }

    /// Run a step only while the account has not been signed out since this access was resolved.
    pub fn while_signed_in<T>(
        &self,
        chatgpt: &ChatgptState,
        step: impl FnOnce() -> NativeResult<T>,
    ) -> NativeResult<T> {
        chatgpt.with_binding(self.account, step)
    }
}
