use super::{ChatgptErrorKind as Kind, ChatgptResult};
use crate::CredentialStorage;
use serde::{Deserialize, Serialize};
#[cfg(any(test, feature = "test-support"))]
use std::sync::Arc;
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

pub(super) const MAX_RECORD: usize = 512 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Registration {
    pub id: Uuid,
    pub label: String,
    pub email: Option<String>,
    pub subject: Option<String>,
    pub client_id: String,
    pub issuer: String,
    pub system_stored: bool,
    pub reconnect_required: bool,
    #[serde(default)]
    pub refresh_pending: Option<Uuid>,
    pub last_plan_enabled: Option<bool>,
    pub welcomed: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Metadata {
    pub version: u32,
    pub host_id: Uuid,
    pub active: Option<Uuid>,
    pub registrations: Vec<Registration>,
}

impl Metadata {
    pub fn registration(&self, id: Uuid) -> ChatgptResult<&Registration> {
        self.registrations
            .iter()
            .find(|registration| registration.id == id)
            .ok_or_else(|| Kind::Reconnect.into())
    }
    fn validate(&self) -> ChatgptResult<()> {
        let mut ids = BTreeSet::new();
        if self.version != 1 || self.host_id.is_nil() || self.registrations.len() > 64 {
            return Err(Kind::Storage.into());
        }
        for registration in &self.registrations {
            if registration.id.is_nil()
                || !ids.insert(registration.id)
                || registration.label.is_empty()
                || registration.label.len() > 240
                || registration.refresh_pending.is_some_and(|id| id.is_nil())
                || !valid_client(&registration.client_id)
                || registration
                    .subject
                    .as_ref()
                    .is_some_and(|value| value.is_empty() || value.len() > 512)
                || registration
                    .email
                    .as_ref()
                    .is_some_and(|value| value.len() > 320)
            {
                return Err(Kind::Storage.into());
            }
        }
        if self.active.is_some_and(|id| !ids.contains(&id)) {
            return Err(Kind::Storage.into());
        }
        Ok(())
    }
}

pub(super) fn valid_client(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value != "dynamic_agent_client"
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct TokenRecord {
    pub version: u32,
    pub registration_id: Uuid,
    pub host_id: Uuid,
    pub client_id: String,
    pub subject: String,
    pub issuer: String,
    pub access_token: String,
    pub refresh_token: String,
    pub id_token: String,
    pub nonce: String,
    pub scopes: Vec<String>,
    pub expires_at: u64,
    pub earliest_refresh_at: u64,
    pub storage: CredentialStorage,
}
impl Drop for TokenRecord {
    fn drop(&mut self) {
        self.access_token.zeroize();
        self.refresh_token.zeroize();
        self.id_token.zeroize();
        self.nonce.zeroize();
    }
}
impl TokenRecord {
    pub fn plan_enabled(&self) -> bool {
        self.scopes
            .iter()
            .any(|scope| scope == "chatgpt.tokens.use.direct")
    }
    pub fn validate_mapping(
        &self,
        metadata: &Metadata,
        registration: &Registration,
    ) -> ChatgptResult<()> {
        if self.version != 1
            || self.host_id != metadata.host_id
            || self.registration_id != registration.id
            || self.client_id != registration.client_id
            || self.subject != registration.subject.as_deref().unwrap_or("")
            || self.issuer != registration.issuer
            || self.access_token.is_empty()
            || self.refresh_token.is_empty()
            || self.id_token.is_empty()
        {
            return Err(Kind::Reconnect.into());
        }
        Ok(())
    }
}

pub(super) struct Store {
    pub directory: PathBuf,
}
impl Store {
    pub fn initialize(directory: &Path) -> ChatgptResult<Self> {
        if directory.exists() {
            let info = fs::symlink_metadata(directory).map_err(|_| Kind::Storage)?;
            if !info.is_dir() || info.file_type().is_symlink() {
                return Err(Kind::Storage.into());
            }
        } else {
            fs::create_dir_all(directory).map_err(|_| Kind::Storage)?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
                .map_err(|_| Kind::Storage)?;
        }
        let store = Self {
            directory: directory.into(),
        };
        let _lock = store.lock()?;
        if store.directory.join("accounts.json").exists() {
            store.read()?;
        } else {
            store.write(&Metadata {
                version: 1,
                host_id: Uuid::new_v4(),
                active: None,
                registrations: vec![],
            })?;
        }
        Ok(store)
    }
    pub fn lock(&self) -> ChatgptResult<File> {
        let file = self.open_lock()?;
        let started = std::time::Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(file),
                Err(std::fs::TryLockError::WouldBlock)
                    if started.elapsed() < std::time::Duration::from_secs(5) =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(20))
                }
                _ => return Err(Kind::Storage.into()),
            }
        }
    }
    pub fn open_lock(&self) -> ChatgptResult<File> {
        let path = self.directory.join("accounts.lock");
        reject_symlink(&path)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path).map_err(|_| Kind::Storage)?;
        Ok(file)
    }
    pub fn read(&self) -> ChatgptResult<Metadata> {
        let path = self.directory.join("accounts.json");
        reject_symlink(&path)?;
        let file = File::open(path).map_err(|_| Kind::Storage)?;
        let mut bytes = vec![];
        file.take((MAX_RECORD + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| Kind::Storage)?;
        if bytes.len() > MAX_RECORD {
            return Err(Kind::Storage.into());
        }
        let metadata: Metadata = serde_json::from_slice(&bytes).map_err(|_| Kind::Storage)?;
        metadata.validate()?;
        Ok(metadata)
    }
    pub fn write(&self, metadata: &Metadata) -> ChatgptResult<()> {
        metadata.validate()?;
        let bytes = serde_json::to_vec(metadata).map_err(|_| Kind::Storage)?;
        if bytes.len() > MAX_RECORD {
            return Err(Kind::Storage.into());
        }
        let destination = self.directory.join("accounts.json");
        reject_symlink(&destination)?;
        let temporary = self
            .directory
            .join(format!("accounts-{}.tmp", Uuid::new_v4()));
        let operation = || -> ChatgptResult<()> {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary).map_err(|_| Kind::Storage)?;
            file.write_all(&bytes).map_err(|_| Kind::Storage)?;
            file.sync_all().map_err(|_| Kind::Storage)?;
            fs::rename(&temporary, &destination).map_err(|_| Kind::Storage)?;
            #[cfg(unix)]
            File::open(&self.directory)
                .and_then(|file| file.sync_all())
                .map_err(|_| Kind::Storage)?;
            Ok(())
        };
        let result = operation();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
}
fn reject_symlink(path: &Path) -> ChatgptResult<()> {
    match fs::symlink_metadata(path) {
        Ok(info) if info.file_type().is_symlink() || !info.is_file() => Err(Kind::Storage.into()),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(Kind::Storage.into()),
    }
}

