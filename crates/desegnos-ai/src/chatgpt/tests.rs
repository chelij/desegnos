use super::*;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{
        Mutex as StdMutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    task::JoinHandle,
};

#[derive(Clone)]
struct Configuration {
    plan: bool,
    subject: String,
    email: String,
    fault: Option<String>,
    kid: String,
    expires: u64,
    token_delay_ms: u64,
    models_delay_ms: u64,
    refresh_error: Option<String>,
    revocation_status: u16,
}
impl Default for Configuration {
    fn default() -> Self {
        Self {
            plan: true,
            subject: "fixture-subject-1".into(),
            email: "same@example.test".into(),
            fault: None,
            kid: "fixture-key-1".into(),
            expires: 3600,
            token_delay_ms: 0,
            models_delay_ms: 0,
            refresh_error: None,
            revocation_status: 200,
        }
    }
}
#[derive(Clone)]
struct Grant {
    client: String,
    nonce: String,
    challenge: String,
    redirect: String,
    subject: String,
}
struct Capture {
    method: String,
    path: String,
    form: BTreeMap<String, String>,
}
struct FixtureState {
    configuration: StdMutex<Configuration>,
    codes: StdMutex<BTreeMap<String, Grant>>,
    refresh: StdMutex<BTreeMap<String, Grant>>,
    requests: StdMutex<Vec<Capture>>,
    sequence: AtomicUsize,
    registrations: AtomicUsize,
    refreshes: AtomicUsize,
    models: AtomicUsize,
    failures: StdMutex<Vec<&'static str>>,
}
struct Fixture {
    base: String,
    state: Arc<FixtureState>,
    cancellation: CancellationToken,
    listener: JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.cancellation.cancel();
        self.listener.abort();
    }
}
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("desegnos-chatgpt-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

impl Fixture {
    async fn new() -> Self {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let state = Arc::new(FixtureState {
            configuration: StdMutex::new(Configuration::default()),
            codes: StdMutex::new(BTreeMap::new()),
            refresh: StdMutex::new(BTreeMap::new()),
            requests: StdMutex::new(vec![]),
            sequence: AtomicUsize::new(0),
            registrations: AtomicUsize::new(0),
            refreshes: AtomicUsize::new(0),
            models: AtomicUsize::new(0),
            failures: StdMutex::new(vec![]),
        });
        let cancellation = CancellationToken::new();
        let stop = cancellation.clone();
        let owned = state.clone();
        let address = base.clone();
        let task = tokio::spawn(async move {
            loop {
                let accepted = tokio::select! { _ = stop.cancelled() => break, value = listener.accept() => value };
                let Ok((stream, _)) = accepted else {
                    break;
                };
                let state = owned.clone();
                let address = address.clone();
                let stop = stop.clone();
                tokio::spawn(async move {
                    tokio::select! { _ = stop.cancelled() => {}, _ = serve(stream, state, &address) => {} }
                });
            }
        });
        Self {
            base,
            state,
            cancellation,
            listener: task,
        }
    }
    fn manager(&self, directory: &Directory, vault: ChatgptTestVault) -> ChatgptManager {
        ChatgptManager::for_loopback(
            &directory.0,
            &self.base,
            OpenAiClient::for_loopback(&format!("{}/v1", self.base)).unwrap(),
            vault,
        )
        .unwrap()
    }
    fn update(&self, update: impl FnOnce(&mut Configuration)) {
        update(&mut self.state.configuration.lock().unwrap());
    }
    async fn browser(&self, authorization: &ChatgptAuthorization) {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .unwrap();
        let response = client.get(authorization.url()).send().await.unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::FOUND);
        let callback = response
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(
            client.get(callback).send().await.unwrap().status(),
            reqwest::StatusCode::OK
        );
    }
    async fn sign_in(
        &self,
        manager: &ChatgptManager,
        storage: CredentialStorage,
        account: Option<Uuid>,
    ) -> ChatgptView {
        let authorization = manager.begin_sign_in(storage, account).await.unwrap();
        self.browser(&authorization).await;
        terminal(manager).await
    }
}

