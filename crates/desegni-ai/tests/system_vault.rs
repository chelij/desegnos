#![cfg(all(target_os = "linux", feature = "test-support"))]

use desegni_ai::{
    CancellationToken, ChatgptManager, ChatgptTestVault, CredentialInfo, CredentialLocation,
    CredentialManager, CredentialStorage, ErrorKind, OpenAiClient, ProviderSettings,
};
use std::{
    env, fs,
    io::{Read, Write},
    net::TcpListener,
    os::unix::fs::MetadataExt,
    path::PathBuf,
    process::Command,
    thread,
    time::{Duration, Instant},
};

const INITIAL: &str = "sk-desegni-private-vault-synthetic-S001";
const REPLACED: &str = "sk-desegni-private-vault-synthetic-S002";
const SESSION: &str = "sk-desegni-private-vault-synthetic-S003";

fn require_private_vault() {
    assert!(
        env::var("DESEGNI_TEST_VAULT_OPT_IN").as_deref() == Ok("private-secret-service-v1"),
        "Use scripts/test-vault.mjs; refusing to access any system vault without its isolation guard."
    );
    let root = fs::canonicalize(
        env::var_os("DESEGNI_TEST_VAULT_ROOT").expect("Missing private vault directory"),
    )
    .expect("Private vault directory unavailable");
    assert!(
        root.parent() == Some(std::path::Path::new("/tmp"))
            && root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("desegni-vault-"),
        "Private vault directory must be owned temporary storage"
    );
    let owner = fs::metadata("/proc/self").unwrap().uid();
    let metadata = fs::metadata(&root).unwrap();
    assert!(
        metadata.uid() == owner && metadata.mode() & 0o077 == 0,
        "Private vault directory permissions are unsafe"
    );
    let marker: serde_json::Value = serde_json::from_slice(
        &fs::read(root.join("isolation.json")).expect("Missing isolation marker"),
    )
    .expect("Invalid isolation marker");
    let token = env::var("DESEGNI_TEST_VAULT_TOKEN").expect("Missing private vault token");
    assert!(
        token.len() == 48 && marker["token"].as_str() == Some(token.as_str()),
        "Private vault token mismatch"
    );
    let address = env::var("DBUS_SESSION_BUS_ADDRESS").expect("Missing private bus");
    assert!(
        marker["busAddress"].as_str() == Some(address.as_str()),
        "Private bus mismatch"
    );
    let pid = marker["keyringPid"]
        .as_u64()
        .expect("Missing private keyring process");
    let process = PathBuf::from(format!("/proc/{pid}"));
    assert!(
        fs::metadata(&process).unwrap().uid() == owner,
        "Private keyring owner mismatch"
    );
    let process_environment =
        fs::read(process.join("environ")).expect("Private keyring environment unavailable");
    for (variable, directory) in [
        ("XDG_DATA_HOME", "data"),
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_CACHE_HOME", "cache"),
        ("XDG_RUNTIME_DIR", "runtime"),
    ] {
        let expected = root.join(directory);
        let actual =
            fs::canonicalize(env::var_os(variable).expect("Missing private XDG directory"))
                .unwrap();
        assert!(
            actual == expected && fs::metadata(&actual).unwrap().mode() & 0o077 == 0,
            "Private XDG isolation mismatch"
        );
        let entry = format!("{variable}={}", expected.display());
        assert!(
            process_environment
                .split(|byte| *byte == 0)
                .any(|value| value == entry.as_bytes()),
            "Keyring daemon uses another XDG directory"
        );
    }
    let bus_entry = format!("DBUS_SESSION_BUS_ADDRESS={address}");
    assert!(
        process_environment
            .split(|byte| *byte == 0)
            .any(|value| value == bus_entry.as_bytes()),
        "Keyring daemon uses another bus"
    );
    let executable = fs::read_link(process.join("exe")).unwrap();
    assert!(
        executable
            .file_name()
            .is_some_and(|name| name == "gnome-keyring-daemon"),
        "Secret Service is not the private keyring daemon"
    );
    let output = Command::new("busctl")
        .args([
            "--address",
            &address,
            "--timeout=2",
            "call",
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "GetConnectionUnixProcessID",
            "s",
            "org.freedesktop.secrets",
        ])
        .output()
        .expect("Cannot verify private Secret Service owner");
    assert!(
        output.status.success()
            && String::from_utf8_lossy(&output.stdout).trim() == format!("u {pid}"),
        "Private bus is owned by another Secret Service"
    );
}

