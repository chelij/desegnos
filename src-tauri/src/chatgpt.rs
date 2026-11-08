use crate::{
    ai_settings::{AiSettingsView, SettingsState, settings_view},
    export::ExportService,
    generation::GenerationService,
    identity::IdentityService,
    native_files::{LockExt, NativeResult},
};
use desegnos_ai::{AuthenticationMethod, ChatgptManager, ChatgptView, CredentialStorage};
use std::{collections::BTreeMap, path::PathBuf, sync::Mutex};
use tauri::State;
use uuid::Uuid;

const USAGE_URL: &str = "https://chatgpt.com/settings/usage";

pub struct ChatgptState {
    manager: Result<ChatgptManager, String>,
    revocations: Mutex<BTreeMap<Uuid, u64>>,
}

impl ChatgptState {
    pub fn initialize(directory: PathBuf) -> Self {
        let manager = crate::testing::chatgpt_manager(directory);
        Self {
            manager,
            revocations: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn manager(&self) -> NativeResult<ChatgptManager> {
        self.manager.clone()
    }

    pub async fn view(&self) -> ChatgptView {
        match &self.manager {
            Ok(manager) => manager.view().await,
            Err(error) => ChatgptView::unavailable(error),
        }
    }

    pub fn epoch(&self, account: Uuid) -> NativeResult<u64> {
        Ok(*self.revocations.locked().get(&account).unwrap_or(&0))
    }

    /// The account a job is bound to with its current sign-out epoch.
    pub fn binding(&self, account: Option<Uuid>) -> NativeResult<Option<(Uuid, u64)>> {
        account
            .map(|id| self.epoch(id).map(|epoch| (id, epoch)))
            .transpose()
    }

    /// Run a step only while the bound account has not been signed out since `binding`.
    pub fn with_binding<T>(
        &self,
        binding: Option<(Uuid, u64)>,
        operation: impl FnOnce() -> NativeResult<T>,
    ) -> NativeResult<T> {
        match binding {
            Some((account, epoch)) => self.with_epoch(account, epoch, operation),
            None => operation(),
        }
    }

    pub fn with_epoch<T>(
        &self,
        account: Uuid,
        epoch: u64,
        operation: impl FnOnce() -> NativeResult<T>,
    ) -> NativeResult<T> {
        let revocations = self.revocations.locked();
        if *revocations.get(&account).unwrap_or(&0) != epoch {
            return Err("This account was signed out while checking access. Sign in again before explicitly sending the request.".into());
        }
        operation()
    }
}

#[tauri::command]
pub async fn set_ai_authentication(
    state: SettingsState<'_>,
    chatgpt: State<'_, ChatgptState>,
    authentication: AuthenticationMethod,
) -> Result<AiSettingsView, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::AiSettings
        .run_async(async {
            state.locked().set_authentication(authentication)?;
            settings_view(&state, &chatgpt).await
        })
        .await
}

#[tauri::command]
pub async fn begin_chatgpt_sign_in(
    state: SettingsState<'_>,
    chatgpt: State<'_, ChatgptState>,
    storage: CredentialStorage,
    account_id: Option<Uuid>,
) -> Result<AiSettingsView, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::AiSettings
        .run_async(async {
            let manager = chatgpt.manager()?;
            state
                .locked()
                .set_authentication(AuthenticationMethod::Chatgpt)?;
            if let Some(account) = account_id {
                state.locked().invalidate_image_account(account)?;
            }
            let authorization = manager
                .begin_sign_in(storage, account_id)
                .await
                .map_err(|error| error.message().to_owned())?;
            if let Err(error) = crate::testing::open_authorization(authorization.url()).await {
                manager.cancel_sign_in().await;
                return Err(error);
            }
            settings_view(&state, &chatgpt).await
        })
        .await
}

#[tauri::command]
pub async fn cancel_chatgpt_sign_in(
    state: SettingsState<'_>,
    chatgpt: State<'_, ChatgptState>,
) -> Result<AiSettingsView, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::AiSettings
        .run_async(async {
            chatgpt.manager()?.cancel_sign_in().await;
            settings_view(&state, &chatgpt).await
        })
        .await
}

#[tauri::command]
pub async fn select_chatgpt_account(
    state: SettingsState<'_>,
    chatgpt: State<'_, ChatgptState>,
    account_id: Uuid,
) -> Result<AiSettingsView, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::AiSettings
        .run_async(async {
            let manager = chatgpt.manager()?;
            let view = manager
                .select_account(account_id)
                .await
                .map_err(|error| error.message().to_owned())?;
            if view
                .accounts
                .iter()
                .any(|account| account.id == account_id && account.usable())
            {
                crate::ai_access::refresh_chatgpt_models(&chatgpt, &state, account_id).await;
            }
            crate::ai_settings::refresh_image_access(&state, &chatgpt, true).await?;
            settings_view(&state, &chatgpt).await
        })
        .await
}