async fn terminal(manager: &ChatgptManager) -> ChatgptView {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let view = manager.view().await;
            if view.sign_in.status != ChatgptSignInStatus::AwaitingBrowser {
                return view;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("The controlled authorization did not complete")
}
async fn until(test: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while !test() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

async fn serve(mut stream: TcpStream, state: Arc<FixtureState>, base: &str) {
    let mut bytes = vec![];
    let (header_end, length) = loop {
        let mut buffer = [0_u8; 2048];
        let count = stream.read(&mut buffer).await.unwrap_or(0);
        if count == 0 {
            return;
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.len() > 1024 * 1024 {
            return;
        }
        if let Some(at) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            let header = String::from_utf8_lossy(&bytes[..at]);
            let length = header
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|value| value.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if bytes.len() >= at + 4 + length {
                break (at + 4, length);
            }
        }
    };
    let header = String::from_utf8_lossy(&bytes[..header_end]);
    let parts: Vec<_> = header.lines().next().unwrap().split(' ').collect();
    let method = parts[0];
    let url = reqwest::Url::parse(base).unwrap().join(parts[1]).unwrap();
    let path = url.path().to_owned();
    let form: BTreeMap<_, _> = reqwest::Url::parse(&format!(
        "{base}/?{}",
        String::from_utf8_lossy(&bytes[header_end..header_end + length])
    ))
    .unwrap()
    .query_pairs()
    .map(|(key, value)| (key.into_owned(), value.into_owned()))
    .collect();
    state.requests.lock().unwrap().push(Capture {
        method: method.into(),
        path: path.clone(),
        form: form.clone(),
    });
    let config = state.configuration.lock().unwrap().clone();
    let response = match path.as_str() {
        "/.well-known/openid-configuration" => {
            json!({"issuer":base,"authorization_endpoint":format!("{base}/api/accounts/authorize"),"token_endpoint":format!("{base}/api/accounts/oauth/token"),"jwks_uri":if config.fault.as_deref()==Some("discovery") { "https://untrusted.example/jwks".into() } else { format!("{base}/.well-known/jwks.json") },"revocation_endpoint":format!("{base}/oauth/revoke")})
        }
        "/.well-known/jwks.json" => {
            let mut key: Value =
                serde_json::from_str(include_str!("fixtures/test-jwk.json")).unwrap();
            key["kid"] = config.kid.clone().into();
            if config.fault.as_deref() == Some("jwk-use") {
                key["use"] = "enc".into();
            }
            if config.fault.as_deref() == Some("duplicate-kid") {
                json!({"keys":[key.clone(),key]})
            } else {
                json!({"keys":[key]})
            }
        }
        "/api/accounts/authorize" => {
            let query: BTreeMap<_, _> = url
                .query_pairs()
                .map(|(key, value)| (key.into_owned(), value.into_owned()))
                .collect();
            let new = query["client_id"] == "dynamic_agent_client";
            let client = if new {
                format!(
                    "oaiapp_fixture_{}",
                    state.registrations.fetch_add(1, Ordering::SeqCst) + 1
                )
            } else {
                query["client_id"].clone()
            };
            let code = format!(
                "fixture_code_{}",
                state.sequence.fetch_add(1, Ordering::SeqCst)
            );
            let grant = Grant {
                client: client.clone(),
                nonce: query["nonce"].clone(),
                challenge: query["code_challenge"].clone(),
                redirect: query["redirect_uri"].clone(),
                subject: config.subject.clone(),
            };
            state.codes.lock().unwrap().insert(code.clone(), grant);
            let mut callback = reqwest::Url::parse(&query["redirect_uri"]).unwrap();
            callback
                .query_pairs_mut()
                .append_pair("code", &code)
                .append_pair("state", &query["state"]);
            if new {
                callback.query_pairs_mut().append_pair("client_id", &client);
            }
            write_response(&mut stream, 302, "", Some(callback.as_str())).await;
            return;
        }
        "/api/accounts/oauth/token" => {
            if config.token_delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(config.token_delay_ms)).await;
            }
            let grant = if form.get("grant_type").map(String::as_str) == Some("authorization_code")
            {
                let grant = state
                    .codes
                    .lock()
                    .unwrap()
                    .remove(form.get("code").map(String::as_str).unwrap_or(""));
                let Some(grant) = grant else {
                    write_response(&mut stream, 400, "{\"error\":\"invalid_grant\"}", None).await;
                    return;
                };
                let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(
                    form.get("code_verifier")
                        .map(String::as_bytes)
                        .unwrap_or_default(),
                ));
                if challenge != grant.challenge
                    || form.get("client_id") != Some(&grant.client)
                    || form.get("redirect_uri") != Some(&grant.redirect)
                    || form.get("resource").map(String::as_str) != Some(http::RESOURCE)
                {
                    state
                        .failures
                        .lock()
                        .unwrap()
                        .push("PKCE, client, resource or redirect mismatch");
                    write_response(&mut stream, 400, "{\"error\":\"invalid_grant\"}", None).await;
                    return;
                }
                grant
            } else {
                state.refreshes.fetch_add(1, Ordering::SeqCst);
                if let Some(error) = &config.refresh_error {
                    write_response(&mut stream, 400, &json!({"error":error}).to_string(), None)
                        .await;
                    return;
                }
                let grant = state
                    .refresh
                    .lock()
                    .unwrap()
                    .remove(form.get("refresh_token").map(String::as_str).unwrap_or(""));
                let Some(grant) = grant else {
                    write_response(
                        &mut stream,
                        400,
                        "{\"error\":\"refresh_token_reused\"}",
                        None,
                    )
                    .await;
                    return;
                };
                if form.get("client_id") != Some(&grant.client)
                    || form.contains_key("scope")
                    || form.get("resource").map(String::as_str) != Some(http::RESOURCE)
                {
                    state
                        .failures
                        .lock()
                        .unwrap()
                        .push("Refresh registration or resource mismatch");
                }
                grant
            };
            let sequence = state.sequence.fetch_add(1, Ordering::SeqCst);
            let refresh = format!("fixture_refresh_{sequence}");
            state
                .refresh
                .lock()
                .unwrap()
                .insert(refresh.clone(), grant.clone());
            json!({"access_token":format!("fixture_access_{sequence}"),"refresh_token":refresh,"id_token":signed_identity(&grant,&config,base),"token_type":"Bearer","expires_in":config.expires,"scope":if config.plan { SCOPES } else { "openid profile email offline_access resource.invoke" },"earliest_refresh_at":0})
        }
        "/v1/models" => {
            state.models.fetch_add(1, Ordering::SeqCst);
            if config.models_delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(config.models_delay_ms)).await;
            }
            json!({"models":[{"slug":"gpt-6.1-sol","display_name":"GPT-6.1 Sol","visibility":"list"},{"slug":"fixture-hidden","display_name":"Hidden","visibility":"hidden"}]})
        }
        "/oauth/revoke" => {
            if form.get("token_type_hint").map(String::as_str) != Some("refresh_token")
                || !form.contains_key("client_id")
            {
                state
                    .failures
                    .lock()
                    .unwrap()
                    .push("Invalid revocation form");
            }
            if config.revocation_status == 200 {
                state
                    .refresh
                    .lock()
                    .unwrap()
                    .remove(form.get("token").map(String::as_str).unwrap_or(""));
            }
            write_response(&mut stream, config.revocation_status, "", None).await;
            return;
        }
        _ => {
            state
                .failures
                .lock()
                .unwrap()
                .push("Unexpected fixture route");
            write_response(&mut stream, 404, "{}", None).await;
            return;
        }
    };
    write_response(&mut stream, 200, &response.to_string(), None).await;
}

