//! Account model discovery. Metadata is not permission to change endpoints or prices.
use crate::{
    ApiKey, CancellationToken, ErrorKind, MAX_OUTPUT_TOKENS, ModelInfo, ProviderError, ProviderId,
    ReasoningEffort, Result,
};
use reqwest::{Client, RequestBuilder, header::HeaderValue};
use serde_json::Value;
use std::{collections::HashSet, time::Duration};
use tokio::time::Instant;

pub(crate) async fn list_models(
    provider: ProviderId,
    client: &Client,
    endpoint: &str,
    key: &ApiKey,
    cancel: &CancellationToken,
) -> Result<Vec<ModelInfo>> {
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut cursor: Option<String> = None;
    let mut cursors = HashSet::new();
    let mut ids = HashSet::new();
    let mut result = Vec::new();
    for _ in 0..32 {
        if cancel.is_cancelled() {
            return Err(failure(ErrorKind::Cancelled));
        }
        let path = if provider == ProviderId::Openrouter {
            "models/user"
        } else {
            "models"
        };
        let mut url = reqwest::Url::parse(&format!("{endpoint}/{path}"))
            .map_err(|_| failure(ErrorKind::InvalidSettings))?;
        if provider == ProviderId::Openrouter {
            url.query_pairs_mut().append_pair("limit", "100");
        }
        if let Some(cursor) = &cursor {
            url.query_pairs_mut().append_pair(
                if provider == ProviderId::Google {
                    "pageToken"
                } else if provider == ProviderId::Openrouter {
                    "offset"
                } else {
                    "after_id"
                },
                cursor,
            );
        }
        let request = client.get(url).header("accept", "application/json");
        let request = authorize(provider, request, key)?;
        let mut response = crate::retry::send(
            request,
            cancel,
            deadline,
            Duration::from_secs(30),
            1024 * 1024,
            &|_| {},
        )
        .await?;
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = crate::transport::bounded_body(&mut response, 65_536, cancel, deadline)
                .await
                .unwrap_or_default();
            return Err(crate::providers::foreign_http_error(
                status, &body, key, None, None,
            ));
        }
        let bytes = crate::transport::bounded_body(&mut response, 1024 * 1024, cancel, deadline)
            .await
            .map_err(|error| failure(error.kind))?;
        let body: Value =
            serde_json::from_slice(&bytes).map_err(|_| failure(ErrorKind::Malformed))?;
        let entries = body[if provider == ProviderId::Google {
            "models"
        } else {
            "data"
        }]
        .as_array()
        .ok_or_else(|| failure(ErrorKind::Malformed))?;
        for entry in entries {
            if ids.len() >= 4096 {
                return Err(failure(ErrorKind::OutputLimit));
            }
            let mut model = parse_model(provider, entry)?;
            if !ids.insert(model.id.clone())
                || key.appears_in(&model.id)
                || key.appears_in(&model.name)
                || model
                    .reasoning_efforts
                    .iter()
                    .any(|effort| key.appears_in(effort.as_str()))
            {
                return Err(failure(ErrorKind::Malformed));
            }
            // Gemini publishes embedding-only models through this same endpoint.
            if provider == ProviderId::Google {
                let methods = entry["supportedGenerationMethods"]
                    .as_array()
                    .ok_or_else(|| failure(ErrorKind::Malformed))?;
                if !methods
                    .iter()
                    .any(|method| method.as_str() == Some("generateContent"))
                {
                    continue;
                }
                if entry["thinking"] == false {
                    model.reasoning_efforts = vec![ReasoningEffort::Other("none".into())];
                }
            }
            result.push(model);
        }
        cursor = match provider {
            ProviderId::Openrouter if entries.len() == 100 => Some(
                (cursor
                    .as_ref()
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(0)
                    + entries.len())
                .to_string(),
            ),
            ProviderId::Google => match body.get("nextPageToken") {
                None | Some(Value::Null) => None,
                Some(Value::String(value)) if value.is_empty() => None,
                Some(Value::String(value)) => Some(value.clone()),
                _ => return Err(failure(ErrorKind::Malformed)),
            },
            ProviderId::Anthropic => match body.get("has_more") {
                Some(Value::Bool(true)) => Some(
                    body["last_id"]
                        .as_str()
                        .ok_or_else(|| failure(ErrorKind::Malformed))?
                        .into(),
                ),
                None | Some(Value::Bool(false)) => None,
                _ => return Err(failure(ErrorKind::Malformed)),
            },
            _ => None,
        };
        let Some(next) = &cursor else {
            return Ok(result);
        };
        if next.is_empty()
            || next.len() > 2048
            || !next.bytes().all(|byte| byte.is_ascii_graphic())
            || key.appears_in(next)
            || !cursors.insert(next.clone())
        {
            return Err(failure(ErrorKind::Malformed));
        }
    }
    Err(failure(ErrorKind::OutputLimit))
}

