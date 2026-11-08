//! Manual update check against GitHub Releases. Nothing here runs at startup: the editor
//! calls this command only when the user picks Check for updates, and it only reads public
//! release metadata. Installing an update stays a user action outside the app.

use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::release::REPOSITORY;

const TIMEOUT: Duration = Duration::from_secs(10);
const API_ROOT: &str = "https://api.github.com/repos/";
const GITHUB_ROOT: &str = "https://github.com/";
const UNREACHABLE: &str = "Couldn't reach GitHub. Check your connection and try again.";
const RATE_LIMITED: &str = "GitHub is limiting requests. Try again later.";
const FAILED: &str = "Couldn't check for updates.";

#[derive(ts_rs::TS, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    pub current: String,
    /// The newest published release, or none when the repository has no releases yet.
    pub latest: Option<Release>,
    pub newer: bool,
}

#[derive(ts_rs::TS, Debug, Clone, PartialEq, Serialize)]
#[ts(rename = "UpdateRelease")]
#[serde(rename_all = "camelCase")]
pub struct Release {
    /// The parsed version without its optional `v` prefix.
    pub version: String,
    pub name: String,
    /// The release page the Download action opens.
    pub url: String,
    pub published_at: String,
}

#[derive(Debug, Deserialize)]
struct ReleasePayload {
    tag_name: String,
    name: Option<String>,
    html_url: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    draft: bool,
}

impl Release {
    /// Drafts and tags that are not versions cannot be offered. Prereleases are allowed:
    /// the preview itself may be published that way.
    fn parse(payload: ReleasePayload) -> Option<(semver::Version, Self)> {
        if payload.draft {
            return None;
        }
        let version = version_tag(&payload.tag_name)?;
        if !payload.html_url.starts_with(GITHUB_ROOT) {
            return None;
        }
        let name = payload
            .name
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| payload.tag_name.clone());
        Some((
            version.clone(),
            Self {
                version: version.to_string(),
                name,
                url: payload.html_url,
                published_at: payload.published_at.unwrap_or_default(),
            },
        ))
    }
}

/// Release tags may carry one leading `v`; anything else must be plain semver.
fn version_tag(tag: &str) -> Option<semver::Version> {
    semver::Version::parse(tag.strip_prefix('v').unwrap_or(tag)).ok()
}

fn newer_than(current: &str, latest: &str) -> bool {
    match (version_tag(current), version_tag(latest)) {
        (Some(current), Some(latest)) => latest > current,
        _ => false,
    }
}

/// `https://github.com/<owner>/<repo>` becomes an API URL under the same repository.
fn api_url(repository: &str, suffix: &str) -> Option<String> {
    let path = repository.strip_prefix(GITHUB_ROOT)?.trim_end_matches('/');
    let mut parts = path.split('/');
    let (Some(owner), Some(repo), None) = (parts.next(), parts.next(), parts.next()) else {
        return None;
    };
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(format!("{API_ROOT}{owner}/{repo}/{suffix}"))
}

fn latest_release_url(repository: &str) -> Option<String> {
    api_url(repository, "releases/latest")
}

/// The fallback list keeps prereleases, which `/releases/latest` skips.
fn release_list_url(repository: &str) -> Option<String> {
    api_url(repository, "releases?per_page=10")
}