fn signed_identity(grant: &Grant, config: &Configuration, issuer: &str) -> String {
    let mut claims = json!({"iss":issuer,"aud":grant.client,"sub":grant.subject,"email":config.email,"nonce":grant.nonce,"iat":now(),"exp":now()+3600});
    match config.fault.as_deref() {
        Some("nonce") => claims["nonce"] = "incorrect-nonce".into(),
        Some("issuer") => claims["iss"] = "https://untrusted.example".into(),
        Some("audience") => claims["aud"] = "oaiapp_other".into(),
        Some("expired") => {
            claims["iat"] = (now() - 120).into();
            claims["exp"] = (now() - 60).into();
        }
        Some("subject") => claims["sub"] = "another-verified-subject".into(),
        Some("future-iat") => claims["iat"] = (now() + 90).into(),
        Some("missing-iat") => {
            claims.as_object_mut().unwrap().remove("iat");
        }
        Some("future-nbf") => claims["nbf"] = (now() + 90).into(),
        Some("azp") => {
            claims["aud"] = json!([grant.client, "oaiapp_other"]);
            claims["azp"] = "oaiapp_other".into();
        }
        _ => {}
    }
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(if config.fault.as_deref() == Some("unknown-kid") {
        "unpublished-key".into()
    } else {
        config.kid.clone()
    });
    let token = encode(
        &header,
        &claims,
        &EncodingKey::from_rsa_der(include_bytes!("fixtures/test-key.der")),
    )
    .unwrap();
    if config.fault.as_deref() == Some("signature") {
        let mut parts: Vec<_> = token.split('.').map(String::from).collect();
        let mut signature = URL_SAFE_NO_PAD.decode(&parts[2]).unwrap();
        signature[0] ^= 1;
        parts[2] = URL_SAFE_NO_PAD.encode(signature);
        parts.join(".")
    } else {
        token
    }
}
async fn write_response(stream: &mut TcpStream, status: u16, body: &str, location: Option<&str>) {
    let location = location
        .map(|value| format!("Location: {value}\r\n"))
        .unwrap_or_default();
    let response = format!(
        "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{location}Connection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
}

#[tokio::test]
async fn real_dynamic_sign_in_validates_pkce_and_caches_models_without_startup_http() {
    let fixture = Fixture::new().await;
    let directory = Directory::new();
    let manager = fixture.manager(&directory, ChatgptTestVault::unavailable());
    assert!(fixture.state.requests.lock().unwrap().is_empty());
    assert!(manager.view().await.accounts.is_empty());
    let authorization = manager
        .begin_sign_in(CredentialStorage::Session, None)
        .await
        .unwrap();
    assert_eq!(
        format!("{authorization:?}"),
        "ChatgptAuthorization([NATIVE ONLY])"
    );
    let url = reqwest::Url::parse(authorization.url()).unwrap();
    let parameters: BTreeMap<_, _> = url.query_pairs().collect();
    assert_eq!(parameters["client_id"], "dynamic_agent_client");
    assert_eq!(parameters["agent_name_hint"], "Desegnos");
    assert!(parameters["ext_agent_host_id"].starts_with("urn:uuid:"));
    let callback = reqwest::Url::parse(&parameters["redirect_uri"]).unwrap();
    assert_eq!(callback.host_str(), Some("127.0.0.1"));
    assert_eq!(callback.path(), "/auth/callback");
    assert_eq!(parameters["scope"], SCOPES);
    assert_eq!(parameters["resource"], http::RESOURCE);
    assert_eq!(parameters["code_challenge_method"], "S256");
    fixture.browser(&authorization).await;
    let view = terminal(&manager).await;
    assert_eq!(view.sign_in.status, ChatgptSignInStatus::Complete);
    assert_eq!(view.accounts.len(), 1);
    assert!(view.accounts[0].connected && view.accounts[0].plan_enabled);
    assert_eq!(view.accounts[0].storage, CredentialLocation::Session);
    assert_eq!(view.models.len(), 1);
    let id = view.active_account_id.unwrap();
    assert!(manager.bearer(id, &CancellationToken::new()).await.is_ok());
    assert!(fixture.state.failures.lock().unwrap().is_empty());
    let model_count = fixture.state.models.load(Ordering::SeqCst);
    manager.view().await;
    manager.view().await;
    assert_eq!(fixture.state.models.load(Ordering::SeqCst), model_count);
    assert!(
        fixture
            .state
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|request| request.method == "POST"
                && request.path == "/api/accounts/oauth/token"
                && request.form.get("grant_type").map(String::as_str)
                    == Some("authorization_code"))
    );
    let metadata = fs::read_to_string(directory.0.join("accounts.json")).unwrap();
    assert!(
        !metadata.contains("fixture_access_")
            && !metadata.contains("fixture_refresh_")
            && !metadata.contains("id_token")
            && !metadata.contains("nonce")
    );
    let serialized = serde_json::to_string(&view).unwrap();
    assert!(
        !serialized.contains("fixture_access_")
            && !serialized.contains("fixture_refresh_")
            && !serialized.contains("code_challenge")
    );
    let restarted = fixture.manager(&directory, ChatgptTestVault::unavailable());
    let reopened = restarted.view().await;
    assert_eq!(reopened.accounts[0].id, id);
    assert!(!reopened.accounts[0].connected);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(directory.0.join("accounts.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o077,
            0
        );
        assert_eq!(
            fs::metadata(&directory.0).unwrap().permissions().mode() & 0o077,
            0
        );
    }
}