#[tauri::command]
pub async fn retry_chatgpt_secure_storage(
    state: SettingsState<'_>,
    chatgpt: State<'_, ChatgptState>,
    account_id: Uuid,
) -> Result<AiSettingsView, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::AiSettings
        .run_async(async {
            chatgpt
                .manager()?
                .retry_secure_storage(account_id)
                .await
                .map_err(|error| error.message().to_owned())?;
            settings_view(&state, &chatgpt).await
        })
        .await
}

#[tauri::command]
pub async fn sign_out_chatgpt(
    generation: State<'_, Mutex<crate::generation::GenerationService>>,
    identity: State<'_, Mutex<crate::identity::IdentityService>>,
    exports: State<'_, Mutex<crate::export::ExportService>>,
    state: SettingsState<'_>,
    chatgpt: State<'_, ChatgptState>,
    account_id: Uuid,
) -> Result<AiSettingsView, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::AiSettings
        .run_async(async {
            let manager = chatgpt.manager()?;
            if !manager
                .view()
                .await
                .accounts
                .iter()
                .any(|account| account.id == account_id)
            {
                return Err("Choose a saved ChatGPT account before signing out.".into());
            }
            let cancellation = {
                let mut revocations = chatgpt.revocations.locked();
                let epoch = revocations.entry(account_id).or_default();
                *epoch = epoch
                    .checked_add(1)
                    .ok_or("Account access state must be restarted before signing out.")?;
                cancel_account(&generation, &identity, &exports, account_id)
            };
            let (view, warning) = sign_out_after_cancellation(cancellation, async {
                manager
                    .sign_out(account_id)
                    .await
                    .map_err(|error| error.message().to_owned())?;
                settings_view(&state, &chatgpt).await
            })
            .await?;
            Ok(view.with_warning(warning))
        })
        .await
}

async fn sign_out_after_cancellation<T>(
    cancellation: NativeResult<()>,
    sign_out: impl std::future::Future<Output = NativeResult<T>>,
) -> NativeResult<(T, Option<String>)> {
    let warning = cancellation.err().map(|reason| {
        format!(
            "Late output is blocked, but a local cancellation receipt needs attention: {reason}"
        )
    });
    let result = sign_out.await;
    match result {
        Ok(value) => Ok((value, warning)),
        Err(reason) => Err(match warning {
            Some(warning) => {
                format!("{reason} Local cancellation also needs attention: {warning}")
            }
            None => reason,
        }),
    }
}

#[tauri::command]
pub fn open_chatgpt_usage() -> Result<(), crate::native_error::NativeFailure> {
    crate::testing::open_usage(USAGE_URL)
        .map_err(|_| {
            "Couldn't open a browser. Open https://chatgpt.com/settings/usage manually.".to_owned()
        })
        .map_err(crate::native_error::NativeErrorKind::AiSettings.failure())
}

/// Sign-out stops running work in every service. All services are attempted; the first
/// failure to record a cancellation is returned afterwards.
fn cancel_account(
    generation: &Mutex<GenerationService>,
    identity: &Mutex<IdentityService>,
    exports: &Mutex<ExportService>,
    account_id: Uuid,
) -> Result<(), String> {
    let results = [
        generation.locked().cancel_account(account_id),
        identity.locked().cancel_account(account_id),
        {
            exports.locked().cancel_account(account_id);
            Ok(())
        },
    ];
    results.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_receipt_failure_cannot_skip_credential_sign_out() {
        tauri::async_runtime::block_on(async {
            let mut signed_out = false;
            let (result, warning) =
                sign_out_after_cancellation(Err("Storage is full".into()), async {
                    signed_out = true;
                    Ok("Credentials removed")
                })
                .await
                .unwrap();
            assert!(signed_out);
            assert_eq!(result, "Credentials removed");
            assert!(warning.unwrap().contains("Storage is full"));
            let mut attempted = false;
            let result =
                sign_out_after_cancellation::<()>(Err("Receipt unavailable".into()), async {
                    attempted = true;
                    Err("Credential deletion failed".into())
                })
                .await;
            assert!(attempted);
            let reason = result.unwrap_err();
            assert!(
                reason.contains("Credential deletion failed")
                    && reason.contains("Receipt unavailable")
            );
        });
    }

    #[test]
    fn sign_out_invalidates_access_obtained_before_submission_intent() {
        let state = ChatgptState {
            manager: Err("Isolated test state".into()),
            revocations: Mutex::new(BTreeMap::new()),
        };
        let account = Uuid::new_v4();
        let epoch = state.epoch(account).unwrap();
        state.with_epoch(account, epoch, || Ok(())).unwrap();
        state.revocations.locked().insert(account, epoch + 1);
        let mut submitted = false;
        assert!(
            state
                .with_epoch(account, epoch, || {
                    submitted = true;
                    Ok(())
                })
                .is_err()
        );
        assert!(!submitted);
        let other = Uuid::new_v4();
        state
            .with_epoch(other, state.epoch(other).unwrap(), || Ok(()))
            .unwrap();
    }
}