pub(crate) fn authorize(
    provider: ProviderId,
    request: RequestBuilder,
    key: &ApiKey,
) -> Result<RequestBuilder> {
    if provider == ProviderId::Local && key.expose() == crate::credentials::LOCAL_ANONYMOUS_KEY {
        return Ok(request);
    }
    let value = if matches!(
        provider,
        ProviderId::Openai | ProviderId::Openrouter | ProviderId::Deepseek | ProviderId::Local
    ) {
        zeroize::Zeroizing::new(format!("Bearer {}", key.expose()))
    } else {
        zeroize::Zeroizing::new(key.expose().into())
    };
    let mut header =
        HeaderValue::from_str(&value).map_err(|_| failure(ErrorKind::InvalidCredential))?;
    header.set_sensitive(true);
    Ok(match provider {
        ProviderId::Openai | ProviderId::Openrouter | ProviderId::Deepseek | ProviderId::Local => {
            request.header("authorization", header)
        }
        ProviderId::Anthropic => request
            .header("x-api-key", header)
            .header("anthropic-version", "2023-06-01"),
        ProviderId::Google => request.header("x-goog-api-key", header),
    })
}

fn parse_model(provider: ProviderId, entry: &Value) -> Result<ModelInfo> {
    let raw = entry[if provider == ProviderId::Google {
        "name"
    } else {
        "id"
    }]
    .as_str()
    .ok_or_else(|| failure(ErrorKind::Malformed))?;
    let id = if provider == ProviderId::Google {
        raw.strip_prefix("models/")
            .ok_or_else(|| failure(ErrorKind::Malformed))?
    } else {
        raw
    };
    let name = match entry.get(match provider {
        ProviderId::Google => "displayName",
        ProviderId::Openrouter => "name",
        _ => "display_name",
    }) {
        Some(Value::String(name)) => Some(name.as_str()),
        None | Some(Value::Null) => None,
        _ => return Err(failure(ErrorKind::Malformed)),
    };
    if let Some(name) = name
        && (name.trim().is_empty() || name.len() > 256 || name.chars().any(char::is_control))
    {
        return Err(failure(ErrorKind::Malformed));
    }
    if !crate::settings::valid_model_id(id) {
        return Err(failure(ErrorKind::Malformed));
    }
    let mut model = provider
        .catalogue()
        .into_iter()
        .find(|model| model.id == id)
        .unwrap_or_else(|| ModelInfo {
            id: id.into(),
            name: id.into(),
            reasoning_efforts: vec![ReasoningEffort::Other("none".into())],
            context_window_tokens: crate::CONTEXT_WINDOW_TOKENS,
            max_output_tokens: MAX_OUTPUT_TOKENS,
            input_usd_per_million: 0.0,
            output_usd_per_million: 0.0,
            long_context_input_usd_per_million: 0.0,
            long_context_output_usd_per_million: 0.0,
            verified_at: String::new(),
            source_url: String::new(),
            pricing_required: true,
            default_pricing: None,
        });
    // Discovery names replace the catalogue name only when the provider publishes one.
    if let Some(name) = name {
        model.name = name.into();
    }
    let (context_field, output_field) = match provider {
        ProviderId::Anthropic => ("max_input_tokens", "max_tokens"),
        ProviderId::Google => ("inputTokenLimit", "outputTokenLimit"),
        ProviderId::Openrouter => ("context_length", "max_output_tokens"),
        _ => ("context_window", "max_output_tokens"),
    };
    if let Some(limit) = positive_limit(entry, context_field)? {
        model.context_window_tokens = limit.min(crate::CONTEXT_WINDOW_TOKENS);
    }
    if let Some(limit) = positive_limit(entry, output_field)? {
        model.max_output_tokens = u32::try_from(limit.min(u64::from(MAX_OUTPUT_TOKENS))).unwrap();
    }
    if provider == ProviderId::Openrouter {
        if let Some(top) = entry.get("top_provider")
            && let Some(limit) = positive_limit(top, "max_completion_tokens")?
        {
            model.max_output_tokens =
                u32::try_from(limit.min(u64::from(MAX_OUTPUT_TOKENS))).unwrap();
        }
        let prompt = price(entry.pointer("/pricing/prompt"), 1_000_000.0)?;
        let completion = price(entry.pointer("/pricing/completion"), 1_000_000.0)?;
        let request = price(entry.pointer("/pricing/request"), 1.0)?
            .or_else(|| entry.pointer("/pricing/request").is_none().then_some(0.0));
        let image = price(entry.pointer("/pricing/image"), 1.0)?
            .or_else(|| entry.pointer("/pricing/image").is_none().then_some(0.0));
        if let (Some(input), Some(output), Some(request_usd), Some(image_usd)) =
            (prompt, completion, request, image)
        {
            model.input_usd_per_million = input;
            model.output_usd_per_million = output;
            model.long_context_input_usd_per_million = input;
            model.long_context_output_usd_per_million = output;
            model.pricing_required = false;
            model.default_pricing = Some(crate::ModelPricing {
                model: model.id.clone(),
                input_usd_per_million: input,
                output_usd_per_million: output,
                request_usd,
                image_usd,
            });
            model.source_url = "https://openrouter.ai/api/v1/models/user".into();
        }
    }
    if provider == ProviderId::Local {
        model.default_pricing = Some(crate::ModelPricing {
            model: model.id.clone(),
            ..Default::default()
        });
    }
    if let Some(levels) = entry
        .get("supported_reasoning_levels")
        .filter(|value| !value.is_null())
    {
        let levels = levels
            .as_array()
            .ok_or_else(|| failure(ErrorKind::Malformed))?;
        model.reasoning_efforts = levels
            .iter()
            .map(|level| {
                serde_json::from_value(level["effort"].clone())
                    .map_err(|_| failure(ErrorKind::Malformed))
            })
            .collect::<Result<Vec<_>>>()?;
    } else if let Some(effort) = entry
        .pointer("/capabilities/effort")
        .filter(|value| !value.is_null())
    {
        let effort = effort
            .as_object()
            .ok_or_else(|| failure(ErrorKind::Malformed))?;
        model.reasoning_efforts = effort
            .iter()
            .filter(|(_, support)| support["supported"] == true)
            .map(|(name, _)| {
                ReasoningEffort::try_from(name.clone()).map_err(|_| failure(ErrorKind::Malformed))
            })
            .collect::<Result<Vec<_>>>()?;
    }
    if model.reasoning_efforts.len() > 32
        || model
            .reasoning_efforts
            .iter()
            .enumerate()
            .any(|(index, effort)| model.reasoning_efforts[..index].contains(effort))
    {
        return Err(failure(ErrorKind::Malformed));
    }
    if model.reasoning_efforts.is_empty() {
        model
            .reasoning_efforts
            .push(ReasoningEffort::Other("none".into()));
    }
    Ok(model)
}

