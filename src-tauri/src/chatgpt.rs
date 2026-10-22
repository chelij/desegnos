use crate::generation::{AiSettingsView, GenerationService, settings_view};
use desegni_ai::{AuthenticationMethod, ChatgptManager, ChatgptView, CredentialStorage};
use std::{collections::BTreeMap, path::PathBuf, sync::Mutex};
use tauri::{AppHandle, State, WebviewWindow};
use uuid::Uuid;

type NativeResult<T> = Result<T, String>;
type AiState<'a> = State<'a, Mutex<GenerationService>>;
const USAGE_URL: &str = "https://chatgpt.com/settings/usage";

pub struct ChatgptState {
    manager: Result<ChatgptManager, String>,
    revocations: Mutex<BTreeMap<Uuid, u64>>,
}

impl ChatgptState {
    pub fn initialize(directory: PathBuf) -> Self {
        #[cfg(not(feature = "native-tests"))]
        let manager =
            ChatgptManager::initialize(directory).map_err(|error| error.message().to_owned());
        #[cfg(feature = "native-tests")]
        let manager = (|| {
            let endpoint = std::env::var("DESEGNI_NATIVE_TEST_OPENAI_ENDPOINT")
                .map_err(|_| "The isolated ChatGPT test endpoint is unavailable.".to_owned())?;
            let base = endpoint
                .strip_suffix("/v1")
                .ok_or_else(|| "The isolated ChatGPT test endpoint is invalid.".to_owned())?;
            let client = desegni_ai::OpenAiClient::for_loopback(&endpoint)
                .map_err(|error| error.message().to_owned())?;
            let vault = desegni_ai::ChatgptTestVault::memory_with_write_failure_flag(
                directory.join("native-test-vault-write-failure"),
            );
            ChatgptManager::for_loopback(directory, base, client, vault)
                .map_err(|error| error.message().to_owned())
        })();
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
        Ok(*self
            .revocations
            .lock()
            .map_err(|_| "Account access state is unavailable.")?
            .get(&account)
            .unwrap_or(&0))
    }

    pub fn with_epoch<T>(
        &self,
        account: Uuid,
        epoch: u64,
        operation: impl FnOnce() -> NativeResult<T>,
    ) -> NativeResult<T> {
        let revocations = self
            .revocations
            .lock()
            .map_err(|_| "Account access state is unavailable.")?;
        if *revocations.get(&account).unwrap_or(&0) != epoch {
            return Err("This account was signed out while checking access. Sign in again before explicitly sending the request.".into());
        }
        operation()
    }
}

fn trusted(window: &WebviewWindow) -> NativeResult<()> {
    if window.label() == "editor" {
        Ok(())
    } else {
        Err("Only the Desegnos editor can manage ChatGPT access.".into())
    }
}

#[cfg(not(feature = "native-tests"))]
async fn open_authorization(_: &AppHandle, url: &str) -> NativeResult<()> {
    crate::system_open::open(url)
        .map_err(|_| "Desegnos could not open your system browser. Check the default browser and start sign-in again.".into())
}

#[cfg(feature = "native-tests")]
async fn open_authorization(_: &AppHandle, url: &str) -> NativeResult<()> {
    use std::{
        io::{Read, Write},
        net::TcpStream,
        time::Duration,
    };
    let url = tauri::Url::parse(url)
        .map_err(|_| "The isolated authorization URL is invalid.".to_owned())?;
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || url.port().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("Native tests can only open their literal loopback OAuth fixture.".into());
    }
    let port = url
        .port()
        .ok_or("The isolated authorization port is missing.")?;
    let target = format!(
        "{}{}",
        url.path(),
        url.query()
            .map(|query| format!("?{query}"))
            .unwrap_or_default()
    );
    tauri::async_runtime::spawn_blocking(move || {
        let mut stream =
            TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(5))
                .map_err(|_| {
                    "The isolated OAuth browser fixture could not be reached.".to_owned()
                })?;
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|_| "The isolated OAuth browser timeout could not be set.".to_owned())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|_| "The isolated OAuth browser timeout could not be set.".to_owned())?;
        write!(
            stream,
            "GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
        )
        .map_err(|_| "The isolated OAuth browser fixture request failed.".to_owned())?;
        let mut response = String::new();
        stream
            .take(4096)
            .read_to_string(&mut response)
            .map_err(|_| "The isolated OAuth browser fixture response failed.".to_owned())?;
        if !response.starts_with("HTTP/1.1 200 ") && !response.starts_with("HTTP/1.0 200 ") {
            return Err("The isolated OAuth browser fixture rejected authorization.".into());
        }
        Ok(())
    })
    .await
    .map_err(|_| "The isolated OAuth browser fixture stopped.".to_owned())?
}

#[tauri::command]
pub async fn set_ai_authentication(
    window: WebviewWindow,
    state: AiState<'_>,
    chatgpt: State<'_, ChatgptState>,
    authentication: AuthenticationMethod,
) -> NativeResult<AiSettingsView> {
    trusted(&window)?;
    state
        .lock()
        .map_err(|_| "AI settings are unavailable.")?
        .set_authentication(authentication)?;
    settings_view(&state, &chatgpt).await
}

