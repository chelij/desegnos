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