fn positive_limit(entry: &Value, field: &str) -> Result<Option<u64>> {
    match entry.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .filter(|value| *value > 0)
            .map(Some)
            .ok_or_else(|| failure(ErrorKind::Malformed)),
    }
}

fn price(value: Option<&Value>, multiplier: f64) -> Result<Option<f64>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let rate = value
                .as_f64()
                .or_else(|| value.as_str().and_then(|value| value.parse::<f64>().ok()))
                .filter(|value| {
                    value.is_finite() && *value >= 0.0 && *value * multiplier <= 1_000_000.0
                });
            Ok(rate.map(|rate| rate * multiplier))
        }
    }
}

fn failure(kind: ErrorKind) -> ProviderError {
    ProviderError::before(kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AuthenticationMethod, ModelPricing, ProviderSettings, SubmissionOutcome};
    use desegnos_core::GenerationMode;
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    async fn server(
        replies: Vec<(u16, Value)>,
    ) -> (String, Arc<Mutex<Vec<String>>>, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let task = tokio::spawn(async move {
            for (status, body) in replies {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut chunk = [0; 1024];
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let read = socket.read(&mut chunk).await.unwrap();
                    if read == 0 {
                        return;
                    }
                    request.extend_from_slice(&chunk[..read]);
                    assert!(request.len() < 16_384);
                }
                captured
                    .lock()
                    .unwrap()
                    .push(String::from_utf8(request).unwrap());
                let body = body.to_string();
                let _ = socket.write_all(format!("HTTP/1.1 {status} Fixture\r\ncontent-type: application/json\r\nretry-after: 0\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).as_bytes()).await;
            }
        });
        (endpoint, requests, task)
    }

    fn client() -> Client {
        Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .unwrap()
    }
    fn key() -> ApiKey {
        ApiKey::new("synthetic-model-discovery-key".into()).unwrap()
    }

    #[tokio::test]
    async fn account_catalogues_follow_pagination_and_preserve_provider_metadata() {
        for provider in [
            ProviderId::Openai,
            ProviderId::Anthropic,
            ProviderId::Google,
        ] {
            let pages = match provider {
                ProviderId::Openai => vec![
                    json!({"data":[{"id":"ft:future-model:account:job"},{"id":crate::MODEL_ID}]}),
                ],
                ProviderId::Anthropic => vec![
                    json!({"data":[{"id":"future-claude","display_name":"New Claude","max_input_tokens":100_000,"max_tokens":4096,"capabilities":{"effort":{"supported":true,"low":{"supported":true},"max":{"supported":true}}}}],"has_more":true,"last_id":"future-claude"}),
                    json!({"data":[{"id":"new-claude","display_name":"Another Claude"}],"has_more":false}),
                ],
                _ => vec![
                    json!({"models":[{"name":"models/embed","supportedGenerationMethods":["embedContent"]},{"name":"models/future-gemini","displayName":"New Gemini","inputTokenLimit":120_000,"outputTokenLimit":2048,"thinking":false,"supportedGenerationMethods":["generateContent"]}],"nextPageToken":"cursor/?+"}),
                    json!({"models":[{"name":"models/new-gemini","supportedGenerationMethods":["generateContent"]}]}),
                ],
            };
            let count = pages.len();
            let (endpoint, requests, task) =
                server(pages.into_iter().map(|page| (200, page)).collect()).await;
            let models = list_models(
                provider,
                &client(),
                &endpoint,
                &key(),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
            task.await.unwrap();
            assert_eq!(models.len(), 2);
            assert!(models[0].pricing_required);
            assert!(models[0].verified_at.is_empty());
            assert!(models[0].source_url.is_empty());
            if provider == ProviderId::Openai {
                assert!(!models[1].pricing_required);
            }
            if provider == ProviderId::Anthropic {
                assert_eq!(models[0].context_window_tokens, 100_000);
                assert_eq!(models[0].max_output_tokens, 4096);
                assert_eq!(
                    models[0]
                        .reasoning_efforts
                        .iter()
                        .map(ReasoningEffort::as_str)
                        .collect::<Vec<_>>(),
                    ["low", "max"]
                );
            }
            if provider == ProviderId::Google {
                assert_eq!(models[0].max_output_tokens, 2048);
            }
            let captured = requests.lock().unwrap();
            assert_eq!(captured.len(), count);
            let auth_header = match provider {
                ProviderId::Openai => "authorization: bearer ",
                ProviderId::Anthropic => "x-api-key: ",
                _ => "x-goog-api-key: ",
            };
            for request in captured.iter() {
                assert!(request.starts_with("GET /v1/models"));
                assert!(request.to_ascii_lowercase().contains(auth_header));
                assert!(!request.lines().next().unwrap().contains(key().expose()));
            }
            if count == 2 {
                let path = captured[1].split_whitespace().nth(1).unwrap();
                let url = reqwest::Url::parse(&format!("http://localhost{path}")).unwrap();
                assert_eq!(
                    url.query_pairs().next().unwrap().1,
                    if provider == ProviderId::Google {
                        "cursor/?+"
                    } else {
                        "future-claude"
                    }
                );
            }
        }
    }

    #[tokio::test]
    async fn openrouter_uses_account_catalogue_pagination_and_prices() {
        let records: Vec<_> = (0..100).map(|i| json!({"id":format!("vendor/future-{i}:free"),"name":format!("Future {i}"),"context_length":100_000,"top_provider":{"max_completion_tokens":4096},"pricing":{"prompt":"0.000002","completion":"0.000008","request":"0.01","image":"0.02"}})).collect();
        let (endpoint, requests, task) = server(vec![
            (200, json!({"data":records})),
            (
                200,
                json!({"data":[{"id":"vendor/last","pricing":{"prompt":"-1","completion":"0"}}]}),
            ),
        ])
        .await;
        let models = list_models(
            ProviderId::Openrouter,
            &client(),
            &endpoint,
            &key(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        task.await.unwrap();
        assert_eq!(models.len(), 101);
        let quoted = &models[0];
        assert_eq!(quoted.id, "vendor/future-0:free");
        assert_eq!(quoted.context_window_tokens, 100_000);
        assert_eq!(quoted.max_output_tokens, 4096);
        assert!(!quoted.pricing_required);
        let pricing = quoted.default_pricing.as_ref().unwrap();
        assert_eq!(
            (
                pricing.input_usd_per_million,
                pricing.output_usd_per_million,
                pricing.request_usd,
                pricing.image_usd
            ),
            (2.0, 8.0, 0.01, 0.02)
        );
        let mut settings = ProviderSettings {
            model: quoted.id.clone(),
            max_output_tokens: 4096,
            reasoning_effort: ReasoningEffort::Other("none".into()),
            pricing: Some(pricing.clone()),
            ..Default::default()
        };
        quoted.validate_api_settings(&settings).unwrap();
        settings.pricing.as_mut().unwrap().request_usd = 0.0;
        assert!(quoted.validate_api_settings(&settings).is_err());
        assert!(models[100].pricing_required && models[100].default_pricing.is_none());
        let requests = requests.lock().unwrap();
        assert!(requests[0].starts_with("GET /v1/models/user?limit=100 "));
        assert!(requests[1].starts_with("GET /v1/models/user?limit=100&offset=100 "));
        assert!(
            requests[0]
                .to_ascii_lowercase()
                .contains("authorization: bearer ")
        );
    }

    #[tokio::test]
    async fn local_discovery_needs_no_key_and_preserves_server_model_identifiers() {
        let (endpoint,requests,task)=server(vec![(200,json!({"data":[{"id":"org/model:latest","context_window":100_000,"max_output_tokens":4096}]}))]).await;
        let models = list_models(
            ProviderId::Local,
            &client(),
            &endpoint,
            &ApiKey::local_anonymous(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        task.await.unwrap();
        assert_eq!(models[0].id, "org/model:latest");
        assert_eq!(
            models[0]
                .default_pricing
                .as_ref()
                .unwrap()
                .input_usd_per_million,
            0.0
        );
        assert!(models[0].pricing_required);
        let requests = requests.lock().unwrap();
        assert!(requests[0].starts_with("GET /v1/models "));
        assert!(!requests[0].to_ascii_lowercase().contains("authorization:"));
    }

    #[tokio::test]
    async fn discovery_rejects_bad_metadata_duplicates_errors_and_cancellation() {
        for body in [
            json!({"data":[{"id":"../response"}]}),
            json!({"data":[{"id":".."}]}),
            json!({"data":[{"id":"future","display_name":"control\ncharacter"}]}),
            json!({"data":[{"id":"future","display_name":key().expose()}]}),
            json!({"data":[{"id":"future","context_window":0}]}),
            json!({"data":[{"id":"future"},{"id":"future"}]}),
            json!({"data":[{"id":"future","supported_reasoning_levels":[{"effort":"low"},{"effort":"low"}]}]}),
            json!({"wrong":[]}),
        ] {
            let (endpoint, requests, task) = server(vec![(200, body)]).await;
            let error = list_models(
                ProviderId::Openai,
                &client(),
                &endpoint,
                &key(),
                &CancellationToken::new(),
            )
            .await
            .unwrap_err();
            task.await.unwrap();
            assert_eq!(error.kind, ErrorKind::Malformed);
            assert_eq!(error.outcome, SubmissionOutcome::NotSubmitted);
            assert!(!error.to_string().contains(key().expose()));
            assert_eq!(requests.lock().unwrap().len(), 1);
        }
        let (endpoint, _, task) = server(vec![(429, json!({"error":key().expose()})); 3]).await;
        assert_eq!(
            list_models(
                ProviderId::Openai,
                &client(),
                &endpoint,
                &key(),
                &CancellationToken::new()
            )
            .await
            .unwrap_err()
            .kind,
            ErrorKind::RateLimited
        );
        task.await.unwrap();
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        assert_eq!(
            list_models(
                ProviderId::Openai,
                &client(),
                &endpoint,
                &key(),
                &cancellation
            )
            .await
            .unwrap_err()
            .kind,
            ErrorKind::Cancelled
        );
        let (endpoint, _, task) = server(vec![(
            200,
            json!({"data":[],"padding":"x".repeat(1024*1024)}),
        )])
        .await;
        let error = list_models(
            ProviderId::Openai,
            &client(),
            &endpoint,
            &key(),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.kind, ErrorKind::OutputLimit);
        assert_eq!(error.outcome, SubmissionOutcome::NotSubmitted);
        task.await.unwrap();
        let pages = vec![
            (
                200,
                json!({"data":[{"id":"one"}],"has_more":true,"last_id":"cursor"}),
            ),
            (
                200,
                json!({"data":[{"id":"two"}],"has_more":true,"last_id":"cursor"}),
            ),
        ];
        let (endpoint, _, task) = server(pages).await;
        assert_eq!(
            list_models(
                ProviderId::Anthropic,
                &client(),
                &endpoint,
                &key(),
                &CancellationToken::new()
            )
            .await
            .unwrap_err()
            .kind,
            ErrorKind::Malformed
        );
        task.await.unwrap();
    }

    #[test]
    fn advertised_output_limits_climb_to_the_api_ceiling_and_stop_there() {
        fn entry(provider: ProviderId, limit: u64) -> Value {
            match provider {
                ProviderId::Google => json!({"name":"models/future","outputTokenLimit":limit}),
                ProviderId::Anthropic => json!({"id":"future","max_tokens":limit}),
                _ => json!({"id":"future","max_output_tokens":limit}),
            }
        }
        for provider in [
            ProviderId::Openai,
            ProviderId::Anthropic,
            ProviderId::Google,
            ProviderId::Openrouter,
        ] {
            let model = parse_model(provider, &entry(provider, 100_000)).unwrap();
            assert_eq!(model.max_output_tokens, 100_000, "{provider:?}");
            ProviderSettings {
                model: model.id.clone(),
                max_output_tokens: model.max_output_tokens,
                reasoning_effort: model.reasoning_efforts[0].clone(),
                spending_limit_usd: 10_000.0,
                ..ProviderSettings::default()
            }
            .validate()
            .unwrap();
            // A limit beyond the API-key ceiling is advertised as the ceiling itself, so the
            // highest setting the UI offers is still one validation accepts.
            let model = parse_model(provider, &entry(provider, 200_000)).unwrap();
            assert_eq!(model.max_output_tokens, MAX_OUTPUT_TOKENS, "{provider:?}");
            ProviderSettings {
                model: model.id.clone(),
                max_output_tokens: model.max_output_tokens,
                reasoning_effort: model.reasoning_efforts[0].clone(),
                spending_limit_usd: 10_000.0,
                ..ProviderSettings::default()
            }
            .validate()
            .unwrap();
        }
    }

    #[test]
    fn new_api_models_use_explicit_prices_and_keep_budget_and_token_checks() {
        let snapshot = crate::tests::snapshot(GenerationMode::Strict);
        let schema = desegnos_core::candidate_schema();
        for provider in [
            ProviderId::Openai,
            ProviderId::Anthropic,
            ProviderId::Google,
        ] {
            let entry = if provider == ProviderId::Google {
                json!({"name":"models/future","outputTokenLimit":4096})
            } else {
                json!({"id":"future"})
            };
            let model = parse_model(provider, &entry).unwrap();
            let mut settings = ProviderSettings {
                model: model.id.clone(),
                max_output_tokens: 4096,
                reasoning_effort: model.reasoning_efforts[0].clone(),
                spending_limit_usd: 10_000.0,
                ..provider.defaults()
            };
            assert_eq!(
                model.validate_api_settings(&settings).unwrap_err().kind,
                ErrorKind::InvalidSettings
            );
            settings.pricing = Some(ModelPricing {
                model: settings.model.clone(),
                input_usd_per_million: 20.0,
                output_usd_per_million: 80.0,
                ..Default::default()
            });
            let saved: ProviderSettings =
                serde_json::from_value(serde_json::to_value(&settings).unwrap()).unwrap();
            model.validate_api_settings(&saved).unwrap();
            let client =
                crate::ProviderClient::new(provider, crate::OpenAiClient::new().unwrap()).unwrap();
            let estimate = client
                .estimate(
                    AuthenticationMethod::ApiKey,
                    &snapshot,
                    GenerationMode::Strict,
                    &schema,
                    &saved,
                )
                .unwrap();
            assert!(
                estimate.estimate_usd >= f64::from(saved.max_output_tokens) * 80.0 / 1_000_000.0
            );
            assert_eq!(
                model
                    .validate_api_input(&saved, model.context_window_tokens)
                    .unwrap_err()
                    .kind,
                ErrorKind::ContextLimit
            );
            settings.spending_limit_usd = 0.01;
            assert_eq!(
                client
                    .estimate(
                        AuthenticationMethod::ApiKey,
                        &snapshot,
                        GenerationMode::Strict,
                        &schema,
                        &settings
                    )
                    .unwrap_err()
                    .kind,
                ErrorKind::SpendingLimit
            );
            settings.pricing.as_mut().unwrap().model = "different-model".into();
            assert_eq!(
                settings
                    .validate_for_auth(AuthenticationMethod::ApiKey)
                    .unwrap_err()
                    .kind,
                ErrorKind::InvalidSettings
            );
        }
    }
    #[tokio::test]
    async fn model_listing_retries_transient_status_without_duplicating_models() {
        let (endpoint, requests, task) = server(vec![
            (500, json!({"code":"internal_error"})),
            (200, json!({"data":[{"id":crate::MODEL_ID}]})),
        ])
        .await;
        let models = list_models(
            ProviderId::Openai,
            &client(),
            &endpoint,
            &key(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        task.await.unwrap();
        assert_eq!(models.len(), 1);
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0], requests[1]);
    }
}
