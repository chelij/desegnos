//! Validated task content, before any provider's wire format is chosen.
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

pub(crate) enum RequestPart<'a> {
    Document { name: &'static str, text: &'a str },
    Text(String),
    Image { uri: &'a str, detail: &'static str },
}

pub(crate) struct RequestContent<'a> {
    pub instructions: &'static str,
    pub parts: Vec<RequestPart<'a>>,
    pub schema: Value,
    pub schema_name: &'static str,
    pub web_search: bool,
}

impl RequestContent<'_> {
    pub fn responses_body(&self) -> Value {
        let content: Vec<_> = self
            .parts
            .iter()
            .map(|part| match part {
                RequestPart::Document { name, text } => {
                    let data = format!(
                        "data:text/markdown;base64,{}",
                        STANDARD.encode(text.as_bytes())
                    );
                    json!({"type":"input_file","filename":name,"file_data":data})
                }
                RequestPart::Text(text) => json!({"type":"input_text","text":text}),
                RequestPart::Image { uri, detail } => {
                    json!({"type":"input_image","image_url":uri,"detail":detail})
                }
            })
            .collect();
        json!({
            "instructions": self.instructions,
            "input": [{"role":"user","content":content}],
            "tools": if self.web_search { json!([{"type":"web_search"}]) } else { json!([]) },
            "tool_choice": if self.web_search { "auto" } else { "none" },
            "text": {"format":{"type":"json_schema","name":self.schema_name,"schema":self.schema,"strict":true}}
        })
    }
}

pub(crate) fn frozen_schema(
    snapshot: &desegnos_core::RequirementsSnapshot,
    schema: &serde_json::Value,
) -> crate::Result<serde_json::Value> {
    use crate::{ErrorKind, ProviderError};
    use serde_json::{Value, json};
    let invalid = || ProviderError::before(ErrorKind::InvalidRequirements);
    if snapshot.markdown.is_empty()
        || snapshot.sha256 != desegnos_core::digest(snapshot.markdown.as_bytes())
        || snapshot.job_id.is_nil()
        || snapshot.project_id.is_nil()
        || schema.get("type").and_then(Value::as_str) != Some("object")
        || schema.get("additionalProperties") != Some(&Value::Bool(false))
    {
        return Err(invalid());
    }
    if snapshot.markdown.len() > crate::MAX_REQUIREMENTS_BYTES
        || serde_json::to_vec(schema).map_err(|_| invalid())?.len() > crate::MAX_SCHEMA_BYTES
    {
        return Err(ProviderError::before(ErrorKind::InputLimit));
    }
    let mut schema = schema.clone();
    for (name, value) in [
        ("jobId", snapshot.job_id.to_string()),
        ("requirementsSha256", snapshot.sha256.clone()),
    ] {
        if !schema
            .get("required")
            .and_then(Value::as_array)
            .is_some_and(|keys| keys.iter().any(|key| key.as_str() == Some(name)))
        {
            return Err(invalid());
        }
        let property = schema
            .get_mut("properties")
            .and_then(Value::as_object_mut)
            .and_then(|properties| properties.get_mut(name))
            .and_then(Value::as_object_mut)
            .ok_or_else(invalid)?;
        if property.get("type").and_then(Value::as_str) != Some("string") {
            return Err(invalid());
        }
        property.insert("enum".into(), json!([value]));
    }
    Ok(schema)
}
