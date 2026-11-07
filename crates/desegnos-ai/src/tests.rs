use crate::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use desegnos_core::{GenerationMode, RequirementsSnapshot, digest};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
use uuid::Uuid;

const KEY: &str = "sk-test-loopback-only-never-an-OpenAI-credential";

#[derive(Debug, Clone)]
struct Captured {
    method: String,
    path: String,
    headers: String,
    body: Vec<u8>,
}

#[derive(Clone)]
pub(crate) struct Reply {
    status: u16,
    content_type: Option<&'static str>,
    headers: String,
    chunks: Vec<Vec<u8>>,
    delay: Duration,
    declared_length: Option<usize>,
    drop_before_headers: bool,
}

impl Reply {
    pub(crate) fn sse(bytes: Vec<u8>) -> Self {
        Self {
            status: 200,
            content_type: Some("text/event-stream; charset=utf-8"),
            headers: String::new(),
            chunks: bytes.chunks(71).map(<[u8]>::to_vec).collect(),
            delay: Duration::ZERO,
            declared_length: None,
            drop_before_headers: false,
        }
    }
    pub(crate) fn json(status: u16, value: Value) -> Self {
        Self {
            status,
            content_type: Some("application/json"),
            headers: if status == 429 || status >= 500 {
                "retry-after: 0\r\n".into()
            } else {
                String::new()
            },
            chunks: vec![serde_json::to_vec(&value).unwrap()],
            delay: Duration::ZERO,
            declared_length: None,
            drop_before_headers: false,
        }
    }
}

pub(crate) struct Server {
    pub(crate) endpoint: String,
    pub(crate) count: Arc<AtomicUsize>,
    captured: Arc<Mutex<Vec<Captured>>>,
    task: JoinHandle<()>,
}

impl Server {
    async fn start(reply: Reply) -> Self {
        Self::start_sequence(vec![reply]).await
    }
    pub(crate) async fn start_sequence(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let captured = Arc::new(Mutex::new(vec![]));
        let count = Arc::new(AtomicUsize::new(0));
        let observed = captured.clone();
        let observed_count = count.clone();
        let task = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let ordinal = observed_count.fetch_add(1, Ordering::SeqCst);
                let reply = replies
                    .get(ordinal)
                    .unwrap_or_else(|| replies.last().unwrap())
                    .clone();
                let request = read_request(&mut socket).await;
                observed.lock().unwrap().push(request);
                if reply.drop_before_headers {
                    continue;
                }
                let framing = match reply.declared_length {
                    Some(length) => format!("Content-Length: {length}\r\n"),
                    None => "Transfer-Encoding: chunked\r\n".into(),
                };
                let content_type = reply
                    .content_type
                    .map(|value| format!("Content-Type: {value}\r\n"))
                    .unwrap_or_default();
                let headers = format!(
                    "HTTP/1.1 {} Test\r\n{}{}{}Connection: close\r\n\r\n",
                    reply.status, content_type, reply.headers, framing
                );
                if socket.write_all(headers.as_bytes()).await.is_err() {
                    continue;
                }
                let mut failed = false;
                for bytes in &reply.chunks {
                    if !reply.delay.is_zero() {
                        tokio::time::sleep(reply.delay).await;
                    }
                    let framed = if reply.declared_length.is_none() {
                        let mut frame = format!("{:X}\r\n", bytes.len()).into_bytes();
                        frame.extend_from_slice(bytes);
                        frame.extend_from_slice(b"\r\n");
                        frame
                    } else {
                        bytes.clone()
                    };
                    if socket.write_all(&framed).await.is_err() {
                        failed = true;
                        break;
                    }
                    if socket.flush().await.is_err() {
                        failed = true;
                        break;
                    }
                }
                if !failed && reply.declared_length.is_none() {
                    let _ = socket.write_all(b"0\r\n\r\n").await;
                }
            }
        });
        Self {
            endpoint: format!("http://{address}/v1"),
            count,
            captured,
            task,
        }
    }

    fn client(&self) -> OpenAiClient {
        OpenAiClient::for_loopback(&self.endpoint).unwrap()
    }
    fn request(&self) -> Captured {
        self.captured
            .lock()
            .unwrap()
            .first()
            .expect("server received a request")
            .clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn read_request(socket: &mut TcpStream) -> Captured {
    let mut bytes = vec![];
    let header_end = loop {
        let mut part = [0_u8; 4096];
        let count = socket.read(&mut part).await.unwrap();
        assert!(count > 0);
        bytes.extend_from_slice(&part[..count]);
        assert!(bytes.len() <= MAX_REQUEST_BYTES + 65_536);
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
    let length: usize = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().unwrap())
        })
        .unwrap_or(0);
    assert!(length <= MAX_REQUEST_BYTES);
    while bytes.len() - header_end < length {
        let mut part = [0_u8; 8192];
        let count = socket.read(&mut part).await.unwrap();
        assert!(count > 0);
        bytes.extend_from_slice(&part[..count]);
    }
    let mut first = headers.lines().next().unwrap().split_whitespace();
    Captured {
        method: first.next().unwrap().into(),
        path: first.next().unwrap().into(),
        body: bytes[header_end..header_end + length].to_vec(),
        headers,
    }
}

pub(super) fn snapshot(mode: GenerationMode) -> RequirementsSnapshot {
    let job_id = Uuid::new_v4();
    let project_id = Uuid::new_v4();
    let format = desegnos_core::CURRENT_REQUIREMENTS_FORMAT;
    let markdown = format!(
        "# Generation requirements\n\nRequirements format: {format}\n\nJob: {job_id}\n\n## Generation setting\n\n```json\n{}\n```\n\n## Requested areas\n\nCreate a café page for Desegnos.\r\nKeep this literal: ```md <script>never run</script> ```\nUnicode: 日本語, Bahasa Indonesia, 💡.\n",
        json!({"generationMode":mode})
    );
    RequirementsSnapshot {
        job_id,
        project_id,
        base_revision: 8,
        sha256: digest(markdown.as_bytes()),
        markdown,
        request_count: 2,
        reference_ids: vec![],
        page_count: 1,
        dependencies: BTreeMap::new(),
        dossier_context: None,
        request_history: String::new(),
        capabilities: Some(desegnos_core::GenerationCapabilities {
            web_search: false,
            image_downloads: 0,
            image_generation: None,
        }),
        images: vec![],
    }
}

fn schema() -> Value {
    json!({"type":"object","additionalProperties":false,"properties":{
        "jobId":{"type":"string"},"requirementsSha256":{"type":"string"},"summary":{"type":"string"}},
        "required":["jobId","requirementsSha256","summary"]})
}

fn candidate(snapshot: &RequirementsSnapshot) -> String {
    serde_json::to_string(
        &json!({"jobId":snapshot.job_id,"requirementsSha256":snapshot.sha256,
        "summary":"Created café / 日本語 / 💡 with literal <script> source."}),
    )
    .unwrap()
}

fn wire_format(snapshot: &RequirementsSnapshot) -> Value {
    let mut bound = schema();
    bound["properties"]["jobId"]["enum"] = json!([snapshot.job_id]);
    bound["properties"]["requirementsSha256"]["enum"] = json!([snapshot.sha256]);
    json!({"type":"json_schema","name":"desegnos_generation_candidate","schema":bound,"strict":true})
}

fn frame(value: Value) -> Vec<u8> {
    format!(
        "event: {}\r\ndata: {}\r\n\r\n",
        value["type"].as_str().unwrap(),
        serde_json::to_string(&value).unwrap()
    )
    .into_bytes()
}

fn completed(text: &str) -> Value {
    json!({"type":"response.completed","response":{"id":"resp_test_123","status":"completed",
        "output":[{"type":"reasoning","summary":[]},{"id":"msg_test_123","type":"message","role":"assistant","status":"completed",
            "content":[{"type":"output_text","text":text,"annotations":[]}]}],
        "usage":{"input_tokens":211,"output_tokens":89,"total_tokens":300}}})
}

fn stream(snapshot: &RequirementsSnapshot) -> Vec<u8> {
    let text = candidate(snapshot);
    let split = text.char_indices().nth(text.chars().count() / 2).unwrap().0;
    let mut bytes = b": keepalive\r\n\r\n".to_vec();
    bytes.extend(frame(
        json!({"type":"response.created","response":{"id":"resp_test_123","status":"in_progress"}}),
    ));
    for delta in [&text[..split], &text[split..]] {
        bytes.extend(frame(json!({"type":"response.output_text.delta","output_index":1,"content_index":0,"item_id":"msg_test_123","delta":delta})));
    }
    bytes.extend(frame(
        json!({"type":"response.output_text.done","output_index":1,"content_index":0,"item_id":"msg_test_123","text":text}),
    ));
    bytes.extend(frame(completed(&text)));
    bytes
}

fn phased_stream(snapshot: &RequirementsSnapshot, with_deltas: bool) -> Vec<u8> {
    let text = candidate(snapshot);
    let split = text.char_indices().nth(text.chars().count() / 2).unwrap().0;
    let commentary = "Preparing the requested page.";
    let mut complete = completed(&text);
    complete["response"]["output"] = json!([
        {"type":"reasoning","summary":[]},
        {"id":"msg_commentary","type":"message","role":"assistant","status":"completed","phase":"commentary",
         "content":[{"type":"output_text","text":commentary,"annotations":[]}]},
        {"id":"msg_final","type":"message","role":"assistant","status":"completed","phase":"final_answer",
         "content":[{"type":"output_text","text":&text[..split],"annotations":[]},
                    {"type":"output_text","text":&text[split..],"annotations":[]}]}
    ]);
    let mut bytes = frame(
        json!({"type":"response.created","response":{"id":"resp_test_123","status":"in_progress"}}),
    );
    if with_deltas {
        for (output_index, content_index, item_id, part) in [
            (1, 0, "msg_commentary", commentary),
            (2, 0, "msg_final", &text[..split]),
            (2, 1, "msg_final", &text[split..]),
        ] {
            bytes.extend(frame(
                json!({"type":"response.output_text.delta","output_index":output_index,
                "content_index":content_index,"item_id":item_id,"delta":part}),
            ));
            bytes.extend(frame(
                json!({"type":"response.output_text.done","output_index":output_index,
                "content_index":content_index,"item_id":item_id,"text":part}),
            ));
        }
    }
    bytes.extend(frame(complete));
    bytes
}

fn sparse_stream_events(snapshot: &RequirementsSnapshot) -> Vec<Value> {
    let frames = crate::sse::SseDecoder::default()
        .push(&phased_stream(snapshot, true))
        .unwrap();
    let mut values: Vec<Value> = frames
        .iter()
        .map(|frame| serde_json::from_str(&frame.data).unwrap())
        .collect();
    let complete = values.pop().unwrap();
    let mut result = vec![
        values.remove(0),
        json!({"type":"response.output_item.added","output_index":0,"item":null}),
    ];
    let mut reasoning = complete["response"]["output"][0].clone();
    reasoning["id"] = json!("rs_sparse");
    result.push(json!({"type":"response.output_item.added","output_index":2,"item":reasoning}));
    result.push(json!({"type":"response.output_item.done","output_index":2,"item":reasoning}));
    for (position, wire_index) in [(1, 5), (2, 9)] {
        let item = complete["response"]["output"][position].clone();
        let mut added = item.clone();
        added["content"] = json!([]);
        added["status"] = json!("in_progress");
        result.push(
            json!({"type":"response.output_item.added","output_index":wire_index,"item":added}),
        );
        for value in values
            .iter()
            .filter(|value| value["output_index"] == position)
        {
            let mut value = value.clone();
            value["output_index"] = json!(wire_index);
            result.push(value);
        }
        result.push(
            json!({"type":"response.output_item.done","output_index":wire_index,"item":item}),
        );
    }
    result.push(complete);
    result
}

fn finalized_answer_events(snapshot: &RequirementsSnapshot, phase: Option<&str>) -> Vec<Value> {
    let text = candidate(snapshot);
    let mut completed = completed(&text);
    let mut message = completed["response"]["output"][1].clone();
    if let Some(phase) = phase {
        message["phase"] = json!(phase);
    }
    let mut added = message.clone();
    added["content"] = json!([]);
    added["status"] = json!("in_progress");
    let reasoning =
        json!({"id":"rs_finalized","type":"reasoning","status":"completed","summary":[]});
    completed["response"]["output"] = json!([]);
    vec![
        json!({"type":"response.created","response":{"id":"resp_test_123","status":"in_progress"}}),
        json!({"type":"response.output_item.added","output_index":0,"item":reasoning}),
        json!({"type":"response.output_item.done","output_index":0,"item":reasoning}),
        json!({"type":"response.output_item.added","output_index":1,"item":added}),
        json!({"type":"response.output_text.delta","output_index":1,"content_index":0,"item_id":"msg_test_123","delta":text}),
        json!({"type":"response.output_text.done","output_index":1,"content_index":0,"item_id":"msg_test_123","text":text}),
        json!({"type":"response.output_item.done","output_index":1,"item":message}),
        completed,
    ]
}

#[tokio::test]
async fn finalized_single_part_answer_survives_an_empty_terminal_output_list() {
    let frozen = snapshot(GenerationMode::Strict);
    for authentication in [AuthenticationMethod::ApiKey, AuthenticationMethod::Chatgpt] {
        for phase in [None, Some("final_answer")] {
            let response = generate_events(
                &frozen,
                finalized_answer_events(&frozen, phase),
                authentication,
            )
            .await
            .unwrap();
            assert_eq!(response.text, candidate(&frozen));
            assert_eq!(response.response_id, "resp_test_123");
            assert_eq!(response.usage.unwrap().total_tokens, 300);
        }
    }
}

#[tokio::test]
async fn finalized_answers_merged_with_terminal_items_still_require_one_consistent_answer() {
    let frozen = snapshot(GenerationMode::Strict);
    let original = finalized_answer_events(&frozen, None);
    let mut cases = vec![];
    let mut two_answers = original.clone();
    let mut other = original[6]["item"].clone();
    other["id"] = json!("msg_other_final");
    two_answers.last_mut().unwrap()["response"]["output"] = json!([other]);
    cases.push((
        two_answers,
        ErrorKind::Malformed,
        Some(ResponseRejection::AnswerCount),
    ));
    let mut changed = original.clone();
    let mut item = original[6]["item"].clone();
    item["content"][0]["text"] = json!("Private changed source");
    changed.last_mut().unwrap()["response"]["output"] = json!([item]);
    cases.push((
        changed,
        ErrorKind::Malformed,
        Some(ResponseRejection::TextMismatch),
    ));
    let mut partial = original.clone();
    partial.retain(|event| event["type"] != "response.output_item.done");
    cases.push((
        partial,
        ErrorKind::Malformed,
        Some(ResponseRejection::TextLocation),
    ));
    let mut delta_only = original.clone();
    delta_only.retain(|event| {
        !matches!(
            event["type"].as_str(),
            Some("response.output_item.added" | "response.output_item.done")
        )
    });
    cases.push((
        delta_only,
        ErrorKind::Malformed,
        Some(ResponseRejection::TextIdentity),
    ));
    for (field, value, reason) in [
        (
            "jobId",
            json!(Uuid::new_v4()),
            ResponseRejection::JobMismatch,
        ),
        (
            "requirementsSha256",
            json!("0".repeat(64)),
            ResponseRejection::RequirementsMismatch,
        ),
        (
            "summary",
            json!(format!("Private credential {KEY}")),
            ResponseRejection::CredentialEcho,
        ),
    ] {
        let mut events = original.clone();
        events.retain(|event| {
            !event["type"]
                .as_str()
                .is_some_and(|kind| kind.starts_with("response.output_text."))
        });
        let mut candidate_value: Value = serde_json::from_str(&candidate(&frozen)).unwrap();
        candidate_value[field] = value;
        let item = events
            .iter_mut()
            .find(|event| {
                event["type"] == "response.output_item.done" && event["output_index"] == 1
            })
            .unwrap();
        item["item"]["content"][0]["text"] =
            json!(serde_json::to_string(&candidate_value).unwrap());
        cases.push((events, ErrorKind::Malformed, Some(reason)));
    }
    let mut invalid_json = original.clone();
    invalid_json.retain(|event| {
        !event["type"]
            .as_str()
            .is_some_and(|kind| kind.starts_with("response.output_text."))
    });
    invalid_json
        .iter_mut()
        .find(|event| event["type"] == "response.output_item.done" && event["output_index"] == 1)
        .unwrap()["item"]["content"][0]["text"] = json!("Private invalid JSON");
    cases.push((
        invalid_json,
        ErrorKind::Malformed,
        Some(ResponseRejection::CandidateJson),
    ));
    let mut refused = original.clone();
    refused.retain(|event| {
        !event["type"]
            .as_str()
            .is_some_and(|kind| kind.starts_with("response.output_text."))
    });
    refused
        .iter_mut()
        .find(|event| event["type"] == "response.output_item.done" && event["output_index"] == 1)
        .unwrap()["item"]["content"] = json!([{"type":"refusal","refusal":"Private refusal"}]);
    cases.push((refused, ErrorKind::Refused, None));
    let mut no_terminal = original.clone();
    no_terminal.pop();
    cases.push((no_terminal, ErrorKind::Truncated, None));
    for (kind, expected) in [
        ("response.failed", ErrorKind::RequestRejected),
        ("response.incomplete", ErrorKind::Incomplete),
    ] {
        let mut events = original.clone();
        events.last_mut().unwrap()["type"] = json!(kind);
        events.last_mut().unwrap()["response"]["status"] = json!(if kind == "response.failed" {
            "failed"
        } else {
            "incomplete"
        });
        cases.push((events, expected, None));
    }
    for authentication in [AuthenticationMethod::ApiKey, AuthenticationMethod::Chatgpt] {
        for (events, kind, reason) in &cases {
            let error = generate_events(&frozen, events.clone(), authentication)
                .await
                .unwrap_err();
            assert_eq!(error.kind, *kind);
            assert_eq!(
                error
                    .response_format
                    .as_ref()
                    .and_then(|format| format.rejection),
                *reason
            );
            let serialized = serde_json::to_string(&error).unwrap();
            assert!(!serialized.contains(KEY));
            if *reason != Some(ResponseRejection::CandidateJson) {
                assert!(!serialized.contains("Private"));
            } else {
                assert!(
                    error
                        .response_format
                        .as_ref()
                        .unwrap()
                        .diagnostic_reply
                        .as_ref()
                        .unwrap()
                        .contains("Private invalid JSON")
                );
            }
        }
    }
}

#[tokio::test]
async fn sparse_wire_indices_correlate_with_compact_completed_messages_by_identity() {
    let frozen = snapshot(GenerationMode::Strict);
    for authentication in [AuthenticationMethod::ApiKey, AuthenticationMethod::Chatgpt] {
        let bytes: Vec<u8> = sparse_stream_events(&frozen)
            .into_iter()
            .flat_map(frame)
            .collect();
        let mut reply = Reply::sse(bytes);
        reply.content_type = None;
        let server = Server::start(reply).await;
        let response = server
            .client()
            .generate(
                crate::GenerationRequest {
                    authentication,
                    snapshot: &frozen,
                    mode: GenerationMode::Strict,
                    schema: &schema(),
                    settings: &ProviderSettings::default(),
                    key: &ApiKey::new(KEY.into()).unwrap(),
                    cancellation: &CancellationToken::new(),
                },
                |_| {},
            )
            .await
            .unwrap();
        assert_eq!(response.text, candidate(&frozen));
        assert_eq!(response.usage.unwrap().total_tokens, 300);
        assert_eq!(server.count.load(Ordering::SeqCst), 1);
    }
}