pub(super) trait Vault: Send + Sync {
    fn get(&self, account: Uuid) -> ChatgptResult<Option<Zeroizing<String>>>;
    fn set(&self, account: Uuid, value: &str) -> ChatgptResult<()>;
    fn remove(&self, account: Uuid) -> ChatgptResult<()>;
}
pub(super) struct SystemVault;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
impl SystemVault {
    fn entry(account: Uuid) -> ChatgptResult<keyring::Entry> {
        keyring::Entry::new("io.github.chelij.desegnos.chatgpt", &account.to_string())
            .map_err(|_| Kind::VaultUnavailable.into())
    }
}
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
impl Vault for SystemVault {
    fn get(&self, account: Uuid) -> ChatgptResult<Option<Zeroizing<String>>> {
        match Self::entry(account)?.get_password() {
            Ok(value) => Ok(Some(Zeroizing::new(value))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(Kind::VaultUnavailable.into()),
        }
    }
    fn set(&self, account: Uuid, value: &str) -> ChatgptResult<()> {
        Self::entry(account)?
            .set_password(value)
            .map_err(|_| Kind::VaultUnavailable.into())
    }
    fn remove(&self, account: Uuid) -> ChatgptResult<()> {
        match Self::entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(Kind::VaultUnavailable.into()),
        }
    }
}
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
impl Vault for SystemVault {
    fn get(&self, _: Uuid) -> ChatgptResult<Option<Zeroizing<String>>> {
        Err(Kind::VaultUnavailable.into())
    }
    fn set(&self, _: Uuid, _: &str) -> ChatgptResult<()> {
        Err(Kind::VaultUnavailable.into())
    }
    fn remove(&self, _: Uuid) -> ChatgptResult<()> {
        Err(Kind::VaultUnavailable.into())
    }
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Clone, Default)]
pub struct ChatgptTestVault {
    pub(super) data: Arc<std::sync::Mutex<std::collections::BTreeMap<Uuid, Zeroizing<String>>>>,
    pub(super) unavailable: Arc<std::sync::atomic::AtomicBool>,
    system: bool,
    write_failure_flag: Option<PathBuf>,
}
#[cfg(any(test, feature = "test-support"))]
impl ChatgptTestVault {
    pub fn memory() -> Self {
        Self::default()
    }
    pub fn memory_with_write_failure_flag(path: PathBuf) -> Self {
        Self {
            write_failure_flag: Some(path),
            ..Self::default()
        }
    }
    pub fn unavailable() -> Self {
        let value = Self::default();
        value
            .unavailable
            .store(true, std::sync::atomic::Ordering::SeqCst);
        value
    }
    /// Uses the real platform vault. The caller must first isolate and verify its OS credential service.
    pub fn system() -> Self {
        Self {
            system: true,
            ..Self::default()
        }
    }
}
#[cfg(any(test, feature = "test-support"))]
impl Vault for ChatgptTestVault {
    fn get(&self, account: Uuid) -> ChatgptResult<Option<Zeroizing<String>>> {
        if self.system {
            return SystemVault.get(account);
        }
        Ok(self
            .data
            .lock()
            .map_err(|_| Kind::VaultUnavailable)?
            .get(&account)
            .cloned())
    }
    fn set(&self, account: Uuid, value: &str) -> ChatgptResult<()> {
        if self.system {
            return SystemVault.set(account, value);
        }
        if self.unavailable.load(std::sync::atomic::Ordering::SeqCst)
            || self
                .write_failure_flag
                .as_ref()
                .is_some_and(|path| path.try_exists().unwrap_or(true))
        {
            return Err(Kind::VaultUnavailable.into());
        }
        self.data
            .lock()
            .map_err(|_| Kind::VaultUnavailable)?
            .insert(account, Zeroizing::new(value.into()));
        Ok(())
    }
    fn remove(&self, account: Uuid) -> ChatgptResult<()> {
        if self.system {
            return SystemVault.remove(account);
        }
        if self.unavailable.load(std::sync::atomic::Ordering::SeqCst)
            || self
                .write_failure_flag
                .as_ref()
                .is_some_and(|path| path.try_exists().unwrap_or(true))
        {
            return Err(Kind::VaultUnavailable.into());
        }
        self.data
            .lock()
            .map_err(|_| Kind::VaultUnavailable)?
            .remove(&account);
        Ok(())
    }
}