fn github_host(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|host| host == "github.com" || host.ends_with(".github.com"))
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .tls_backend_rustls()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 10 {
                attempt.error("Too many redirects while checking for updates.")
            } else if github_host(attempt.url()) {
                attempt.follow()
            } else {
                attempt.error("Updates only follow GitHub redirects.")
            }
        }))
        .retry(reqwest::retry::never())
        .timeout(TIMEOUT)
        .user_agent(concat!("Desegnos/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| FAILED.to_owned())
}

fn transport_error(error: &reqwest::Error) -> String {
    if error.is_redirect() {
        FAILED.to_owned()
    } else {
        UNREACHABLE.to_owned()
    }
}

/// GitHub's newest release, falling back to the release list when `/latest` has none.
/// A repository without releases yields `None` instead of an error.
async fn newest(client: &reqwest::Client) -> Result<Option<(semver::Version, Release)>, String> {
    let url = latest_release_url(REPOSITORY).ok_or_else(|| FAILED.to_owned())?;
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|error| transport_error(&error))?;
    match response.status() {
        reqwest::StatusCode::OK => {
            let payload = response
                .json::<ReleasePayload>()
                .await
                .map_err(|_| FAILED.to_owned())?;
            match Release::parse(payload) {
                Some(release) => Ok(Some(release)),
                None => newest_listed(client).await,
            }
        }
        reqwest::StatusCode::NOT_FOUND => newest_listed(client).await,
        reqwest::StatusCode::FORBIDDEN | reqwest::StatusCode::TOO_MANY_REQUESTS => {
            Err(RATE_LIMITED.to_owned())
        }
        _ => Err(FAILED.to_owned()),
    }
}

async fn newest_listed(
    client: &reqwest::Client,
) -> Result<Option<(semver::Version, Release)>, String> {
    let url = release_list_url(REPOSITORY).ok_or_else(|| FAILED.to_owned())?;
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|error| transport_error(&error))?;
    match response.status() {
        status if status.is_success() => {
            let payloads = response
                .json::<Vec<ReleasePayload>>()
                .await
                .map_err(|_| FAILED.to_owned())?;
            Ok(newest_of(payloads))
        }
        reqwest::StatusCode::NOT_FOUND => Ok(None),
        reqwest::StatusCode::FORBIDDEN | reqwest::StatusCode::TOO_MANY_REQUESTS => {
            Err(RATE_LIMITED.to_owned())
        }
        _ => Err(FAILED.to_owned()),
    }
}

/// The highest version among usable releases; drafts and unreadable tags are skipped.
fn newest_of(payloads: Vec<ReleasePayload>) -> Option<(semver::Version, Release)> {
    payloads
        .into_iter()
        .filter_map(Release::parse)
        .max_by(|(left, _), (right, _)| left.cmp(right))
}

