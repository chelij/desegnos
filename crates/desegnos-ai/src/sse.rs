use crate::{
    ApiKey, AuthenticationMethod, CHATGPT_MAX_OUTPUT_TOKENS, CorrelationCause, ErrorKind,
    MAX_OUTPUT_BYTES, ProviderError, ProviderResponse, ResponseBodyKind, ResponseCorrelation,
    ResponseRejection, Result, SubmissionOutcome, TerminalOutput, TokenUsage,
};
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) const MAX_STREAM_BYTES: usize = 32 * 1024 * 1024;
pub(crate) const MAX_EVENT_BYTES: usize = MAX_OUTPUT_BYTES * 2 + 65_536;
// A JSON reply never needs this much whitespace in a row; strict structured output can get
// stuck emitting it until the provider closes the stream.
pub(crate) const MAX_WHITESPACE_RUN: usize = 2_000;
// A model stuck in a loop repeats the same operations until the stream is cut off.
const REPETITION_CHECK_BYTES: usize = 4096;
const REPETITION_TIMES: usize = 8;
// Large enough that text written to repeat on purpose (a marquee, a looping list) is not a loop.
const REPETITION_MIN_BYTES: usize = 32 * 1024;
const REPETITION_MAX_PERIOD: usize = 16 * 1024;
const REPETITION_NEEDLE: usize = 64;

// True when the end of the text is one block repeated back to back.
pub(crate) fn ends_in_repetition(text: &[u8]) -> bool {
    let Some(start) = text.len().checked_sub(REPETITION_NEEDLE) else {
        return false;
    };
    let needle = &text[start..];
    let earliest = start.saturating_sub(REPETITION_MAX_PERIOD);
    // Try the nearest few earlier copies of the tail as the loop's period.
    (earliest..start)
        .rev()
        .filter(|&at| &text[at..at + REPETITION_NEEDLE] == needle)
        .take(4)
        .any(|at| {
            let period = start - at;
            let span = period * REPETITION_TIMES.max(REPETITION_MIN_BYTES.div_ceil(period));
            span <= text.len()
                && text[text.len() - span + period..]
                    == text[text.len() - span..text.len() - period]
        })
}

// About six bytes per token of the ChatGPT plan's output allowance: far beyond any real candidate.
const CHATGPT_MAX_TEXT_BYTES: usize = CHATGPT_MAX_OUTPUT_TOKENS as usize * 6;

#[derive(Default)]
pub(crate) struct SseDecoder {
    line: Vec<u8>,
    data: String,
    event: Option<String>,
    event_bytes: usize,
    skip_lf: bool,
    started: bool,
}

pub(crate) struct Frame {
    pub event: Option<String>,
    pub data: String,
}

impl SseDecoder {
    pub(crate) fn finish(self) -> Result<()> {
        if self.line.is_empty() && self.data.is_empty() {
            Ok(())
        } else {
            Err(ProviderError::new(
                ErrorKind::Truncated,
                SubmissionOutcome::Unknown,
            ))
        }
    }
    pub(crate) fn push(&mut self, bytes: &[u8]) -> Result<Vec<Frame>> {
        let mut frames = vec![];
        for &byte in bytes {
            if self.skip_lf {
                self.skip_lf = false;
                if byte == b'\n' {
                    continue;
                }
            }
            self.event_bytes += 1;
            if self.event_bytes > MAX_EVENT_BYTES {
                return Err(ProviderError::new(
                    ErrorKind::OutputLimit,
                    SubmissionOutcome::Unknown,
                ));
            }
            if byte != b'\n' && byte != b'\r' {
                self.line.push(byte);
                continue;
            }
            self.skip_lf = byte == b'\r';
            let line = std::str::from_utf8(&self.line).map_err(|_| {
                ProviderError::new(ErrorKind::Malformed, SubmissionOutcome::Unknown)
            })?;
            let line = if self.started {
                line
            } else {
                self.started = true;
                line.strip_prefix('\u{feff}').unwrap_or(line)
            };
            if line.is_empty() {
                if !self.data.is_empty() {
                    self.data.pop();
                    frames.push(Frame {
                        event: self.event.take(),
                        data: std::mem::take(&mut self.data),
                    });
                } else {
                    self.event = None;
                }
                self.event_bytes = 0;
            } else if !line.starts_with(':') {
                let (field, value) = line.split_once(':').unwrap_or((line, ""));
                let value = value.strip_prefix(' ').unwrap_or(value);
                match field {
                    "data" => {
                        self.data.push_str(value);
                        self.data.push('\n');
                    }
                    "event" => {
                        if value.len() > 128 || !value.is_ascii() {
                            return Err(ProviderError::new(
                                ErrorKind::Malformed,
                                SubmissionOutcome::Unknown,
                            ));
                        }
                        self.event = Some(value.into());
                    }
                    _ => {}
                }
            }
            self.line.clear();
        }
        Ok(frames)
    }
}

