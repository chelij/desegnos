use crate::{ChatgptModel, CredentialLocation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatgptErrorKind {
    Storage,
    VaultUnavailable,
    Busy,
    Cancelled,
    Deadline,
    Callback,
    StateMismatch,
    RegistrationIncomplete,
    ClientMismatch,
    TokenRejected,
    InvalidClient,
    InvalidToken,
    IdentityMismatch,
    PlanPermission,
    Reconnect,
    Transport,
    Discovery,
    RefreshTooEarly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatgptError {
    pub kind: ChatgptErrorKind,
}

impl ChatgptError {
    pub fn message(&self) -> &'static str {
        match self.kind {
            ChatgptErrorKind::Storage => {
                "ChatGPT account metadata could not be saved safely. Existing local credentials were preserved. Check the private authentication directory before trying again."
            }
            ChatgptErrorKind::VaultUnavailable => {
                "System credential storage is unavailable. Unlock it or explicitly choose session-only storage and sign in again."
            }
            ChatgptErrorKind::Busy => {
                "Another ChatGPT sign-in is pending. Complete or cancel it before starting again."
            }
            ChatgptErrorKind::Cancelled => {
                "ChatGPT sign-in was cancelled. Your previously selected account was preserved."
            }
            ChatgptErrorKind::Deadline => {
                "ChatGPT sign-in timed out. Start a fresh authorization; the expired callback cannot be reused."
            }
            ChatgptErrorKind::Callback => {
                "The ChatGPT browser callback was invalid. Start sign-in again."
            }
            ChatgptErrorKind::StateMismatch => {
                "The browser callback did not match this sign-in attempt. No authorization code was exchanged."
            }
            ChatgptErrorKind::RegistrationIncomplete => {
                "ChatGPT registration did not supply an issued client ID. Start sign-in again; the dynamic entrypoint cannot be used as a saved client."
            }
            ChatgptErrorKind::ClientMismatch => {
                "The browser returned a different registered client. The selected account mapping was preserved."
            }
            ChatgptErrorKind::TokenRejected => {
                "ChatGPT rejected the authorization code. Start a fresh sign-in using the saved account registration."
            }
            ChatgptErrorKind::InvalidClient => {
                "ChatGPT rejected this client registration. Review the integration configuration before reconnecting."
            }
            ChatgptErrorKind::InvalidToken => {
                "ChatGPT identity could not be verified. The signature, issuer, audience, expiry or authorization nonce was invalid."
            }
            ChatgptErrorKind::IdentityMismatch => {
                "The verified ChatGPT identity does not match the selected registration. Add a separate account instead."
            }
            ChatgptErrorKind::PlanPermission => {
                "This ChatGPT account has not granted permission to use its plan. Reconnect and enable plan usage, or explicitly choose API-key mode."
            }
            ChatgptErrorKind::Reconnect => {
                "This ChatGPT session is no longer renewable. Reconnect the saved account before generating."
            }
            ChatgptErrorKind::Transport => {
                "The ChatGPT connection failed. Existing credentials were preserved; try again when the connection is available."
            }
            ChatgptErrorKind::Discovery => {
                "ChatGPT returned an untrusted or invalid discovery configuration. Desegnos did not send credentials to it."
            }
            ChatgptErrorKind::RefreshTooEarly => {
                "ChatGPT has not yet allowed this token to refresh. Wait until its refresh time or reconnect the saved account."
            }
        }
    }
}

impl std::fmt::Display for ChatgptError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message())
    }
}
impl std::error::Error for ChatgptError {}
impl From<ChatgptErrorKind> for ChatgptError {
    fn from(kind: ChatgptErrorKind) -> Self {
        Self { kind }
    }
}
pub type ChatgptResult<T> = std::result::Result<T, ChatgptError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatgptAccount {
    pub id: Uuid,
    pub label: String,
    pub email: Option<String>,
    pub connected: bool,
    pub plan_enabled: bool,
    pub storage: CredentialLocation,
    pub storage_repair_required: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ChatgptSignInStatus {
    Idle,
    AwaitingBrowser,
    Complete,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatgptSignIn {
    pub status: ChatgptSignInStatus,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChatgptView {
    pub accounts: Vec<ChatgptAccount>,
    pub active_account_id: Option<Uuid>,
    pub sign_in: ChatgptSignIn,
    pub models: Vec<ChatgptModel>,
    pub models_error: Option<String>,
    pub storage_error: Option<String>,
    pub show_welcome: bool,
    pub message: Option<String>,
}

impl ChatgptView {
    pub fn selected_model(
        &self,
        settings: &crate::ProviderSettings,
    ) -> crate::Result<&ChatgptModel> {
        if !self.accounts.iter().any(|account| {
            Some(account.id) == self.active_account_id
                && account.connected
                && account.plan_enabled
                && !account.storage_repair_required
        }) {
            return Err(
                crate::ProviderError::before(crate::ErrorKind::MissingCredential)
                    .with_authentication(crate::AuthenticationMethod::Chatgpt),
            );
        }
        let model = self
            .models
            .iter()
            .find(|model| model.id == settings.model)
            .ok_or_else(|| {
                crate::ProviderError::before(crate::ErrorKind::ModelUnavailable)
                    .with_authentication(crate::AuthenticationMethod::Chatgpt)
            })?;
        model.validate_settings(settings)?;
        Ok(model)
    }

    pub fn unavailable(message: &str) -> Self {
        Self {
            accounts: vec![],
            active_account_id: None,
            sign_in: ChatgptSignIn {
                status: ChatgptSignInStatus::Idle,
                message: None,
            },
            models: vec![],
            models_error: None,
            storage_error: Some(message.into()),
            show_welcome: false,
            message: None,
        }
    }
}

pub struct ChatgptAuthorization {
    pub(super) url: String,
}
impl ChatgptAuthorization {
    pub fn url(&self) -> &str {
        &self.url
    }
}
impl Drop for ChatgptAuthorization {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.url.zeroize();
    }
}
impl std::fmt::Debug for ChatgptAuthorization {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ChatgptAuthorization([NATIVE ONLY])")
    }
}
