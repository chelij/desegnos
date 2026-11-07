use serde::de::DeserializeOwned;

/// Parse a provider reply, repairing raw control characters that a model emitted inside JSON
/// string literals.
///
/// Strict JSON forbids U+0000..U+001F inside a string, yet models routinely return literal
/// newlines and tabs in generated HTML, CSS or JavaScript text. The repair runs only when strict
/// parsing already failed on such a character; any other failure is reported unchanged, and a
/// reply that still fails after the repair reports the original error.
pub fn parse_provider_json<T: DeserializeOwned>(
    source: &str,
) -> std::result::Result<T, serde_json::Error> {
    match serde_json::from_str(source) {
        Err(error) if is_control_character_error(&error) => {
            serde_json::from_str(&escape_raw_control_characters(source)).map_err(|_| error)
        }
        result => result,
    }
}

fn is_control_character_error(error: &serde_json::Error) -> bool {
    // serde_json has no distinct category for this, so the parse message is the only signal.
    error.to_string().contains("control character")
}

/// Escape raw control characters that appear inside JSON string literals, leaving everything
/// else byte-for-byte intact. Nothing outside a string is touched: there whitespace is valid.
fn escape_raw_control_characters(source: &str) -> String {
    let mut repaired = String::with_capacity(source.len());
    let mut in_string = false;
    let mut escaped = false;
    for character in source.chars() {
        if !in_string {
            in_string = character == '"';
            repaired.push(character);
        } else if escaped {
            escaped = false;
            repaired.push(character);
        } else if character == '\\' {
            escaped = true;
            repaired.push(character);
        } else if character == '"' {
            in_string = false;
            repaired.push(character);
        } else if character <= '\u{1f}' {
            push_control_escape(&mut repaired, character);
        } else {
            repaired.push(character);
        }
    }
    repaired
}

fn push_control_escape(target: &mut String, character: char) {
    match character {
        '\u{8}' => target.push_str("\\b"),
        '\t' => target.push_str("\\t"),
        '\n' => target.push_str("\\n"),
        '\u{c}' => target.push_str("\\f"),
        '\r' => target.push_str("\\r"),
        character => target.push_str(&format!("\\u{:04x}", u32::from(character))),
    }
}

/// Restore only absent host echoes; null, malformed and differing supplied values are errors.
pub fn fill_reply_echoes(
    value: &mut serde_json::Value,
    job_id: uuid::Uuid,
    sha256: &str,
) -> crate::Result<()> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| crate::Error("The AI reply must be a JSON object.".into()))?;
    for (key, expected) in [
        ("jobId", job_id.to_string()),
        ("requirementsSha256", sha256.into()),
    ] {
        if let Some(actual) = object.get(key) {
            crate::ensure(
                actual.as_str() == Some(expected.as_str()),
                format!("The AI reply has a different {key} from this frozen request."),
            )?;
        } else {
            object.insert(key.into(), serde_json::Value::String(expected));
        }
    }
    Ok(())
}

/// Parse a structured model reply using the same schema sent with its frozen request.
pub fn parse_structured_reply<T: DeserializeOwned>(
    source: &str,
    schema: &serde_json::Value,
    job_id: uuid::Uuid,
    sha256: &str,
) -> crate::Result<T> {
    let mut value = parse_provider_json(source).map_err(|error| crate::Error(error.to_string()))?;
    fill_reply_echoes(&mut value, job_id, sha256)?;
    crate::conform_to_schema(&mut value, schema);
    serde_json::from_value(value).map_err(|error| crate::Error(error.to_string()))
}