async fn generate_events(
    snapshot: &RequirementsSnapshot,
    events: Vec<Value>,
    authentication: AuthenticationMethod,
) -> Result<ProviderResponse> {
    let mut reply = Reply::sse(events.into_iter().flat_map(frame).collect());
    reply.content_type = None;
    let server = Server::start(reply).await;
    let result = server
        .client()
        .generate(
            crate::GenerationRequest {
                authentication,
                snapshot,
                mode: GenerationMode::Strict,
                schema: &schema(),
                settings: &ProviderSettings::default(),
                key: &ApiKey::new(KEY.into()).unwrap(),
                cancellation: &CancellationToken::new(),
            },
            |_| {},
        )
        .await;
    assert_eq!(server.count.load(Ordering::SeqCst), 1);
    result
}

#[tokio::test]
async fn completed_items_match_by_identity_when_reordered_compacted_or_omitted() {
    let frozen = snapshot(GenerationMode::Strict);
    let original = sparse_stream_events(&frozen);
    let mut cases = vec![];
    for positions in [vec![2, 1, 0], vec![1, 2], vec![2], vec![0], vec![]] {
        let mut events = original.clone();
        let output = events.last().unwrap()["response"]["output"].clone();
        events.last_mut().unwrap()["response"]["output"] = Value::Array(
            positions
                .into_iter()
                .map(|index| output[index].clone())
                .collect(),
        );
        cases.push(events);
    }
    for absent in [false, true] {
        let mut events = original.clone();
        if absent {
            events.last_mut().unwrap()["response"]
                .as_object_mut()
                .unwrap()
                .remove("output");
        } else {
            events.last_mut().unwrap()["response"]["output"] = Value::Null;
        }
        cases.push(events);
    }
    let mut inferred = original.clone();
    for event in &mut inferred {
        if event["type"]
            .as_str()
            .is_some_and(|kind| kind.starts_with("response.output_text."))
        {
            event.as_object_mut().unwrap().remove("item_id");
        }
    }
    cases.push(inferred);
    let mut done_only = original.clone();
    done_only.retain(|event| event["type"] != "response.output_item.added");
    cases.push(done_only);
    let mut terminal_finishes_messages = original.clone();
    terminal_finishes_messages.retain(|event| {
        !(event["type"] == "response.output_item.done" && event["item"]["type"] == "message")
    });
    cases.push(terminal_finishes_messages);
    for authentication in [AuthenticationMethod::ApiKey, AuthenticationMethod::Chatgpt] {
        for events in &cases {
            let response = generate_events(&frozen, events.clone(), authentication)
                .await
                .unwrap();
            assert_eq!(response.text, candidate(&frozen));
            assert_eq!(response.usage.unwrap().total_tokens, 300);
        }
    }
}

#[tokio::test]
async fn finalized_item_correlations_reject_missing_changed_duplicate_and_unfinished_content() {
    let frozen = snapshot(GenerationMode::Strict);
    let original = sparse_stream_events(&frozen);
    let mut cases = vec![];
    let mut empty = original.clone();
    empty.last_mut().unwrap()["response"]["output"] = json!([]);
    empty.retain(|event| {
        !(event["type"] == "response.output_item.done" && event["output_index"] == 9)
    });
    cases.push((
        empty,
        ResponseRejection::TextLocation,
        Some(CorrelationCause::UnfinishedItem),
    ));
    let mut duplicate = original.clone();
    let output = &mut duplicate.last_mut().unwrap()["response"]["output"];
    let repeated = output[2].clone();
    output.as_array_mut().unwrap().push(repeated);
    cases.push((
        duplicate,
        ResponseRejection::TextIdentity,
        Some(CorrelationCause::ItemDuplicate),
    ));
    let mut wrong_part = original.clone();
    for event in &mut wrong_part {
        if event["output_index"] == 9 && event["content_index"] == 1 {
            event["content_index"] = json!(7);
        }
    }
    cases.push((
        wrong_part,
        ResponseRejection::TextLocation,
        Some(CorrelationCause::ContentPartAbsent),
    ));
    let mut wrong_done = original.clone();
    for event in &mut wrong_done {
        if event["type"] == "response.output_item.done" && event["output_index"] == 5 {
            event["item"]["content"][0]["text"] = json!("Private changed commentary");
        }
    }
    let final_message = wrong_done.last().unwrap()["response"]["output"][2].clone();
    wrong_done.last_mut().unwrap()["response"]["output"] = json!([final_message]);
    cases.push((
        wrong_done,
        ResponseRejection::TextMismatch,
        Some(CorrelationCause::ItemChanged),
    ));
    let mut changed_id = original.clone();
    for event in &mut changed_id {
        if event["type"] == "response.output_item.done" && event["output_index"] == 9 {
            event["item"]["id"] = json!("msg_changed_private");
        }
    }
    cases.push((
        changed_id,
        ResponseRejection::TextIdentity,
        Some(CorrelationCause::IdentityChanged),
    ));
    let mut changed_phase = original.clone();
    changed_phase.last_mut().unwrap()["response"]["output"][2]["phase"] = json!("commentary");
    cases.push((
        changed_phase,
        ResponseRejection::TextMismatch,
        Some(CorrelationCause::ItemChanged),
    ));
    let mut unfinished = original.clone();
    unfinished.retain(|event| {
        !(event["type"] == "response.output_item.done" && event["output_index"] == 9)
    });
    unfinished.last_mut().unwrap()["response"]["output"] = Value::Null;
    cases.push((
        unfinished,
        ResponseRejection::OutputShape,
        Some(CorrelationCause::UnfinishedItem),
    ));
    let mut duplicate_index = original.clone();
    for event in &mut duplicate_index {
        if event["type"] == "response.output_item.added" && event["output_index"] == 9 {
            event["output_index"] = json!(5);
        }
    }
    cases.push((
        duplicate_index,
        ResponseRejection::TextIdentity,
        Some(CorrelationCause::IdentityChanged),
    ));
    let mut duplicate_id = original.clone();
    for event in &mut duplicate_id {
        if event["type"] == "response.output_item.added" && event["output_index"] == 9 {
            event["item"]["id"] = json!("msg_commentary");
        }
    }
    cases.push((
        duplicate_id,
        ResponseRejection::TextIdentity,
        Some(CorrelationCause::ItemDuplicate),
    ));
    let mut wrong_wire_index = original.clone();
    wrong_wire_index.retain(|event| {
        !(event["type"] == "response.output_item.done" && event["item"]["type"] == "message")
    });
    for event in &mut wrong_wire_index {
        if event["type"]
            .as_str()
            .is_some_and(|kind| kind.starts_with("response.output_text."))
            && event["output_index"] == 9
        {
            event["output_index"] = json!(13);
        }
    }
    cases.push((
        wrong_wire_index,
        ResponseRejection::TextIdentity,
        Some(CorrelationCause::IdentityChanged),
    ));
    let mut no_identity = original.clone();
    for event in &mut no_identity {
        if event["type"] == "response.output_item.done" && event["output_index"] == 9 {
            event["item"].as_object_mut().unwrap().remove("id");
        }
    }
    cases.push((
        no_identity,
        ResponseRejection::TextIdentity,
        Some(CorrelationCause::MissingIdentity),
    ));
    let mut invalid_index = original.clone();
    invalid_index[2]["output_index"] = json!(-1);
    cases.push((
        invalid_index,
        ResponseRejection::TextLocation,
        Some(CorrelationCause::InvalidIndex),
    ));
    for (field, value, reason) in [
        (
            "status",
            json!("in_progress"),
            ResponseRejection::MessageState,
        ),
        ("phase", json!("unknown"), ResponseRejection::MessagePhase),
        ("role", json!("user"), ResponseRejection::MessageRole),
    ] {
        let mut invalid_done = original.clone();
        for event in &mut invalid_done {
            if event["type"] == "response.output_item.done" && event["output_index"] == 9 {
                event["item"][field] = value.clone();
            }
        }
        cases.push((invalid_done, reason, None));
    }
    for authentication in [AuthenticationMethod::ApiKey, AuthenticationMethod::Chatgpt] {
        for (events, reason, cause) in &cases {
            let error = generate_events(&frozen, events.clone(), authentication)
                .await
                .unwrap_err();
            let format = error.response_format.as_ref().unwrap();
            assert_eq!(format.rejection, Some(*reason));
            assert_eq!(format.correlation.as_ref().map(|value| value.cause), *cause);
            let serialized = serde_json::to_string(&error).unwrap();
            for private in [
                KEY,
                "Private changed commentary",
                "msg_changed_private",
                "msg_final",
                "msg_commentary",
            ] {
                assert!(!serialized.contains(private));
            }
            assert_eq!(
                serde_json::from_str::<ProviderError>(&serialized).unwrap(),
                error
            );
        }
    }
}

#[tokio::test]
async fn finalized_items_never_replace_a_successful_terminal_event() {
    let frozen = snapshot(GenerationMode::Strict);
    let mut events = sparse_stream_events(&frozen);
    events.pop();
    for authentication in [AuthenticationMethod::ApiKey, AuthenticationMethod::Chatgpt] {
        let error = generate_events(&frozen, events.clone(), authentication)
            .await
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Truncated);
        assert_eq!(error.outcome, SubmissionOutcome::Unknown);
    }
}

#[tokio::test]
async fn identityless_legacy_text_requires_one_unambiguous_completed_part() {
    let frozen = snapshot(GenerationMode::Strict);
    let text = candidate(&frozen);
    let mut complete = completed(&text);
    complete["response"]["output"][1]
        .as_object_mut()
        .unwrap()
        .remove("id");
    let delta = json!({"type":"response.output_text.delta","delta":text});
    for authentication in [AuthenticationMethod::ApiKey, AuthenticationMethod::Chatgpt] {
        let response = generate_events(
            &frozen,
            vec![delta.clone(), complete.clone()],
            authentication,
        )
        .await
        .unwrap();
        assert_eq!(response.text, text);
        let mut positioned = delta.clone();
        positioned["output_index"] = json!(1);
        assert_eq!(
            generate_events(
                &frozen,
                vec![positioned.clone(), complete.clone()],
                authentication
            )
            .await
            .unwrap()
            .text,
            text
        );
        positioned["output_index"] = json!(9);
        let error = generate_events(&frozen, vec![positioned, complete.clone()], authentication)
            .await
            .unwrap_err();
        assert_eq!(
            error.response_format.unwrap().correlation.unwrap().cause,
            CorrelationCause::MissingIdentity
        );
        let added = json!({"type":"response.output_item.added","output_index":5,"item":{"id":"msg_bound","type":"message","role":"assistant","status":"in_progress","content":[]}});
        let mut terminal = complete.clone();
        terminal["response"]["output"][1]["id"] = json!("msg_bound");
        let mut unidentified = delta.clone();
        unidentified["output_index"] = json!(9);
        let error = generate_events(&frozen, vec![added, unidentified, terminal], authentication)
            .await
            .unwrap_err();
        assert_eq!(
            error.response_format.unwrap().correlation.unwrap().cause,
            CorrelationCause::MissingIdentity
        );
        let mut ambiguous = complete.clone();
        let mut commentary = ambiguous["response"]["output"][1].clone();
        commentary["phase"] = json!("commentary");
        ambiguous["response"]["output"]
            .as_array_mut()
            .unwrap()
            .push(commentary);
        let error = generate_events(&frozen, vec![delta.clone(), ambiguous], authentication)
            .await
            .unwrap_err();
        assert_eq!(
            error.response_format.unwrap().correlation.unwrap().cause,
            CorrelationCause::MissingIdentity
        );
    }
}

