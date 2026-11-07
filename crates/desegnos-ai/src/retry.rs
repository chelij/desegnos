//! Retry only before a successful response has delivered its first body byte.
use crate::{
    CancellationToken, ErrorKind, ProviderError, ProviderProgress, Result, SubmissionOutcome,
};
use reqwest::{Method, RequestBuilder, Response, StatusCode};
use std::{
    ops::Deref,
    time::{Duration, SystemTime},
};
use tokio::time::Instant;

pub(crate) struct RetryResponse {
    response: Response,
    first: Option<bytes::Bytes>,
    content_length: Option<u64>,
}
impl Deref for RetryResponse {
    type Target = Response;
    fn deref(&self) -> &Response {
        &self.response
    }
}
impl RetryResponse {
    pub(crate) fn content_length(&self) -> Option<u64> {
        self.content_length
    }
    pub(crate) async fn chunk(&mut self) -> reqwest::Result<Option<bytes::Bytes>> {
        if let Some(first) = self.first.take() {
            return Ok(Some(first));
        }
        self.response.chunk().await
    }
}

fn retry_after(value: Option<&str>, now: SystemTime) -> Option<Duration> {
    let value = value?;
    let delay = value
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
        .or_else(|| {
            httpdate::parse_http_date(value)
                .ok()
                .map(|date| date.duration_since(now).unwrap_or_default())
        })?;
    // Do not resend earlier than a provider asked. Long waits require an explicit later attempt.
    (delay <= Duration::from_secs(30)).then_some(delay)
}

/// Wait before attempt `attempt + 1`: two seconds, then eight. Unit tests scale both down so
/// they exercise the same policy without waiting out real backoff.
fn backoff(attempt: u32) -> Duration {
    let delay = if attempt == 1 {
        Duration::from_secs(2)
    } else {
        Duration::from_secs(8)
    };
    if cfg!(test) {
        delay / 100
    } else {
        delay
    }
}

/// Keep one cloneable, frozen wire request for at most three submissions. Error response bodies
/// are diagnostics, not model output. Once the first successful body byte arrives, ownership
/// passes to the existing parser and this policy can never resend it.
pub(crate) async fn send(
    request: RequestBuilder,
    cancel: &CancellationToken,
    deadline: Instant,
    idle: Duration,
    max_body: usize,
    progress: &(dyn Fn(ProviderProgress) + Send + Sync),
) -> Result<RetryResponse> {
    let (client, request) = request.build_split();
    let request = request.map_err(|_| ProviderError::before(ErrorKind::InvalidRequirements))?;
    // A failed catalogue/access GET cannot have submitted a generation.
    let outcome = if request.method() == Method::GET {
        SubmissionOutcome::NotSubmitted
    } else {
        SubmissionOutcome::Unknown
    };
    for attempt in 1u32..=3 {
        // A refused connection or failed DNS lookup won't heal in seconds: report it at once
        // rather than making every probe (models, access checks) wait through the backoff.
        let unreachable = std::sync::atomic::AtomicBool::new(false);
        let copy = request
            .try_clone()
            .ok_or_else(|| ProviderError::before(ErrorKind::InvalidRequirements))?;
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(ProviderError::before(ErrorKind::Cancelled)),
            _ = tokio::time::sleep_until(deadline.min(Instant::now() + idle)) => Err(ProviderError::new(ErrorKind::Deadline, outcome)),
            result = async {
                let mut response = client.execute(copy).await.map_err(|error| {
                    unreachable.store(error.is_connect() && !error.is_timeout(), std::sync::atomic::Ordering::Relaxed);
                    network_error(error, outcome)
                })?;
                let content_length = response.content_length();
                if !response.status().is_success() || content_length.is_some_and(|length| length > max_body as u64) {
                    return Ok(RetryResponse { response, first: None, content_length });
                }
                let first = loop {
                    match response.chunk().await.map_err(|error| network_error(error, outcome))? {
                        Some(bytes) if bytes.is_empty() => continue,
                        bytes => break bytes,
                    }
                };
                Ok(RetryResponse { response, first, content_length })
            } => result,
        };
        let (kind, delay) = match &result {
            Ok(response)
                if response.status().is_server_error()
                    || response.status() == StatusCode::TOO_MANY_REQUESTS =>
            {
                let kind = if response.status().is_server_error() {
                    ErrorKind::Server
                } else {
                    ErrorKind::RateLimited
                };
                let delay = match response.headers().get("retry-after") {
                    Some(value) => match retry_after(value.to_str().ok(), SystemTime::now()) {
                        Some(delay) => delay,
                        None => return result,
                    },
                    None => backoff(attempt),
                };
                (kind, delay)
            }
            Err(error)
                if matches!(error.kind, ErrorKind::Transport | ErrorKind::Deadline)
                    && !unreachable.load(std::sync::atomic::Ordering::Relaxed) =>
            {
                (error.kind, backoff(attempt))
            }
            _ => return result,
        };
        if attempt == 3 || Instant::now() + delay >= deadline {
            return result;
        }
        progress(ProviderProgress {
            retry: Some(crate::ProviderRetry {
                attempt: attempt as u8 + 1,
                kind,
            }),
            received_bytes: 0,
            response_id: None,
            web_search_calls: 0,
            citations: 0,
            text_delta: String::new(),
        });
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(ProviderError::before(ErrorKind::Cancelled)),
            _ = tokio::time::sleep(delay) => {}
        }
    }
    unreachable!("three attempts return their final response")
}
fn network_error(error: reqwest::Error, outcome: SubmissionOutcome) -> ProviderError {
    ProviderError::new(
        if error.is_timeout() {
            ErrorKind::Deadline
        } else {
            ErrorKind::Transport
        },
        outcome,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retry_after_accepts_seconds_and_dates_but_never_shortens_a_long_wait() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        assert_eq!(retry_after(Some("12"), now), Some(Duration::from_secs(12)));
        assert_eq!(
            retry_after(
                Some(&httpdate::fmt_http_date(now + Duration::from_secs(20))),
                now
            ),
            Some(Duration::from_secs(20))
        );
        assert_eq!(retry_after(Some("31"), now), None);
        assert_eq!(retry_after(Some("invalid"), now), None);
    }
}