pub(crate) fn event_stream_prefix(bytes: &[u8]) -> Option<bool> {
    const BOM: &[u8] = b"\xef\xbb\xbf";
    if bytes.len() < BOM.len() && BOM.starts_with(bytes) {
        return None;
    }
    let bytes = bytes.strip_prefix(BOM).unwrap_or(bytes);
    let bytes = bytes
        .iter()
        .position(|byte| !matches!(byte, b'\r' | b'\n'))
        .map(|start| &bytes[start..])
        .unwrap_or_default();
    if bytes.is_empty() {
        return None;
    }
    if bytes[0] == b':' {
        return Some(true);
    }
    for field in [b"data".as_slice(), b"event", b"id", b"retry"] {
        if bytes.starts_with(field) {
            return bytes
                .get(field.len())
                .map(|byte| matches!(byte, b':' | b'\r' | b'\n'));
        }
        if field.starts_with(bytes) {
            return None;
        }
    }
    Some(false)
}

/// One streamed output text part.
#[derive(Default)]
struct StreamedText {
    item_id: Option<String>,
    phase: TextPhase,
}

/// A text part is either still growing by deltas or already complete with its final text.
enum TextPhase {
    /// Still growing; `delta` is the text received so far.
    Streaming { delta: String },
    /// The provider sent the part's final text.
    Done { text: String },
}

impl Default for TextPhase {
    fn default() -> Self {
        Self::Streaming {
            delta: String::new(),
        }
    }
}

impl StreamedText {
    /// Whether the provider already sent this part's final text.
    fn is_done(&self) -> bool {
        matches!(self.phase, TextPhase::Done { .. })
    }

    /// The deltas received so far, while the part is still streaming.
    fn streaming_delta(&self) -> Option<&str> {
        match &self.phase {
            TextPhase::Streaming { delta } => Some(delta),
            TextPhase::Done { .. } => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ItemKind {
    Message,
    Reasoning,
    WebSearch,
}

struct StreamedItem {
    id: String,
    kind: ItemKind,
    phase: ItemPhase,
}

/// An output item is announced first and completed later; only completing carries data.
enum ItemPhase {
    /// Announced by `response.output_item.added`.
    Added,
    /// Completed by `response.output_item.done`; a message keeps the body that is later
    /// checked against the terminal output.
    Done { message: Option<Value> },
}

impl ItemPhase {
    fn is_done(&self) -> bool {
        matches!(self, Self::Done { .. })
    }

    /// The body a completed message item carried.
    fn message(&self) -> Option<&Value> {
        match self {
            Self::Added => None,
            Self::Done { message } => message.as_ref(),
        }
    }
}

#[derive(PartialEq, Eq)]
enum MessagePart<'a> {
    Text(&'a str),
    Refusal,
}

#[derive(PartialEq, Eq)]
struct Message<'a> {
    id: Option<&'a str>,
    phase: Option<&'a str>,
    parts: Vec<MessagePart<'a>>,
}

pub(crate) struct ResponseStream<'a> {
    authentication: AuthenticationMethod,
    key: &'a ApiKey,
    job_id: String,
    digest: &'a str,
    max_output_tokens: u32,
    max_text_bytes: usize,
    whitespace_run: usize,
    next_repetition_check: usize,
    request_id: Option<Box<str>>,
    content_type: Option<Box<str>>,
    pub(crate) received_bytes: usize,
    pub(crate) body_kind: ResponseBodyKind,
    pub(crate) response_id: Option<String>,
    pub(crate) usage: Option<TokenUsage>,
    texts: BTreeMap<(Option<u64>, u64), StreamedText>,
    items: BTreeMap<u64, StreamedItem>,
    text_bytes: usize,
    finalized_text_bytes: usize,
    empty_added_items: u32,
    terminal_output: TerminalOutput,
    terminal_items: u32,
    terminal_text_parts: u32,
    refused: bool,
}

impl<'a> ResponseStream<'a> {
    pub(crate) fn new(
        authentication: AuthenticationMethod,
        key: &'a ApiKey,
        job_id: String,
        digest: &'a str,
        max_output_tokens: u32,
        request_id: Option<&str>,
        content_type: Option<&str>,
    ) -> Self {
        let request_id = ProviderError::new(ErrorKind::Malformed, SubmissionOutcome::Unknown)
            .with_provider_metadata(key, Some(200), None, None, request_id)
            .request_id;
        Self {
            authentication,
            key,
            job_id,
            digest,
            max_output_tokens,
            // ChatGPT ignores max_output_tokens, so its reply text is capped here instead.
            max_text_bytes: match authentication {
                AuthenticationMethod::ApiKey => MAX_OUTPUT_BYTES,
                AuthenticationMethod::Chatgpt => CHATGPT_MAX_TEXT_BYTES,
            },
            whitespace_run: 0,
            next_repetition_check: REPETITION_CHECK_BYTES,
            request_id,
            content_type: content_type.map(Box::from),
            received_bytes: 0,
            body_kind: ResponseBodyKind::Unavailable,
            response_id: None,
            usage: None,
            texts: BTreeMap::new(),
            items: BTreeMap::new(),
            text_bytes: 0,
            finalized_text_bytes: 0,
            empty_added_items: 0,
            terminal_output: TerminalOutput::NotReceived,
            terminal_items: 0,
            terminal_text_parts: 0,
            refused: false,
        }
    }