#[tokio::test]
async fn completed_commentary_and_split_final_answer_select_only_the_verified_candidate() {
    let frozen = snapshot(GenerationMode::Strict);
    for with_deltas in [false, true] {
        let server = Server::start(Reply::sse(phased_stream(&frozen, with_deltas))).await;
        let response = generate(&server, &frozen).await.unwrap();
        assert_eq!(response.text, candidate(&frozen));
        assert_eq!(response.usage.unwrap().total_tokens, 300);
        assert_eq!(server.count.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn completion_rejections_identify_the_failed_check_without_retaining_output() {
    let frozen = snapshot(GenerationMode::Strict);
    let valid = completed(&candidate(&frozen));
    let mut cases = vec![];
    for (field, value, reason) in [
        ("role", json!("user"), ResponseRejection::MessageRole),
        (
            "status",
            json!("in_progress"),
            ResponseRejection::MessageState,
        ),
        (
            "phase",
            json!("unrecognized"),
            ResponseRejection::MessagePhase,
        ),
        (
            "content",
            json!([{ "type":"image", "data":"Private source" }]),
            ResponseRejection::ContentShape,
        ),
    ] {
        let mut value_response = valid.clone();
        value_response["response"]["output"][1][field] = value;
        cases.push((value_response, reason));
    }
    for text in ["not JSON", "[]"] {
        cases.push((
            completed(text),
            if text == "[]" {
                ResponseRejection::CandidateShape
            } else {
                ResponseRejection::CandidateJson
            },
        ));
    }
    for (field, value, reason) in [
        (
            "jobId",
            json!(Uuid::new_v4()),
            ResponseRejection::JobMismatch,
        ),
        (
            "requirementsSha256",
            json!("0".repeat(64)),
            ResponseRejection::RequirementsMismatch,
        ),
        (
            "summary",
            json!(format!("Never disclose {KEY}")),
            ResponseRejection::CredentialEcho,
        ),
    ] {
        let mut value_candidate: Value = serde_json::from_str(&candidate(&frozen)).unwrap();
        value_candidate[field] = value;
        cases.push((
            completed(&serde_json::to_string(&value_candidate).unwrap()),
            reason,
        ));
    }
    for phase in [None, Some("final_answer")] {
        let mut duplicate = valid.clone();
        let mut message = duplicate["response"]["output"][1].clone();
        if let Some(phase) = phase {
            message["phase"] = json!(phase);
        }
        let mut other = message.clone();
        other["id"] = json!("msg_second_answer");
        duplicate["response"]["output"] = json!([message, other]);
        cases.push((duplicate, ResponseRejection::AnswerCount));
    }
    let mut commentary_only = valid.clone();
    commentary_only["response"]["output"][1]["phase"] = json!("commentary");
    cases.push((commentary_only, ResponseRejection::AnswerCount));
    let mut missing = valid.clone();
    missing["response"]
        .as_object_mut()
        .unwrap()
        .remove("output");
    cases.push((missing, ResponseRejection::OutputShape));
    let mut usage = valid.clone();
    usage["response"]["usage"]["total_tokens"] = json!(1);
    cases.push((usage, ResponseRejection::Usage));
    for authentication in [AuthenticationMethod::ApiKey, AuthenticationMethod::Chatgpt] {
        for (value, reason) in &cases {
            let server = Server::start(Reply::sse(frame(value.clone()))).await;
            let error = server
                .client()
                .generate(
                    crate::GenerationRequest {
                        authentication,
                        snapshot: &frozen,
                        mode: GenerationMode::Strict,
                        schema: &schema(),
                        settings: &ProviderSettings::default(),
                        key: &ApiKey::new(KEY.into()).unwrap(),
                        cancellation: &CancellationToken::new(),
                    },
                    |_| {},
                )
                .await
                .unwrap_err();
            assert_eq!(error.kind, ErrorKind::Malformed);
            assert_eq!(error.outcome, SubmissionOutcome::Received);
            assert_eq!(
                error.response_format.as_ref().unwrap().rejection,
                Some(*reason)
            );
            assert!(error.response_format.as_ref().unwrap().body_bytes > 0);
            assert!(!format!("{error:?} {error}").contains(KEY));
            assert!(
                !serde_json::to_string(&error)
                    .unwrap()
                    .contains("Private source")
            );
            assert!(
                !error
                    .to_string()
                    .contains("invalid or inconsistent structured")
            );
            assert_eq!(server.count.load(Ordering::SeqCst), 1);
        }
    }
}

#[tokio::test]
async fn tracked_parts_validate_independently_including_intermediate_commentary() {
    let frozen = snapshot(GenerationMode::Strict);
    let frames = crate::sse::SseDecoder::default()
        .push(&phased_stream(&frozen, true))
        .unwrap();
    let values: Vec<Value> = frames
        .iter()
        .map(|frame| serde_json::from_str(&frame.data).unwrap())
        .collect();
    let mut reordered = vec![];
    for index in [0, 5, 1, 3, 6, 2, 4, 7] {
        reordered.extend(frame(values[index].clone()));
    }
    let server = Server::start(Reply::sse(reordered)).await;
    assert_eq!(
        generate(&server, &frozen).await.unwrap().text,
        candidate(&frozen)
    );

    let mut cases = vec![];
    let mut mismatch = values.clone();
    mismatch[7]["response"]["output"][1]["content"][0]["text"] =
        json!("Changed intermediate update");
    cases.push((mismatch, ResponseRejection::TextMismatch));
    let mut identity = values.clone();
    identity[7]["response"]["output"][2]["id"] = json!("msg_other");
    cases.push((identity, ResponseRejection::TextIdentity));
    let mut wrong_part = values.clone();
    wrong_part[5]["content_index"] = json!(5);
    wrong_part[6]["content_index"] = json!(5);
    cases.push((wrong_part, ResponseRejection::TextLocation));
    let mut after_done = values.clone();
    after_done.insert(3, values[1].clone());
    cases.push((after_done, ResponseRejection::TextOrder));
    let mut wrong_done = values.clone();
    wrong_done[2]["text"] = json!("Different completed update");
    cases.push((wrong_done, ResponseRejection::TextMismatch));
    let mut changed_id = values.clone();
    changed_id[4]["item_id"] = json!("msg_other");
    cases.push((changed_id, ResponseRejection::TextIdentity));
    for (events, reason) in cases {
        let bytes: Vec<u8> = events.into_iter().flat_map(frame).collect();
        let server = Server::start(Reply::sse(bytes)).await;
        let error = generate(&server, &frozen).await.unwrap_err();
        assert_eq!(
            error.response_format.as_ref().unwrap().rejection,
            Some(reason)
        );
        assert_eq!(server.count.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn commentary_and_final_parts_share_the_existing_output_bound() {
    let frozen = snapshot(GenerationMode::Strict);
    let mut value = completed(&candidate(&frozen));
    let message = value["response"]["output"][1].clone();
    let mut commentary = message.clone();
    commentary["id"] = json!("msg_large_commentary");
    commentary["phase"] = json!("commentary");
    commentary["content"][0]["text"] = json!("x".repeat(MAX_OUTPUT_BYTES / 2));
    let mut final_answer = message;
    final_answer["phase"] = json!("final_answer");
    final_answer["content"][0]["text"] = json!("x".repeat(MAX_OUTPUT_BYTES / 2 + 1));
    value["response"]["output"] = json!([commentary, final_answer]);
    let mut reply = Reply::sse(vec![]);
    reply.chunks = vec![frame(value)];
    let server = Server::start(reply).await;
    let error = generate(&server, &frozen).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::OutputLimit);
    assert_eq!(
        error.response_format.as_ref().unwrap().limit_bytes,
        Some(MAX_OUTPUT_BYTES)
    );
    assert_eq!(server.count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn credentials_split_between_final_text_parts_are_still_rejected() {
    let frozen = snapshot(GenerationMode::Strict);
    let mut candidate_value: Value = serde_json::from_str(&candidate(&frozen)).unwrap();
    candidate_value["summary"] = json!(format!("Never disclose {KEY}"));
    let text = serde_json::to_string(&candidate_value).unwrap();
    let boundary = text.find(KEY).unwrap() + KEY.len() / 2;
    let mut value = completed(&text);
    value["response"]["output"][1]["phase"] = json!("final_answer");
    value["response"]["output"][1]["content"] = json!([
        {"type":"output_text","text":&text[..boundary],"annotations":[]},
        {"type":"output_text","text":&text[boundary..],"annotations":[]}
    ]);
    for authentication in [AuthenticationMethod::ApiKey, AuthenticationMethod::Chatgpt] {
        let server = Server::start(Reply::sse(frame(value.clone()))).await;
        let error = server
            .client()
            .generate(
                crate::GenerationRequest {
                    authentication,
                    snapshot: &frozen,
                    mode: GenerationMode::Strict,
                    schema: &schema(),
                    settings: &ProviderSettings::default(),
                    key: &ApiKey::new(KEY.into()).unwrap(),
                    cancellation: &CancellationToken::new(),
                },
                |_| {},
            )
            .await
            .unwrap_err();
        assert_eq!(
            error.response_format.as_ref().unwrap().rejection,
            Some(ResponseRejection::CredentialEcho)
        );
        assert!(!format!("{error:?} {error}").contains(KEY));
        assert_eq!(server.count.load(Ordering::SeqCst), 1);
    }
}

async fn generate(server: &Server, snapshot: &RequirementsSnapshot) -> Result<ProviderResponse> {
    server
        .client()
        .generate(
            crate::GenerationRequest {
                authentication: AuthenticationMethod::ApiKey,
                snapshot,
                mode: GenerationMode::Strict,
                schema: &schema(),
                settings: &ProviderSettings::default(),
                key: &ApiKey::new(KEY.into()).unwrap(),
                cancellation: &CancellationToken::new(),
            },
            |_| {},
        )
        .await
}

#[tokio::test]
async fn responses_adapter_contract_through_a_real_loopback_http_server() {
    for mode in [GenerationMode::Fuzzy, GenerationMode::Strict] {
        let frozen = snapshot(mode);
        let server = Server::start(Reply::sse(stream(&frozen))).await;
        let progress = Mutex::new(vec![]);
        let response = server
            .client()
            .generate(
                crate::GenerationRequest {
                    authentication: AuthenticationMethod::ApiKey,
                    snapshot: &frozen,
                    mode,
                    schema: &schema(),
                    settings: &ProviderSettings::default(),
                    key: &ApiKey::new(KEY.into()).unwrap(),
                    cancellation: &CancellationToken::new(),
                },
                |value| progress.lock().unwrap().push(value),
            )
            .await
            .unwrap();
        assert_eq!(response.text, candidate(&frozen));
        assert_eq!(response.response_id, "resp_test_123");
        assert_eq!(
            response.usage,
            Some(TokenUsage {
                input_tokens: 211,
                output_tokens: 89,
                total_tokens: 300
            })
        );
        let request = server.request();
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/v1/responses");
        assert!(
            request
                .headers
                .to_lowercase()
                .contains(&format!("authorization: bearer {}", KEY.to_lowercase()))
        );
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(body["model"], MODEL_ID);
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
        assert_eq!(body["service_tier"], "default");
        assert_eq!(body["max_output_tokens"], DEFAULT_MAX_OUTPUT_TOKENS);
        assert_eq!(body["reasoning"]["effort"], "medium");
        assert_eq!(body["tools"], json!([]));
        assert_eq!(body["tool_choice"], "none");
        let content = body["input"][0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "input_file");
        assert_eq!(content[0]["filename"], "REQUIREMENTS.md");
        let attachment = content[0]["file_data"]
            .as_str()
            .unwrap()
            .strip_prefix("data:text/markdown;base64,")
            .unwrap();
        assert_eq!(
            STANDARD.decode(attachment).unwrap(),
            frozen.markdown.as_bytes()
        );
        assert_eq!(digest(&STANDARD.decode(attachment).unwrap()), frozen.sha256);
        let metadata: Value = serde_json::from_str(content[1]["text"].as_str().unwrap()).unwrap();
        assert_eq!(
            metadata,
            json!({"jobId":frozen.job_id,"requirementsSha256":frozen.sha256,"generationMode":mode})
        );
        assert_eq!(body["text"]["format"], wire_format(&frozen));
        assert!(
            !request
                .body
                .windows(KEY.len())
                .any(|bytes| bytes == KEY.as_bytes())
        );
        let updates = progress.lock().unwrap();
        assert_eq!(updates[0].received_bytes, 0);
        assert!(updates.last().unwrap().received_bytes > 0);
        assert_eq!(
            updates.last().unwrap().response_id.as_deref(),
            Some("resp_test_123")
        );
        assert!(
            updates
                .windows(2)
                .all(|updates| updates[0].received_bytes <= updates[1].received_bytes)
        );
    }
}

#[tokio::test]
async fn refusal_and_incomplete_responses_never_become_candidates_and_keep_usage() {
    let frozen = snapshot(GenerationMode::Strict);
    let mut refusal = completed(&candidate(&frozen));
    refusal["response"]["output"] = json!([{"type":"message","role":"assistant","content":[{"type":"refusal","refusal":format!("Private {KEY} must never appear in diagnostics")}]}]);
    let incomplete = json!({"type":"response.incomplete","response":{"id":"resp_test_123","status":"incomplete",
        "incomplete_details":{"reason":"max_output_tokens"},"usage":{"input_tokens":211,"output_tokens":89,"total_tokens":300}}});
    for (event, kind) in [
        (refusal, ErrorKind::Refused),
        (incomplete, ErrorKind::Incomplete),
    ] {
        let server = Server::start(Reply::sse(frame(event))).await;
        let error = generate(&server, &frozen).await.unwrap_err();
        assert_eq!(error.kind, kind);
        assert_eq!(error.outcome, SubmissionOutcome::Received);
        assert_eq!(error.response_id.as_deref(), Some("resp_test_123"));
        assert_eq!(error.usage.unwrap().total_tokens, 300);
        assert!(!format!("{error:?} {error}").contains(KEY));
    }
}

#[tokio::test]
async fn malformed_json_mismatched_stream_job_hash_and_usage_are_rejected() {
    let frozen = snapshot(GenerationMode::Strict);
    let mut wrong_usage = completed(&candidate(&frozen));
    wrong_usage["response"]["usage"]["total_tokens"] = json!(299);
    let mut excess_usage = completed(&candidate(&frozen));
    excess_usage["response"]["usage"] =
        json!({"input_tokens":1,"output_tokens":32769,"total_tokens":32770});
    let mut wrong_job: Value = serde_json::from_str(&candidate(&frozen)).unwrap();
    wrong_job["jobId"] = json!(Uuid::new_v4());
    let mut wrong_hash: Value = serde_json::from_str(&candidate(&frozen)).unwrap();
    wrong_hash["requirementsSha256"] = json!("0".repeat(64));
    let mut inconsistent =
        frame(json!({"type":"response.output_text.delta","delta":"different text"}));
    inconsistent.extend(frame(completed(&candidate(&frozen))));
    for (bytes, kind) in [
        (
            b"event: response.completed\ndata: {bad json}\n\n".to_vec(),
            ErrorKind::Malformed,
        ),
        (frame(completed("not JSON")), ErrorKind::Malformed),
        (frame(completed("[]")), ErrorKind::Malformed),
        (
            frame(completed(&wrong_job.to_string())),
            ErrorKind::Malformed,
        ),
        (
            frame(completed(&wrong_hash.to_string())),
            ErrorKind::Malformed,
        ),
        (inconsistent, ErrorKind::Malformed),
        (frame(wrong_usage), ErrorKind::Malformed),
        (frame(excess_usage), ErrorKind::OutputLimit),
    ] {
        let server = Server::start(Reply::sse(bytes)).await;
        assert_eq!(generate(&server, &frozen).await.unwrap_err().kind, kind);
    }
}

#[tokio::test]
async fn authentication_quota_rate_model_and_server_failures_have_redacted_typed_errors() {
    let frozen = snapshot(GenerationMode::Strict);
    for (status, code, kind, outcome) in [
        (
            401,
            "invalid_api_key",
            ErrorKind::Authentication,
            SubmissionOutcome::Rejected,
        ),
        (
            403,
            "permission_denied",
            ErrorKind::Authentication,
            SubmissionOutcome::Rejected,
        ),
        (
            429,
            "insufficient_quota",
            ErrorKind::Quota,
            SubmissionOutcome::Rejected,
        ),
        (
            429,
            "rate_limit_exceeded",
            ErrorKind::RateLimited,
            SubmissionOutcome::Rejected,
        ),
        (
            404,
            "model_not_found",
            ErrorKind::ModelUnavailable,
            SubmissionOutcome::Rejected,
        ),
        (
            400,
            "invalid_request",
            ErrorKind::RequestRejected,
            SubmissionOutcome::Rejected,
        ),
        (
            503,
            "server_error",
            ErrorKind::Server,
            SubmissionOutcome::Rejected,
        ),
    ] {
        let server = Server::start(Reply::json(
            status,
            json!({"error":{"code":code,"message":format!("DO NOT LEAK {KEY}"),"secret":KEY}}),
        ))
        .await;
        let error = generate(&server, &frozen).await.unwrap_err();
        assert_eq!(error.kind, kind);
        assert_eq!(error.outcome, outcome);
        assert!(
            !format!(
                "{error:?} {error} {}",
                serde_json::to_string(&error).unwrap()
            )
            .contains(KEY)
        );
        assert_eq!(
            server.count.load(Ordering::SeqCst),
            if status == 429 || status >= 500 { 3 } else { 1 }
        );
    }
}

#[tokio::test]
async fn json_error_before_a_stream_keeps_its_actual_code_and_request_id() {
    let frozen = snapshot(GenerationMode::Strict);
    let mut reply = Reply::json(
        200,
        json!({"error":{"code":"subscription_sharing_unsupported_capability","param":"text.format","message":KEY}}),
    );
    reply.headers = "x-request-id: req_json_admission\r\n".into();
    let server = Server::start(reply).await;
    let error = generate_plan(&server, &frozen, &ProviderSettings::default())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::ChatgptUnsupportedCapability);
    assert_eq!(error.outcome, SubmissionOutcome::Received);
    assert_eq!(error.http_status, Some(200));
    assert_eq!(
        error.code.as_deref(),
        Some("subscription_sharing_unsupported_capability")
    );
    assert_eq!(error.param.as_deref(), Some("text.format"));
    assert_eq!(error.request_id.as_deref(), Some("req_json_admission"));
    assert_eq!(
        error.response_format.as_ref().unwrap().body_kind,
        ResponseBodyKind::JsonError
    );
    assert_eq!(
        error
            .response_format
            .as_ref()
            .unwrap()
            .content_type
            .as_deref(),
        Some("application/json")
    );
    assert!(!serde_json::to_string(&error).unwrap().contains(KEY));
    assert_eq!(server.count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn streaming_media_type_is_case_insensitive() {
    let frozen = snapshot(GenerationMode::Strict);
    let mut reply = Reply::sse(stream(&frozen));
    reply.content_type = Some("Text/Event-Stream; charset=UTF-8");
    let server = Server::start(reply).await;
    assert_eq!(
        generate(&server, &frozen).await.unwrap().text,
        candidate(&frozen)
    );
    assert_eq!(server.count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_large_valid_stream_without_a_content_type_reaches_the_site_reader() {
    let frozen = snapshot(GenerationMode::Strict);
    let mut value: Value = serde_json::from_str(&candidate(&frozen)).unwrap();
    value["summary"] = json!("Controlled large stream. ".repeat(4_000));
    let text = value.to_string();
    assert!(text.len() > 65_536);
    let mut bytes = frame(json!({"type":"response.created","response":{"id":"resp_test_123"}}));
    bytes.extend(frame(
        json!({"type":"response.output_text.delta","output_index":1,"content_index":0,"item_id":"msg_test_123","delta":text}),
    ));
    bytes.extend(frame(
        json!({"type":"response.output_text.done","text":text}),
    ));
    bytes.extend(frame(completed(&text)));
    for authentication in [AuthenticationMethod::ApiKey, AuthenticationMethod::Chatgpt] {
        for content_type in [None, Some("text/plain"), Some("application/json")] {
            let mut reply = Reply::sse(bytes.clone());
            reply.content_type = content_type;
            reply.chunks = bytes.chunks(4096).map(<[u8]>::to_vec).collect();
            let server = Server::start(reply).await;
            let response = server
                .client()
                .generate(
                    crate::GenerationRequest {
                        authentication,
                        snapshot: &frozen,
                        mode: GenerationMode::Strict,
                        schema: &schema(),
                        settings: &ProviderSettings::default(),
                        key: &ApiKey::new(KEY.into()).unwrap(),
                        cancellation: &CancellationToken::new(),
                    },
                    |_| {},
                )
                .await
                .unwrap();
            assert_eq!(response.text, text);
            assert_eq!(response.response_id, "resp_test_123");
            assert_eq!(server.count.load(Ordering::SeqCst), 1);
        }
    }
}

#[tokio::test]
async fn fragmented_unlabeled_streams_accept_standard_prefixes_and_keep_validation() {
    let frozen = snapshot(GenerationMode::Strict);
    let text = candidate(&frozen);
    for mut bytes in [
        b"\xef\xbb\xbf".to_vec(),
        format!("{}id\nretry: 1000\n: keepalive\n\n", "\r\n".repeat(3000)).into_bytes(),
    ] {
        bytes.extend(format!("data: {}\n\n", completed(&text)).into_bytes());
        let mut reply = Reply::sse(bytes.clone());
        reply.content_type = None;
        reply.chunks = bytes.chunks(1).map(<[u8]>::to_vec).collect();
        let server = Server::start(reply).await;
        assert_eq!(generate(&server, &frozen).await.unwrap().text, text);
        assert_eq!(server.count.load(Ordering::SeqCst), 1);
    }
    let mut wrong: Value = serde_json::from_str(&text).unwrap();
    wrong["jobId"] = json!(Uuid::new_v4());
    let mut truncated = frame(json!({"type":"response.created","response":{"id":"resp_test_123"}}));
    truncated.extend(frame(
        json!({"type":"response.output_text.delta","delta":text}),
    ));
    for (bytes, kind, outcome) in [
        (
            frame(completed(&wrong.to_string())),
            ErrorKind::Malformed,
            SubmissionOutcome::Received,
        ),
        (truncated, ErrorKind::Truncated, SubmissionOutcome::Unknown),
        (
            frame(completed(
                "<html>Unwrapped HTML is not a structured candidate</html>",
            )),
            ErrorKind::Malformed,
            SubmissionOutcome::Received,
        ),
    ] {
        let mut reply = Reply::sse(bytes);
        reply.content_type = None;
        let server = Server::start(reply).await;
        let error = generate(&server, &frozen).await.unwrap_err();
        assert_eq!(error.kind, kind);
        assert_eq!(error.outcome, outcome);
        assert_eq!(
            error.response_format.as_ref().unwrap().body_kind,
            ResponseBodyKind::EventStream
        );
        assert!(error.response_format.as_ref().unwrap().body_bytes > 0);
        assert_eq!(server.count.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn non_stream_bodies_are_bounded_diagnostics_and_never_candidates() {
    let frozen = snapshot(GenerationMode::Strict);
    let cases = [
        (
            "application/json",
            serde_json::to_vec(&json!({"detail": KEY})).unwrap(),
            ResponseBodyKind::JsonDetail,
            ErrorKind::RequestRejected,
            SubmissionOutcome::Received,
        ),
        (
            "application/json",
            serde_json::to_vec(&json!({"object":"response","status":"completed","output":KEY}))
                .unwrap(),
            ResponseBodyKind::JsonResponse,
            ErrorKind::UnexpectedResponseFormat,
            SubmissionOutcome::Unknown,
        ),
        (
            "application/json",
            serde_json::to_vec(&json!({"other": KEY})).unwrap(),
            ResponseBodyKind::JsonOther,
            ErrorKind::UnexpectedResponseFormat,
            SubmissionOutcome::Unknown,
        ),
        (
            "text/html; charset=utf-8",
            format!("<!doctype html><html>{KEY}</html>").into_bytes(),
            ResponseBodyKind::Html,
            ErrorKind::UnexpectedResponseFormat,
            SubmissionOutcome::Unknown,
        ),
        (
            "text/plain",
            KEY.as_bytes().to_vec(),
            ResponseBodyKind::Text,
            ErrorKind::UnexpectedResponseFormat,
            SubmissionOutcome::Unknown,
        ),
        (
            "",
            vec![],
            ResponseBodyKind::Empty,
            ErrorKind::UnexpectedResponseFormat,
            SubmissionOutcome::Unknown,
        ),
        (
            KEY,
            vec![0xff],
            ResponseBodyKind::Binary,
            ErrorKind::UnexpectedResponseFormat,
            SubmissionOutcome::Unknown,
        ),
    ];
    for (content_type, body, body_kind, kind, outcome) in cases {
        for authentication in [AuthenticationMethod::ApiKey, AuthenticationMethod::Chatgpt] {
            let mut reply = Reply::json(200, json!({}));
            reply.content_type = Some(content_type);
            reply.chunks = vec![body.clone()];
            reply.headers = "x-request-id: req_wrong_format\r\n".into();
            let server = Server::start(reply).await;
            let error = match authentication {
                AuthenticationMethod::ApiKey => generate(&server, &frozen).await,
                AuthenticationMethod::Chatgpt => {
                    generate_plan(&server, &frozen, &ProviderSettings::default()).await
                }
            }
            .unwrap_err();
            assert_eq!(error.kind, kind);
            assert_eq!(error.outcome, outcome);
            assert_eq!(error.http_status, Some(200));
            assert_eq!(error.request_id.as_deref(), Some("req_wrong_format"));
            let format = error.response_format.as_ref().unwrap();
            assert_eq!(format.body_kind, body_kind);
            assert_eq!(format.body_bytes, body.len());
            if content_type.is_empty() || content_type == KEY {
                assert_eq!(format.content_type, None);
            }
            let saved = serde_json::to_string(&error).unwrap();
            assert!(!saved.contains(KEY));
            assert!(!saved.contains("<!doctype"));
            assert_eq!(server.count.load(Ordering::SeqCst), 1);
        }
    }
    let mut reply = Reply::json(200, json!({}));
    reply.chunks = vec![vec![b'x'; 65_537]];
    let server = Server::start(reply).await;
    let error = generate(&server, &frozen).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::UnexpectedResponseFormat);
    assert_eq!(error.http_status, Some(200));
    let format = error.response_format.unwrap();
    assert_eq!(format.body_kind, ResponseBodyKind::Text);
    assert!(format.body_truncated);
    assert_eq!(format.limit_bytes, Some(65_536));
    assert!(format.body_bytes > 65_536);
    assert_eq!(server.count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn provider_explanations_without_codes_are_useful_bounded_and_credential_safe() {
    let frozen = snapshot(GenerationMode::Strict);
    for (status, body, explanation) in [
        (
            200,
            json!({"error":{"code":null,"message":"  REQUIREMENTS.md was not accepted.\n Check the file type.  "}}),
            "REQUIREMENTS.md was not accepted. Check the file type.",
        ),
        (
            403,
            json!({"detail":"This serving region is unavailable for the selected account."}),
            "This serving region is unavailable for the selected account.",
        ),
    ] {
        let server = Server::start(Reply::json(status, body)).await;
        let error = generate_plan(&server, &frozen, &ProviderSettings::default())
            .await
            .unwrap_err();
        assert_eq!(error.code, None);
        assert_eq!(error.http_status, Some(status));
        assert_eq!(
            error
                .response_format
                .as_ref()
                .unwrap()
                .provider_message
                .as_deref(),
            Some(explanation)
        );
        assert_eq!(
            error.kind,
            if status == 200 {
                ErrorKind::RequestRejected
            } else {
                ErrorKind::Authentication
            }
        );
        let message = error.message();
        if status == 200 {
            assert_eq!(
                message,
                format!("The AI service rejected this request: {explanation}")
            );
        } else {
            assert!(!message.contains(explanation));
        }
        assert_eq!(server.count.load(Ordering::SeqCst), 1);
    }
    for explanation in [
        format!("Do not expose {KEY}"),
        "Bearer synthetic-opaque-token".into(),
        "Invalid key sk-synthetic-different-credential".into(),
        "Token eyJsynthetic.header.signature was not accepted".into(),
        "access_token=synthetic-opaque-token".into(),
        "refresh_token=synthetic-opaque-token".into(),
        "Untrusted\u{202e}direction".into(),
        "Control\u{0}character".into(),
        "x".repeat(1025),
    ] {
        let server =
            Server::start(Reply::json(200, json!({"error":{"message":explanation}}))).await;
        let error = generate_plan(&server, &frozen, &ProviderSettings::default())
            .await
            .unwrap_err();
        assert_eq!(
            error.response_format.as_ref().unwrap().provider_message,
            None
        );
        let diagnostic = serde_json::to_string(&error).unwrap();
        if explanation.len() <= 1024 {
            assert!(!diagnostic.contains(&explanation));
        }
        assert!(
            error
                .response_format
                .as_ref()
                .unwrap()
                .diagnostic_reply
                .as_ref()
                .is_none_or(|text| text.len() <= 64 * 1024)
        );
        assert!(!diagnostic.contains(KEY));
        assert_eq!(server.count.load(Ordering::SeqCst), 1);
        assert_eq!(
            error.message(),
            "The AI service rejected this request. Review its settings and requirements before explicitly retrying."
        );
    }
}

#[test]
fn a_rejected_reply_schema_is_reported_as_a_desegnos_bug() {
    let mut error = ProviderError::new(ErrorKind::RequestRejected, SubmissionOutcome::Received);
    error.code = Some("invalid_json_schema".into());
    error.param = Some("text.format.schema".into());
    assert_eq!(
        error.message(),
        "Desegnos sent the AI service a reply format it doesn't accept. This is a Desegnos bug, not your request."
    );

    let mut param_only =
        ProviderError::new(ErrorKind::RequestRejected, SubmissionOutcome::Received);
    param_only.param = Some("text.format.schema".into());
    assert!(param_only.message().contains("Desegnos bug"));

    let mut schema_only =
        ProviderError::new(ErrorKind::RequestRejected, SubmissionOutcome::Received);
    schema_only.code = Some("invalid_json_schema".into());
    assert!(schema_only.message().contains("Desegnos bug"));

    let mut long = ProviderError::new(ErrorKind::RequestRejected, SubmissionOutcome::Received);
    long.response_format = Some(Box::new(ResponseFormat {
        diagnostic_reply: None,
        content_type: None,
        body_kind: ResponseBodyKind::JsonError,
        body_bytes: 0,
        provider_message: Some("x".repeat(900).into_boxed_str()),
        body_truncated: false,
        limit_bytes: None,
        rejection: None,
        correlation: None,
    }));
    let message = long.message();
    assert!(message.starts_with("The AI service rejected this request: "));
    assert!(message.chars().count() <= 439);
}

#[tokio::test]
async fn streamed_errors_and_truncated_streams_never_trigger_an_automatic_retry() {
    let frozen = snapshot(GenerationMode::Strict);
    let created = frame(json!({"type":"response.created","response":{"id":"resp_test_123"}}));
    let mut quota = created.clone();
    quota.extend(frame(
        json!({"type":"error","code":"insufficient_quota","message":KEY}),
    ));
    for (bytes, kind) in [(created, ErrorKind::Truncated), (quota, ErrorKind::Quota)] {
        let server = Server::start(Reply::sse(bytes)).await;
        let error = generate(&server, &frozen).await.unwrap_err();
        assert_eq!(error.kind, kind);
        assert_eq!(error.outcome, SubmissionOutcome::Unknown);
        assert_eq!(error.response_id.as_deref(), Some("resp_test_123"));
        assert_eq!(server.count.load(Ordering::SeqCst), 1);
    }
    let mut reply = Reply::sse(vec![]);
    reply.drop_before_headers = true;
    let server = Server::start(reply).await;
    let error = generate(&server, &frozen).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Transport);
    assert_eq!(error.outcome, SubmissionOutcome::Unknown);
    assert_eq!(server.count.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn redirects_are_refused_without_forwarding_authorization() {
    let frozen = snapshot(GenerationMode::Strict);
    let target = Server::start(Reply::sse(stream(&frozen))).await;
    let mut reply = Reply::json(307, json!({}));
    reply.headers = format!("Location: {}/responses\r\n", target.endpoint);
    let redirect = Server::start(reply).await;
    let error = generate(&redirect, &frozen).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Redirect);
    assert_eq!(target.count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn request_digest_mode_size_context_and_spending_fail_before_connecting() {
    let frozen = snapshot(GenerationMode::Strict);
    let server = Server::start(Reply::sse(stream(&frozen))).await;
    let key = ApiKey::new(KEY.into()).unwrap();
    let run = |snapshot: RequirementsSnapshot, mode, settings: ProviderSettings, schema: Value| {
        let client = server.client();
        let key = key.clone();
        async move {
            client
                .generate(
                    crate::GenerationRequest {
                        authentication: AuthenticationMethod::ApiKey,
                        snapshot: &snapshot,
                        mode,
                        schema: &schema,
                        settings: &settings,
                        key: &key,
                        cancellation: &CancellationToken::new(),
                    },
                    |_| {},
                )
                .await
                .unwrap_err()
        }
    };
    let mut wrong = frozen.clone();
    wrong.markdown.push(' ');
    assert_eq!(
        run(
            wrong,
            GenerationMode::Strict,
            ProviderSettings::default(),
            schema()
        )
        .await
        .kind,
        ErrorKind::InvalidRequirements
    );
    assert_eq!(
        run(
            frozen.clone(),
            GenerationMode::Fuzzy,
            ProviderSettings::default(),
            schema()
        )
        .await
        .kind,
        ErrorKind::InvalidRequirements
    );
    let mut huge = frozen.clone();
    huge.markdown.push_str(&"a".repeat(MAX_REQUIREMENTS_BYTES));
    huge.sha256 = digest(huge.markdown.as_bytes());
    assert_eq!(
        run(
            huge,
            GenerationMode::Strict,
            ProviderSettings::default(),
            schema()
        )
        .await
        .kind,
        ErrorKind::InputLimit
    );
    let mut long = frozen.clone();
    long.markdown.push_str(&"a".repeat(800_000));
    long.sha256 = digest(long.markdown.as_bytes());
    let rich = ProviderSettings {
        spending_limit_usd: 10_000.0,
        ..ProviderSettings::default()
    };
    assert_eq!(
        run(long, GenerationMode::Strict, rich, schema()).await.kind,
        ErrorKind::ContextLimit
    );
    let poor = ProviderSettings {
        spending_limit_usd: 0.01,
        ..ProviderSettings::default()
    };
    assert_eq!(
        run(frozen.clone(), GenerationMode::Strict, poor, schema())
            .await
            .kind,
        ErrorKind::SpendingLimit
    );
    assert_eq!(
        run(
            frozen,
            GenerationMode::Strict,
            ProviderSettings::default(),
            json!({})
        )
        .await
        .kind,
        ErrorKind::InvalidRequirements
    );
    assert_eq!(server.count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn cancellation_before_and_during_streaming_prevents_a_candidate() {
    let frozen = snapshot(GenerationMode::Strict);
    let mut reply = Reply::sse(stream(&frozen));
    reply.delay = Duration::from_millis(10);
    let server = Server::start(reply).await;
    let key = ApiKey::new(KEY.into()).unwrap();
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let error = server
        .client()
        .generate(
            crate::GenerationRequest {
                authentication: AuthenticationMethod::ApiKey,
                snapshot: &frozen,
                mode: GenerationMode::Strict,
                schema: &schema(),
                settings: &ProviderSettings::default(),
                key: &key,
                cancellation: &cancelled,
            },
            |_| {},
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Cancelled);
    assert_eq!(error.outcome, SubmissionOutcome::NotSubmitted);
    assert_eq!(server.count.load(Ordering::SeqCst), 0);
    // A Stop arriving after preparation but immediately before send is still free.
    let cancelled_at_send = CancellationToken::new();
    let signal = cancelled_at_send.clone();
    let error = server
        .client()
        .generate(
            crate::GenerationRequest {
                authentication: AuthenticationMethod::ApiKey,
                snapshot: &frozen,
                mode: GenerationMode::Strict,
                schema: &schema(),
                settings: &ProviderSettings::default(),
                key: &key,
                cancellation: &cancelled_at_send,
            },
            move |_| signal.cancel(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Cancelled);
    assert_eq!(error.outcome, SubmissionOutcome::NotSubmitted);
    assert_eq!(server.count.load(Ordering::SeqCst), 0);

    let skipped = CancellationToken::new();
    skipped.cancel();
    let mut preparation_polled = false;
    assert!(
        skipped
            .run_until_cancelled(async {
                preparation_polled = true;
            })
            .await
            .is_none()
    );
    assert!(
        !preparation_polled,
        "cancelled credential preparation must not start"
    );
    let cancelled = CancellationToken::new();
    let signal = cancelled.clone();
    let error = server
        .client()
        .generate(
            crate::GenerationRequest {
                authentication: AuthenticationMethod::ApiKey,
                snapshot: &frozen,
                mode: GenerationMode::Strict,
                schema: &schema(),
                settings: &ProviderSettings::default(),
                key: &key,
                cancellation: &cancelled,
            },
            move |progress| {
                if progress.received_bytes > 0 {
                    signal.cancel();
                }
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Cancelled);
    assert_eq!(error.outcome, SubmissionOutcome::Unknown);
    assert_eq!(server.count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_deadline_during_a_live_request_records_uncertainty_and_never_retries() {
    let frozen = snapshot(GenerationMode::Strict);
    let mut reply = Reply::sse(stream(&frozen));
    reply.delay = Duration::from_secs(1);
    let server = Server::start(reply).await;
    let error = server
        .client()
        .with_test_deadline(Duration::from_millis(30))
        .generate(
            crate::GenerationRequest {
                authentication: AuthenticationMethod::ApiKey,
                snapshot: &frozen,
                mode: GenerationMode::Strict,
                schema: &schema(),
                settings: &ProviderSettings::default(),
                key: &ApiKey::new(KEY.into()).unwrap(),
                cancellation: &CancellationToken::new(),
            },
            |_| {},
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Deadline);
    assert_eq!(error.outcome, SubmissionOutcome::Unknown);
    assert_eq!(server.count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_silent_provider_times_out_long_before_the_overall_deadline() {
    let frozen = snapshot(GenerationMode::Strict);
    let mut reply = Reply::sse(stream(&frozen));
    reply.delay = Duration::from_secs(1);
    let server = Server::start(reply).await;
    let started = std::time::Instant::now();
    let attempts = std::sync::Mutex::new(vec![]);
    let error = server
        .client()
        .with_test_idle(Duration::from_millis(30))
        .generate(
            crate::GenerationRequest {
                authentication: AuthenticationMethod::ApiKey,
                snapshot: &frozen,
                mode: GenerationMode::Strict,
                schema: &schema(),
                settings: &ProviderSettings::default(),
                key: &ApiKey::new(KEY.into()).unwrap(),
                cancellation: &CancellationToken::new(),
            },
            |update| {
                if let Some(retry) = update.retry {
                    attempts.lock().unwrap().push(retry.attempt);
                }
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Deadline);
    // Idle timeouts of 30 ms, then the test-scaled 20 ms and 80 ms backoffs.
    assert!(started.elapsed() >= Duration::from_millis(100));
    assert!(started.elapsed() < Duration::from_secs(5));
    // Three submissions, each abandoned on the idle timeout.
    assert_eq!(*attempts.lock().unwrap(), [2, 3]);
}

#[tokio::test]
async fn stream_body_and_candidate_size_limits_are_enforced_without_truncation() {
    let frozen = snapshot(GenerationMode::Strict);
    let mut reply = Reply::sse(vec![]);
    reply.declared_length = Some(sse::MAX_STREAM_BYTES + 1);
    let server = Server::start(reply).await;
    let error = generate(&server, &frozen).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::OutputLimit);
    assert_eq!(
        error.response_format.unwrap().limit_bytes,
        Some(sse::MAX_STREAM_BYTES)
    );
    let bytes =
        frame(json!({"type":"response.output_text.delta","delta":"x".repeat(MAX_OUTPUT_BYTES+1)}));
    let mut reply = Reply::sse(bytes);
    reply.chunks = vec![reply.chunks.concat()];
    let server = Server::start(reply).await;
    let error = generate(&server, &frozen).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::OutputLimit);
    assert_eq!(
        error.response_format.unwrap().limit_bytes,
        Some(MAX_OUTPUT_BYTES)
    );
}

#[tokio::test]
async fn model_access_test_is_an_authenticated_get_with_no_generation_or_input_file() {
    let server = Server::start(Reply::json(200, json!({"id":MODEL_ID,"object":"model"}))).await;
    server
        .client()
        .test_model_access(
            &ProviderSettings::default(),
            &ApiKey::new(KEY.into()).unwrap(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    let request = server.request();
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, format!("/v1/models/{MODEL_ID}"));
    assert!(request.body.is_empty());
    let server = Server::start(Reply::json(
        404,
        json!({"error":{"code":"model_not_found","message":KEY}}),
    ))
    .await;
    let error = server
        .client()
        .test_model_access(
            &ProviderSettings::default(),
            &ApiKey::new(KEY.into()).unwrap(),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::ModelUnavailable);
    assert!(!error.to_string().contains(KEY));
}

#[tokio::test]
async fn keys_in_provider_text_or_ids_cannot_enter_results_progress_or_errors() {
    let frozen = snapshot(GenerationMode::Strict);
    let mut text: Value = serde_json::from_str(&candidate(&frozen)).unwrap();
    text["summary"] = json!(KEY);
    let mut id = completed(&candidate(&frozen));
    id["response"]["id"] = json!(format!("resp_{KEY}"));
    for event in [completed(&text.to_string()), id] {
        let server = Server::start(Reply::sse(frame(event))).await;
        let progress = Mutex::new(vec![]);
        let error = server
            .client()
            .generate(
                crate::GenerationRequest {
                    authentication: AuthenticationMethod::ApiKey,
                    snapshot: &frozen,
                    mode: GenerationMode::Strict,
                    schema: &schema(),
                    settings: &ProviderSettings::default(),
                    key: &ApiKey::new(KEY.into()).unwrap(),
                    cancellation: &CancellationToken::new(),
                },
                |value| progress.lock().unwrap().push(value),
            )
            .await
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Malformed);
        assert!(!format!("{error:?} {error} {:?}", progress.lock().unwrap()).contains(KEY));
    }
}

#[test]
fn settings_catalogue_cost_and_loopback_gate_are_explicit_and_bounded() {
    assert!(OpenAiClient::new().is_ok());
    assert!(OpenAiClient::for_loopback("http://[::1]:8080/v1").is_ok());
    let settings = ProviderSettings::default();
    settings.validate().unwrap();
    // The API-key ceiling reaches the model maximum and accepts exactly that.
    let mut raised = settings.clone();
    raised.max_output_tokens = MAX_OUTPUT_TOKENS;
    raised.spending_limit_usd = 10_000.0;
    raised.validate().unwrap();
    for invalid in [
        ProviderSettings {
            model: "../invalid-model".into(),
            ..settings.clone()
        },
        ProviderSettings {
            max_output_tokens: MAX_OUTPUT_TOKENS + 1,
            ..settings.clone()
        },
        ProviderSettings {
            max_output_tokens: 0,
            ..settings.clone()
        },
        ProviderSettings {
            spending_limit_usd: f64::NAN,
            ..settings.clone()
        },
        ProviderSettings {
            spending_limit_usd: -1.0,
            ..settings.clone()
        },
    ] {
        assert_eq!(
            invalid.validate().unwrap_err().kind,
            ErrorKind::InvalidSettings
        );
    }
    let frozen = snapshot(GenerationMode::Strict);
    let estimate = estimate_cost(&frozen, GenerationMode::Strict, &schema(), &settings).unwrap();
    assert!(estimate.estimate_usd >= f64::from(DEFAULT_MAX_OUTPUT_TOKENS) * 15.0 / 1_000_000.0);
    assert!(estimate.input_token_upper_bound > frozen.markdown.len() as u64);
    for endpoint in [
        "https://api.openai.com/v1",
        "http://localhost:8080/v1",
        "http://192.168.1.1:8000/v1",
        "http://127.0.0.1/v1",
        "http://127.0.0.1:8000/evil",
        "http://user:secret@127.0.0.1:8080/v1",
        "http://127.0.0.1:8080/v1?redirect=evil",
    ] {
        assert!(OpenAiClient::for_loopback(endpoint).is_err());
    }
}

#[tokio::test]
async fn event_stream_accepts_multiline_data_and_cr_lines_split_at_every_byte() {
    let frozen = snapshot(GenerationMode::Strict);
    let mut payload = String::from("event: response.completed\r");
    for line in serde_json::to_string_pretty(&completed(&candidate(&frozen)))
        .unwrap()
        .lines()
    {
        payload.push_str("data: ");
        payload.push_str(line);
        payload.push('\r');
    }
    payload.push('\r');
    let mut reply = Reply::sse(payload.into_bytes());
    reply.chunks = reply
        .chunks
        .concat()
        .chunks(1)
        .map(<[u8]>::to_vec)
        .collect();
    let server = Server::start(reply).await;
    assert_eq!(
        generate(&server, &frozen).await.unwrap().text,
        candidate(&frozen)
    );
}

async fn generate_plan(
    server: &Server,
    snapshot: &RequirementsSnapshot,
    settings: &ProviderSettings,
) -> Result<ProviderResponse> {
    server
        .client()
        .generate(
            crate::GenerationRequest {
                authentication: AuthenticationMethod::Chatgpt,
                snapshot,
                mode: GenerationMode::Strict,
                schema: &schema(),
                settings,
                key: &ApiKey::from_chatgpt_token(KEY.into()).unwrap(),
                cancellation: &CancellationToken::new(),
            },
            |_| {},
        )
        .await
}

#[test]
fn authentication_and_current_errors_preserve_serialization_and_secret_bounds() {
    assert_eq!(
        AuthenticationMethod::default(),
        AuthenticationMethod::ApiKey
    );
    assert_eq!(
        serde_json::to_value(AuthenticationMethod::ApiKey).unwrap(),
        "apiKey"
    );
    assert_eq!(
        serde_json::to_value(AuthenticationMethod::Chatgpt).unwrap(),
        "chatgpt"
    );
    assert_eq!(
        serde_json::to_value(ErrorKind::ChatgptUsageLimit).unwrap(),
        "chatgptUsageLimit"
    );
    let current = ProviderError::before(ErrorKind::Cancelled);
    let reopened: ProviderError =
        serde_json::from_value(serde_json::to_value(&current).unwrap()).unwrap();
    assert_eq!(reopened, current);
    let settings = serde_json::to_value(ProviderSettings::default()).unwrap();
    assert_eq!(
        settings
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "imageGeneration",
            "maxOutputTokens",
            "model",
            "reasoningEffort",
            "spendingLimitUsd"
        ]
    );
    assert!(ApiKey::new("a".repeat(4096)).is_ok());
    assert!(ApiKey::new("a".repeat(4097)).is_err());
    let token = "x".repeat(16 * 1024);
    let accepted = ApiKey::from_chatgpt_token(token.clone()).unwrap();
    assert!(!format!("{accepted:?}").contains(&token));
    for invalid in [
        String::new(),
        "a".repeat(16 * 1024 + 1),
        "token with space".into(),
        "token\nheader".into(),
    ] {
        let error = ApiKey::from_chatgpt_token(invalid).unwrap_err();
        assert_eq!(error.kind, ErrorKind::InvalidCredential);
        assert_eq!(error.authentication, AuthenticationMethod::Chatgpt);
    }
}

#[tokio::test]
async fn chatgpt_exact_file_and_global_modes_omit_api_only_fields_and_cost_caps() {
    for mode in [GenerationMode::Fuzzy, GenerationMode::Strict] {
        let frozen = snapshot(mode);
        let server = Server::start(Reply::sse(stream(&frozen))).await;
        let settings = ProviderSettings {
            max_output_tokens: 1,
            spending_limit_usd: 0.0,
            ..ProviderSettings::default()
        };
        let estimate = estimate_for_auth(
            AuthenticationMethod::Chatgpt,
            &frozen,
            mode,
            &schema(),
            &settings,
        )
        .unwrap();
        assert_eq!(estimate.estimate_usd, 0.0);
        assert!(estimate.input_token_upper_bound > frozen.markdown.len() as u64);
        assert_eq!(
            settings.validate().unwrap_err().kind,
            ErrorKind::InvalidSettings
        );
        let response = server
            .client()
            .generate(
                crate::GenerationRequest {
                    authentication: AuthenticationMethod::Chatgpt,
                    snapshot: &frozen,
                    mode,
                    schema: &schema(),
                    settings: &settings,
                    key: &ApiKey::from_chatgpt_token(KEY.into()).unwrap(),
                    cancellation: &CancellationToken::new(),
                },
                |_| {},
            )
            .await
            .unwrap();
        assert_eq!(response.text, candidate(&frozen));
        assert_eq!(response.usage.unwrap().output_tokens, 89);
        let captured = server.request();
        assert_eq!(captured.method, "POST");
        assert_eq!(captured.path, "/v1/responses");
        assert!(
            captured
                .headers
                .to_lowercase()
                .contains(&format!("authorization: bearer {}", KEY.to_lowercase()))
        );
        let body: Value = serde_json::from_slice(&captured.body).unwrap();
        assert_eq!(
            body.as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            [
                "input",
                "instructions",
                "model",
                "reasoning",
                "store",
                "stream",
                "text",
                "tool_choice",
                "tools"
            ]
        );
        assert_eq!(body["model"], MODEL_ID);
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
        assert_eq!(body["reasoning"]["effort"], "medium");
        assert_eq!(body["tools"], json!([]));
        assert_eq!(body["tool_choice"], "none");
        let content = body["input"][0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "input_file");
        assert_eq!(content[0]["filename"], "REQUIREMENTS.md");
        let encoded = content[0]["file_data"]
            .as_str()
            .unwrap()
            .strip_prefix("data:text/markdown;base64,")
            .unwrap();
        let bytes = STANDARD.decode(encoded).unwrap();
        assert_eq!(bytes, frozen.markdown.as_bytes());
        assert_eq!(digest(&bytes), frozen.sha256);
        assert_eq!(
            serde_json::from_str::<Value>(content[1]["text"].as_str().unwrap()).unwrap(),
            json!({"jobId":frozen.job_id,"requirementsSha256":frozen.sha256,"generationMode":mode})
        );
        assert_eq!(body["text"]["format"], wire_format(&frozen));
        assert!(!String::from_utf8(captured.body).unwrap().contains(KEY));
        assert_eq!(server.count.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn chatgpt_preflight_preserves_scope_and_reserves_model_output_capacity() {
    let frozen = snapshot(GenerationMode::Strict);
    let server = Server::start(Reply::sse(stream(&frozen))).await;
    let settings = ProviderSettings {
        max_output_tokens: 1,
        spending_limit_usd: 10_000.0,
        ..ProviderSettings::default()
    };
    let plan_body = crate::settings::prepare_body(
        AuthenticationMethod::Chatgpt,
        &frozen,
        GenerationMode::Strict,
        &schema(),
        &settings,
    )
    .unwrap();
    let base_upper = plan_body.len() as u64 + 4096;
    let extra = ((CONTEXT_WINDOW_TOKENS - base_upper - 10_000) / 4 * 3) as usize;
    let mut capacity = frozen.clone();
    capacity.markdown.push_str(&"a".repeat(extra));
    capacity.sha256 = digest(capacity.markdown.as_bytes());
    assert!(estimate_cost(&capacity, GenerationMode::Strict, &schema(), &settings).is_ok());
    assert_eq!(
        generate_plan(&server, &capacity, &settings)
            .await
            .unwrap_err()
            .kind,
        ErrorKind::ContextLimit
    );
    let mut tampered = frozen.clone();
    tampered.markdown.push(' ');
    assert_eq!(
        generate_plan(&server, &tampered, &settings)
            .await
            .unwrap_err()
            .kind,
        ErrorKind::InvalidRequirements
    );
    let mut wrong_mode = snapshot(GenerationMode::Fuzzy);
    assert_eq!(
        generate_plan(&server, &wrong_mode, &settings)
            .await
            .unwrap_err()
            .kind,
        ErrorKind::InvalidRequirements
    );
    wrong_mode
        .markdown
        .push_str(&"x".repeat(MAX_REQUIREMENTS_BYTES));
    wrong_mode.sha256 = digest(wrong_mode.markdown.as_bytes());
    assert_eq!(
        generate_plan(&server, &wrong_mode, &settings)
            .await
            .unwrap_err()
            .kind,
        ErrorKind::InputLimit
    );
    let unavailable = ProviderSettings {
        model: "../invalid-model".into(),
        ..settings
    };
    assert_eq!(
        generate_plan(&server, &frozen, &unavailable)
            .await
            .unwrap_err()
            .kind,
        ErrorKind::InvalidSettings
    );
    assert_eq!(server.count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn chatgpt_accepts_verified_model_output_capacity_without_using_the_api_cap() {
    let frozen = snapshot(GenerationMode::Strict);
    let settings = ProviderSettings {
        max_output_tokens: 1,
        spending_limit_usd: 0.0,
        ..ProviderSettings::default()
    };
    for output in [DEFAULT_MAX_OUTPUT_TOKENS + 1, MODEL_MAX_OUTPUT_TOKENS] {
        let mut completion = completed(&candidate(&frozen));
        completion["response"]["usage"] = json!({
            "input_tokens":211,"output_tokens":output,"total_tokens":211+u64::from(output)
        });
        let server = Server::start(Reply::sse(frame(completion))).await;
        let response = generate_plan(&server, &frozen, &settings).await.unwrap();
        assert_eq!(response.text, candidate(&frozen));
        assert_eq!(response.usage.unwrap().output_tokens, u64::from(output));
        let body: Value = serde_json::from_slice(&server.request().body).unwrap();
        assert!(body.get("max_output_tokens").is_none());
        assert_eq!(server.count.load(Ordering::SeqCst), 1);
    }
    let mut api_completion = completed(&candidate(&frozen));
    api_completion["response"]["usage"] = json!({
        "input_tokens":211,"output_tokens":DEFAULT_MAX_OUTPUT_TOKENS+1,
        "total_tokens":212+u64::from(DEFAULT_MAX_OUTPUT_TOKENS)
    });
    let api_server = Server::start(Reply::sse(frame(api_completion))).await;
    assert_eq!(
        generate(&api_server, &frozen).await.unwrap_err().kind,
        ErrorKind::OutputLimit
    );
}

#[tokio::test]
async fn chatgpt_context_reserves_full_verified_model_output_capacity() {
    let frozen = snapshot(GenerationMode::Strict);
    let settings = ProviderSettings {
        max_output_tokens: DEFAULT_MAX_OUTPUT_TOKENS,
        spending_limit_usd: 10_000.0,
        ..ProviderSettings::default()
    };
    let base = estimate_for_auth(
        AuthenticationMethod::Chatgpt,
        &frozen,
        GenerationMode::Strict,
        &schema(),
        &settings,
    )
    .unwrap();
    let added = ((CONTEXT_WINDOW_TOKENS - base.input_token_upper_bound - 80_000) / 4 * 3) as usize;
    let mut capacity = frozen.clone();
    capacity.markdown.push_str(&"a".repeat(added));
    capacity.sha256 = digest(capacity.markdown.as_bytes());
    let api = estimate_cost(&capacity, GenerationMode::Strict, &schema(), &settings).unwrap();
    assert!(
        api.input_token_upper_bound + u64::from(DEFAULT_MAX_OUTPUT_TOKENS) <= CONTEXT_WINDOW_TOKENS
    );
    assert!(
        api.input_token_upper_bound + u64::from(MODEL_MAX_OUTPUT_TOKENS) > CONTEXT_WINDOW_TOKENS
    );
    let server = Server::start(Reply::sse(stream(&capacity))).await;
    assert_eq!(
        generate_plan(&server, &capacity, &settings)
            .await
            .unwrap_err()
            .kind,
        ErrorKind::ContextLimit
    );
    assert_eq!(server.count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn available_openai_models_remain_selectable_and_reach_the_requested_transport() {
    let account_models = [
        ("gpt-6-astra", "Account Astra"),
        ("gpt-6.1-terra", "Account Terra"),
        ("gpt-6-luna", "Account Luna"),
        (MODEL_ID, "Account Sol"),
        ("unverified-account-model", "Unsupported"),
    ]
    .map(|(id, name)| ChatgptModel {
        id: id.into(),
        name: name.into(),
        reasoning_efforts: crate::settings::default_reasoning_efforts(),
        ..Default::default()
    });
    let selectable = chatgpt_model_catalogue(&account_models[..4]);
    assert_eq!(
        selectable
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        ["gpt-6-astra", "gpt-6.1-terra", "gpt-6-luna", MODEL_ID]
    );
    assert_eq!(selectable[0].name, "Account Astra");
    assert_eq!(chatgpt_model_catalogue(&account_models[2..3]).len(), 1);
    assert!(chatgpt_model_catalogue(&[]).is_empty());
    assert_eq!(
        chatgpt_model_catalogue(&account_models[4..])[0].id,
        "unverified-account-model"
    );

    let frozen = snapshot(GenerationMode::Strict);
    for model in model_catalogue() {
        let settings = ProviderSettings {
            model: model.id.clone(),
            spending_limit_usd: 10.0,
            ..Default::default()
        };
        let saved: ProviderSettings =
            serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
        for auth in [AuthenticationMethod::ApiKey, AuthenticationMethod::Chatgpt] {
            ProviderId::Openai.validate(&saved, auth).unwrap();
            let server = Server::start(Reply::sse(stream(&frozen))).await;
            let response = server
                .client()
                .generate(
                    crate::GenerationRequest {
                        authentication: auth,
                        snapshot: &frozen,
                        mode: GenerationMode::Strict,
                        schema: &schema(),
                        settings: &saved,
                        key: &ApiKey::new(KEY.into()).unwrap(),
                        cancellation: &CancellationToken::new(),
                    },
                    |_| {},
                )
                .await
                .unwrap();
            assert_eq!(response.text, candidate(&frozen));
            let body: Value = serde_json::from_slice(&server.request().body).unwrap();
            assert_eq!(body["model"], model.id);
        }
        let estimate = estimate_cost(&frozen, GenerationMode::Strict, &schema(), &saved).unwrap();
        let expected = ((estimate.input_token_upper_bound as f64
            * model.long_context_input_usd_per_million
            * 1.25
            + f64::from(saved.max_output_tokens) * model.long_context_output_usd_per_million)
            / 1_000_000.0
            * 100.0)
            .ceil()
            / 100.0;
        assert_eq!(estimate.estimate_usd, expected);
        let denied = ProviderSettings {
            spending_limit_usd: expected / 2.0,
            ..saved
        };
        assert_eq!(
            estimate_cost(&frozen, GenerationMode::Strict, &schema(), &denied)
                .unwrap_err()
                .kind,
            ErrorKind::SpendingLimit
        );
    }
}

#[tokio::test]
async fn newly_discovered_chatgpt_models_need_no_local_catalogue_entry() {
    let future_id = format!("future-{}", Uuid::new_v4());
    let plain_id = format!("plain-{}", Uuid::new_v4());
    let discovery = Server::start(Reply::json(200, json!({"models":[
        {"slug":future_id,"display_name":"New account model","visibility":"list",
         "supported_reasoning_levels":[{"effort":"adaptive","description":"Account option"},{"effort":"max"}],
         "context_window":100_000,"max_output_tokens":10_000},
        {"slug":"hidden-future","display_name":"Hidden","visibility":"hide"},
        {"slug":plain_id,"display_name":"No reasoning model","visibility":"list","supported_reasoning_levels":[]}
    ]}))).await;
    let key = ApiKey::from_chatgpt_token(KEY.into()).unwrap();
    let models = discovery
        .client()
        .list_chatgpt_models(&key, &CancellationToken::new())
        .await
        .unwrap();
    let catalogue = chatgpt_model_catalogue(&models);
    assert_eq!(
        catalogue
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        [future_id.as_str(), plain_id.as_str()]
    );
    assert_eq!(catalogue[0].name, "New account model");
    assert_eq!(catalogue[0].context_window_tokens, 100_000);
    assert_eq!(
        catalogue[0]
            .reasoning_efforts
            .iter()
            .map(ReasoningEffort::as_str)
            .collect::<Vec<_>>(),
        ["adaptive", "max"]
    );

    let account_id = Uuid::new_v4();
    let mut view = ChatgptView::unavailable("fixture");
    view.active_account_id = Some(account_id);
    view.accounts.push(ChatgptAccount {
        id: account_id,
        label: "Fixture".into(),
        email: None,
        connected: true,
        plan_enabled: true,
        storage: CredentialLocation::Session,
        storage_repair_required: false,
    });
    view.models = models;
    let frozen = snapshot(GenerationMode::Strict);
    for model in &catalogue {
        let settings = ProviderSettings {
            model: model.id.clone(),
            reasoning_effort: model.reasoning_efforts[0].clone(),
            ..Default::default()
        };
        let saved: ProviderSettings =
            serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
        ProviderId::Openai
            .validate(&saved, AuthenticationMethod::Chatgpt)
            .unwrap();
        assert_eq!(
            estimate_cost(&frozen, GenerationMode::Strict, &schema(), &saved)
                .unwrap_err()
                .kind,
            ErrorKind::InvalidSettings
        );
        let selected = view.selected_model(&saved).unwrap();
        let estimate = estimate_for_auth(
            AuthenticationMethod::Chatgpt,
            &frozen,
            GenerationMode::Strict,
            &schema(),
            &saved,
        )
        .unwrap();
        selected
            .validate_input(estimate.input_token_upper_bound)
            .unwrap();
        let server = Server::start(Reply::sse(stream(&frozen))).await;
        let reply = generate_plan(&server, &frozen, &saved).await.unwrap();
        assert_eq!(reply.text, candidate(&frozen));
        let body: Value = serde_json::from_slice(&server.request().body).unwrap();
        assert_eq!(body["model"], model.id);
        if model.id == future_id {
            assert_eq!(body["reasoning"]["effort"], "adaptive");
            assert_eq!(
                selected.validate_input(90_001).unwrap_err().kind,
                ErrorKind::ContextLimit
            );
            assert_eq!(
                selected
                    .validate_settings(&ProviderSettings {
                        reasoning_effort: ReasoningEffort::Low,
                        ..saved.clone()
                    })
                    .unwrap_err()
                    .kind,
                ErrorKind::InvalidSettings
            );
        } else {
            assert!(body.get("reasoning").is_none());
        }
        let mut changed = view.clone();
        changed.models.clear();
        assert_eq!(
            changed.selected_model(&saved).unwrap_err().kind,
            ErrorKind::ModelUnavailable
        );
        changed.models.clone_from(&view.models);
        changed.active_account_id = Some(Uuid::new_v4());
        assert_eq!(
            changed.selected_model(&saved).unwrap_err().kind,
            ErrorKind::MissingCredential
        );
        changed.active_account_id = Some(account_id);
        changed.accounts[0].storage_repair_required = true;
        assert_eq!(
            changed.selected_model(&saved).unwrap_err().kind,
            ErrorKind::MissingCredential
        );
        assert_eq!(server.count.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn new_openai_api_model_uses_its_selected_id_and_explicit_prices() {
    let frozen = snapshot(GenerationMode::Strict);
    let model = format!("ft:future-model:account:{}", Uuid::new_v4());
    let settings = ProviderSettings {
        model: model.clone(),
        reasoning_effort: ReasoningEffort::Other("none".into()),
        max_output_tokens: 4096,
        spending_limit_usd: 10_000.0,
        pricing: Some(ModelPricing {
            model: model.clone(),
            input_usd_per_million: 20.0,
            output_usd_per_million: 80.0,
            ..Default::default()
        }),
        ..Default::default()
    };
    let server = Server::start(Reply::sse(stream(&frozen))).await;
    server
        .client()
        .generate(
            crate::GenerationRequest {
                authentication: AuthenticationMethod::ApiKey,
                snapshot: &frozen,
                mode: GenerationMode::Strict,
                schema: &schema(),
                settings: &settings,
                key: &ApiKey::new(KEY.into()).unwrap(),
                cancellation: &CancellationToken::new(),
            },
            |_| {},
        )
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&server.request().body).unwrap();
    assert_eq!(body["model"], model);
    assert!(body.get("pricing").is_none());
    assert!(body.get("reasoning").is_none());
    let no_price = ProviderSettings {
        pricing: None,
        ..settings
    };
    assert_eq!(
        server
            .client()
            .generate(
                crate::GenerationRequest {
                    authentication: AuthenticationMethod::ApiKey,
                    snapshot: &frozen,
                    mode: GenerationMode::Strict,
                    schema: &schema(),
                    settings: &no_price,
                    key: &ApiKey::new(KEY.into()).unwrap(),
                    cancellation: &CancellationToken::new()
                },
                |_| {}
            )
            .await
            .unwrap_err()
            .kind,
        ErrorKind::InvalidSettings
    );
    assert_eq!(server.count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn chatgpt_catalogue_is_authenticated_account_data_in_server_order() {
    let server = Server::start(Reply::json(200, json!({"models":[
        {"slug":"unverified-account-model","display_name":"Account model 日本語","visibility":"list","other_server_field":true},
        {"slug":"hidden-model","display_name":"Hidden model","visibility":"hidden"},
        {"slug":MODEL_ID,"display_name":"GPT-6.1 Sol","visibility":"list"}
    ],"unrelated_server_field":1}))).await;
    let token = "header-safe-synthetic-chatgpt-token-".to_owned() + &"x".repeat(5000);
    assert!(ApiKey::new(token.clone()).is_err());
    let models = server
        .client()
        .list_chatgpt_models(
            &ApiKey::from_chatgpt_token(token.clone()).unwrap(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(
        models,
        [
            ChatgptModel {
                id: "unverified-account-model".into(),
                name: "Account model 日本語".into(),
                reasoning_efforts: crate::settings::default_reasoning_efforts(),
                ..Default::default()
            },
            ChatgptModel {
                id: MODEL_ID.into(),
                name: "GPT-6.1 Sol".into(),
                reasoning_efforts: crate::settings::default_reasoning_efforts(),
                ..Default::default()
            }
        ]
    );
    let request = server.request();
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/v1/models");
    assert!(request.body.is_empty());
    assert!(
        request
            .headers
            .to_lowercase()
            .contains(&format!("authorization: bearer {token}"))
    );
    assert!(!format!("{models:?}").contains(&token));
    assert_eq!(server.count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn chatgpt_catalogue_rejects_malformed_duplicate_unsafe_and_oversized_data() {
    let valid = json!({"slug":MODEL_ID,"display_name":"GPT-6.1 Sol","visibility":"list"});
    let mut oversized = Reply::json(200, json!({"models":[]}));
    oversized.declared_length = Some(1024 * 1024 + 1);
    let excessive = (0..2049).map(|index| json!({"slug":format!("model-{index}"),"display_name":"Model","visibility":"list"})).collect::<Vec<_>>();
    let mut cases = vec![
        (
            Reply::json(200, json!({"data":[valid.clone()]})),
            ErrorKind::Malformed,
        ),
        (
            Reply::json(
                200,
                json!({"models":[{"slug":MODEL_ID,"display_name":"Name"}]}),
            ),
            ErrorKind::Malformed,
        ),
        (
            Reply::json(200, json!({"models":[valid.clone(),valid]})),
            ErrorKind::Malformed,
        ),
        (
            Reply::json(
                200,
                json!({"models":[{"slug":"../responses","display_name":"Name","visibility":"list"}]}),
            ),
            ErrorKind::Malformed,
        ),
        (
            Reply::json(
                200,
                json!({"models":[{"slug":MODEL_ID,"display_name":KEY,"visibility":"list"}]}),
            ),
            ErrorKind::Malformed,
        ),
        (
            Reply::json(
                200,
                json!({"models":[{"slug":MODEL_ID,"display_name":"Name\nControl","visibility":"list"}]}),
            ),
            ErrorKind::Malformed,
        ),
        (
            Reply::json(200, json!({"models":excessive})),
            ErrorKind::OutputLimit,
        ),
        (oversized, ErrorKind::OutputLimit),
    ];
    for metadata in [
        json!({"context_window":0}),
        json!({"context_window":-1}),
        json!({"max_output_tokens":0}),
        json!({"supported_reasoning_levels":[{"effort":"adaptive\ncontrol"}]}),
        json!({"supported_reasoning_levels":[{"effort":"max"},{"effort":"max"}]}),
        json!({"supported_reasoning_levels":[{"effort":"x".repeat(33)}]}),
    ] {
        let mut model = json!({"slug":"future-model","display_name":"Future","visibility":"list"});
        model
            .as_object_mut()
            .unwrap()
            .extend(metadata.as_object().unwrap().clone());
        cases.push((
            Reply::json(200, json!({"models":[model]})),
            ErrorKind::Malformed,
        ));
    }
    let mut actual_oversized = Reply::json(200, json!({}));
    actual_oversized.chunks = vec![vec![b' '; 1024 * 1024 + 1]];
    cases.push((actual_oversized, ErrorKind::OutputLimit));
    let key = ApiKey::from_chatgpt_token(KEY.into()).unwrap();
    for (reply, kind) in cases {
        let server = Server::start(reply).await;
        let error = server
            .client()
            .list_chatgpt_models(&key, &CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(error.kind, kind);
        assert_eq!(error.authentication, AuthenticationMethod::Chatgpt);
        assert!(!format!("{error:?} {error}").contains(KEY));
        assert_eq!(server.count.load(Ordering::SeqCst), 1);
    }
    let empty = Server::start(Reply::json(200, json!({"models":[]}))).await;
    assert!(
        empty
            .client()
            .list_chatgpt_models(&key, &CancellationToken::new())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn chatgpt_structured_admission_failures_keep_safe_metadata_after_bounded_retries() {
    let frozen = snapshot(GenerationMode::Strict);
    for (status, code, kind) in [
        (
            403,
            "subscription_sharing_user_not_eligible",
            ErrorKind::ChatgptNotEligible,
        ),
        (
            429,
            "subscription_sharing_usage_limit_exceeded",
            ErrorKind::ChatgptUsageLimit,
        ),
        (
            503,
            "subscription_sharing_usage_unavailable",
            ErrorKind::ChatgptUsageUnavailable,
        ),
        (
            400,
            "subscription_sharing_unsupported_capability",
            ErrorKind::ChatgptUnsupportedCapability,
        ),
        (
            403,
            "subscription_sharing_route_not_supported",
            ErrorKind::ChatgptRouteNotSupported,
        ),
        (
            401,
            "subscription_sharing_invalid_user",
            ErrorKind::ChatgptInvalidUser,
        ),
        (
            403,
            "chatpass_v2_scope_not_authorized",
            ErrorKind::ChatgptPermission,
        ),
        (
            403,
            "chatpass_v2_invalid_authorization_context",
            ErrorKind::ChatgptPermission,
        ),
        (
            503,
            "subscription_sharing_user_unavailable",
            ErrorKind::ChatgptUserUnavailable,
        ),
    ] {
        let mut reply = Reply::json(
            status,
            json!({"error":{"code":code,"param":"input[0].content","message":KEY,"raw_private_data":KEY}}),
        );
        reply.headers.push_str("x-request-id: req_plan_123\r\n");
        let server = Server::start(reply).await;
        let error = generate_plan(&server, &frozen, &ProviderSettings::default())
            .await
            .unwrap_err();
        assert_eq!(error.kind, kind);
        assert_eq!(error.outcome, SubmissionOutcome::Rejected);
        assert_eq!(error.authentication, AuthenticationMethod::Chatgpt);
        assert_eq!(error.http_status, Some(status));
        assert_eq!(error.code.as_deref(), Some(code));
        assert_eq!(error.param.as_deref(), Some("input[0].content"));
        assert_eq!(error.request_id.as_deref(), Some("req_plan_123"));
        assert!(
            !format!(
                "{error:?} {error} {}",
                serde_json::to_string(&error).unwrap()
            )
            .contains(KEY)
        );
        assert_eq!(
            server.count.load(Ordering::SeqCst),
            if status == 429 || status >= 500 { 3 } else { 1 }
        );
    }
}

#[tokio::test]
async fn chatgpt_admission_detail_is_not_a_machine_code_and_metadata_cannot_echo_credentials() {
    let frozen = snapshot(GenerationMode::Strict);
    for (status, kind) in [
        (401, ErrorKind::Authentication),
        (403, ErrorKind::Authentication),
        (503, ErrorKind::Server),
    ] {
        let mut reply = Reply::json(
            status,
            json!({"detail":format!("subscription_sharing_usage_limit_exceeded {KEY}")}),
        );
        reply.headers = "x-request-id: req_admission_123\r\n".into();
        let server = Server::start(reply).await;
        let error = generate_plan(&server, &frozen, &ProviderSettings::default())
            .await
            .unwrap_err();
        assert_eq!(error.kind, kind);
        assert_eq!(error.http_status, Some(status));
        assert_eq!(error.code, None);
        assert_eq!(error.param, None);
        assert_eq!(error.request_id.as_deref(), Some("req_admission_123"));
        assert!(!format!("{error:?} {error}").contains(KEY));
    }
    for (code, param, request_id) in [
        (KEY.to_owned(), KEY.to_owned(), KEY.to_owned()),
        ("a".repeat(129), "b".repeat(257), "c".repeat(129)),
        (
            "unsafe code".into(),
            "unsafe/path".into(),
            "unsafe.request".into(),
        ),
    ] {
        let mut reply = Reply::json(
            400,
            json!({"error":{"code":code,"param":param,"message":KEY}}),
        );
        reply.headers = format!("x-request-id: {request_id}\r\n");
        let server = Server::start(reply).await;
        let error = generate_plan(&server, &frozen, &ProviderSettings::default())
            .await
            .unwrap_err();
        assert_eq!(error.code, None);
        assert_eq!(error.param, None);
        assert_eq!(error.request_id, None);
        assert!(!serde_json::to_string(&error).unwrap().contains(KEY));
    }
}

#[tokio::test]
async fn chatgpt_failed_terminal_event_after_partial_output_never_becomes_a_candidate() {
    let frozen = snapshot(GenerationMode::Strict);
    for (code, kind) in [
        (
            "subscription_sharing_usage_limit_exceeded",
            ErrorKind::ChatgptUsageLimit,
        ),
        (
            "subscription_sharing_usage_unavailable",
            ErrorKind::ChatgptUsageUnavailable,
        ),
    ] {
        let mut bytes = frame(json!({"type":"response.created","response":{"id":"resp_test_123"}}));
        bytes.extend(frame(
            json!({"type":"response.output_text.delta","delta":"{\"partial\":\"never apply\""}),
        ));
        bytes.extend(frame(
            json!({"type":"response.failed","response":{"id":"resp_test_123","status":"failed",
            "error":{"code":code,"param":"model","message":KEY},
            "usage":{"input_tokens":211,"output_tokens":89,"total_tokens":300}}}),
        ));
        bytes.extend(frame(completed(&candidate(&frozen))));
        let mut reply = Reply::sse(bytes);
        reply.headers = "x-request-id: req_terminal_123\r\n".into();
        let server = Server::start(reply).await;
        let error = generate_plan(
            &server,
            &frozen,
            &ProviderSettings {
                max_output_tokens: 1,
                ..ProviderSettings::default()
            },
        )
        .await
        .unwrap_err();
        assert_eq!(error.kind, kind);
        assert_eq!(error.outcome, SubmissionOutcome::Received);
        assert_eq!(error.response_id.as_deref(), Some("resp_test_123"));
        assert_eq!(error.usage.unwrap().total_tokens, 300);
        assert_eq!(error.http_status, Some(200));
        assert_eq!(error.code.as_deref(), Some(code));
        assert_eq!(error.param.as_deref(), Some("model"));
        assert_eq!(error.request_id.as_deref(), Some("req_terminal_123"));
        assert!(!format!("{error:?} {error}").contains(KEY));
        assert_eq!(server.count.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn chatgpt_refusal_incomplete_interruption_and_explicit_errors_keep_terminal_semantics() {
    let frozen = snapshot(GenerationMode::Strict);
    let mut refusal = completed(&candidate(&frozen));
    refusal["response"]["output"] =
        json!([{"type":"message","role":"assistant","content":[{"type":"refusal","refusal":KEY}]}]);
    let mut interrupted =
        frame(json!({"type":"response.created","response":{"id":"resp_test_123"}}));
    interrupted.extend(frame(
        json!({"type":"response.output_text.delta","delta":"partial"}),
    ));
    for (bytes, kind, outcome) in [
        (
            frame(refusal),
            ErrorKind::Refused,
            SubmissionOutcome::Received,
        ),
        (
            frame(
                json!({"type":"response.incomplete","response":{"id":"resp_test_123","status":"incomplete",
            "usage":{"input_tokens":211,"output_tokens":89,"total_tokens":300}}}),
            ),
            ErrorKind::Incomplete,
            SubmissionOutcome::Received,
        ),
        (
            interrupted,
            ErrorKind::Truncated,
            SubmissionOutcome::Unknown,
        ),
        (
            frame(
                json!({"type":"error","code":"subscription_sharing_usage_limit_exceeded","param":KEY,"message":KEY}),
            ),
            ErrorKind::ChatgptUsageLimit,
            SubmissionOutcome::Unknown,
        ),
    ] {
        let server = Server::start(Reply::sse(bytes)).await;
        let error = generate_plan(
            &server,
            &frozen,
            &ProviderSettings {
                max_output_tokens: 1,
                ..ProviderSettings::default()
            },
        )
        .await
        .unwrap_err();
        assert_eq!(error.kind, kind);
        assert_eq!(error.outcome, outcome);
        assert_eq!(error.authentication, AuthenticationMethod::Chatgpt);
        assert_eq!(error.param, None);
        assert!(!format!("{error:?} {error}").contains(KEY));
        assert_eq!(server.count.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn chatgpt_cancel_deadline_and_redirect_preserve_authentication_without_retry() {
    let frozen = snapshot(GenerationMode::Strict);
    let key = ApiKey::from_chatgpt_token(KEY.into()).unwrap();
    let mut reply = Reply::sse(stream(&frozen));
    reply.delay = Duration::from_millis(10);
    let server = Server::start(reply).await;
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let before = server
        .client()
        .generate(
            crate::GenerationRequest {
                authentication: AuthenticationMethod::Chatgpt,
                snapshot: &frozen,
                mode: GenerationMode::Strict,
                schema: &schema(),
                settings: &ProviderSettings::default(),
                key: &key,
                cancellation: &cancellation,
            },
            |_| {},
        )
        .await
        .unwrap_err();
    assert_eq!(before.kind, ErrorKind::Cancelled);
    assert_eq!(before.outcome, SubmissionOutcome::NotSubmitted);
    assert_eq!(before.authentication, AuthenticationMethod::Chatgpt);
    assert_eq!(server.count.load(Ordering::SeqCst), 0);
    let cancellation = CancellationToken::new();
    let signal = cancellation.clone();
    let during = server
        .client()
        .generate(
            crate::GenerationRequest {
                authentication: AuthenticationMethod::Chatgpt,
                snapshot: &frozen,
                mode: GenerationMode::Strict,
                schema: &schema(),
                settings: &ProviderSettings::default(),
                key: &key,
                cancellation: &cancellation,
            },
            move |progress| {
                if progress.received_bytes > 0 {
                    signal.cancel();
                }
            },
        )
        .await
        .unwrap_err();
    assert_eq!(during.kind, ErrorKind::Cancelled);
    assert_eq!(during.outcome, SubmissionOutcome::Unknown);
    assert!(during.message().contains("plan or credit usage"));
    assert_eq!(server.count.load(Ordering::SeqCst), 1);
    let mut slow = Reply::sse(stream(&frozen));
    slow.delay = Duration::from_secs(1);
    let deadline_server = Server::start(slow).await;
    let deadline = deadline_server
        .client()
        .with_test_deadline(Duration::from_millis(30))
        .generate(
            crate::GenerationRequest {
                authentication: AuthenticationMethod::Chatgpt,
                snapshot: &frozen,
                mode: GenerationMode::Strict,
                schema: &schema(),
                settings: &ProviderSettings::default(),
                key: &key,
                cancellation: &CancellationToken::new(),
            },
            |_| {},
        )
        .await
        .unwrap_err();
    assert_eq!(deadline.kind, ErrorKind::Deadline);
    assert_eq!(deadline.outcome, SubmissionOutcome::Unknown);
    assert_eq!(deadline.authentication, AuthenticationMethod::Chatgpt);
    assert_eq!(deadline_server.count.load(Ordering::SeqCst), 1);
    let target = Server::start(Reply::sse(stream(&frozen))).await;
    let mut redirect = Reply::json(307, json!({}));
    redirect.headers = format!("Location: {}/responses\r\n", target.endpoint);
    let redirect_server = Server::start(redirect).await;
    assert_eq!(
        generate_plan(&redirect_server, &frozen, &ProviderSettings::default())
            .await
            .unwrap_err()
            .kind,
        ErrorKind::Redirect
    );
    assert_eq!(target.count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn chatgpt_candidates_still_require_matching_identity_bounded_usage_and_no_credential_echo() {
    let frozen = snapshot(GenerationMode::Strict);
    let mut wrong: Value = serde_json::from_str(&candidate(&frozen)).unwrap();
    wrong["jobId"] = json!(Uuid::new_v4());
    let mut token_text: Value = serde_json::from_str(&candidate(&frozen)).unwrap();
    token_text["summary"] = json!(format!("Never expose {KEY}"));
    let mut token_id = completed(&candidate(&frozen));
    token_id["response"]["id"] = json!(format!("resp_{KEY}"));
    let mut excess_usage = completed(&candidate(&frozen));
    excess_usage["response"]["usage"] = json!({
        "input_tokens":1,"output_tokens":MODEL_MAX_OUTPUT_TOKENS+1,
        "total_tokens":2+u64::from(MODEL_MAX_OUTPUT_TOKENS)
    });
    let mut oversized = Reply::sse(vec![]);
    oversized.declared_length = Some(sse::MAX_STREAM_BYTES + 1);
    for (reply, kind) in [
        (
            Reply::sse(frame(completed(&wrong.to_string()))),
            ErrorKind::Malformed,
        ),
        (
            Reply::sse(frame(completed(&token_text.to_string()))),
            ErrorKind::Malformed,
        ),
        (Reply::sse(frame(token_id)), ErrorKind::Malformed),
        (Reply::sse(frame(excess_usage)), ErrorKind::OutputLimit),
        (oversized, ErrorKind::OutputLimit),
    ] {
        let server = Server::start(reply).await;
        let progress = Mutex::new(vec![]);
        let error = server
            .client()
            .generate(
                crate::GenerationRequest {
                    authentication: AuthenticationMethod::Chatgpt,
                    snapshot: &frozen,
                    mode: GenerationMode::Strict,
                    schema: &schema(),
                    settings: &ProviderSettings::default(),
                    key: &ApiKey::from_chatgpt_token(KEY.into()).unwrap(),
                    cancellation: &CancellationToken::new(),
                },
                |update| progress.lock().unwrap().push(update),
            )
            .await
            .unwrap_err();
        assert_eq!(error.kind, kind);
        assert_eq!(error.authentication, AuthenticationMethod::Chatgpt);
        assert!(!format!("{error:?} {error} {:?}", progress.lock().unwrap()).contains(KEY));
        assert_eq!(server.count.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn chatgpt_catalogue_cancel_and_deadline_use_the_same_native_cancellation_signal() {
    let key = ApiKey::from_chatgpt_token(KEY.into()).unwrap();
    let mut reply = Reply::json(200, json!({"models":[]}));
    reply.delay = Duration::from_secs(1);
    let server = Server::start(reply).await;
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let error = server
        .client()
        .list_chatgpt_models(&key, &cancellation)
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Cancelled);
    assert_eq!(error.authentication, AuthenticationMethod::Chatgpt);
    assert_eq!(server.count.load(Ordering::SeqCst), 0);
    let error = server
        .client()
        .with_test_deadline(Duration::from_millis(30))
        .list_chatgpt_models(&key, &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Deadline);
    assert_eq!(error.authentication, AuthenticationMethod::Chatgpt);
    assert_eq!(server.count.load(Ordering::SeqCst), 1);
}

#[path = "identity_tests.rs"]
mod identity_contracts;

#[tokio::test]
async fn web_search_events_preserve_the_single_structured_answer_and_reject_tool_text() {
    let frozen = snapshot(GenerationMode::Strict);
    let mut events = finalized_answer_events(&frozen, Some("final_answer"));
    let item = json!({"id":"ws_fixture","type":"web_search_call","status":"completed","action":{"type":"search","query":"requested image"}});
    events.insert(
        1,
        json!({"type":"response.output_item.added","output_index":4,"item":item}),
    );
    events.insert(
        2,
        json!({"type":"response.output_item.done","output_index":4,"item":item}),
    );
    let response = generate_events(&frozen, events.clone(), AuthenticationMethod::ApiKey)
        .await
        .unwrap();
    assert_eq!(response.text, candidate(&frozen));
    events.insert(3, json!({"type":"response.output_text.done","output_index":4,"content_index":0,"item_id":"ws_fixture","text":"untrusted search content"}));
    assert!(
        generate_events(&frozen, events, AuthenticationMethod::ApiKey)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn citation_annotations_do_not_modify_the_structured_json_answer() {
    let frozen = snapshot(GenerationMode::Strict);
    let mut events = finalized_answer_events(&frozen, Some("final_answer"));
    let done = events
        .iter_mut()
        .find(|event| {
            event["type"] == "response.output_item.done" && event["item"]["type"] == "message"
        })
        .unwrap();
    done["item"]["content"][0]["annotations"] = json!([{"type":"url_citation","url":"https://example.com/history","title":"Lighthouse history","start_index":0,"end_index":10}]);
    let response = generate_events(&frozen, events, AuthenticationMethod::ApiKey)
        .await
        .unwrap();
    assert_eq!(response.text, candidate(&frozen));
}

#[test]
fn attachment_previews_are_sent_as_low_detail_images_and_estimated_per_image() {
    let mut frozen = snapshot(GenerationMode::Strict);
    let settings = ProviderSettings::default();
    let plain = estimate_cost(&frozen, GenerationMode::Strict, &schema(), &settings).unwrap();
    let preview = format!("data:image/jpeg;base64,{}", "A".repeat(40_000));
    frozen.images = vec![desegnos_core::RequestImage {
        attachment_id: "a1b2c1".into(),
        data_uri: preview.clone(),
        stored_format: None,
        transparent: None,
        transparent_percent: None,
    }];
    let body: Value = serde_json::from_slice(
        &crate::settings::request_body(
            AuthenticationMethod::ApiKey,
            &frozen,
            GenerationMode::Strict,
            &schema(),
            &settings,
        )
        .unwrap(),
    )
    .unwrap();
    let content = body["input"][0]["content"].as_array().unwrap();
    assert_eq!(content[2]["text"], "Preview of image attachment a1b2c1.");
    assert_eq!(content[3]["image_url"], preview);
    assert_eq!(content[3]["detail"], "low");
    let with_image = estimate_cost(&frozen, GenerationMode::Strict, &schema(), &settings).unwrap();
    assert_eq!(
        with_image.input_token_upper_bound - plain.input_token_upper_bound,
        desegnos_core::AI_PREVIEW_TOKEN_RESERVE,
        "a preview costs a fixed reserve, not one token per Base64 byte"
    );
}

#[tokio::test]
async fn image_probe_uses_invalid_size_and_classifies_access_without_generating() {
    use ImageAvailability::{No, Unknown, Yes};
    let cases = [
        (
            400,
            json!({"error":{"param":"size","message":"Invalid size: 16x16"}}),
            Yes,
        ),
        (
            400,
            json!({"error":{"message":"size 16x16 is not supported"}}),
            Yes,
        ),
        (
            400,
            json!({"error":{"message":"User subscription sharing currently supports only web_search and function tools"}}),
            No,
        ),
        (
            400,
            json!({"error":{"param":"size","message":"Your organization must be verified"}}),
            No,
        ),
        (
            400,
            json!({"error":{"message":"API Organization Verification required"}}),
            No,
        ),
        (401, json!({"error":{"message":"Invalid credential"}}), No),
        (403, json!({"error":{"message":"Not allowed"}}), No),
        (
            429,
            json!({"error":{"param":"size","message":"Subscription rate limit"}}),
            Unknown,
        ),
        (
            503,
            json!({"error":{"param":"size","message":"Unavailable"}}),
            Unknown,
        ),
        (400, json!({"error":{"message":"Invalid model"}}), Unknown),
        (200, json!({"data":[]}), Unknown),
    ];
    {
        for (status, body, expected) in &cases {
            let server = Server::start(Reply::json(*status, body.clone())).await;
            let access = server
                .client()
                .probe_image_access(&ApiKey::new(KEY.into()).unwrap())
                .await;
            assert_eq!(access.available, *expected, "{body}");
            if body.to_string().to_lowercase().contains("verif") {
                assert!(
                    access
                        .reason
                        .starts_with("API Organization Verification required:")
                );
            }
            assert_eq!(server.count.load(Ordering::SeqCst), 1);
            let request = server.request();
            assert_eq!(request.path, "/v1/images/generations");
            assert!(
                request
                    .headers
                    .to_ascii_lowercase()
                    .contains("authorization: bearer ")
            );
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            assert_eq!(body["size"], "16x16");
            assert_eq!(body["model"], "gpt-image-2.5-flare");
            assert!(body.get("tools").is_none());
        }
    }
    let mut network = Reply::json(200, json!({}));
    network.drop_before_headers = true;
    let server = Server::start(network).await;
    assert_eq!(
        server
            .client()
            .probe_image_access(&ApiKey::new(KEY.into()).unwrap())
            .await
            .available,
        Unknown
    );
}

#[tokio::test]
async fn image_probe_shares_text_authorization_and_transport() {
    for auth in [AuthenticationMethod::ApiKey, AuthenticationMethod::Chatgpt] {
        let frozen = snapshot(GenerationMode::Strict);
        let settings = ProviderSettings::default();
        let key = ApiKey::from_chatgpt_token(KEY.into()).unwrap();
        let server = Server::start_sequence(vec![
            Reply::sse(stream(&frozen)),
            Reply::json(
                400,
                json!({"error":{"param":"size","message":"Invalid size 16x16"}}),
            ),
        ])
        .await;
        let client = server.client();
        client
            .generate(
                crate::GenerationRequest {
                    authentication: auth,
                    snapshot: &frozen,
                    mode: GenerationMode::Strict,
                    schema: &schema(),
                    settings: &settings,
                    key: &key,
                    cancellation: &CancellationToken::new(),
                },
                |_| {},
            )
            .await
            .unwrap();
        assert_eq!(
            client.probe_image_access(&key).await.available,
            ImageAvailability::Yes
        );
        let captured = server.captured.lock().unwrap();
        for name in ["authorization", "user-agent"] {
            let header = |request: &Captured| {
                request
                    .headers
                    .lines()
                    .find(|line| line.to_ascii_lowercase().starts_with(&format!("{name}:")))
                    .unwrap()
                    .to_owned()
            };
            assert_eq!(header(&captured[0]), header(&captured[1]));
        }
        let body: Value = serde_json::from_slice(&captured[0].body).unwrap();
        assert!(
            !body["tools"]
                .as_array()
                .is_some_and(|tools| tools.iter().any(|tool| tool["type"] == "image_generation"))
        );
    }
}

#[test]
fn image_credentials_prefer_main_then_confirmed_image_key() {
    let yes = ImageAccess::new(ImageAvailability::Yes, "validation");
    let no = ImageAccess::new(ImageAvailability::No, "subscription sharing");
    let unknown = ImageAccess::new(ImageAvailability::Unknown, "network");
    assert!(!use_image_key(Some(&yes), Some(&yes)));
    assert!(!use_image_key(Some(&yes), Some(&no)));
    assert!(use_image_key(Some(&no), Some(&yes)));
    assert!(use_image_key(Some(&unknown), Some(&yes)));
    assert!(use_image_key(None, Some(&yes)));
    assert!(!use_image_key(Some(&no), Some(&unknown)));
    assert!(!use_image_key(None, None));
    let mut expired = yes.clone();
    expired.checked_at -= 8 * 86400;
    assert!(!use_image_key(Some(&no), Some(&expired)));
}

#[test]
fn chatgpt_response_configuration_strips_builtin_images_but_keeps_functions() {
    let mut body = json!({"tools":[{"type":"image_generation"}, {"type":"function","name":"request_image"}], "tool_choice":{"type":"image_generation"}});
    crate::settings::configure_response_body(
        &mut body,
        AuthenticationMethod::Chatgpt,
        &ProviderSettings::default(),
    );
    assert_eq!(
        body["tools"],
        json!([{"type":"function","name":"request_image"}])
    );
    assert!(body.get("tool_choice").is_none());
}

#[test]
fn image_probe_error_detail_scrubs_secrets_before_unicode_safe_truncation() {
    let key = ApiKey::from_chatgpt_token("opaque-active-secret".into()).unwrap();
    let secrets = [
        "opaque-active-secret",
        "sk-secret",
        "eyJheader.eyJpayload.signature",
        "refresh-private",
        "abcdefghijklmnopqrstuvwxyz0123456789",
    ];
    let message = format!(
        "{} sk-secret eyJheader.eyJpayload.signature refresh_token: refresh-private abcdefghijklmnopqrstuvwxyz0123456789",
        key.expose()
    );
    let access = crate::image_access::image_access_error(
        400,
        &json!({"error":{"code":"invalid_request_error","message":message}}),
        &key,
    );
    for secret in secrets {
        assert!(!access.reason.contains(secret), "leaked {secret}");
    }
    assert!(access.reason.contains("[redacted]"));
    let message = format!("{} {}", "界".repeat(190), "sk-crosses-truncation-boundary");
    let access =
        crate::image_access::image_access_error(400, &json!({"error":{"message":message}}), &key);
    assert_eq!(access.reason.chars().count(), 200);
    assert!(!access.reason.contains("sk-"));
}

#[test]
fn image_cache_expiry_and_capabilities_apply_equally_to_both_authentication_methods() {
    {
        for available in [
            ImageAvailability::Yes,
            ImageAvailability::No,
            ImageAvailability::Unknown,
        ] {
            let mut access = ImageAccess::new(available, "test");
            let ttl = if available == ImageAvailability::Unknown {
                300
            } else {
                7 * 86400
            };
            assert!(!access.expired(access.checked_at + ttl - 1));
            assert!(access.expired(access.checked_at + ttl));
            assert!(access.expired(access.checked_at - 1));
            for enabled in [true, false] {
                let settings = ProviderSettings {
                    image_generation: enabled,
                    ..Default::default()
                };
                assert_eq!(
                    ProviderId::Openai
                        .capabilities_for(&settings, Some(&access))
                        .image_generation
                        .is_some(),
                    enabled && available == ImageAvailability::Yes
                );
                assert!(
                    ProviderId::Openai
                        .capabilities_for(&settings, None)
                        .image_generation
                        .is_none()
                );
                assert!(
                    ProviderId::Google
                        .capabilities_for(&settings, Some(&access))
                        .image_generation
                        .is_none()
                );
            }
            access.checked_at -= 7 * 86400;
            assert!(
                ProviderId::Openai
                    .capabilities_for(&ProviderSettings::default(), Some(&access))
                    .image_generation
                    .is_none()
            );
        }
    }
}

#[tokio::test]
async fn runaway_replies_stop_before_the_provider_closes_the_stream() {
    let frozen = snapshot(GenerationMode::Strict);
    let delta = |text: String| json!({"type":"response.output_text.delta","delta":text});
    for authentication in [AuthenticationMethod::ApiKey, AuthenticationMethod::Chatgpt] {
        let blank = vec![
            delta("{\"a\":".into()),
            delta("\n".repeat(1_000)),
            delta(" ".repeat(1_001)),
        ];
        let error = generate_events(&frozen, blank, authentication)
            .await
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Repetition);
        let spaced = vec![
            delta(" ".repeat(1_500)),
            delta("1,".into()),
            delta(" ".repeat(1_500)),
        ];
        let error = generate_events(&frozen, spaced, authentication)
            .await
            .unwrap_err();
        assert_eq!(
            error.kind,
            ErrorKind::Truncated,
            "separate whitespace runs are not runaway"
        );
    }
    // Varied text, so only the size limit applies and not the loop check.
    let varied = (0..100_000).map(|i| format!("{i},")).collect::<String>();
    let long = || {
        vec![delta(
            varied[..CHATGPT_MAX_OUTPUT_TOKENS as usize * 6 + 1].into(),
        )]
    };
    let error = generate_events(&frozen, long(), AuthenticationMethod::Chatgpt)
        .await
        .unwrap_err();
    assert_eq!(
        error.kind,
        ErrorKind::OutputLimit,
        "ChatGPT ignores max_output_tokens, so the text is capped locally"
    );
    let error = generate_events(&frozen, long(), AuthenticationMethod::ApiKey)
        .await
        .unwrap_err();
    assert_eq!(
        error.kind,
        ErrorKind::Truncated,
        "the API enforces its own token limit"
    );
}

#[tokio::test]
async fn a_reply_that_loops_on_the_same_operations_is_stopped() {
    let frozen = snapshot(GenerationMode::Strict);
    let delta = |text: String| json!({"type":"response.output_text.delta","delta":text});
    let operation = |field: &str| {
        format!(
            "{{\"kind\":\"update\",\"patches\":[{{\"elementId\":\"fce93171\",\"{field}\":true,\"rect\":null,\"style\":null}}]}},"
        )
    };
    let mut events = vec![delta("{\"operations\":[".into())];
    for _ in 0..250 {
        events.push(delta(operation("visible")));
        events.push(delta(operation("locked")));
    }
    for authentication in [AuthenticationMethod::ApiKey, AuthenticationMethod::Chatgpt] {
        let error = generate_events(&frozen, events.clone(), authentication)
            .await
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Repetition);
    }
    let distinct = (0..400)
        .map(|index| {
            format!(
                "{{\"kind\":\"insert\",\"name\":\"Card {index}\",\"rect\":{{\"x\":{},\"y\":{}}}}},",
                index % 7 * 40,
                index * 24
            )
        })
        .collect::<String>();
    assert!(
        !sse::ends_in_repetition(distinct.as_bytes()),
        "similar but different operations are not a loop"
    );
    let marquee = format!(
        "{{\"kind\":\"insert\",\"text\":\"{}",
        "As an AI language model, I love toast and the colour seven. ".repeat(120)
    );
    assert!(
        !sse::ends_in_repetition(marquee.as_bytes()),
        "text written to repeat on purpose is not a loop"
    );
}

#[tokio::test]
async fn deepseek_discovery_merges_catalogue_prices_and_limits() {
    let server = Server::start(Reply::json(
        200,
        json!({"object":"list","data":[
            {"id":"deepseek-flash","object":"model","owned_by":"deepseek"},
            {"id":"deepseek-preview","object":"model","owned_by":"deepseek"},
        ]}),
    ))
    .await;
    let client = OpenAiClient::for_loopback(&server.endpoint).unwrap();
    let models = crate::model_discovery::list_models(
        ProviderId::Deepseek,
        &client.client,
        &server.endpoint,
        &ApiKey::new("synthetic-deepseek-key".into()).unwrap(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    let request = server.request();
    assert_eq!(
        (request.method.as_str(), request.path.as_str()),
        ("GET", "/v1/models")
    );
    assert!(
        request
            .headers
            .to_ascii_lowercase()
            .contains("authorization: bearer ")
    );
    assert_eq!(models.len(), 2);
    let quoted = &models[0];
    assert_eq!(quoted.id, "deepseek-flash");
    assert_eq!(quoted.name, "DeepSeek V4.1 Flash");
    assert_eq!(quoted.input_usd_per_million, 0.30);
    assert_eq!(quoted.output_usd_per_million, 1.20);
    assert_eq!(quoted.context_window_tokens, 1_000_000);
    assert_eq!(quoted.max_output_tokens, MAX_OUTPUT_TOKENS);
    assert_eq!(quoted.verified_at, "2026-11-06");
    assert_eq!(
        quoted.source_url,
        "https://api-docs.deepseek.com/quick_start/pricing"
    );
    assert!(!quoted.pricing_required);
    assert!(quoted.default_pricing.is_none());
    assert_eq!(
        quoted
            .reasoning_efforts
            .iter()
            .map(ReasoningEffort::as_str)
            .collect::<Vec<_>>(),
        ["low", "high", "max"]
    );
    // Unpublished ids keep the generic bounds and must have their prices entered.
    assert_eq!(models[1].name, "deepseek-preview");
    assert!(models[1].pricing_required);
    assert_eq!(
        ProviderId::Deepseek
            .catalogue()
            .iter()
            .map(|model| {
                (
                    model.id.as_str(),
                    model.input_usd_per_million,
                    model.output_usd_per_million,
                )
            })
            .collect::<Vec<_>>(),
        [
            ("deepseek-flash", 0.30, 1.20),
            ("deepseek-v4-pro", 1.32, 3.96)
        ]
    );
}

#[test]
fn deepseek_requests_use_system_role_and_effort_names() {
    for (effort, expected) in [
        ("minimal", "low"),
        ("low", "low"),
        ("medium", "high"),
        ("high", "high"),
        ("xhigh", "high"),
        ("max", "max"),
    ] {
        let effort = ReasoningEffort::try_from(effort.to_string()).unwrap();
        assert_eq!(deepseek_reasoning_effort(&effort), expected, "{effort:?}");
    }
    let snapshot = snapshot(GenerationMode::Strict);
    let mut packet =
        crate::settings::request_content(&snapshot, GenerationMode::Strict, &schema()).unwrap();
    packet.web_search = false;
    let settings = ProviderSettings {
        reasoning_effort: ReasoningEffort::Medium,
        ..ProviderId::Deepseek.defaults()
    };
    let body = crate::providers::foreign_body(ProviderId::Deepseek, &packet, &settings).unwrap();
    // DeepSeek's chat API has no json_schema response format, so the schema is prompt text and
    // local validation enforces it.
    assert_eq!(body["response_format"], json!({"type":"json_object"}));
    let system = body["messages"][0]["content"].as_str().unwrap();
    assert!(system.starts_with(packet.instructions));
    assert!(system.contains(&serde_json::to_string(&packet.schema).unwrap()));
    assert!(system.contains(&snapshot.job_id.to_string()));
    assert!(system.contains(&snapshot.sha256));
    assert!(system.to_ascii_lowercase().contains("json object"));
    let identity =
        crate::identity::identity_content(&snapshot, &desegnos_core::candidate_schema(), &[])
            .unwrap();
    let identity_body =
        crate::providers::foreign_body(ProviderId::Deepseek, &identity, &settings).unwrap();
    assert_eq!(
        identity_body["response_format"],
        json!({"type":"json_object"})
    );
    let identity_system = identity_body["messages"][0]["content"].as_str().unwrap();
    assert!(identity_system.contains(&serde_json::to_string(&identity.schema).unwrap()));
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][1]["role"], "user");
    assert!(body.get("developer").is_none());
    assert_eq!(body["reasoning_effort"], "high");
    assert_eq!(body["max_tokens"], settings.max_output_tokens);
    assert!(body.get("max_completion_tokens").is_none());
    assert!(body.get("tools").is_none() && body.get("provider").is_none());
    assert_eq!(ProviderId::Deepseek.service(), "io.desegnos.deepseek");
    assert_eq!(ProviderId::Deepseek.defaults().model, "deepseek-flash");
    assert_eq!(ProviderId::Deepseek.defaults().endpoint, None);
    // Thinking counts toward max_tokens, and the ceiling still fits the default budget.
    assert_eq!(
        ProviderId::Deepseek.defaults().max_output_tokens,
        MAX_OUTPUT_TOKENS
    );
    ProviderId::Deepseek.catalogue()[0]
        .validate_api_settings(&ProviderId::Deepseek.defaults())
        .unwrap();
    assert_eq!(
        provider_catalogue()
            .iter()
            .map(|provider| (provider.id, provider.name))
            .collect::<Vec<_>>(),
        [
            (ProviderId::Openai, "OpenAI"),
            (ProviderId::Anthropic, "Anthropic"),
            (ProviderId::Google, "Google"),
            (ProviderId::Openrouter, "OpenRouter"),
            (ProviderId::Deepseek, "DeepSeek"),
            (ProviderId::Local, "Local"),
        ]
    );
    let capabilities = ProviderId::Deepseek.capabilities();
    assert!(!capabilities.web_search);
    assert!(capabilities.image_generation.is_none());
    assert_eq!(capabilities.image_downloads, 8);
    ProviderId::Deepseek
        .validate(
            &ProviderId::Deepseek.defaults(),
            AuthenticationMethod::ApiKey,
        )
        .unwrap();
    // A DeepSeek key is required and its endpoint is fixed.
    assert_eq!(
        ProviderId::Deepseek
            .validate(
                &ProviderId::Deepseek.defaults(),
                AuthenticationMethod::Chatgpt,
            )
            .unwrap_err()
            .kind,
        ErrorKind::InvalidSettings
    );
    assert_eq!(
        ProviderId::Deepseek
            .validate(
                &ProviderSettings {
                    endpoint: Some("https://example.test/v1".into()),
                    ..ProviderId::Deepseek.defaults()
                },
                AuthenticationMethod::ApiKey
            )
            .unwrap_err()
            .kind,
        ErrorKind::InvalidSettings
    );
}

#[tokio::test]
async fn local_discovery_authentication_failure_asks_for_a_key() {
    let server = Server::start(Reply::json(
        401,
        json!({"error":{"message":"Authentication required"}}),
    ))
    .await;
    let client = OpenAiClient::for_loopback(&server.endpoint).unwrap();
    let error = crate::model_discovery::list_models(
        ProviderId::Local,
        &client.client,
        &server.endpoint,
        &ApiKey::local_anonymous(),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Authentication);
    assert!(models_need_key(ProviderId::Local, false, &error));
    assert_eq!(
        models_error(ProviderId::Local, false, &error),
        "This server needs an API key. Add one from the account menu."
    );
    // A configured key that the server rejects keeps the ordinary key error.
    assert!(!models_need_key(ProviderId::Local, true, &error));
    assert_eq!(
        models_error(ProviderId::Local, true, &error),
        error.to_string()
    );
    assert!(!models_need_key(ProviderId::Openrouter, false, &error));
    assert_eq!(
        models_error(ProviderId::Openrouter, false, &error),
        error.to_string()
    );
}

// A generated website page with `layers` layers over three connected sizes, plus one behaviour
// pointer per section: the shape of a real, large export page.
fn large_export_page(layers: usize) -> (desegnos_core::Project, Vec<Uuid>) {
    use desegnos_core::{
        Appearance, BoundaryKind, Command, Element, ElementKind, PageSize, Pointer, PointerKind,
        PointerSource, PointerTarget, Project, Rect, Viewport, empty_element,
    };
    let project = Project::new(
        "Nightfall".into(),
        Viewport {
            width: 1440,
            height: 900,
        },
        Appearance::Dark,
    )
    .unwrap();
    let page = project.home_page_id;
    let mut project = project;
    for (label, width, height, x) in [("Tablet", 820, 1180, 1600.0), ("Phone", 390, 844, 2600.0)] {
        project = project
            .apply(
                project.revision,
                Command::AddPageSize {
                    page_id: page,
                    size: PageSize {
                        id: Uuid::new_v4(),
                        label: label.into(),
                        width,
                        height,
                        kind: BoundaryKind::Page,
                        x,
                        y: 0.0,
                    },
                    moves: vec![],
                    reference: vec![],
                },
            )
            .unwrap();
    }
    let sections = 9;
    let mut top: Vec<Element> = Vec::new();
    let layer = |index: usize| {
        let mut element = empty_element(
            if index.is_multiple_of(3) {
                ElementKind::Box
            } else {
                ElementKind::Text
            },
            Rect {
                x: (index % 3) as f64 * 420.0,
                y: (index / 3) as f64 * 96.0,
                width: 400.0,
                height: 88.0,
            },
        );
        element.text = format!("Layer {index} of the Nightfall landing page, with its copy.");
        element
    };
    let mut index = 0;
    let mut section_ids = Vec::new();
    for section in 0..sections {
        let mut group = empty_element(
            ElementKind::Box,
            Rect {
                x: 0.0,
                y: section as f64 * 560.0,
                width: 1440.0,
                height: 540.0,
            },
        );
        group.name = Some(format!("Section {section}"));
        section_ids.push(group.id);
        let per_section = (layers - sections) / sections;
        group.children = (0..per_section)
            .map(|_| {
                index += 1;
                layer(index)
            })
            .collect();
        top.push(group);
    }
    let mut project = project
        .apply(
            project.revision,
            Command::InsertElements {
                page_id: page,
                parent_id: None,
                index: 0,
                elements: top,
                picture_data_uri: None,
                size_id: None,
            },
        )
        .unwrap();
    for (pointer, section) in section_ids.iter().copied().enumerate() {
        // One behaviour per source: the boundary's own, then each section's own.
        let (from, to) = if pointer == 0 {
            (
                PointerSource::Boundary {
                    page_id: page,
                    size_id: None,
                },
                PointerTarget::Boundary {
                    page_id: page,
                    size_id: None,
                },
            )
        } else {
            (
                PointerSource::Element {
                    page_id: page,
                    element_id: section,
                    size_id: None,
                },
                PointerTarget::Element {
                    page_id: page,
                    element_id: section,
                    size_id: None,
                },
            )
        };
        project = project
            .apply(
                project.revision,
                Command::AddPointer {
                    pointer: Pointer {
                        id: Uuid::new_v4(),
                        kind: PointerKind::Change,
                        from,
                        to,
                        comment: format!(
                            "Section {pointer} parallax-slides its cards on scroll and fades them in."
                        ),
                        wiring: None,
                        play: None,
                        play_comment: String::new(),
                    },
                },
            )
            .unwrap();
    }
    (project, vec![page])
}

/// A page of hundreds of layers keeps its export behaviour request inside a 1M-token model.
#[test]
fn large_export_behaviour_request_fits_a_million_token_context_window() {
    let (project, page_ids) = large_export_page(450);
    let plan = desegnos_core::plan_export(&project, &page_ids, Uuid::new_v4()).unwrap();
    assert_eq!(plan.pointers.len(), 9, "every section keeps its behaviour");
    let snapshot = desegnos_core::plan_snapshot(&project, &plan);
    // DeepSeek Flash: a 1,000,000-token window and the 128,000-token output ceiling.
    let settings = ProviderId::Deepseek.defaults();
    let client = ProviderClient::with_settings(
        ProviderId::Deepseek,
        OpenAiClient::for_loopback("http://127.0.0.1:1/v1").unwrap(),
        &settings,
    )
    .unwrap();
    let estimate = client
        .estimate(
            AuthenticationMethod::ApiKey,
            &snapshot,
            project.generation_mode,
            &plan.schema,
            &settings,
        )
        .expect("a page of hundreds of layers fits a 1M-token model");
    // The byte-per-token cost bound is larger than the window, so a pre-flight that counted every
    // wire byte as a token refused this request. It fits once bytes are bounded as tokens.
    assert!(
        estimate.input_token_upper_bound + u64::from(settings.max_output_tokens) > 1_000_000,
        "the request still exercises the window bound: {}",
        estimate.input_token_upper_bound
    );
    // A request that is really too long still stops before it is sent.
    let mut oversize = snapshot.clone();
    oversize.markdown.push_str(&"a".repeat(2_000_000));
    oversize.sha256 = digest(oversize.markdown.as_bytes());
    assert_eq!(
        client
            .estimate(
                AuthenticationMethod::ApiKey,
                &oversize,
                project.generation_mode,
                &plan.schema,
                &settings,
            )
            .unwrap_err()
            .kind,
        ErrorKind::InputLimit
    );
}

#[tokio::test]
async fn stream_accepts_absent_host_echoes_but_rejects_present_nulls() {
    let frozen = snapshot(GenerationMode::Strict);
    let mut reply: Value = serde_json::from_str(&candidate(&frozen)).unwrap();
    reply.as_object_mut().unwrap().remove("jobId");
    reply.as_object_mut().unwrap().remove("requirementsSha256");
    let server = Server::start(Reply::sse(frame(completed(&reply.to_string())))).await;
    assert!(generate(&server, &frozen).await.is_ok());
    for key in ["jobId", "requirementsSha256"] {
        let mut invalid = reply.clone();
        invalid[key] = Value::Null;
        let server = Server::start(Reply::sse(frame(completed(&invalid.to_string())))).await;
        assert!(generate(&server, &frozen).await.is_err());
    }
}

#[tokio::test]
async fn http_error_statuses_are_definite_rejections_after_only_transient_retries() {
    let frozen = snapshot(GenerationMode::Strict);
    for status in [400, 401, 402, 403, 408, 429, 500, 502, 503, 504] {
        let server = Server::start(Reply::json(
            status,
            json!({"code":"internal_error", "providerMessage":"Internal Server Error (request_id: req_test)"}),
        )).await;
        let error = generate(&server, &frozen).await.unwrap_err();
        assert_eq!(error.http_status, Some(status));
        assert_eq!(error.outcome, SubmissionOutcome::Rejected);
        assert_eq!(error.code.as_deref(), Some("internal_error"));
        assert!(error.response_id.is_none());
        assert!(error.message().contains("Nothing was changed"));
        assert!(!error.message().contains("unknown outcome"));
        assert_eq!(
            server.count.load(Ordering::SeqCst),
            if status == 429 || status >= 500 { 3 } else { 1 }
        );
    }
}

#[tokio::test]
async fn an_interrupted_http_error_body_does_not_make_the_generation_outcome_unknown() {
    let frozen = snapshot(GenerationMode::Strict);
    let mut reply = Reply::json(500, json!({"code":"internal_error"}));
    reply.declared_length = Some(10_000);
    let server = Server::start(reply).await;
    let error = generate(&server, &frozen).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Server);
    assert_eq!(error.outcome, SubmissionOutcome::Rejected);
    assert_eq!(error.http_status, Some(500));
    assert_eq!(server.count.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn transient_http_retries_reuse_the_frozen_request_and_count_only_returned_usage() {
    let frozen = snapshot(GenerationMode::Strict);
    for (status, failures) in [(500, 1), (500, 2), (429, 1), (429, 2)] {
        let mut replies = vec![Reply::json(status, json!({"code":"internal_error"})); failures];
        replies.push(Reply::sse(stream(&frozen)));
        let server = Server::start_sequence(replies).await;
        let updates = Mutex::new(vec![]);
        let response = server
            .client()
            .generate(
                crate::GenerationRequest {
                    authentication: AuthenticationMethod::ApiKey,
                    snapshot: &frozen,
                    mode: GenerationMode::Strict,
                    schema: &schema(),
                    settings: &ProviderSettings::default(),
                    key: &ApiKey::new(KEY.into()).unwrap(),
                    cancellation: &CancellationToken::new(),
                },
                |update| updates.lock().unwrap().push(update),
            )
            .await
            .unwrap();
        assert_eq!(response.text, candidate(&frozen));
        assert_eq!(response.usage.unwrap().total_tokens, 300);
        assert_eq!(server.count.load(Ordering::SeqCst), failures + 1);
        let requests = server.captured.lock().unwrap();
        assert!(
            requests
                .windows(2)
                .all(|pair| pair[0].body == pair[1].body && pair[0].path == pair[1].path)
        );
        let retries = updates
            .lock()
            .unwrap()
            .iter()
            .filter_map(|update| update.retry.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            retries
                .iter()
                .map(|retry| retry.attempt)
                .collect::<Vec<_>>(),
            (2..=(failures + 1) as u8).collect::<Vec<_>>()
        );
        assert_eq!(
            retries[0].message(ProviderId::Deepseek),
            if status == 500 {
                "DeepSeek had a server problem — retrying (2 of 3)…"
            } else {
                "DeepSeek is rate-limiting requests — retrying (2 of 3)…"
            }
        );
    }
}

#[tokio::test]
async fn three_http_500s_produce_one_definite_failure_without_usage() {
    let frozen = snapshot(GenerationMode::Strict);
    let server = Server::start(Reply::json(500, json!({"code":"internal_error"}))).await;
    let error = generate(&server, &frozen).await.unwrap_err();
    assert_eq!(server.count.load(Ordering::SeqCst), 3);
    assert_eq!(error.kind, ErrorKind::Server);
    assert_eq!(error.outcome, SubmissionOutcome::Rejected);
    assert_eq!(error.http_status, Some(500));
    assert_eq!(error.usage, None);
}

#[tokio::test]
async fn output_then_transport_error_never_resends_even_when_the_next_request_would_succeed() {
    let frozen = snapshot(GenerationMode::Strict);
    let mut partial = Reply::sse(frame(
        json!({"type":"response.output_text.delta","delta":"partial"}),
    ));
    partial.declared_length = Some(10_000);
    let server = Server::start_sequence(vec![partial, Reply::sse(stream(&frozen))]).await;
    let error = generate(&server, &frozen).await.unwrap_err();
    assert!(matches!(
        error.kind,
        ErrorKind::Transport | ErrorKind::Truncated
    ));
    assert_eq!(server.count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cancelling_during_backoff_never_submits_the_next_attempt() {
    let frozen = snapshot(GenerationMode::Strict);
    let mut failed = Reply::json(500, json!({"code":"internal_error"}));
    failed.headers.clear(); // Exercise the real backoff, not Retry-After: 0.
    let server = Server::start_sequence(vec![failed, Reply::sse(stream(&frozen))]).await;
    let cancel = CancellationToken::new();
    let started = tokio::time::Instant::now();
    let error = server
        .client()
        .generate(
            crate::GenerationRequest {
                authentication: AuthenticationMethod::ApiKey,
                snapshot: &frozen,
                mode: GenerationMode::Strict,
                schema: &schema(),
                settings: &ProviderSettings::default(),
                key: &ApiKey::new(KEY.into()).unwrap(),
                cancellation: &cancel,
            },
            |update| {
                if update.retry.is_some() {
                    let cancel = cancel.clone();
                    cancel.cancel();
                }
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(server.count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn retry_after_wait_is_honoured_and_long_waits_are_not_shortened() {
    let frozen = snapshot(GenerationMode::Strict);
    let mut limited = Reply::json(429, json!({"code":"rate_limit_exceeded"}));
    limited.headers = "Retry-After: 1\r\n".into();
    let server = Server::start_sequence(vec![limited, Reply::sse(stream(&frozen))]).await;
    let start = tokio::time::Instant::now();
    generate(&server, &frozen).await.unwrap();
    assert!(start.elapsed() >= Duration::from_secs(1));
    assert_eq!(server.count.load(Ordering::SeqCst), 2);
    let mut limited = Reply::json(429, json!({"code":"rate_limit_exceeded"}));
    limited.headers = "Retry-After: 31\r\n".into();
    let server = Server::start(limited).await;
    assert_eq!(
        generate(&server, &frozen).await.unwrap_err().http_status,
        Some(429)
    );
    assert_eq!(server.count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn reset_before_headers_or_first_body_byte_retries_but_4xx_never_do() {
    let frozen = snapshot(GenerationMode::Strict);
    // The initial successful response's socket closes without its promised first byte.
    let mut reset = Reply::sse(vec![]);
    reset.declared_length = Some(100);
    let server = Server::start_sequence(vec![reset, Reply::sse(stream(&frozen))]).await;
    let response = generate(&server, &frozen).await.unwrap();
    assert_eq!(response.text, candidate(&frozen));
    assert_eq!(server.count.load(Ordering::SeqCst), 2);
    for status in [400, 401, 402, 403, 404, 408, 422] {
        let server = Server::start_sequence(vec![
            Reply::json(status, json!({})),
            Reply::sse(stream(&frozen)),
        ])
        .await;
        assert_eq!(
            generate(&server, &frozen).await.unwrap_err().http_status,
            Some(status)
        );
        assert_eq!(server.count.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn read_only_preflight_network_failures_do_not_claim_an_unknown_generation_outcome() {
    let mut reset = Reply::sse(vec![]);
    reset.drop_before_headers = true;
    let server = Server::start(reset).await;
    let cancel = CancellationToken::new();
    let client = reqwest::Client::builder()
        .retry(reqwest::retry::never())
        .build()
        .unwrap();
    let error = crate::retry::send(
        client.get(&server.endpoint),
        &cancel,
        crate::retry::RetryPolicy::new(
            tokio::time::Instant::now() + Duration::from_secs(5),
            Duration::from_secs(30),
            1024,
        ),
        &|_| {},
    )
    .await
    .err()
    .unwrap();
    assert_eq!(error.kind, ErrorKind::Transport);
    assert_eq!(error.outcome, SubmissionOutcome::NotSubmitted);
    // Every resend keeps the read-only outcome instead of claiming an unknown submission.
    assert_eq!(server.count.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn an_unreachable_provider_fails_at_once_instead_of_retrying() {
    // Bind and drop a listener so the port is closed: the connection is refused immediately.
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let client = OpenAiClient::for_loopback(&format!("http://127.0.0.1:{port}/v1")).unwrap();
    let frozen = snapshot(GenerationMode::Strict);
    let updates = Mutex::new(vec![]);
    let started = std::time::Instant::now();
    let error = client
        .generate(
            crate::GenerationRequest {
                authentication: AuthenticationMethod::ApiKey,
                snapshot: &frozen,
                mode: GenerationMode::Strict,
                schema: &schema(),
                settings: &ProviderSettings::default(),
                key: &ApiKey::new(KEY.into()).unwrap(),
                cancellation: &CancellationToken::new(),
            },
            |update| updates.lock().unwrap().push(update),
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Transport);
    assert!(started.elapsed() < std::time::Duration::from_secs(1), "waited {:?}", started.elapsed());
    assert!(updates.lock().unwrap().iter().all(|update| update.retry.is_none()));
}