#[tauri::command]
pub async fn begin_chatgpt_sign_in(
    app: AppHandle,
    window: WebviewWindow,
    state: AiState<'_>,
    chatgpt: State<'_, ChatgptState>,
    storage: CredentialStorage,
    account_id: Option<Uuid>,
) -> NativeResult<AiSettingsView> {
    trusted(&window)?;
    let manager = chatgpt.manager()?;
    state
        .lock()
        .map_err(|_| "AI settings are unavailable.")?
        .set_authentication(AuthenticationMethod::Chatgpt)?;
    if let Some(account) = account_id {
        state
            .lock()
            .map_err(|_| "AI settings are unavailable.")?
            .invalidate_image_account(account)?;
    }
    let authorization = manager
        .begin_sign_in(storage, account_id)
        .await
        .map_err(|error| error.message().to_owned())?;
    if let Err(error) = open_authorization(&app, authorization.url()).await {
        manager.cancel_sign_in().await;
        return Err(error);
    }
    settings_view(&state, &chatgpt).await
}

#[tauri::command]
pub async fn cancel_chatgpt_sign_in(
    window: WebviewWindow,
    state: AiState<'_>,
    chatgpt: State<'_, ChatgptState>,
) -> NativeResult<AiSettingsView> {
    trusted(&window)?;
    chatgpt.manager()?.cancel_sign_in().await;
    settings_view(&state, &chatgpt).await
}

#[tauri::command]
pub async fn select_chatgpt_account(
    window: WebviewWindow,
    state: AiState<'_>,
    chatgpt: State<'_, ChatgptState>,
    account_id: Uuid,
) -> NativeResult<AiSettingsView> {
    trusted(&window)?;
    let manager = chatgpt.manager()?;
    let view = manager
        .select_account(account_id)
        .await
        .map_err(|error| error.message().to_owned())?;
    if view
        .accounts
        .iter()
        .any(|account| account.id == account_id && account.connected && account.plan_enabled)
    {
        let client = state
            .lock()
            .map_err(|_| "AI settings are unavailable.")?
            .client();
        if let Ok(client) = client {
            // Discovery failures remain in the accepted account's public model status.
            let _ = manager
                .discover_models(account_id, &client, &desegni_ai::CancellationToken::new())
                .await;
        }
    }
    crate::generation::refresh_image_access(&state, &chatgpt, true).await?;
    settings_view(&state, &chatgpt).await
}

#[tauri::command]
pub async fn retry_chatgpt_secure_storage(
    window: WebviewWindow,
    state: AiState<'_>,
    chatgpt: State<'_, ChatgptState>,
    account_id: Uuid,
) -> NativeResult<AiSettingsView> {
    trusted(&window)?;
    chatgpt
        .manager()?
        .retry_secure_storage(account_id)
        .await
        .map_err(|error| error.message().to_owned())?;
    settings_view(&state, &chatgpt).await
}

#[tauri::command]
pub async fn sign_out_chatgpt(
    identity: State<'_, Mutex<crate::identity::IdentityService>>,
    window: WebviewWindow,
    state: AiState<'_>,
    chatgpt: State<'_, ChatgptState>,
    account_id: Uuid,
) -> NativeResult<AiSettingsView> {
    trusted(&window)?;
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
        let mut revocations = chatgpt
            .revocations
            .lock()
            .map_err(|_| "Account access state is unavailable.")?;
        let epoch = revocations.entry(account_id).or_default();
        *epoch = epoch
            .checked_add(1)
            .ok_or("Account access state must be restarted before signing out.")?;
        state
            .lock()
            .map_err(|_| "AI settings are unavailable.")?
            .cancel_account(account_id)?;
        identity
            .lock()
            .map_err(|_| "Identity analysis is unavailable.".to_owned())
            .and_then(|mut service| service.cancel_account(account_id))
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
}

async fn sign_out_after_cancellation<T>(
    cancellation: NativeResult<()>,
    sign_out: impl std::future::Future<Output = NativeResult<T>>,
) -> NativeResult<(T, Option<String>)> {
    let warning = cancellation.err().map(|reason| format!("Late identity output is blocked, but its local cancellation receipt needs attention: {reason}"));
    let result = sign_out.await;
    match result {
        Ok(value) => Ok((value, warning)),
        Err(reason) => Err(match warning {
            Some(warning) => {
                format!("{reason} Local identity cancellation also needs attention: {warning}")
            }
            None => reason,
        }),
    }
}

#[tauri::command]
pub async fn acknowledge_chatgpt_welcome(
    window: WebviewWindow,
    state: AiState<'_>,
    chatgpt: State<'_, ChatgptState>,
) -> NativeResult<AiSettingsView> {
    trusted(&window)?;
    chatgpt
        .manager()?
        .acknowledge_welcome()
        .await
        .map_err(|error| error.message().to_owned())?;
    settings_view(&state, &chatgpt).await
}

#[tauri::command]
pub fn open_chatgpt_usage(app: AppHandle, window: WebviewWindow) -> NativeResult<()> {
    trusted(&window)?;
    #[cfg(not(feature = "native-tests"))]
    {
        let _ = app;
        crate::system_open::open(USAGE_URL).map_err(|_| {
            "Couldn't open a browser. Open https://chatgpt.com/settings/usage manually.".into()
        })
    }
    #[cfg(feature = "native-tests")]
    {
        use tauri::Emitter;
        app.emit("desegni-test-usage-opened", USAGE_URL)
            .map_err(|_| "The isolated usage event could not be recorded.".into())
    }
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
        state.revocations.lock().unwrap().insert(account, epoch + 1);
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