    pub(crate) fn error(&self, kind: ErrorKind, outcome: SubmissionOutcome) -> ProviderError {
        ProviderError::new(kind, outcome)
            .with_authentication(self.authentication)
            .with_response(self.response_id.clone(), self.usage)
            .with_provider_metadata(self.key, Some(200), None, None, self.request_id.as_deref())
            .with_response_format(
                self.key,
                self.content_type.as_deref(),
                self.body_kind,
                self.received_bytes,
                None,
            )
    }

    pub(crate) fn byte_limit_error(
        &self,
        limit: usize,
        outcome: SubmissionOutcome,
    ) -> ProviderError {
        let mut error = self.error(ErrorKind::OutputLimit, outcome);
        let format = error
            .response_format
            .as_mut()
            .expect("response format added");
        format.body_truncated = true;
        format.limit_bytes = Some(limit);
        error
    }

    fn reject(&self, reason: ResponseRejection, outcome: SubmissionOutcome) -> ProviderError {
        let mut error = self.error(ErrorKind::Malformed, outcome);
        error
            .response_format
            .as_mut()
            .expect("response format added")
            .rejection = Some(reason);
        error
    }

    fn correlation_error(
        &self,
        reason: ResponseRejection,
        cause: CorrelationCause,
        location: (Option<u64>, Option<u64>),
        outcome: SubmissionOutcome,
    ) -> ProviderError {
        let count = |value: usize| u32::try_from(value).unwrap_or(u32::MAX);
        let done_items = self
            .items
            .values()
            .filter(|item| item.phase.is_done())
            .count();
        let mut error = self.reject(reason, outcome);
        error
            .response_format
            .as_mut()
            .expect("response format added")
            .correlation = Some(Box::new(ResponseCorrelation {
            terminal_output: self.terminal_output,
            terminal_items: self.terminal_items,
            terminal_text_parts: self.terminal_text_parts,
            streamed_parts: count(self.texts.len()),
            finalized_items: count(done_items),
            empty_added_items: self.empty_added_items,
            cause,
            output_index: location.0,
            content_index: location.1,
        }));
        error
    }

