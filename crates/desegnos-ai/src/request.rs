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