fn verify_view(view: &CredentialInfo, location: CredentialLocation, suffix: &str) {
    assert!(
        view.configured && view.storage == location && view.vault_error.is_none(),
        "Credential status differs from the accepted storage operation"
    );
    assert!(
        view.masked_identifier.as_deref() == Some(format!("••••{suffix}").as_str()),
        "Credential masking is incorrect"
    );
    let serialized = serde_json::to_string(view).unwrap();
    assert!(
        [INITIAL, REPLACED, SESSION]
            .iter()
            .all(|key| !serialized.contains(key)),
        "Credential status exposed a key"
    );
}

fn verify_authentication(manager: &CredentialManager, expected: &'static str) {
    let key = manager
        .key()
        .expect("Stored credential cannot be retrieved");
    assert!(
        format!("{key:?}") == "ApiKey([REDACTED])",
        "Opaque credential debug is not redacted"
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(6);
        let mut socket = loop {
            match listener.accept() {
                Ok((socket, _)) => break socket,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(10))
                }
                _ => return false,
            }
        };
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut received = Vec::new();
        let mut buffer = [0_u8; 1024];
        while !received.windows(4).any(|value| value == b"\r\n\r\n") && received.len() <= 16_384 {
            match socket.read(&mut buffer) {
                Ok(0) | Err(_) => return false,
                Ok(count) => received.extend_from_slice(&buffer[..count]),
            }
        }
        let header = String::from_utf8_lossy(&received);
        let correct = header.starts_with("GET /v1/models/gpt-6.1-sol HTTP/1.1\r\n")
            && header
                .lines()
                .filter_map(|line| line.split_once(':'))
                .any(|(name, value)| {
                    name.eq_ignore_ascii_case("authorization")
                        && value.trim() == format!("Bearer {expected}")
                });
        let body = r#"{"id":"gpt-6.1-sol","object":"model"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(response.as_bytes()).is_ok() && correct
    });
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let client = OpenAiClient::for_loopback(&format!("http://{address}/v1")).unwrap();
    let result = runtime.block_on(async {
        tokio::time::timeout(
            Duration::from_secs(5),
            client.test_model_access(
                &ProviderSettings::default(),
                &key,
                &CancellationToken::new(),
            ),
        )
        .await
    });
    assert!(
        result.is_ok_and(|value| value.is_ok()) && server.join().unwrap(),
        "Vault credential failed the controlled authenticated request"
    );
}

fn verify_missing(manager: &CredentialManager) {
    let status = manager.status();
    assert!(
        !status.configured
            && status.storage == CredentialLocation::None
            && status.masked_identifier.is_none()
            && status.vault_error.is_none(),
        "Credential entry remains after removal"
    );
    assert!(
        manager
            .key()
            .is_err_and(|error| error.kind == ErrorKind::MissingCredential),
        "Removed credential remains retrievable"
    );
}