#[tokio::test]
async fn missing_plan_scope_retains_identity_without_models_or_inference_authority() {
    let fixture = Fixture::new().await;
    fixture.update(|config| config.plan = false);
    let directory = Directory::new();
    let manager = fixture.manager(&directory, ChatgptTestVault::unavailable());
    let view = fixture
        .sign_in(&manager, CredentialStorage::Session, None)
        .await;
    assert_eq!(view.sign_in.status, ChatgptSignInStatus::Complete);
    assert!(view.accounts[0].connected);
    assert!(!view.accounts[0].plan_enabled);
    assert!(view.models.is_empty());
    assert_eq!(fixture.state.models.load(Ordering::SeqCst), 0);
    let error = manager
        .bearer(view.active_account_id.unwrap(), &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.kind, Kind::PlanPermission);
    let authorization = manager
        .begin_sign_in(CredentialStorage::Session, view.active_account_id)
        .await
        .unwrap();
    assert!(authorization.url().contains("prompt=consent"));
    manager.cancel_sign_in().await;
}

#[tokio::test]
async fn invalid_signed_identities_never_activate_and_never_reach_models() {
    for fault in [
        "signature",
        "issuer",
        "audience",
        "expired",
        "nonce",
        "future-iat",
        "missing-iat",
        "future-nbf",
        "azp",
        "unknown-kid",
        "jwk-use",
        "duplicate-kid",
        "discovery",
    ] {
        let fixture = Fixture::new().await;
        fixture.update(|config| config.fault = Some(fault.into()));
        let directory = Directory::new();
        let manager = fixture.manager(&directory, ChatgptTestVault::unavailable());
        let view = fixture
            .sign_in(&manager, CredentialStorage::Session, None)
            .await;
        assert_eq!(view.sign_in.status, ChatgptSignInStatus::Failed, "{fault}");
        assert!(view.active_account_id.is_none(), "{fault}");
        assert!(
            view.accounts.iter().all(|account| !account.connected),
            "{fault}"
        );
        assert_eq!(fixture.state.models.load(Ordering::SeqCst), 0, "{fault}");
    }
}

#[tokio::test]
async fn state_replay_and_callback_client_changes_are_rejected_before_exchange() {
    let fixture = Fixture::new().await;
    let directory = Directory::new();
    let manager = fixture.manager(&directory, ChatgptTestVault::unavailable());
    let authorization = manager
        .begin_sign_in(CredentialStorage::Session, None)
        .await
        .unwrap();
    let url = reqwest::Url::parse(authorization.url()).unwrap();
    let query: BTreeMap<_, _> = url.query_pairs().collect();
    let mut callback = reqwest::Url::parse(&query["redirect_uri"]).unwrap();
    callback
        .query_pairs_mut()
        .append_pair("state", "wrong-state")
        .append_pair("code", "never-exchange")
        .append_pair("client_id", "oaiapp_bad");
    assert_eq!(
        reqwest::Client::new()
            .get(callback.clone())
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::BAD_REQUEST
    );
    let failed = terminal(&manager).await;
    assert_eq!(failed.sign_in.status, ChatgptSignInStatus::Failed);
    assert!(fixture.state.requests.lock().unwrap().is_empty());
    assert!(reqwest::Client::new().get(callback).send().await.is_err());
    let connected = fixture
        .sign_in(&manager, CredentialStorage::Session, None)
        .await;
    let id = connected.active_account_id.unwrap();
    let authorization = manager
        .begin_sign_in(CredentialStorage::Session, Some(id))
        .await
        .unwrap();
    let url = reqwest::Url::parse(authorization.url()).unwrap();
    let query: BTreeMap<_, _> = url.query_pairs().collect();
    let mut callback = reqwest::Url::parse(&query["redirect_uri"]).unwrap();
    callback
        .query_pairs_mut()
        .append_pair("state", &query["state"])
        .append_pair("code", "never-exchange")
        .append_pair("client_id", "oaiapp_substitute");
    let count = fixture.state.requests.lock().unwrap().len();
    reqwest::Client::new().get(callback).send().await.unwrap();
    let failed = terminal(&manager).await;
    assert_eq!(failed.active_account_id, Some(id));
    assert!(failed.accounts[0].connected);
    assert_eq!(fixture.state.requests.lock().unwrap().len(), count);
}

#[tokio::test]
async fn same_email_registrations_stay_distinct_and_returning_subject_must_match() {
    let fixture = Fixture::new().await;
    let directory = Directory::new();
    let manager = fixture.manager(&directory, ChatgptTestVault::unavailable());
    let first = fixture
        .sign_in(&manager, CredentialStorage::Session, None)
        .await;
    let first_id = first.active_account_id.unwrap();
    fixture.update(|config| config.subject = "fixture-subject-2".into());
    let second = fixture
        .sign_in(&manager, CredentialStorage::Session, None)
        .await;
    let second_id = second.active_account_id.unwrap();
    assert_ne!(first_id, second_id);
    assert_eq!(second.accounts.len(), 2);
    assert_eq!(second.accounts[0].email, second.accounts[1].email);
    assert_ne!(second.accounts[0].label, second.accounts[1].label);
    manager.select_account(first_id).await.unwrap();
    let authorization = manager
        .begin_sign_in(CredentialStorage::Session, Some(first_id))
        .await
        .unwrap();
    let query: BTreeMap<_, _> = reqwest::Url::parse(authorization.url())
        .unwrap()
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    assert_eq!(query["client_id"], "oaiapp_fixture_1");
    assert!(!query.contains_key("agent_name_hint"));
    assert!(query.contains_key("id_token_hint"));
    assert_eq!(query["login_hint"], "same@example.test");
    fixture.update(|config| config.fault = Some("subject".into()));
    fixture.browser(&authorization).await;
    let failed = terminal(&manager).await;
    assert_eq!(failed.sign_in.status, ChatgptSignInStatus::Failed);
    assert_eq!(failed.active_account_id, Some(first_id));
    assert!(failed.accounts.iter().all(|account| account.connected));
}

