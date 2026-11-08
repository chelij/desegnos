//! The only place production commands meet native-test fixtures. Every seam has one function
//! here: with the `native-tests` feature it points at an isolated loopback fixture, without it
//! it is the real behaviour.
use std::{io, path::Path};

#[cfg(feature = "native-tests")]
fn openai_endpoint() -> Option<String> {
    std::env::var("DESEGNOS_NATIVE_TEST_OPENAI_ENDPOINT").ok()
}

/// Native tests send API-key requests to the isolated OpenAI-compatible server with throwaway
/// credentials; the debug benchmark also gets throwaway credentials.
pub fn isolated_openai(
    credentials: std::sync::Arc<desegnos_ai::CredentialManager>,
    client: Result<desegnos_ai::OpenAiClient, String>,
) -> (
    std::sync::Arc<desegnos_ai::CredentialManager>,
    Result<desegnos_ai::OpenAiClient, String>,
) {
    #[cfg(feature = "native-tests")]
    {
        use desegnos_ai::{CredentialManager, OpenAiClient};
        if let Some(endpoint) = openai_endpoint() {
            return (
                std::sync::Arc::new(CredentialManager::isolated_for_tests()),
                OpenAiClient::for_loopback(&endpoint).map_err(|error| error.to_string()),
            );
        }
        if cfg!(debug_assertions)
            && std::env::var("DESEGNOS_GENERATION_BENCHMARK").as_deref() == Ok("1")
        {
            return (
                std::sync::Arc::new(CredentialManager::isolated_for_tests()),
                client,
            );
        }
    }
    (credentials, client)
}

/// Real builds start the ChatGPT manager on the data folder; native tests give it the
/// isolated endpoint and an in-memory vault.
pub fn chatgpt_manager(
    directory: std::path::PathBuf,
) -> Result<desegnos_ai::ChatgptManager, String> {
    #[cfg(not(feature = "native-tests"))]
    return desegnos_ai::ChatgptManager::initialize(directory)
        .map_err(|error| error.message().to_owned());
    #[cfg(feature = "native-tests")]
    (|| {
        let endpoint = openai_endpoint()
            .ok_or_else(|| "The isolated ChatGPT test endpoint is unavailable.".to_owned())?;
        let base = endpoint
            .strip_suffix("/v1")
            .ok_or_else(|| "The isolated ChatGPT test endpoint is invalid.".to_owned())?;
        let client = desegnos_ai::OpenAiClient::for_loopback(&endpoint)
            .map_err(|error| error.message().to_owned())?;
        let vault = desegnos_ai::ChatgptTestVault::memory_with_write_failure_flag(
            directory.join("native-test-vault-write-failure"),
        );
        desegnos_ai::ChatgptManager::for_loopback(directory, base, client, vault)
            .map_err(|error| error.message().to_owned())
    })()
}

/// Website downloads go to the fixture origin named by the environment, when one is set.
pub fn website_fetcher() -> Result<desegnos_ai::WebsiteFetcher, String> {
    #[cfg(feature = "native-tests")]
    match std::env::var("DESEGNOS_NATIVE_TEST_WEBSITE_ORIGIN") {
        Ok(origin) => {
            return desegnos_ai::WebsiteFetcher::for_fixture(
                &desegnos_ai::WebUrl::parse(&origin).map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string());
        }
        Err(std::env::VarError::NotPresent) => {}
        Err(error) => return Err(error.to_string()),
    }
    Ok(desegnos_ai::WebsiteFetcher::new())
}

/// The ChatGPT usage page; native tests never launch a browser.
pub fn open_usage(url: &str) -> io::Result<()> {
    #[cfg(not(feature = "native-tests"))]
    return crate::system_open::open(url);
    #[cfg(feature = "native-tests")]
    {
        let _ = url;
        Ok(())
    }
}

/// The sign-in page; native tests drive their loopback OAuth fixture instead of a browser.
#[cfg(not(feature = "native-tests"))]
pub async fn open_authorization(url: &str) -> Result<(), String> {
    crate::system_open::open(url)
        .map_err(|_| "Desegnos could not open your system browser. Check the default browser and start sign-in again.".into())
}

#[cfg(feature = "native-tests")]
pub async fn open_authorization(url: &str) -> Result<(), String> {
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

/// An owner-run benchmark keeps each model reply beside its job record.
pub fn capture_benchmark_output(
    directory: &Path,
    ordinal: usize,
    text: &str,
) -> Result<(), String> {
    #[cfg(feature = "native-tests")]
    if std::env::var("DESEGNOS_GENERATION_BENCHMARK").as_deref() == Ok("1") {
        crate::native_files::private_directory(directory, "generation record")?;
        desegnos_storage::write_text(
            &directory.join(format!("provider-output-{ordinal}.txt")),
            text,
        )
        .map_err(|error| error.to_string())?;
    }
    #[cfg(not(feature = "native-tests"))]
    let _ = (directory, ordinal, text);
    Ok(())
}