#[test]
#[ignore = "Requires scripts/test-vault.mjs with a private Secret Service and private XDG storage"]
fn private_system_vault_lifecycle() {
    require_private_vault();
    let phase = env::var("DESEGNI_TEST_VAULT_PHASE").expect("Missing private vault test phase");
    let manager = CredentialManager::new();
    match phase.as_str() {
        "initial" => {
            verify_missing(&manager);
            verify_view(
                &manager
                    .set_key(INITIAL.into(), CredentialStorage::System)
                    .unwrap(),
                CredentialLocation::System,
                "S001",
            );
            let reopened = CredentialManager::new();
            verify_view(&reopened.status(), CredentialLocation::System, "S001");
            verify_authentication(&reopened, INITIAL);
        }
        "replace-session" => {
            verify_view(&manager.status(), CredentialLocation::System, "S001");
            verify_authentication(&manager, INITIAL);
            verify_view(
                &manager
                    .set_key(REPLACED.into(), CredentialStorage::System)
                    .unwrap(),
                CredentialLocation::System,
                "S002",
            );
            verify_authentication(&CredentialManager::new(), REPLACED);
            verify_view(
                &manager
                    .set_key(SESSION.into(), CredentialStorage::Session)
                    .unwrap(),
                CredentialLocation::Session,
                "S003",
            );
            verify_authentication(&manager, SESSION);
            let durable = CredentialManager::new();
            verify_view(&durable.status(), CredentialLocation::System, "S002");
            verify_authentication(&durable, REPLACED);
        }
        "restart-remove" => {
            verify_view(&manager.status(), CredentialLocation::System, "S002");
            verify_authentication(&manager, REPLACED);
            manager
                .set_key(SESSION.into(), CredentialStorage::Session)
                .unwrap();
            manager.remove_key().unwrap();
            verify_missing(&manager);
            verify_missing(&CredentialManager::new());
        }
        "empty" => verify_missing(&manager),
        _ => panic!("Unknown private vault test phase"),
    }
    verify_chatgpt_vault(&phase);
}

fn verify_chatgpt_vault(phase: &str) {
    let root = PathBuf::from(env::var_os("DESEGNI_TEST_VAULT_ROOT").unwrap());
    let endpoint =
        env::var("DESEGNI_TEST_CHATGPT_ENDPOINT").expect("Missing isolated OAuth fixture");
    let client = OpenAiClient::for_loopback(&format!("{endpoint}/v1"))
        .expect("Invalid isolated model fixture");
    let manager = ChatgptManager::for_loopback(
        root.join("chatgpt"),
        &endpoint,
        client.clone(),
        ChatgptTestVault::system(),
    )
    .expect("Cannot open isolated ChatGPT manager");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        if phase == "initial" {
            let authorization = manager
                .begin_sign_in(CredentialStorage::System, None)
                .await
                .expect("Cannot begin private OAuth check");
            let opened = reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap()
                .get(authorization.url())
                .send()
                .await;
            assert!(
                opened.is_ok_and(|response| response.status().is_success()),
                "Private OAuth fixture did not accept browser authorization"
            );
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let view = manager.view().await;
                if view.sign_in.status == desegni_ai::ChatgptSignInStatus::Complete {
                    break;
                }
                assert!(
                    Instant::now() < deadline
                        && view.sign_in.status != desegni_ai::ChatgptSignInStatus::Failed,
                    "Private OAuth sign-in failed"
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
        let view = manager.view().await;
        assert_eq!(
            view.accounts.len(),
            1,
            "Private OAuth registration was not preserved"
        );
        let account = &view.accounts[0];
        if phase == "empty" {
            assert!(
                !account.connected
                    && !account.plan_enabled
                    && account.storage == CredentialLocation::None,
                "Signed-out OAuth access reappeared after a daemon restart"
            );
            assert!(
                manager
                    .bearer(account.id, &CancellationToken::new())
                    .await
                    .is_err(),
                "Signed-out OAuth token remains available"
            );
            return;
        }
        assert!(
            account.connected
                && account.plan_enabled
                && account.storage == CredentialLocation::System,
            "Complete OAuth token bundle did not survive a fresh process or private daemon restart"
        );
        let bearer = manager
            .bearer(account.id, &CancellationToken::new())
            .await
            .expect("Private OAuth bearer unavailable");
        assert_eq!(format!("{bearer:?}"), "ApiKey([REDACTED])");
        let models = client
            .list_chatgpt_models(&bearer, &CancellationToken::new())
            .await
            .expect("Stored OAuth token failed native model authentication");
        assert!(models.iter().any(|model| model.id == "gpt-6.1-sol"));
        if phase == "restart-remove" {
            let signed_out = manager
                .sign_out(account.id)
                .await
                .expect("Private OAuth revocation failed");
            assert!(
                !signed_out.accounts[0].connected
                    && signed_out.accounts[0].storage == CredentialLocation::None
            );
            assert!(
                signed_out
                    .message
                    .is_some_and(|message| message.contains("revoked"))
            );
        }
    });
}
