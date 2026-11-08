use crate::{CancellationToken, ErrorKind, ProviderError, Result, SubmissionOutcome};
use std::time::Duration;
use tokio::time::Instant;

pub(crate) const DEADLINE: Duration = Duration::from_secs(20 * 60);
pub(crate) const IDLE: Duration = Duration::from_secs(90);
pub(crate) const PROBE_IDLE: Duration = Duration::from_secs(30);
pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
pub(crate) const PUBLIC_FETCH_TIMEOUT: Duration = Duration::from_secs(8);
pub(crate) const PUBLIC_CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
pub(crate) const IMAGE_DEADLINE: Duration = Duration::from_secs(180);
pub(crate) const ERROR_BODY_LIMIT: usize = 65_536;
pub(crate) const MODELS_BODY_LIMIT: usize = 1024 * 1024;

pub(crate) fn hardened_client(https_only: bool) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .tls_backend_rustls()
        .https_only(https_only)
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .no_proxy()
        .referer(false)
        .connection_verbose(false)
        .tls_sslkeylogfile(false)
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .user_agent(concat!("Desegnos/", env!("CARGO_PKG_VERSION")))
}

pub(crate) trait BodyResponse {
    fn body_length(&self) -> Option<u64>;
    fn body_chunk(
        &mut self,
    ) -> impl std::future::Future<Output = reqwest::Result<Option<bytes::Bytes>>> + Send;
}
impl BodyResponse for reqwest::Response {
    fn body_length(&self) -> Option<u64> {
        self.content_length()
    }
    async fn body_chunk(&mut self) -> reqwest::Result<Option<bytes::Bytes>> {
        self.chunk().await
    }
}
impl BodyResponse for crate::retry::RetryResponse {
    fn body_length(&self) -> Option<u64> {
        self.content_length()
    }
    async fn body_chunk(&mut self) -> reqwest::Result<Option<bytes::Bytes>> {
        self.chunk().await
    }
}

pub(crate) enum ChunkFailure {
    Cancelled,
    Deadline,
    Transport,
}
impl ChunkFailure {
    pub fn kind(self) -> ErrorKind {
        match self {
            Self::Cancelled => ErrorKind::Cancelled,
            Self::Deadline => ErrorKind::Deadline,
            Self::Transport => ErrorKind::Transport,
        }
    }
}

/// The next body chunk, ending early on cancellation, the overall deadline or `idle` of silence.
pub(crate) async fn next_chunk(
    response: &mut (impl BodyResponse + Send),
    cancellation: &CancellationToken,
    deadline: Instant,
    idle: Duration,
) -> std::result::Result<Option<bytes::Bytes>, ChunkFailure> {
    let silence = Instant::now()
        .checked_add(idle)
        .map_or(deadline, |limit| deadline.min(limit));
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(ChunkFailure::Cancelled),
        _ = tokio::time::sleep_until(silence) => Err(ChunkFailure::Deadline),
        result = response.body_chunk() => result.map_err(|error| {
            if error.is_timeout() { ChunkFailure::Deadline } else { ChunkFailure::Transport }
        }),
    }
}

pub(crate) async fn bounded_body(
    response: &mut (impl BodyResponse + Send),
    limit: usize,
    cancellation: &CancellationToken,
    deadline: Instant,
) -> Result<zeroize::Zeroizing<Vec<u8>>> {
    let error = |kind| ProviderError::new(kind, SubmissionOutcome::Unknown);
    if response
        .body_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(error(ErrorKind::OutputLimit));
    }
    let mut body = zeroize::Zeroizing::new(Vec::new());
    loop {
        let chunk = next_chunk(response, cancellation, deadline, Duration::MAX)
            .await
            .map_err(|failure| error(failure.kind()))?;
        let Some(chunk) = chunk else { return Ok(body) };
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(error(ErrorKind::OutputLimit));
        }
        body.extend_from_slice(&chunk);
    }
}

/// What both provider stacks keep from a failed response before reading its body.
pub(crate) struct ErrorHead {
    pub status: u16,
    pub request_id: Option<String>,
    pub content_type: Option<String>,
}
impl ErrorHead {
    pub fn of(response: &crate::retry::RetryResponse) -> Self {
        let headers = response.headers();
        Self {
            status: response.status().as_u16(),
            request_id: request_id(headers).map(str::to_owned),
            content_type: headers
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
        }
    }
}

pub(crate) fn status_kind(status: u16) -> ErrorKind {
    match status {
        401 | 403 => ErrorKind::Authentication,
        402 => ErrorKind::Quota,
        404 => ErrorKind::ModelUnavailable,
        408 => ErrorKind::Deadline,
        429 => ErrorKind::RateLimited,
        300..=399 => ErrorKind::Redirect,
        500..=599 => ErrorKind::Server,
        _ => ErrorKind::RequestRejected,
    }
}
pub(crate) fn request_id(headers: &reqwest::header::HeaderMap) -> Option<&str> {
    headers
        .get("x-request-id")
        .or_else(|| headers.get("request-id"))
        .and_then(|value| value.to_str().ok())
}