#[tokio::test]
async fn explicit_session_is_required_after_vault_failure_and_old_account_is_preserved() {
    let fixture = Fixture::new().await;
    let directory = Directory::new();
    let manager = fixture.manager(&directory, ChatgptTestVault::unavailable());
    let first = fixture
        .sign_in(&manager, CredentialStorage::Session, None)
        .await;
    let id = first.active_account_id.unwrap();
    let failed = fixture
        .sign_in(&manager, CredentialStorage::System, None)
        .await;
    assert_eq!(failed.sign_in.status, ChatgptSignInStatus::Failed);
    assert_eq!(failed.active_account_id, Some(id));
    assert!(failed.accounts[0].connected);
    assert!(!failed.accounts[1].connected);
    assert!(failed.storage_error.is_some());
}

#[tokio::test]
async fn rotating_refresh_is_serialized_across_independent_managers_and_jwks_rotates() {
    let fixture = Fixture::new().await;
    fixture.update(|config| config.expires = 1);
    let directory = Directory::new();
    let vault = ChatgptTestVault::memory();
    let first = fixture.manager(&directory, vault.clone());
    let signed = fixture
        .sign_in(&first, CredentialStorage::System, None)
        .await;
    assert_eq!(signed.sign_in.status, ChatgptSignInStatus::Complete);
    let id = signed.active_account_id.unwrap();
    let second = fixture.manager(&directory, vault.clone());
    fixture.update(|config| {
        config.expires = 3600;
        config.kid = "fixture-key-2".into();
        config.token_delay_ms = 120;
    });
    let cancellation = CancellationToken::new();
    let (left, right) = tokio::join!(
        first.bearer(id, &cancellation),
        second.bearer(id, &cancellation)
    );
    assert!(left.is_ok() && right.is_ok());
    assert_eq!(fixture.state.refreshes.load(Ordering::SeqCst), 1);
    assert!(fixture.state.failures.lock().unwrap().is_empty());
    let record = vault.data.lock().unwrap().get(&id).cloned().unwrap();
    let tokens: TokenRecord = serde_json::from_str(&record).unwrap();
    assert!(tokens.expires_at > now() + 3000);
    assert!(tokens.id_token.split('.').count() == 3);
    assert!(tokens.plan_enabled());
    assert!(
        fixture
            .state
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request.path == "/.well-known/jwks.json")
            .count()
            >= 2
    );
    let metadata = fs::read_to_string(directory.0.join("accounts.json")).unwrap();
    assert!(!metadata.contains("fixture_refresh_") && !metadata.contains("fixture_access_"));
}

#[tokio::test]
async fn terminal_refresh_disconnects_but_temporary_failures_preserve_credentials() {
    for code in [
        "invalid_grant",
        "invalid_refresh_token",
        "token_expired",
        "refresh_token_expired",
        "refresh_token_invalidated",
        "refresh_token_reused",
        "invalid_client",
        "temporarily_unavailable",
    ] {
        let fixture = Fixture::new().await;
        fixture.update(|config| config.expires = 1);
        let directory = Directory::new();
        let vault = ChatgptTestVault::memory();
        let manager = fixture.manager(&directory, vault.clone());
        let view = fixture
            .sign_in(&manager, CredentialStorage::System, None)
            .await;
        let id = view.active_account_id.unwrap();
        let before = vault.data.lock().unwrap().get(&id).cloned().unwrap();
        fixture.update(|config| config.refresh_error = Some(code.into()));
        let error = manager
            .bearer(id, &CancellationToken::new())
            .await
            .unwrap_err();
        let terminal = !matches!(code, "invalid_client" | "temporarily_unavailable");
        assert_eq!(error.kind == Kind::Reconnect, terminal, "{code}");
        let after = manager.view().await;
        assert_eq!(!after.accounts[0].connected, terminal, "{code}");
        if !terminal {
            assert!(
                vault
                    .data
                    .lock()
                    .unwrap()
                    .get(&id)
                    .is_some_and(|value| value.as_str() == before.as_str())
            );
        }
    }
}

#[tokio::test]
async fn refresh_storage_failure_preserves_the_previous_secure_record() {
    let fixture = Fixture::new().await;
    fixture.update(|config| config.expires = 1);
    let directory = Directory::new();
    let vault = ChatgptTestVault::memory();
    let manager = fixture.manager(&directory, vault.clone());
    let view = fixture
        .sign_in(&manager, CredentialStorage::System, None)
        .await;
    let id = view.active_account_id.unwrap();
    let previous = vault.data.lock().unwrap().get(&id).cloned().unwrap();
    fixture.update(|config| config.expires = 3600);
    vault.unavailable.store(true, Ordering::SeqCst);
    assert_eq!(
        manager
            .bearer(id, &CancellationToken::new())
            .await
            .unwrap_err()
            .kind,
        Kind::VaultUnavailable
    );
    assert!(
        vault
            .data
            .lock()
            .unwrap()
            .get(&id)
            .is_some_and(|value| value.as_str() == previous.as_str())
    );
    assert!(manager.view().await.accounts[0].storage_repair_required);
    let refreshes = fixture.state.refreshes.load(Ordering::SeqCst);
    assert_eq!(
        manager
            .bearer(id, &CancellationToken::new())
            .await
            .unwrap_err()
            .kind,
        Kind::VaultUnavailable
    );
    assert_eq!(fixture.state.refreshes.load(Ordering::SeqCst), refreshes);
    let independent = fixture.manager(&directory, vault.clone());
    let blocked = independent.view().await;
    assert!(!blocked.accounts[0].connected);
    assert!(!blocked.accounts[0].storage_repair_required);
    assert!(
        blocked
            .storage_error
            .as_deref()
            .unwrap()
            .contains("interrupted")
    );
    assert_eq!(
        independent
            .bearer(id, &CancellationToken::new())
            .await
            .unwrap_err()
            .kind,
        Kind::Reconnect
    );
    assert_eq!(fixture.state.refreshes.load(Ordering::SeqCst), refreshes);
    vault.unavailable.store(false, Ordering::SeqCst);
    let repaired = manager.retry_secure_storage(id).await.unwrap();
    assert!(!repaired.accounts[0].storage_repair_required);
    assert!(repaired.storage_error.is_none());
    assert!(manager.bearer(id, &CancellationToken::new()).await.is_ok());
    assert!(
        independent
            .bearer(id, &CancellationToken::new())
            .await
            .is_ok()
    );
    assert_eq!(fixture.state.refreshes.load(Ordering::SeqCst), refreshes);
}