/// Reports the newest public release and its page. This performs one read-only request
/// only when the user asks; there is no startup check and no automatic install.
#[tauri::command]
pub async fn check_for_updates() -> Result<UpdateCheck, crate::native_error::NativeFailure> {
    crate::native_error::NativeErrorKind::Command
        .run_async(async {
            let current = env!("CARGO_PKG_VERSION");
            let latest = newest(&client()?).await?;
            let newer = latest
                .as_ref()
                .is_some_and(|(version, _)| newer_than(current, &version.to_string()));
            Ok(UpdateCheck {
                current: current.to_owned(),
                latest: latest.map(|(_, release)| release),
                newer,
            })
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    const LATEST: &str = r#"{
        "tag_name": "v0.2.0",
        "name": "Desegnos 0.2.0",
        "html_url": "https://github.com/chelij/desegnos/releases/tag/v0.2.0",
        "published_at": "2026-11-01T10:00:00Z",
        "draft": false,
        "prerelease": false
    }"#;

    const LIST: &str = r#"[
        {"tag_name": "v0.2.0", "name": "Draft", "html_url": "https://github.com/chelij/desegnos/releases/tag/v0.2.0", "published_at": "2026-11-02T10:00:00Z", "draft": true},
        {"tag_name": "nightly", "name": "Nightly", "html_url": "https://github.com/chelij/desegnos/releases/tag/nightly", "published_at": "2026-10-01T10:00:00Z", "draft": false},
        {"tag_name": "v0.1.0", "name": "Desegnos 0.1.0", "html_url": "https://github.com/chelij/desegnos/releases/tag/v0.1.0", "published_at": "2026-10-25T10:00:00Z", "draft": false},
        {"tag_name": "v0.1.1-beta.2", "name": "", "html_url": "https://github.com/chelij/desegnos/releases/tag/v0.1.1-beta.2", "published_at": "2026-10-26T10:00:00Z", "draft": false},
        {"tag_name": "v0.1.1-beta.10", "name": null, "html_url": "https://github.com/chelij/desegnos/releases/tag/v0.1.1-beta.10", "published_at": "2026-10-27T10:00:00Z", "draft": false}
    ]"#;

    #[test]
    fn release_urls_come_from_the_public_repository() {
        assert_eq!(
            latest_release_url(REPOSITORY).as_deref(),
            Some("https://api.github.com/repos/chelij/desegnos/releases/latest")
        );
        assert_eq!(
            release_list_url(REPOSITORY).as_deref(),
            Some("https://api.github.com/repos/chelij/desegnos/releases?per_page=10")
        );
        assert_eq!(
            latest_release_url("https://example.com/chelij/desegnos"),
            None
        );
        assert_eq!(latest_release_url("https://github.com/chelij"), None);
        assert_eq!(
            latest_release_url("https://github.com/chelij/desegnos/"),
            latest_release_url("https://github.com/chelij/desegnos")
        );
    }

    #[test]
    fn versions_accept_one_v_prefix_and_order_prereleases() {
        assert!(newer_than("0.1.0", "0.1.1"));
        assert!(newer_than("0.1.0", "v0.1.1"));
        assert!(!newer_than("0.1.0", "v0.1.0"));
        assert!(!newer_than("0.1.0", "0.0.9"));
        assert!(newer_than("0.1.0-beta.2", "0.1.0"));
        assert!(newer_than("0.1.0-beta.2", "0.1.0-beta.10"));
        assert!(!newer_than("0.1.0", "not-a-version"));
        assert!(!newer_than("not-a-version", "0.2.0"));
    }

    #[test]
    fn a_release_payload_becomes_plain_display_fields() {
        let payload: ReleasePayload = serde_json::from_str(LATEST).unwrap();
        let (version, release) = Release::parse(payload).unwrap();
        assert_eq!(version.to_string(), "0.2.0");
        assert_eq!(release.version, "0.2.0");
        assert_eq!(release.name, "Desegnos 0.2.0");
        assert_eq!(
            release.url,
            "https://github.com/chelij/desegnos/releases/tag/v0.2.0"
        );
        assert_eq!(release.published_at, "2026-11-01T10:00:00Z");
    }

    #[test]
    fn drafts_and_unreadable_tags_are_skipped_in_the_fallback_list() {
        let payloads: Vec<ReleasePayload> = serde_json::from_str(LIST).unwrap();
        let (version, release) = newest_of(payloads).unwrap();
        assert_eq!(version.to_string(), "0.1.1-beta.10");
        assert_eq!(release.name, "v0.1.1-beta.10");
        assert_eq!(newest_of(Vec::new()), None);
    }

    #[test]
    fn release_pages_outside_github_are_not_offered() {
        let payload: ReleasePayload = serde_json::from_str(
            r#"{"tag_name": "v0.2.0", "name": "Desegnos", "html_url": "https://example.com/0.2.0"}"#,
        )
        .unwrap();
        assert!(Release::parse(payload).is_none());
    }

    #[test]
    fn redirects_stay_on_github_hosts() {
        for allowed in [
            "https://api.github.com/repos/chelij/desegnos/releases/latest",
            "https://github.com/chelij/desegnos/releases/tag/v0.1.0",
        ] {
            assert!(github_host(&reqwest::Url::parse(allowed).unwrap()));
        }
        for blocked in [
            "http://api.github.com/repos/chelij/desegnos/releases/latest",
            "https://example.com/chelij/desegnos",
            "https://github.com.example.com/chelij/desegnos",
            "https://notgithub.com/chelij/desegnos",
        ] {
            assert!(!github_host(&reqwest::Url::parse(blocked).unwrap()));
        }
    }
}
