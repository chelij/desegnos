use crate::{ErrorKind, ProviderError, Result};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use zeroize::Zeroizing;

const ACCOUNT: &str = "api-key";

#[derive(Clone)]
pub struct ApiKey(Option<Arc<Zeroizing<String>>>);

impl ApiKey {
    pub fn local_anonymous() -> Self {
        Self(None)
    }
    pub(crate) fn is_anonymous(&self) -> bool {
        self.0.is_none()
    }
    pub fn new(value: String) -> Result<Self> {
        Self::bounded(value, 4096)
    }

    pub(crate) fn from_chatgpt_token(value: String) -> Result<Self> {
        Self::bounded(value, 16 * 1024)
            .map_err(|error| error.with_authentication(crate::AuthenticationMethod::Chatgpt))
    }

    fn bounded(value: String, maximum: usize) -> Result<Self> {
        let value = Zeroizing::new(value);
        if value.is_empty()
            || value.len() > maximum
            || !value.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(ProviderError::before(ErrorKind::InvalidCredential));
        }
        Ok(Self(Some(Arc::new(value))))
    }

    /// Non-secret cache identity; never persist the credential itself.
    pub fn image_access_cache_key(&self) -> String {
        format!("apiKey:{}", desegnos_core::digest(self.expose().as_bytes()))
    }

    pub(crate) fn expose(&self) -> &str {
        self.0.as_deref().map_or("", |value| value.as_str())
    }
    pub(crate) fn appears_in(&self, value: &str) -> bool {
        !self.is_anonymous() && value.contains(self.expose())
    }
    fn masked(&self) -> String {
        if self.expose().len() < 8 {
            "••••".into()
        } else {
            format!("••••{}", &self.expose()[self.expose().len() - 4..])
        }
    }
}

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ApiKey([REDACTED])")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CredentialStorage {
    System,
    Session,
}

#[derive(ts_rs::TS, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CredentialLocation {
    System,
    Session,
    None,
}

#[derive(ts_rs::TS, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CredentialInfo {
    pub configured: bool,
    pub storage: CredentialLocation,
    pub masked_identifier: Option<String>,
    pub vault_error: Option<String>,
}

trait Vault: Send + Sync {
    fn get(&self) -> Result<Option<ApiKey>>;
    fn set(&self, key: &ApiKey) -> Result<()>;
    fn remove(&self) -> Result<()>;
}

struct SystemVault {
    service: String,
}