    fn item_id<'b>(
        &self,
        value: Option<&'b Value>,
        outcome: SubmissionOutcome,
    ) -> Result<Option<&'b str>> {
        let Some(value) = value.filter(|value| !value.is_null()) else {
            return Ok(None);
        };
        let id = value
            .as_str()
            .ok_or_else(|| self.reject(ResponseRejection::TextIdentity, outcome))?;
        if self.key.appears_in(id) {
            return Err(self.reject(ResponseRejection::CredentialEcho, outcome));
        }
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err(self.reject(ResponseRejection::TextIdentity, outcome));
        }
        Ok(Some(id))
    }

    fn item_kind(&self, item: &Value, outcome: SubmissionOutcome) -> Result<ItemKind> {
        match item.get("type").and_then(Value::as_str) {
            Some("message") => Ok(ItemKind::Message),
            Some("reasoning") => Ok(ItemKind::Reasoning),
            Some("web_search_call") => Ok(ItemKind::WebSearch),
            _ => Err(self.reject(ResponseRejection::OutputShape, outcome)),
        }
    }

    fn message<'b>(&self, item: &'b Value, outcome: SubmissionOutcome) -> Result<Message<'b>> {
        let id = self.item_id(item.get("id"), outcome)?;
        if item.get("role").and_then(Value::as_str) != Some("assistant") {
            return Err(self.reject(ResponseRejection::MessageRole, outcome));
        }
        if item
            .get("status")
            .is_some_and(|status| status.as_str() != Some("completed"))
        {
            return Err(self.reject(ResponseRejection::MessageState, outcome));
        }
        let phase = match item.get("phase") {
            None | Some(Value::Null) => None,
            Some(Value::String(phase))
                if matches!(phase.as_str(), "commentary" | "final_answer") =>
            {
                Some(phase.as_str())
            }
            _ => return Err(self.reject(ResponseRejection::MessagePhase, outcome)),
        };
        let content = item
            .get("content")
            .and_then(Value::as_array)
            .ok_or_else(|| self.reject(ResponseRejection::ContentShape, outcome))?;
        let mut parts = vec![];
        let mut bytes = 0_usize;
        for part in content {
            match part.get("type").and_then(Value::as_str) {
                Some("refusal") => parts.push(MessagePart::Refusal),
                Some("output_text") => {
                    let text = part
                        .get("text")
                        .and_then(Value::as_str)
                        .ok_or_else(|| self.reject(ResponseRejection::ContentShape, outcome))?;
                    bytes = bytes.saturating_add(text.len());
                    if bytes > MAX_OUTPUT_BYTES {
                        return Err(self.byte_limit_error(MAX_OUTPUT_BYTES, outcome));
                    }
                    if self.key.appears_in(text) {
                        return Err(self
                            .reject(ResponseRejection::CredentialEcho, outcome)
                            .with_diagnostic_reply(text, self.key));
                    }
                    parts.push(MessagePart::Text(text));
                }
                _ => return Err(self.reject(ResponseRejection::ContentShape, outcome)),
            }
        }
        Ok(Message { id, phase, parts })
    }

    fn track_item(&mut self, data: &Value, done: bool) -> Result<()> {
        let outcome = SubmissionOutcome::Unknown;
        let Some(item) = data.get("item").filter(|item| !item.is_null()) else {
            if !done {
                self.empty_added_items = self.empty_added_items.saturating_add(1);
                return Ok(());
            }
            return Err(self.reject(ResponseRejection::OutputShape, outcome));
        };
        let index = data
            .get("output_index")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                self.correlation_error(
                    ResponseRejection::TextLocation,
                    CorrelationCause::InvalidIndex,
                    (None, None),
                    outcome,
                )
            })?;
        let location = (Some(index), None);
        let id = self.item_id(item.get("id"), outcome)?.ok_or_else(|| {
            self.correlation_error(
                ResponseRejection::TextIdentity,
                CorrelationCause::MissingIdentity,
                location,
                outcome,
            )
        })?;
        let kind = self.item_kind(item, outcome)?;
        if self
            .items
            .iter()
            .any(|(other, value)| *other != index && value.id == id)
        {
            return Err(self.correlation_error(
                ResponseRejection::TextIdentity,
                CorrelationCause::ItemDuplicate,
                location,
                outcome,
            ));
        }
        if let Some(previous) = self.items.get(&index) {
            if previous.id != id || previous.kind != kind {
                return Err(self.correlation_error(
                    ResponseRejection::TextIdentity,
                    CorrelationCause::IdentityChanged,
                    location,
                    outcome,
                ));
            }
            if !done || previous.phase.is_done() {
                return Err(self.correlation_error(
                    ResponseRejection::TextOrder,
                    CorrelationCause::ItemChanged,
                    location,
                    outcome,
                ));
            }
        }
        if self.texts.iter().any(|((other, _), text)| {
            (*other == Some(index)
                && (kind != ItemKind::Message
                    || text
                        .item_id
                        .as_deref()
                        .is_some_and(|previous| previous != id)))
                || (*other != Some(index) && text.item_id.as_deref() == Some(id))
        }) {
            return Err(self.correlation_error(
                ResponseRejection::TextIdentity,
                CorrelationCause::IdentityChanged,
                location,
                outcome,
            ));
        }
        let phase = if !done {
            ItemPhase::Added
        } else if kind == ItemKind::Message {
            let parsed = self.message(item, outcome)?;
            let bytes: usize = parsed
                .parts
                .iter()
                .map(|part| match part {
                    MessagePart::Text(text) => text.len(),
                    MessagePart::Refusal => 0,
                })
                .sum();
            let total = self.finalized_text_bytes.saturating_add(bytes);
            if total > MAX_OUTPUT_BYTES {
                return Err(self.byte_limit_error(MAX_OUTPUT_BYTES, outcome));
            }
            self.finalized_text_bytes = total;
            ItemPhase::Done {
                message: Some(item.clone()),
            }
        } else {
            ItemPhase::Done { message: None }
        };
        self.items.insert(
            index,
            StreamedItem {
                id: id.into(),
                kind,
                phase,
            },
        );
        Ok(())
    }

    fn track_text(&mut self, data: &Value, done: bool) -> Result<()> {
        let invalid = || {
            self.correlation_error(
                ResponseRejection::TextLocation,
                CorrelationCause::InvalidIndex,
                (None, None),
                SubmissionOutcome::Unknown,
            )
        };
        let output_index = data
            .get("output_index")
            .map(|value| value.as_u64().ok_or_else(invalid))
            .transpose()?;
        let content_index = data
            .get("content_index")
            .map(|value| value.as_u64().ok_or_else(invalid))
            .transpose()?;
        let location = if output_index.is_none() && content_index.is_none() && self.texts.len() == 1
        {
            *self.texts.first_key_value().expect("one text part").0
        } else {
            (output_index, content_index.unwrap_or(0))
        };
        let text = data
            .get(if done { "text" } else { "delta" })
            .and_then(Value::as_str)
            .ok_or_else(|| {
                self.reject(ResponseRejection::ContentShape, SubmissionOutcome::Unknown)
            })?;
        let item_id = self.item_id(data.get("item_id"), SubmissionOutcome::Unknown)?;
        let tracked_item = location.0.and_then(|index| self.items.get(&index));
        if let Some(item) = tracked_item {
            if item.kind != ItemKind::Message || item_id.is_some_and(|id| id != item.id) {
                return Err(self.correlation_error(
                    ResponseRejection::TextIdentity,
                    CorrelationCause::IdentityChanged,
                    (location.0, Some(location.1)),
                    SubmissionOutcome::Unknown,
                ));
            }
            if item.phase.is_done() {
                return Err(self.reject(ResponseRejection::TextOrder, SubmissionOutcome::Unknown));
            }
        }
        let item_id = item_id.or_else(|| tracked_item.map(|item| item.id.as_str()));
        if let (Some(index), Some(id)) = (location.0, item_id)
            && self
                .items
                .iter()
                .any(|(other, item)| *other != index && item.id == id)
        {
            return Err(self.correlation_error(
                ResponseRejection::TextIdentity,
                CorrelationCause::IdentityChanged,
                (location.0, Some(location.1)),
                SubmissionOutcome::Unknown,
            ));
        }
        let previous = self.texts.get(&location);
        if previous.is_some_and(StreamedText::is_done) {
            return Err(self.reject(ResponseRejection::TextOrder, SubmissionOutcome::Unknown));
        }
        if let Some(id) = item_id
            && self.texts.iter().any(|(key, part)| {
                (key.0 == location.0
                    && part
                        .item_id
                        .as_deref()
                        .is_some_and(|previous| previous != id))
                    || (key.0 != location.0 && part.item_id.as_deref() == Some(id))
            })
        {
            return Err(self.correlation_error(
                ResponseRejection::TextIdentity,
                CorrelationCause::IdentityChanged,
                (location.0, Some(location.1)),
                SubmissionOutcome::Unknown,
            ));
        }
        if done
            && previous
                .and_then(StreamedText::streaming_delta)
                .is_some_and(|delta| delta != text)
        {
            return Err(self.reject(ResponseRejection::TextMismatch, SubmissionOutcome::Unknown));
        }
        let previous_bytes = previous
            .and_then(StreamedText::streaming_delta)
            .map_or(0, str::len);
        let next_bytes = if done {
            text.len()
        } else {
            previous_bytes.saturating_add(text.len())
        };
        let total = self
            .text_bytes
            .saturating_sub(previous_bytes)
            .saturating_add(next_bytes);
        if total > self.max_text_bytes {
            return Err(self.byte_limit_error(self.max_text_bytes, SubmissionOutcome::Unknown));
        }
        if !done {
            let trailing = text.len() - text.trim_end_matches([' ', '\n', '\r', '\t']).len();
            self.whitespace_run = if trailing == text.len() {
                self.whitespace_run + trailing
            } else {
                trailing
            };
            if self.whitespace_run > MAX_WHITESPACE_RUN {
                return Err(self.error(ErrorKind::Repetition, SubmissionOutcome::Unknown));
            }
        }
        self.text_bytes = total;
        let part = self.texts.entry(location).or_default();
        if let Some(id) = item_id {
            part.item_id = Some(id.into());
        }
        if done {
            part.phase = TextPhase::Done { text: text.into() };
            return Ok(());
        }
        let TextPhase::Streaming { delta } = &mut part.phase else {
            // Unreachable: a completed part rejects new text above.
            return Err(self.reject(ResponseRejection::TextOrder, SubmissionOutcome::Unknown));
        };
        delta.push_str(text);
        if delta.len() >= self.next_repetition_check {
            self.next_repetition_check = delta.len() + REPETITION_CHECK_BYTES;
            if ends_in_repetition(delta.as_bytes()) {
                return Err(self.error(ErrorKind::Repetition, SubmissionOutcome::Unknown));
            }
        }
        Ok(())
    }

    pub(crate) fn web_search_calls(&self) -> usize {
        self.items
            .values()
            .filter(|item| item.kind == ItemKind::WebSearch)
            .count()
    }

    pub(crate) fn citations(&self) -> usize {
        self.items
            .values()
            .filter_map(|item| item.phase.message())
            .filter_map(|message| message.get("content").and_then(Value::as_array))
            .flatten()
            .filter_map(|part| part.get("annotations").and_then(Value::as_array))
            .flatten()
            .filter(|annotation| {
                annotation.get("type").and_then(Value::as_str) == Some("url_citation")
            })
            .count()
    }

    pub(crate) fn accept(&mut self, frame: Frame) -> Result<Option<ProviderResponse>> {
        if frame.data == "[DONE]" {
            return Err(self.error(ErrorKind::Truncated, SubmissionOutcome::Unknown));
        }
        let data: Value = serde_json::from_str(&frame.data)
            .map_err(|_| self.reject(ResponseRejection::EventJson, SubmissionOutcome::Unknown))?;
        let kind = data
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| self.reject(ResponseRejection::EventType, SubmissionOutcome::Unknown))?;
        if frame.event.as_deref().is_some_and(|event| event != kind) {
            return Err(self.reject(ResponseRejection::EventName, SubmissionOutcome::Unknown));
        }
        match kind {
            "response.created" | "response.in_progress" => {
                let response = data.get("response").ok_or_else(|| {
                    self.reject(
                        ResponseRejection::ResponseIdentity,
                        SubmissionOutcome::Unknown,
                    )
                })?;
                self.read_identity(response)?;
            }
            "response.output_text.delta" => {
                self.track_text(&data, false)?;
            }
            "response.output_text.done" => {
                self.track_text(&data, true)?;
            }
            "response.output_item.added" => self.track_item(&data, false)?,
            "response.output_item.done" => self.track_item(&data, true)?,
            "response.refusal.delta" | "response.refusal.done" => self.refused = true,
            "response.completed" => {
                let response = data.get("response").ok_or_else(|| {
                    self.reject(
                        ResponseRejection::ResponseIdentity,
                        SubmissionOutcome::Received,
                    )
                })?;
                self.read_identity(response)?;
                self.read_usage(response)?;
                if response.get("status").and_then(Value::as_str) != Some("completed") {
                    return Err(self.error(ErrorKind::Incomplete, SubmissionOutcome::Received));
                }
                return self.complete(response).map(Some);
            }
            "response.failed" | "response.incomplete" => {
                let response = data.get("response").ok_or_else(|| {
                    self.reject(
                        ResponseRejection::ResponseIdentity,
                        SubmissionOutcome::Unknown,
                    )
                })?;
                self.read_identity(response)?;
                self.read_usage(response)?;
                let code = response.pointer("/error/code").and_then(Value::as_str);
                let kind = if kind == "response.incomplete" {
                    ErrorKind::Incomplete
                } else {
                    error_code(self.authentication, code)
                };
                return Err(self
                    .error(kind, SubmissionOutcome::Received)
                    .with_provider_metadata(
                        self.key,
                        Some(200),
                        code,
                        response.pointer("/error/param").and_then(Value::as_str),
                        self.request_id.as_deref(),
                    ));
            }
            "error" => {
                let code = data.get("code").and_then(Value::as_str);
                return Err(self
                    .error(
                        error_code(self.authentication, code),
                        SubmissionOutcome::Unknown,
                    )
                    .with_provider_metadata(
                        self.key,
                        Some(200),
                        code,
                        data.get("param").and_then(Value::as_str),
                        self.request_id.as_deref(),
                    ));
            }
            _ => {
                if !kind.starts_with("response.") {
                    return Err(
                        self.reject(ResponseRejection::EventType, SubmissionOutcome::Unknown)
                    );
                }
            }
        }
        Ok(None)
    }

    fn read_identity(&mut self, response: &Value) -> Result<()> {
        let id = response.get("id").and_then(Value::as_str).ok_or_else(|| {
            self.reject(
                ResponseRejection::ResponseIdentity,
                SubmissionOutcome::Unknown,
            )
        })?;
        if id.len() < 6
            || id.len() > 128
            || !id.starts_with("resp_")
            || self.key.appears_in(id)
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            || self
                .response_id
                .as_deref()
                .is_some_and(|previous| previous != id)
        {
            return Err(self.reject(
                ResponseRejection::ResponseIdentity,
                SubmissionOutcome::Unknown,
            ));
        }
        self.response_id = Some(id.into());
        Ok(())
    }

    fn read_usage(&mut self, response: &Value) -> Result<()> {
        let Some(usage) = response.get("usage").filter(|usage| !usage.is_null()) else {
            return Ok(());
        };
        let input = usage.get("input_tokens").and_then(Value::as_u64);
        let output = usage.get("output_tokens").and_then(Value::as_u64);
        let total = usage.get("total_tokens").and_then(Value::as_u64);
        let (Some(input_tokens), Some(output_tokens), Some(total_tokens)) = (input, output, total)
        else {
            return Err(self.reject(ResponseRejection::Usage, SubmissionOutcome::Received));
        };
        if input_tokens.checked_add(output_tokens) != Some(total_tokens) {
            return Err(self.reject(ResponseRejection::Usage, SubmissionOutcome::Received));
        }
        self.usage = Some(TokenUsage {
            input_tokens,
            output_tokens,
            total_tokens,
        });
        if output_tokens > u64::from(self.max_output_tokens)
            || total_tokens > crate::CONTEXT_WINDOW_TOKENS
        {
            return Err(self.error(ErrorKind::OutputLimit, SubmissionOutcome::Received));
        }
        Ok(())
    }

    fn complete(&mut self, response: &Value) -> Result<ProviderResponse> {
        let outcome = SubmissionOutcome::Received;
        self.terminal_output = match response.get("output") {
            None => TerminalOutput::Absent,
            Some(Value::Null) => TerminalOutput::Null,
            Some(Value::Array(_)) => TerminalOutput::Array,
            _ => TerminalOutput::Invalid,
        };
        let terminal = response.get("output").and_then(Value::as_array);
        self.terminal_items =
            terminal.map_or(0, |items| u32::try_from(items.len()).unwrap_or(u32::MAX));
        self.terminal_text_parts = terminal
            .into_iter()
            .flatten()
            .filter_map(|item| item.get("content").and_then(Value::as_array))
            .flatten()
            .filter(|part| part.get("type").and_then(Value::as_str) == Some("output_text"))
            .fold(0_u32, |count, _| count.saturating_add(1));
        let mut resolved = vec![];
        let mut terminal_ids = BTreeMap::new();
        if let Some(output) = terminal {
            for item in output {
                self.item_kind(item, outcome)?;
                if let Some(id) = self.item_id(item.get("id"), outcome)?
                    && terminal_ids.insert(id, item).is_some()
                {
                    return Err(self.correlation_error(
                        ResponseRejection::TextIdentity,
                        CorrelationCause::ItemDuplicate,
                        (None, None),
                        outcome,
                    ));
                }
                resolved.push(item);
            }
            for (index, tracked) in &self.items {
                if let Some(item) = terminal_ids.get(tracked.id.as_str()) {
                    if self.item_kind(item, outcome)? != tracked.kind {
                        return Err(self.correlation_error(
                            ResponseRejection::TextIdentity,
                            CorrelationCause::IdentityChanged,
                            (Some(*index), None),
                            outcome,
                        ));
                    }
                    if let Some(done) = tracked.phase.message()
                        && self.message(done, outcome)? != self.message(item, outcome)?
                    {
                        return Err(self.correlation_error(
                            ResponseRejection::TextMismatch,
                            CorrelationCause::ItemChanged,
                            (Some(*index), None),
                            outcome,
                        ));
                    }
                } else if !tracked.phase.is_done() {
                    return Err(self.correlation_error(
                        ResponseRejection::TextLocation,
                        CorrelationCause::UnfinishedItem,
                        (Some(*index), None),
                        outcome,
                    ));
                } else if let Some(done) = tracked.phase.message() {
                    resolved.push(done);
                }
            }
        } else if matches!(
            self.terminal_output,
            TerminalOutput::Absent | TerminalOutput::Null
        ) && !self.items.is_empty()
            && self.items.values().all(|item| item.phase.is_done())
        {
            resolved.extend(self.items.values().filter_map(|item| item.phase.message()));
        } else {
            return Err(self.correlation_error(
                ResponseRejection::OutputShape,
                CorrelationCause::UnfinishedItem,
                (None, None),
                outcome,
            ));
        }
        let mut parts_by_id = BTreeMap::new();
        let mut all_parts = vec![];
        let mut message_ids = BTreeMap::new();
        let mut answers = vec![];
        let mut terminal_bytes = 0_usize;
        let mut refused = self.refused;
        for (output_position, item) in resolved.into_iter().enumerate() {
            if self.item_kind(item, outcome)? != ItemKind::Message {
                continue;
            }
            let message = self.message(item, outcome)?;
            if let Some(id) = message.id {
                message_ids.insert(id, ());
            }
            let mut parts = vec![];
            for (content_index, part) in message.parts.iter().enumerate() {
                match part {
                    MessagePart::Refusal => refused = true,
                    MessagePart::Text(text) => {
                        terminal_bytes = terminal_bytes.saturating_add(text.len());
                        if terminal_bytes > MAX_OUTPUT_BYTES {
                            return Err(self.byte_limit_error(MAX_OUTPUT_BYTES, outcome));
                        }
                        let content_index = content_index as u64;
                        if let Some(id) = message.id {
                            parts_by_id.insert((id, content_index), *text);
                        }
                        all_parts.push((output_position as u64, content_index, *text));
                        parts.push(*text);
                    }
                }
            }
            if message.phase != Some("commentary") {
                answers.push(parts);
            }
        }
        if refused {
            return Err(self.error(ErrorKind::Refused, outcome));
        }
        for ((output_index, content_index), streamed) in &self.texts {
            let location = (*output_index, Some(*content_index));
            let id = streamed.item_id.as_deref().or_else(|| {
                output_index
                    .and_then(|index| self.items.get(&index))
                    .map(|item| item.id.as_str())
            });
            let text = if let Some(id) = id {
                if !message_ids.contains_key(id) {
                    return Err(self.correlation_error(
                        ResponseRejection::TextIdentity,
                        CorrelationCause::ItemAbsent,
                        location,
                        outcome,
                    ));
                }
                parts_by_id
                    .get(&(id, *content_index))
                    .copied()
                    .ok_or_else(|| {
                        self.correlation_error(
                            ResponseRejection::TextLocation,
                            CorrelationCause::ContentPartAbsent,
                            location,
                            outcome,
                        )
                    })?
            } else if self.texts.len() == 1
                && all_parts.len() == 1
                && self.items.is_empty()
                && output_index.is_none_or(|index| index == all_parts[0].0)
                && all_parts[0].1 == *content_index
            {
                all_parts[0].2
            } else {
                return Err(self.correlation_error(
                    ResponseRejection::TextLocation,
                    CorrelationCause::MissingIdentity,
                    location,
                    outcome,
                ));
            };
            // The streamed text is either the deltas received so far or the part's final text.
            let streamed_text = match &streamed.phase {
                TextPhase::Streaming { delta } => delta.as_str(),
                TextPhase::Done { text: done } => done.as_str(),
            };
            if streamed_text != text {
                return Err(self.correlation_error(
                    ResponseRejection::TextMismatch,
                    CorrelationCause::ItemChanged,
                    location,
                    outcome,
                ));
            }
        }
        if answers.len() != 1 || answers[0].is_empty() {
            return Err(self.reject(ResponseRejection::AnswerCount, outcome));
        }
        let text = answers[0].concat();
        if self.key.appears_in(&text) {
            return Err(self
                .reject(ResponseRejection::CredentialEcho, outcome)
                .with_diagnostic_reply(&text, self.key));
        }
        let candidate: Value = serde_json::from_str(&text).map_err(|_| {
            self.reject(ResponseRejection::CandidateJson, outcome)
                .with_diagnostic_reply(&text, self.key)
        })?;
        if !candidate.is_object() {
            return Err(self
                .reject(ResponseRejection::CandidateShape, outcome)
                .with_diagnostic_reply(&text, self.key));
        }
        if candidate
            .get("jobId")
            .is_some_and(|value| value.as_str() != Some(&self.job_id))
        {
            return Err(self
                .reject(ResponseRejection::JobMismatch, outcome)
                .with_diagnostic_reply(&text, self.key));
        }
        if candidate
            .get("requirementsSha256")
            .is_some_and(|value| value.as_str() != Some(self.digest))
        {
            return Err(self
                .reject(ResponseRejection::RequirementsMismatch, outcome)
                .with_diagnostic_reply(&text, self.key));
        }
        Ok(ProviderResponse {
            text,
            response_id: self.response_id.clone().expect("response identity checked"),
            usage: self.usage,
        })
    }
}