/// Repair a reply that was cut off mid-stream so the operations that did arrive can still be
/// used. The text is cut back to the last complete object of the innermost open `operations`
/// array, then every open container is closed. Returns `None` when the text is already complete
/// JSON or when no `operations` array was open (nothing usable arrived). Validation of the
/// repaired reply is unchanged; a half-written operation never survives the cut.
pub fn complete_truncated_operations(text: &str) -> Option<String> {
    struct Open {
        array: bool,
        key: Option<String>,
        /// Byte offset just after this container's last complete child container.
        last_child_end: Option<usize>,
        /// Byte offset just after the opening bracket.
        start: usize,
    }
    let mut stack: Vec<Open> = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    let mut string_start = 0;
    let mut last_string: Option<String> = None;
    let mut pending_key: Option<String> = None;
    for (index, character) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
                last_string = Some(text[string_start..index].to_owned());
            }
            continue;
        }
        match character {
            '"' => {
                in_string = true;
                string_start = index + 1;
            }
            ':' => pending_key = last_string.take(),
            '{' | '[' => {
                let key = if stack.last().is_some_and(|open| !open.array) {
                    pending_key.take()
                } else {
                    None
                };
                stack.push(Open { array: character == '[', key, last_child_end: None, start: index + 1 });
            }
            '}' | ']' => {
                stack.pop()?;
                if let Some(parent) = stack.last_mut() {
                    parent.last_child_end = Some(index + 1);
                }
                pending_key = None;
            }
            ',' => {
                last_string = None;
                pending_key = None;
            }
            _ => {}
        }
    }
    if stack.is_empty() {
        return None;
    }
    let operations = stack
        .iter()
        .rposition(|open| open.array && open.key.as_deref() == Some("operations"))?;
    let cut = stack[operations].last_child_end.unwrap_or(stack[operations].start);
    let mut repaired = text[..cut].trim_end().to_owned();
    for open in stack[..=operations].iter().rev() {
        repaired.push(if open.array { ']' } else { '}' });
    }
    Some(repaired)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn raw_newline_inside_a_string_is_repaired() {
        let parsed: Value = parse_provider_json("{\"html\":\"<p>one\n<p>two\"}").unwrap();
        assert_eq!(parsed, json!({"html": "<p>one\n<p>two"}));
    }

    #[test]
    fn escaped_newline_is_untouched() {
        let source = r#"{"text":"one\ntwo"}"#;
        assert_eq!(escape_raw_control_characters(source), source);
        let parsed: Value = serde_json::from_str(source).unwrap();
        assert_eq!(parsed, json!({"text": "one\ntwo"}));
    }

    #[test]
    fn whitespace_between_tokens_is_untouched() {
        let source = "{\n\t\"a\": 1,\n\t\"b\": \"two\"\n}\n";
        assert_eq!(escape_raw_control_characters(source), source);
        let parsed: Value = parse_provider_json(source).unwrap();
        assert_eq!(parsed, json!({"a": 1, "b": "two"}));
    }

    #[test]
    fn escaped_quote_before_a_raw_newline_is_repaired() {
        let parsed: Value =
            parse_provider_json("{\"css\":\"content: \\\"x\\\"\n  color: red;\n\"}").unwrap();
        assert_eq!(parsed, json!({"css": "content: \"x\"\n  color: red;\n"}));
    }

    #[test]
    fn control_characters_get_their_json_escapes() {
        let source = "{\"a\":\"\u{0}\u{1}\u{8}\t\n\u{c}\r\"}";
        assert_eq!(
            escape_raw_control_characters(source),
            "{\"a\":\"\\u0000\\u0001\\b\\t\\n\\f\\r\"}"
        );
    }

    #[test]
    fn other_parse_errors_are_unchanged() {
        let source = "{\"a\": 1,}";
        let strict = serde_json::from_str::<Value>(source)
            .unwrap_err()
            .to_string();
        let parsed = parse_provider_json::<Value>(source)
            .unwrap_err()
            .to_string();
        assert_eq!(parsed, strict);
        assert!(strict.contains("trailing comma"), "{strict}");
    }

    #[test]
    fn truncated_operations_keep_only_complete_objects() {
        let head = r#"{"images":[],"requests":[{"requestId":"r","status":"implemented","operations":[{"kind":"insertInBoundary","pageId":"a","element":{"kind":"text","children":[]}},{"kind":"insertInBoundary","pageId":"b","element":{"kind":"te"#;
        let repaired = super::complete_truncated_operations(head).unwrap();
        let value: serde_json::Value = serde_json::from_str(&repaired).unwrap();
        let operations = value["requests"][0]["operations"].as_array().unwrap();
        assert_eq!(operations.len(), 1);
        assert_eq!(operations[0]["pageId"], "a");
        assert_eq!(value["requests"][0]["requestId"], "r");
    }

    #[test]
    fn truncated_operations_handle_cuts_in_strings_and_between_items() {
        for (cut, kept) in [
            (r#"{"requests":[{"requestId":"r","operations":[{"kind":"setPage","pageId":"a","name":"Ho"#, 0),
            (r#"{"requests":[{"requestId":"r","operations":[{"kind":"setPage","pageId":"a"},"#, 1),
            (r#"{"requests":[{"requestId":"r","operations":[{"kind":"setPage","pageId":"a","name":"A \"quoted\" {brace"},{"#, 1),
        ] {
            let repaired = super::complete_truncated_operations(cut).unwrap();
            let value: serde_json::Value = serde_json::from_str(&repaired).unwrap();
            assert_eq!(value["requests"][0]["operations"].as_array().unwrap().len(), kept, "{cut}");
        }
    }

    #[test]
    fn complete_or_operation_free_text_is_not_repaired() {
        assert_eq!(super::complete_truncated_operations(r#"{"requests":[]}"#), None);
        assert_eq!(super::complete_truncated_operations(r#"{"summary":"cut"#), None);
        let empty = super::complete_truncated_operations(r#"{"requests":[{"requestId":"r","operations":[{"kind":"ins"#).unwrap();
        let value: serde_json::Value = serde_json::from_str(&empty).unwrap();
        assert!(value["requests"][0]["operations"].as_array().unwrap().is_empty());
    }
}