#[tokio::test]
async fn stale_rotation_cannot_replace_or_revoke_a_newer_registration_bundle() {
    for (newer_pending, repair_first) in
        [(false, false), (false, true), (true, false), (true, true)]
    {
        let fixture = Fixture::new().await;
        fixture.update(|config| config.expires = 1);
        let directory = Directory::new();
        let vault = ChatgptTestVault::memory();
        let owner = fixture.manager(&directory, vault.clone());
        let initial = fixture
            .sign_in(&owner, CredentialStorage::System, None)
            .await;
        let id = initial.active_account_id.unwrap();
        fixture.update(|config| config.expires = 3600);
        vault.unavailable.store(true, Ordering::SeqCst);
        assert_eq!(
            owner
                .bearer(id, &CancellationToken::new())
                .await
                .unwrap_err()
                .kind,
            Kind::VaultUnavailable
        );
        let (old_operation, old_refresh) = {
            let state = owner.inner.state.lock().await;
            let pending = state.pending_rotations.get(&id).unwrap();
            (pending.id, pending.tokens.refresh_token.clone())
        };
        vault.unavailable.store(false, Ordering::SeqCst);
        let peer = fixture.manager(&directory, vault.clone());
        fixture.update(|config| config.expires = if newer_pending { 1 } else { 3600 });
        let reconnected = fixture
            .sign_in(&peer, CredentialStorage::System, Some(id))
            .await;
        assert_eq!(reconnected.sign_in.status, ChatgptSignInStatus::Complete);
        assert_eq!(reconnected.active_account_id, Some(id));
        if newer_pending {
            fixture.update(|config| config.expires = 3600);
            vault.unavailable.store(true, Ordering::SeqCst);
            assert_eq!(
                peer.bearer(id, &CancellationToken::new())
                    .await
                    .unwrap_err()
                    .kind,
                Kind::VaultUnavailable
            );
            vault.unavailable.store(false, Ordering::SeqCst);
        }
        let current_operation = peer
            .inner
            .store
            .read()
            .unwrap()
            .registration(id)
            .unwrap()
            .refresh_pending;
        assert_eq!(current_operation.is_some(), newer_pending);
        assert_ne!(current_operation, Some(old_operation));
        let accepted = vault.data.lock().unwrap().get(&id).cloned().unwrap();
        if repair_first {
            let request_count = fixture.state.requests.lock().unwrap().len();
            assert_eq!(
                owner.retry_secure_storage(id).await.unwrap_err().kind,
                Kind::Reconnect
            );
            assert!(
                !owner
                    .inner
                    .state
                    .lock()
                    .await
                    .pending_rotations
                    .contains_key(&id)
            );
            assert_eq!(fixture.state.requests.lock().unwrap().len(), request_count);
        } else {
            assert!(
                owner
                    .inner
                    .state
                    .lock()
                    .await
                    .pending_rotations
                    .contains_key(&id)
            );
        }
        assert_eq!(
            vault.data.lock().unwrap().get(&id).unwrap().as_str(),
            accepted.as_str()
        );
        assert_eq!(
            owner
                .inner
                .store
                .read()
                .unwrap()
                .registration(id)
                .unwrap()
                .refresh_pending,
            current_operation
        );
        if newer_pending {
            peer.retry_secure_storage(id).await.unwrap();
        }
        let latest: TokenRecord =
            serde_json::from_str(vault.data.lock().unwrap().get(&id).unwrap()).unwrap();
        assert_ne!(latest.refresh_token, old_refresh);
        let signed_out = owner.sign_out(id).await.unwrap();
        assert!(!signed_out.accounts[0].connected);
        assert!(
            !signed_out
                .message
                .as_deref()
                .unwrap()
                .contains("not confirmed")
        );
        assert!(!vault.data.lock().unwrap().contains_key(&id));
        let captures = fixture.state.requests.lock().unwrap();
        let revocation = captures
            .iter()
            .rev()
            .find(|request| request.path == "/oauth/revoke")
            .unwrap();
        assert_eq!(revocation.form.get("token"), Some(&latest.refresh_token));
        assert_eq!(revocation.form.get("client_id"), Some(&latest.client_id));
        assert!(
            !fixture
                .state
                .refresh
                .lock()
                .unwrap()
                .contains_key(&latest.refresh_token)
        );
        assert!(fixture.state.failures.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn refresh_fence_precedes_submission_and_survives_an_interrupted_process() {
    let fixture = Fixture::new().await;
    fixture.update(|config| config.expires = 1);
    let directory = Directory::new();
    let vault = ChatgptTestVault::memory();
    let manager = fixture.manager(&directory, vault.clone());
    let first = fixture
        .sign_in(&manager, CredentialStorage::System, None)
        .await;
    let id = first.active_account_id.unwrap();
    fixture.update(|config| config.token_delay_ms = 5000);
    let request_manager = manager.clone();
    let task =
        tokio::spawn(async move { request_manager.bearer(id, &CancellationToken::new()).await });
    until(|| {
        fixture
            .state
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request.path == "/api/accounts/oauth/token")
            .count()
            == 2
    })
    .await;
    assert!(
        manager
            .inner
            .store
            .read()
            .unwrap()
            .registration(id)
            .unwrap()
            .refresh_pending
            .is_some()
    );
    task.abort();
    let _ = task.await;
    let restarted = fixture.manager(&directory, vault.clone());
    assert!(!restarted.view().await.accounts[0].connected);
    assert_eq!(
        restarted
            .bearer(id, &CancellationToken::new())
            .await
            .unwrap_err()
            .kind,
        Kind::Reconnect
    );
    assert_eq!(
        fixture
            .state
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request.path == "/api/accounts/oauth/token")
            .count(),
        2
    );
    let saved = fs::read_to_string(directory.0.join("accounts.json")).unwrap();
    assert!(!saved.contains("fixture_access_") && !saved.contains("fixture_refresh_"));
    fixture.update(|config| {
        config.token_delay_ms = 0;
        config.expires = 3600;
    });
    let reconnected = fixture
        .sign_in(&restarted, CredentialStorage::System, Some(id))
        .await;
    assert_eq!(reconnected.sign_in.status, ChatgptSignInStatus::Complete);
    assert_eq!(reconnected.active_account_id, Some(id));
    assert!(reconnected.accounts[0].connected);
    assert!(
        !restarted
            .inner
            .store
            .read()
            .unwrap()
            .registration(id)
            .unwrap()
            .refresh_pending
            .is_some()
    );
}

#[tokio::test]
async fn session_reconnect_retires_owned_system_tokens_and_cannot_resurrect_after_restart() {
    let fixture = Fixture::new().await;
    let directory = Directory::new();
    let vault = ChatgptTestVault::memory();
    let manager = fixture.manager(&directory, vault.clone());
    let first = fixture
        .sign_in(&manager, CredentialStorage::System, None)
        .await;
    let id = first.active_account_id.unwrap();
    assert!(vault.data.lock().unwrap().contains_key(&id));
    let session = fixture
        .sign_in(&manager, CredentialStorage::Session, Some(id))
        .await;
    assert_eq!(session.sign_in.status, ChatgptSignInStatus::Complete);
    assert_eq!(session.accounts[0].storage, CredentialLocation::Session);
    assert!(!vault.data.lock().unwrap().contains_key(&id));
    let restarted = fixture.manager(&directory, vault.clone());
    let view = restarted.view().await;
    assert!(!view.accounts[0].connected);
    assert_eq!(view.accounts[0].storage, CredentialLocation::None);
    assert_eq!(
        restarted
            .bearer(id, &CancellationToken::new())
            .await
            .unwrap_err()
            .kind,
        Kind::Reconnect
    );
}

#[tokio::test]
async fn failed_system_retirement_preserves_the_accepted_system_registration() {
    let fixture = Fixture::new().await;
    let directory = Directory::new();
    let vault = ChatgptTestVault::memory();
    let manager = fixture.manager(&directory, vault.clone());
    let first = fixture
        .sign_in(&manager, CredentialStorage::System, None)
        .await;
    let id = first.active_account_id.unwrap();
    let old = vault.data.lock().unwrap().get(&id).cloned().unwrap();
    vault.unavailable.store(true, Ordering::SeqCst);
    let rejected = fixture
        .sign_in(&manager, CredentialStorage::Session, Some(id))
        .await;
    assert_eq!(rejected.sign_in.status, ChatgptSignInStatus::Failed);
    assert_eq!(rejected.active_account_id, Some(id));
    assert!(rejected.accounts[0].connected);
    assert_eq!(rejected.accounts[0].storage, CredentialLocation::System);
    assert!(
        vault
            .data
            .lock()
            .unwrap()
            .get(&id)
            .is_some_and(|record| record.as_str() == old.as_str())
    );
}

#[tokio::test]
async fn sign_out_revokes_the_saved_client_and_reports_remote_failure_honestly() {
    for remote in [200, 503] {
        let fixture = Fixture::new().await;
        let directory = Directory::new();
        let manager = fixture.manager(&directory, ChatgptTestVault::unavailable());
        let connected = fixture
            .sign_in(&manager, CredentialStorage::Session, None)
            .await;
        let id = connected.active_account_id.unwrap();
        fixture.update(|config| config.revocation_status = remote);
        let signed_out = manager.sign_out(id).await.unwrap();
        assert!(!signed_out.accounts[0].connected);
        assert!(signed_out.models.is_empty());
        assert_eq!(signed_out.accounts[0].id, id);
        assert_eq!(
            signed_out
                .message
                .as_deref()
                .unwrap()
                .contains("not confirmed"),
            remote != 200
        );
        let revocations = fixture
            .state
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request.path == "/oauth/revoke")
            .count();
        assert_eq!(revocations, if remote == 200 { 1 } else { 3 });
        let authorization = manager
            .begin_sign_in(CredentialStorage::Session, Some(id))
            .await
            .unwrap();
        let url = reqwest::Url::parse(authorization.url()).unwrap();
        let query: BTreeMap<_, _> = url.query_pairs().collect();
        assert_eq!(query["client_id"], "oaiapp_fixture_1");
        assert!(!query.contains_key("id_token_hint"));
        assert!(!query.contains_key("prompt"));
        manager.cancel_sign_in().await;
        assert!(fixture.state.failures.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn sign_out_cancels_a_delayed_reconnect_before_it_can_reactivate() {
    let fixture = Fixture::new().await;
    let directory = Directory::new();
    let manager = fixture.manager(&directory, ChatgptTestVault::unavailable());
    let view = fixture
        .sign_in(&manager, CredentialStorage::Session, None)
        .await;
    let id = view.active_account_id.unwrap();
    fixture.update(|config| config.token_delay_ms = 500);
    let authorization = manager
        .begin_sign_in(CredentialStorage::Session, Some(id))
        .await
        .unwrap();
    fixture.browser(&authorization).await;
    let signed_out = manager.sign_out(id).await.unwrap();
    assert!(!signed_out.accounts[0].connected);
    let after = manager.view().await;
    assert!(!after.accounts[0].connected);
    assert_eq!(after.sign_in.status, ChatgptSignInStatus::Cancelled);
    assert!(after.models.is_empty());
}

#[tokio::test]
async fn model_discovery_cannot_repopulate_a_signed_out_account() {
    let fixture = Fixture::new().await;
    let directory = Directory::new();
    let manager = fixture.manager(&directory, ChatgptTestVault::unavailable());
    let view = fixture
        .sign_in(&manager, CredentialStorage::Session, None)
        .await;
    let id = view.active_account_id.unwrap();
    fixture.update(|config| config.models_delay_ms = 500);
    let before = fixture.state.models.load(Ordering::SeqCst);
    let clone = manager.clone();
    let client = OpenAiClient::for_loopback(&format!("{}/v1", fixture.base)).unwrap();
    let task = tokio::spawn(async move {
        clone
            .discover_models(id, &client, &CancellationToken::new())
            .await
    });
    until(|| fixture.state.models.load(Ordering::SeqCst) > before).await;
    manager.sign_out(id).await.unwrap();
    let result = task.await.unwrap().unwrap();
    assert!(result.models.is_empty());
    assert!(!result.accounts[0].connected);
}

#[tokio::test]
async fn cancellation_closes_literal_loopback_and_does_not_replace_existing_account() {
    let fixture = Fixture::new().await;
    let directory = Directory::new();
    let manager = fixture.manager(&directory, ChatgptTestVault::unavailable());
    let view = fixture
        .sign_in(&manager, CredentialStorage::Session, None)
        .await;
    let id = view.active_account_id;
    let authorization = manager
        .begin_sign_in(CredentialStorage::Session, None)
        .await
        .unwrap();
    let query: BTreeMap<_, _> = reqwest::Url::parse(authorization.url())
        .unwrap()
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    assert_eq!(
        manager
            .begin_sign_in(CredentialStorage::Session, None)
            .await
            .unwrap_err()
            .kind,
        Kind::Busy
    );
    let cancelled = manager.cancel_sign_in().await;
    assert_eq!(cancelled.sign_in.status, ChatgptSignInStatus::Cancelled);
    assert_eq!(cancelled.active_account_id, id);
    assert!(
        reqwest::Client::new()
            .get(&query["redirect_uri"])
            .send()
            .await
            .is_err()
    );
}

#[tokio::test(start_paused = true)]
async fn authorization_deadline_expires_without_network_or_plaintext_credentials() {
    let fixture = Fixture::new().await;
    let directory = Directory::new();
    let manager = fixture.manager(&directory, ChatgptTestVault::unavailable());
    let _authorization = manager
        .begin_sign_in(CredentialStorage::Session, None)
        .await
        .unwrap();
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(301)).await;
    tokio::task::yield_now().await;
    let view = manager.view().await;
    assert_eq!(view.sign_in.status, ChatgptSignInStatus::Failed);
    assert!(
        view.sign_in
            .message
            .as_deref()
            .unwrap()
            .contains("timed out")
    );
    assert!(fixture.state.requests.lock().unwrap().is_empty());
}

#[test]
fn endpoints_and_metadata_reject_untrusted_locations_and_damage() {
    for endpoint in [
        "http://localhost:9000",
        "http://127.0.0.1",
        "http://[::1]:9000",
        "http://127.0.0.1:9000/path",
        "http://user@127.0.0.1:9000",
        "https://127.0.0.1:9000",
        "http://127.0.0.1:9000/?x=1",
    ] {
        assert!(Endpoints::loopback(endpoint).is_err());
    }
    let directory = Directory::new();
    fs::write(directory.0.join("accounts.json"), b"damaged").unwrap();
    assert!(Store::initialize(&directory.0).is_err());
    assert_eq!(
        fs::read(directory.0.join("accounts.json")).unwrap(),
        b"damaged"
    );
}

#[tokio::test]
async fn file_lock_wait_is_cancellable_and_keeps_explicit_account_binding() {
    let fixture = Fixture::new().await;
    let directory = Directory::new();
    let manager = fixture.manager(&directory, ChatgptTestVault::unavailable());
    let view = fixture
        .sign_in(&manager, CredentialStorage::Session, None)
        .await;
    let id = view.active_account_id.unwrap();
    let file = manager.inner.store.open_lock().unwrap();
    file.lock().unwrap();
    let cancellation = CancellationToken::new();
    let copy = cancellation.clone();
    let other = manager.clone();
    let mut task = Box::pin(other.bearer(id, &copy));
    std::future::poll_fn(|cx| {
        assert!(std::future::Future::poll(task.as_mut(), cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    cancellation.cancel();
    assert_eq!(task.await.unwrap_err().kind, Kind::Cancelled);
    drop(file);
    assert_eq!(
        manager
            .bearer(Uuid::new_v4(), &CancellationToken::new())
            .await
            .unwrap_err()
            .kind,
        Kind::Reconnect
    );
    assert_eq!(manager.view().await.active_account_id, Some(id));
}