impl SystemVault {
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    fn entry(&self) -> Result<keyring::Entry> {
        keyring::Entry::new(&self.service, ACCOUNT)
            .map_err(|_| ProviderError::before(ErrorKind::VaultUnavailable))
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
impl Vault for SystemVault {
    fn get(&self) -> Result<Option<ApiKey>> {
        match self.entry()?.get_password() {
            Ok(value) => ApiKey::new(value).map(Some),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(ProviderError::before(ErrorKind::VaultUnavailable)),
        }
    }
    fn set(&self, key: &ApiKey) -> Result<()> {
        self.entry()?
            .set_password(key.expose())
            .map_err(|_| ProviderError::before(ErrorKind::VaultUnavailable))
    }
    fn remove(&self) -> Result<()> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(ProviderError::before(ErrorKind::VaultUnavailable)),
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
impl Vault for SystemVault {
    fn get(&self) -> Result<Option<ApiKey>> {
        Err(ProviderError::before(ErrorKind::VaultUnavailable))
    }
    fn set(&self, _: &ApiKey) -> Result<()> {
        Err(ProviderError::before(ErrorKind::VaultUnavailable))
    }
    fn remove(&self) -> Result<()> {
        Err(ProviderError::before(ErrorKind::VaultUnavailable))
    }
}

#[derive(Default)]
struct CredentialState {
    session: Option<ApiKey>,
    suppress_system: bool,
    /// The last system-keyring read. A keyring that is locked, missing or slow to activate can
    /// take seconds per lookup, and settings, generation and image checks each read the key, so
    /// one read is reused: keys this app sets or removes update it, and a failed read expires.
    /// A key changed by another running instance shows after this one restarts.
    system: Option<(std::time::Instant, Result<Option<ApiKey>>)>,
}

/// How long a failed keyring read is remembered before the keyring is asked again.
const FAILED_READ_TTL: std::time::Duration = std::time::Duration::from_secs(30);

impl CredentialState {
    fn system_key(&mut self, vault: &dyn Vault) -> Result<Option<ApiKey>> {
        let fresh = self
            .system
            .as_ref()
            .is_some_and(|(at, result)| result.is_ok() || at.elapsed() < FAILED_READ_TTL);
        if !fresh {
            self.system = Some((std::time::Instant::now(), vault.get()));
        }
        self.system.as_ref().expect("read above").1.clone()
    }
}

pub struct CredentialManager {
    state: Mutex<CredentialState>,
    vault: Box<dyn Vault>,
}

impl Default for CredentialManager {
    fn default() -> Self {
        Self::new()
    }
}

impl CredentialManager {
    pub fn new() -> Self {
        Self::for_provider(crate::ProviderId::Openai)
    }

    pub fn for_provider(provider: crate::ProviderId) -> Self {
        Self {
            state: Mutex::new(CredentialState::default()),
            vault: Box::new(SystemVault {
                service: provider.service().into(),
            }),
        }
    }

    /// Custom-server keys belong to the exact base URL, so switching hosts cannot forward a key.
    pub fn for_custom_endpoint(endpoint: &str) -> Self {
        Self {
            state: Mutex::new(CredentialState::default()),
            vault: Box::new(SystemVault {
                service: format!(
                    "io.desegnos.custom.{}",
                    desegnos_core::digest(endpoint.as_bytes())
                ),
            }),
        }
    }

    pub fn for_images() -> Self {
        Self {
            state: Mutex::new(CredentialState::default()),
            vault: Box::new(SystemVault {
                service: "io.desegnos.openai.images".into(),
            }),
        }
    }

    pub fn status(&self) -> CredentialInfo {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(key) = &state.session {
            return info(Some(key), CredentialLocation::Session, None);
        }
        if state.suppress_system {
            return info(None, CredentialLocation::None, None);
        }
        match state.system_key(self.vault.as_ref()) {
            Ok(key) => info(key.as_ref(), CredentialLocation::System, None),
            Err(error) => info(None, CredentialLocation::None, Some(error.message())),
        }
    }

    pub fn set_key(&self, value: String, storage: CredentialStorage) -> Result<CredentialInfo> {
        let key = ApiKey::new(value)?;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match storage {
            CredentialStorage::System => {
                self.vault.set(&key)?;
                state.system = Some((std::time::Instant::now(), Ok(Some(key.clone()))));
                state.session = None;
                state.suppress_system = false;
                Ok(info(Some(&key), CredentialLocation::System, None))
            }
            CredentialStorage::Session => {
                state.session = Some(key);
                state.suppress_system = true;
                Ok(info(
                    state.session.as_ref(),
                    CredentialLocation::Session,
                    None,
                ))
            }
        }
    }

    pub fn remove_key(&self) -> Result<CredentialInfo> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.vault.remove()?;
        state.system = Some((std::time::Instant::now(), Ok(None)));
        state.session = None;
        state.suppress_system = true;
        Ok(info(None, CredentialLocation::None, None))
    }

    pub fn key(&self) -> Result<ApiKey> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(key) = &state.session {
            return Ok(key.clone());
        }
        if state.suppress_system {
            return Err(ProviderError::before(ErrorKind::MissingCredential));
        }
        state
            .system_key(self.vault.as_ref())?
            .ok_or_else(|| ProviderError::before(ErrorKind::MissingCredential))
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn isolated_for_tests() -> Self {
        Self {
            state: Mutex::new(CredentialState::default()),
            vault: Box::new(UnavailableVault),
        }
    }
}

fn info(
    key: Option<&ApiKey>,
    location: CredentialLocation,
    vault_error: Option<String>,
) -> CredentialInfo {
    CredentialInfo {
        configured: key.is_some(),
        storage: if key.is_some() {
            location
        } else {
            CredentialLocation::None
        },
        masked_identifier: key.map(ApiKey::masked),
        vault_error,
    }
}

#[cfg(any(test, feature = "test-support"))]
struct UnavailableVault;

#[cfg(any(test, feature = "test-support"))]
impl Vault for UnavailableVault {
    fn get(&self) -> Result<Option<ApiKey>> {
        Ok(None)
    }
    fn set(&self, _: &ApiKey) -> Result<()> {
        Err(ProviderError::before(ErrorKind::VaultUnavailable))
    }
    fn remove(&self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "sk-test-credential-that-never-leaves-Rust";

    #[test]
    fn explicit_memory_fallback_is_masked_and_never_serializes_the_key() {
        let manager = CredentialManager::isolated_for_tests();
        assert!(!manager.status().configured);
        let failure = manager
            .set_key(KEY.into(), CredentialStorage::System)
            .unwrap_err();
        assert_eq!(failure.kind, ErrorKind::VaultUnavailable);
        assert!(!manager.status().configured);
        let view = manager
            .set_key(KEY.into(), CredentialStorage::Session)
            .unwrap();
        assert_eq!(view.storage, CredentialLocation::Session);
        assert_eq!(view.masked_identifier.as_deref(), Some("••••Rust"));
        assert!(!serde_json::to_string(&view).unwrap().contains(KEY));
        assert!(!format!("{:?}", manager.key().unwrap()).contains(KEY));
        let other = CredentialManager::isolated_for_tests();
        assert!(!other.status().configured);
        manager.remove_key().unwrap();
        assert_eq!(
            manager.key().unwrap_err().kind,
            ErrorKind::MissingCredential
        );
    }

    #[derive(Default)]
    struct MemoryVault(Mutex<Option<ApiKey>>);
    impl Vault for Arc<MemoryVault> {
        fn get(&self) -> Result<Option<ApiKey>> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn set(&self, key: &ApiKey) -> Result<()> {
            *self.0.lock().unwrap() = Some(key.clone());
            Ok(())
        }
        fn remove(&self) -> Result<()> {
            *self.0.lock().unwrap() = None;
            Ok(())
        }
    }

    #[test]
    fn system_replace_and_remove_touch_only_the_owned_vault_entry() {
        let vault = Arc::new(MemoryVault::default());
        let manager = CredentialManager {
            state: Mutex::default(),
            vault: Box::new(vault.clone()),
        };
        manager
            .set_key(KEY.into(), CredentialStorage::System)
            .unwrap();
        assert_eq!(manager.status().storage, CredentialLocation::System);
        let replacement = "sk-test-replacement-credential-is-private";
        manager
            .set_key(replacement.into(), CredentialStorage::System)
            .unwrap();
        assert_eq!(vault.get().unwrap().unwrap().expose(), replacement);
        manager.remove_key().unwrap();
        assert!(vault.get().unwrap().is_none());
        assert_eq!(manager.status().storage, CredentialLocation::None);
    }

    #[test]
    fn invalid_keys_do_not_replace_the_accepted_key_or_echo_in_errors() {
        let manager = CredentialManager::isolated_for_tests();
        manager
            .set_key(KEY.into(), CredentialStorage::Session)
            .unwrap();
        for invalid in [
            "",
            "sk-test\r\nInjected: credential-that-is-invalid",
            " sk-test-credential-with-leading-space",
        ] {
            let error = manager
                .set_key(invalid.into(), CredentialStorage::Session)
                .unwrap_err();
            assert_eq!(error.kind, ErrorKind::InvalidCredential);
            if !invalid.is_empty() {
                assert!(!format!("{error:?} {error}").contains(invalid));
            }
            assert_eq!(manager.key().unwrap().expose(), KEY);
        }
    }

    #[test]
    fn key_validation_allows_header_safe_formats_without_guessing_provider_prefixes() {
        for value in ["future-provider_key.format+/=~", "short"] {
            let key = ApiKey::new(value.into()).unwrap();
            assert_eq!(key.expose(), value);
            assert_ne!(key.masked(), value);
        }
        assert_eq!(
            ApiKey::new("x".repeat(4097)).unwrap_err().kind,
            ErrorKind::InvalidCredential
        );
    }

    struct LockedVault;
    impl Vault for LockedVault {
        fn get(&self) -> Result<Option<ApiKey>> {
            Err(ProviderError::before(ErrorKind::VaultUnavailable))
        }
        fn set(&self, _: &ApiKey) -> Result<()> {
            Err(ProviderError::before(ErrorKind::VaultUnavailable))
        }
        fn remove(&self) -> Result<()> {
            Err(ProviderError::before(ErrorKind::VaultUnavailable))
        }
    }

    #[test]
    fn failed_vault_removal_preserves_the_accepted_session_key_and_reports_the_failure() {
        let manager = CredentialManager {
            state: Mutex::default(),
            vault: Box::new(LockedVault),
        };
        assert!(manager.status().vault_error.is_some());
        manager
            .set_key(KEY.into(), CredentialStorage::Session)
            .unwrap();
        let before = manager.status();
        assert_eq!(
            manager.remove_key().unwrap_err().kind,
            ErrorKind::VaultUnavailable
        );
        assert_eq!(manager.status(), before);
        assert_eq!(manager.key().unwrap().expose(), KEY);
    }

    #[derive(Default)]
    struct CountingVault {
        reads: std::sync::atomic::AtomicUsize,
        key: Mutex<Option<ApiKey>>,
        fail: bool,
    }
    impl Vault for Arc<CountingVault> {
        fn get(&self) -> Result<Option<ApiKey>> {
            self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.fail {
                return Err(ProviderError::before(ErrorKind::VaultUnavailable));
            }
            Ok(self.key.lock().unwrap().clone())
        }
        fn set(&self, key: &ApiKey) -> Result<()> {
            *self.key.lock().unwrap() = Some(key.clone());
            Ok(())
        }
        fn remove(&self) -> Result<()> {
            *self.key.lock().unwrap() = None;
            Ok(())
        }
    }

    #[test]
    fn the_keyring_is_read_once_and_kept_current_by_this_apps_own_changes() {
        let vault = Arc::new(CountingVault::default());
        let manager = CredentialManager {
            state: Mutex::default(),
            vault: Box::new(vault.clone()),
        };
        let reads = || vault.reads.load(std::sync::atomic::Ordering::SeqCst);
        assert!(!manager.status().configured);
        assert!(manager.key().is_err());
        assert!(!manager.status().configured);
        assert_eq!(
            reads(),
            1,
            "a slow keyring must not be asked on every settings read"
        );
        manager
            .set_key(KEY.into(), CredentialStorage::System)
            .unwrap();
        assert!(manager.status().configured);
        assert_eq!(manager.key().unwrap().expose(), KEY);
        manager.remove_key().unwrap();
        assert!(!manager.status().configured);
        assert_eq!(reads(), 1);
    }

    #[test]
    fn a_failed_keyring_read_is_remembered_briefly_not_retried_on_every_call() {
        let vault = Arc::new(CountingVault {
            fail: true,
            ..Default::default()
        });
        let manager = CredentialManager {
            state: Mutex::default(),
            vault: Box::new(vault.clone()),
        };
        assert!(manager.status().vault_error.is_some());
        assert_eq!(manager.key().unwrap_err().kind, ErrorKind::VaultUnavailable);
        assert_eq!(vault.reads.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
