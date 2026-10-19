use super::{ChatgptErrorKind as Kind, ChatgptResult};
use crate::CancellationToken;
use reqwest::Url;
use std::{collections::BTreeMap, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use zeroize::{Zeroize, Zeroizing};

pub(super) struct Callback {
    pub code: String,
    pub client_id: Option<String>,
}
impl Drop for Callback {
    fn drop(&mut self) {
        self.code.zeroize();
    }
}

pub(super) async fn receive(
    listener: TcpListener,
    redirect: &str,
    expected_state: &str,
    cancellation: &CancellationToken,
) -> ChatgptResult<Callback> {
    let expected = Url::parse(redirect).map_err(|_| Kind::Callback)?;
    for _ in 0..32 {
        let (mut stream, peer) = tokio::select! { biased; _ = cancellation.cancelled() => return Err(Kind::Cancelled.into()), accepted = listener.accept() => accepted.map_err(|_| Kind::Callback)? };
        if !peer.ip().is_loopback() {
            continue;
        }
        let request = tokio::select! { biased; _ = cancellation.cancelled() => return Err(Kind::Cancelled.into()), value = tokio::time::timeout(Duration::from_secs(5), request(&mut stream)) => value.map_err(|_| Kind::Callback)?? };
        let parsed = parse(&request, &expected, expected_state);
        match parsed {
            Ok(None) => {
                respond(
                    &mut stream,
                    404,
                    "This sign-in listener accepts only the authorization callback.",
                )
                .await;
            }
            Ok(Some(value)) => {
                respond(&mut stream, 200, "Sign-in result received. Return to Desegnos to see the verified account status.").await;
                return Ok(value);
            }
            Err(error) => {
                respond(&mut stream, 400, "This sign-in result could not be verified. Return to Desegnos and start a fresh sign-in.").await;
                return Err(error);
            }
        }
    }
    Err(Kind::Callback.into())
}

async fn request(stream: &mut TcpStream) -> ChatgptResult<Zeroizing<Vec<u8>>> {
    let mut bytes = Zeroizing::new(Vec::new());
    loop {
        let mut chunk = [0_u8; 1024];
        let length = stream.read(&mut chunk).await.map_err(|_| Kind::Callback)?;
        if length == 0 {
            return Err(Kind::Callback.into());
        }
        bytes.extend_from_slice(&chunk[..length]);
        chunk.zeroize();
        if bytes.len() > 16384 {
            return Err(Kind::Callback.into());
        }
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            return Ok(bytes);
        }
    }
}

fn parse(request: &[u8], expected: &Url, state: &str) -> ChatgptResult<Option<Callback>> {
    let text = std::str::from_utf8(request).map_err(|_| Kind::Callback)?;
    let mut lines = text.split("\r\n");
    let parts: Vec<_> = lines.next().ok_or(Kind::Callback)?.split(' ').collect();
    if parts.len() != 3
        || parts[0] != "GET"
        || !matches!(parts[2], "HTTP/1.1" | "HTTP/1.0")
        || !parts[1].starts_with('/')
        || parts[1].starts_with("//")
    {
        return Err(Kind::Callback.into());
    }
    let mut host = None;
    for line in lines.take_while(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').ok_or(Kind::Callback)?;
        if name.eq_ignore_ascii_case("host") && host.replace(value.trim()).is_some() {
            return Err(Kind::Callback.into());
        }
        if name.eq_ignore_ascii_case("content-length") && value.trim() != "0" {
            return Err(Kind::Callback.into());
        }
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(Kind::Callback.into());
        }
    }
    let authority = format!("127.0.0.1:{}", expected.port().ok_or(Kind::Callback)?);
    if host != Some(authority.as_str()) {
        return Err(Kind::Callback.into());
    }
    let url = expected.join(parts[1]).map_err(|_| Kind::Callback)?;
    if url.path() != "/auth/callback" {
        return Ok(None);
    }
    let mut parameters = BTreeMap::new();
    for (name, value) in url.query_pairs() {
        if parameters
            .insert(name.into_owned(), value.into_owned())
            .is_some()
        {
            return Err(Kind::Callback.into());
        }
    }
    if parameters.get("state").map(String::as_str) != Some(state) {
        return Err(Kind::StateMismatch.into());
    }
    if let Some(error) = parameters.get("error") {
        return Err(if error == "access_denied" {
            Kind::Cancelled
        } else {
            Kind::Callback
        }
        .into());
    }
    let code = parameters
        .remove("code")
        .filter(|value| {
            !value.is_empty() && value.len() <= 8192 && !value.chars().any(char::is_control)
        })
        .ok_or(Kind::Callback)?;
    Ok(Some(Callback {
        code,
        client_id: parameters.remove("client_id"),
    }))
}

async fn respond(stream: &mut TcpStream, status: u16, message: &str) {
    let response = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nContent-Security-Policy: default-src 'none'; frame-ancestors 'none'\r\nX-Content-Type-Options: nosniff\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{message}",
        if status == 200 { "OK" } else { "Bad Request" },
        message.len()
    );
    let _ = tokio::time::timeout(
        Duration::from_secs(2),
        stream.write_all(response.as_bytes()),
    )
    .await;
}