pub(crate) fn error_code(authentication: AuthenticationMethod, code: Option<&str>) -> ErrorKind {
    if authentication == AuthenticationMethod::Chatgpt {
        let kind = match code {
            Some("subscription_sharing_user_not_eligible") => Some(ErrorKind::ChatgptNotEligible),
            Some("subscription_sharing_usage_limit_exceeded") => Some(ErrorKind::ChatgptUsageLimit),
            Some("subscription_sharing_usage_unavailable") => {
                Some(ErrorKind::ChatgptUsageUnavailable)
            }
            Some("subscription_sharing_unsupported_capability") => {
                Some(ErrorKind::ChatgptUnsupportedCapability)
            }
            Some("subscription_sharing_route_not_supported") => {
                Some(ErrorKind::ChatgptRouteNotSupported)
            }
            Some("subscription_sharing_invalid_user") => Some(ErrorKind::ChatgptInvalidUser),
            Some(
                "chatpass_v2_scope_not_authorized" | "chatpass_v2_invalid_authorization_context",
            ) => Some(ErrorKind::ChatgptPermission),
            Some("subscription_sharing_user_unavailable") => {
                Some(ErrorKind::ChatgptUserUnavailable)
            }
            _ => None,
        };
        if let Some(kind) = kind {
            return kind;
        }
    }
    match code {
        Some("insufficient_quota" | "billing_hard_limit_reached" | "billing_not_active") => {
            ErrorKind::Quota
        }
        Some("rate_limit_exceeded") => ErrorKind::RateLimited,
        Some("invalid_api_key" | "authentication_error" | "permission_denied") => {
            ErrorKind::Authentication
        }
        Some("model_not_found" | "model_not_available") => ErrorKind::ModelUnavailable,
        _ => ErrorKind::RequestRejected,
    }
}
