use super::{ChatgptErrorKind as Kind, ChatgptResult};
use crate::CancellationToken;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use reqwest::{Client, Response, Url};
use serde::{Deserialize, de::DeserializeOwned};
use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use zeroize::Zeroize;

pub(super) const RESOURCE: &str = "https://api.openai.com/v1";
pub(super) const SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";

pub(super) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |value| value.as_secs())
}

#[derive(Clone)]
pub(super) struct Endpoints {
    pub issuer: String,
    pub authorize: String,
    pub token: String,
    pub jwks: String,
    pub discovery: String,
    test: bool,
}
impl Endpoints {
    pub fn production() -> Self {
        Self::base("https://auth.openai.com", false)
    }
    fn base(base: &str, test: bool) -> Self {
        Self {
            issuer: base.into(),
            authorize: format!("{base}/api/accounts/authorize"),
            token: format!("{base}/api/accounts/oauth/token"),
            jwks: format!("{base}/.well-known/jwks.json"),
            discovery: format!("{base}/.well-known/openid-configuration"),
            test,
        }
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn loopback(base: &str) -> ChatgptResult<Self> {
        let value = Url::parse(base).map_err(|_| Kind::Discovery)?;
        if value.scheme() != "http"
            || value.host_str() != Some("127.0.0.1")
            || value.port().is_none()
            || value.path() != "/"
            || !value.username().is_empty()
            || value.password().is_some()
            || value.query().is_some()
            || value.fragment().is_some()
        {
            return Err(Kind::Discovery.into());
        }
        Ok(Self::base(base.trim_end_matches('/'), true))
    }
    fn trusted_revocation(&self, value: &str) -> bool {
        let Ok(url) = Url::parse(value) else {
            return false;
        };
        let Ok(issuer) = Url::parse(&self.issuer) else {
            return false;
        };
        url.origin() == issuer.origin()
            && url.scheme() == if self.test { "http" } else { "https" }
            && !url.path().is_empty()
            && url.path() != "/"
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
    }
}

#[derive(Deserialize, Clone)]
struct Discovery {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
    revocation_endpoint: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub id_token: String,
    pub token_type: String,
    pub expires_in: u64,
    pub scope: String,
    #[serde(default)]
    pub earliest_refresh_at: u64,
}
impl Drop for TokenResponse {
    fn drop(&mut self) {
        self.access_token.zeroize();
        self.refresh_token.zeroize();
        self.id_token.zeroize();
    }
}
impl TokenResponse {
    pub fn validate(&self) -> ChatgptResult<()> {
        if !self.token_type.eq_ignore_ascii_case("Bearer")
            || self.expires_in == 0
            || self.expires_in > 86400
            || self.scope.len() > 8192
            || !valid_secret(&self.access_token, 131072)
            || !valid_secret(&self.refresh_token, 65536)
            || !valid_secret(&self.id_token, 131072)
            || self.earliest_refresh_at > now().saturating_add(self.expires_in)
        {
            return Err(Kind::InvalidToken.into());
        }
        Ok(())
    }
    pub fn scopes(&self) -> Vec<String> {
        self.scope
            .split_ascii_whitespace()
            .map(String::from)
            .collect()
    }
}
fn valid_secret(value: &str, maximum: usize) -> bool {
    !value.is_empty() && value.len() <= maximum && value.bytes().all(|byte| byte.is_ascii_graphic())
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    Single(String),
    Multiple(Vec<String>),
}
impl Audience {
    fn len(&self) -> usize {
        match self {
            Self::Single(_) => 1,
            Self::Multiple(values) => values.len(),
        }
    }
}
#[derive(Deserialize)]
pub(super) struct Identity {
    pub sub: String,
    pub email: Option<String>,
    pub nonce: Option<String>,
    iss: String,
    aud: Audience,
    exp: u64,
    iat: u64,
    azp: Option<String>,
}

#[derive(Clone)]
pub(super) struct AuthHttp {
    pub endpoints: Endpoints,
    client: Client,
    discovery: Arc<Mutex<Option<(Instant, Discovery)>>>,
    jwks: Arc<Mutex<Option<(Instant, JwkSet)>>>,
}
impl AuthHttp {
    pub fn new(endpoints: Endpoints) -> ChatgptResult<Self> {
        let client = crate::net::hardened_client(!endpoints.test)
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|_| Kind::Transport)?;
        Ok(Self {
            endpoints,
            client,
            discovery: Arc::new(Mutex::new(None)),
            jwks: Arc::new(Mutex::new(None)),
        })
    }
    async fn configuration(&self, cancellation: &CancellationToken) -> ChatgptResult<Discovery> {
        let mut cache = self.discovery.lock().await;
        if let Some((at, value)) = cache.as_ref()
            && at.elapsed() < Duration::from_secs(3600)
        {
            return Ok(value.clone());
        }
        let response = self
            .send(self.client.get(&self.endpoints.discovery), cancellation)
            .await?;
        if !response.status().is_success() {
            return Err(Kind::Discovery.into());
        }
        let value: Discovery = bounded_json(response, cancellation).await?;
        if value.issuer != self.endpoints.issuer
            || value.authorization_endpoint != self.endpoints.authorize
            || value.token_endpoint != self.endpoints.token
            || value.jwks_uri != self.endpoints.jwks
            || value
                .revocation_endpoint
                .as_ref()
                .is_some_and(|endpoint| !self.endpoints.trusted_revocation(endpoint))
        {
            return Err(Kind::Discovery.into());
        }
        *cache = Some((Instant::now(), value.clone()));
        Ok(value)
    }
    pub async fn exchange(
        &self,
        client: &str,
        code: &str,
        verifier: &str,
        redirect: &str,
        cancellation: &CancellationToken,
    ) -> ChatgptResult<TokenResponse> {
        self.configuration(cancellation).await?;
        let form = [
            ("grant_type", "authorization_code"),
            ("client_id", client),
            ("code", code),
            ("code_verifier", verifier),
            ("redirect_uri", redirect),
            ("resource", RESOURCE),
        ];
        self.tokens(
            self.client.post(&self.endpoints.token).form(&form),
            cancellation,
            false,
        )
        .await
    }
    pub async fn refresh(
        &self,
        client: &str,
        refresh: &str,
        cancellation: &CancellationToken,
    ) -> ChatgptResult<TokenResponse> {
        self.configuration(cancellation).await?;
        let form = [
            ("grant_type", "refresh_token"),
            ("client_id", client),
            ("refresh_token", refresh),
            ("resource", RESOURCE),
        ];
        self.tokens(
            self.client.post(&self.endpoints.token).form(&form),
            cancellation,
            true,
        )
        .await
    }
    async fn tokens(
        &self,
        request: reqwest::RequestBuilder,
        cancellation: &CancellationToken,
        refresh: bool,
    ) -> ChatgptResult<TokenResponse> {
        let response = self.send(request, cancellation).await?;
        if !response.status().is_success() {
            let data: serde_json::Value = bounded_json(response, cancellation)
                .await
                .map_err(|_| Kind::TokenRejected)?;
            let code = data.get("error").and_then(|value| {
                value
                    .as_str()
                    .or_else(|| value.get("code").and_then(serde_json::Value::as_str))
            });
            return Err(match code {
                Some("invalid_client") => Kind::InvalidClient,
                Some(
                    "invalid_grant"
                    | "invalid_refresh_token"
                    | "token_expired"
                    | "refresh_token_expired"
                    | "refresh_token_invalidated"
                    | "refresh_token_reused",
                ) if refresh => Kind::Reconnect,
                _ => Kind::TokenRejected,
            }
            .into());
        }
        let value: TokenResponse = bounded_json(response, cancellation).await?;
        value.validate()?;
        Ok(value)
    }
    pub async fn identity(
        &self,
        token: &str,
        client: &str,
        nonce: Option<&str>,
        cancellation: &CancellationToken,
    ) -> ChatgptResult<Identity> {
        self.configuration(cancellation).await?;
        let header = decode_header(token).map_err(|_| Kind::InvalidToken)?;
        let permitted = matches!(
            header.alg,
            Algorithm::RS256
                | Algorithm::RS384
                | Algorithm::RS512
                | Algorithm::PS256
                | Algorithm::PS384
                | Algorithm::PS512
                | Algorithm::ES256
                | Algorithm::ES384
                | Algorithm::EdDSA
        );
        if !permitted
            || header
                .crit
                .as_ref()
                .is_some_and(|values| !values.is_empty())
        {
            return Err(Kind::InvalidToken.into());
        }
        let kid = header
            .kid
            .as_deref()
            .filter(|value| !value.is_empty() && value.len() <= 256)
            .ok_or(Kind::InvalidToken)?;
        let mut cache = self.jwks.lock().await;
        let needs_keys = cache.as_ref().is_none_or(|(at, value)| {
            at.elapsed() >= Duration::from_secs(600)
                || !value
                    .keys
                    .iter()
                    .any(|key| key.common.key_id.as_deref() == Some(kid))
        });
        if needs_keys {
            let response = self
                .send(self.client.get(&self.endpoints.jwks), cancellation)
                .await?;
            if !response.status().is_success() {
                return Err(Kind::InvalidToken.into());
            }
            let keys: JwkSet = bounded_json(response, cancellation).await?;
            if keys.keys.is_empty() || keys.keys.len() > 64 {
                return Err(Kind::InvalidToken.into());
            }
            *cache = Some((Instant::now(), keys));
        }
        let keys = &cache.as_ref().ok_or(Kind::InvalidToken)?.1;
        let matching: Vec<_> = keys
            .keys
            .iter()
            .filter(|key| key.common.key_id.as_deref() == Some(kid))
            .collect();
        if matching.len() != 1 {
            return Err(Kind::InvalidToken.into());
        }
        let key = matching[0];
        if key
            .common
            .key_algorithm
            .is_some_and(|algorithm| Algorithm::try_from(algorithm).ok() != Some(header.alg))
            || key
                .common
                .public_key_use
                .as_ref()
                .is_some_and(|usage| !matches!(usage, jsonwebtoken::jwk::PublicKeyUse::Signature))
            || key
                .common
                .key_operations
                .as_ref()
                .is_some_and(|operations| {
                    !operations.contains(&jsonwebtoken::jwk::KeyOperations::Verify)
                })
        {
            return Err(Kind::InvalidToken.into());
        }
        let key = DecodingKey::from_jwk(key).map_err(|_| Kind::InvalidToken)?;
        let mut validation = Validation::new(header.alg);
        validation.leeway = 5;
        validation.validate_nbf = true;
        validation.set_required_spec_claims(&["sub", "exp", "iat", "iss", "aud"]);
        validation.set_issuer(&[&self.endpoints.issuer]);
        validation.set_audience(&[client]);
        let identity = decode::<Identity>(token, &key, &validation)
            .map_err(|_| Kind::InvalidToken)?
            .claims;
        if identity.sub.is_empty()
            || identity.sub.len() > 512
            || identity.iss != self.endpoints.issuer
            || identity.iat > now().saturating_add(5)
            || identity.exp <= identity.iat
            || identity
                .email
                .as_ref()
                .is_some_and(|value| value.len() > 320 || value.chars().any(char::is_control))
            || nonce.is_some_and(|expected| identity.nonce.as_deref() != Some(expected))
            || identity.azp.as_ref().is_some_and(|value| value != client)
            || (identity.aud.len() > 1 && identity.azp.as_deref() != Some(client))
        {
            return Err(Kind::InvalidToken.into());
        }
        if let Audience::Single(value) = &identity.aud
            && value != client
        {
            return Err(Kind::InvalidToken.into());
        }
        Ok(identity)
    }
    pub async fn revoke(
        &self,
        client: &str,
        token: &str,
        cancellation: &CancellationToken,
    ) -> ChatgptResult<()> {
        let config = self.configuration(cancellation).await?;
        let endpoint = config.revocation_endpoint.ok_or(Kind::Discovery)?;
        for attempt in 0..3 {
            let request = self.client.post(&endpoint).form(&[
                ("token", token),
                ("token_type_hint", "refresh_token"),
                ("client_id", client),
            ]);
            match self.send(request, cancellation).await {
                Ok(response) if response.status() == reqwest::StatusCode::OK => return Ok(()),
                Ok(response) if !response.status().is_server_error() => {
                    return Err(Kind::TokenRejected.into());
                }
                Err(error) if error.kind == Kind::Cancelled => return Err(error),
                _ => {}
            }
            if attempt < 2 {
                tokio::select! { _ = cancellation.cancelled() => return Err(Kind::Cancelled.into()), _ = tokio::time::sleep(Duration::from_millis(150 * (1 << attempt))) => {} }
            }
        }
        Err(Kind::Transport.into())
    }
    async fn send(
        &self,
        request: reqwest::RequestBuilder,
        cancellation: &CancellationToken,
    ) -> ChatgptResult<Response> {
        tokio::select! { biased; _ = cancellation.cancelled() => Err(Kind::Cancelled.into()), result = request.send() => {
            let response = result.map_err(|_| Kind::Transport)?;
            if response.status().is_redirection() { return Err(Kind::Discovery.into()); } Ok(response)
        } }
    }
}

async fn bounded_json<T: DeserializeOwned>(
    mut response: Response,
    cancellation: &CancellationToken,
) -> ChatgptResult<T> {
    let bytes = crate::net::bounded_body(
        &mut response,
        512 * 1024,
        cancellation,
        tokio::time::Instant::now() + Duration::from_secs(30),
    )
    .await
    .map_err(|error| match error.kind {
        crate::ErrorKind::Cancelled => Kind::Cancelled,
        crate::ErrorKind::Deadline => Kind::Deadline,
        crate::ErrorKind::OutputLimit => Kind::InvalidToken,
        _ => Kind::Transport,
    })?;
    serde_json::from_slice(&bytes).map_err(|_| Kind::InvalidToken.into())
}
