use serde::{Serialize, Serializer, ser::SerializeMap, ser::SerializeSeq};
use serde_json::Value;

/// Structured output follows schema property order. Keep identifiers and discriminators
/// ahead of content, and recursive children last. Do this only on the wire: changing
/// serde_json's global map ordering would also change persisted project fingerprints.
pub(crate) fn serialize_body(body: &Value) -> serde_json::Result<Vec<u8>> {
    serde_json::to_vec(&WireValue(body, false))
}

struct WireValue<'a>(&'a Value, bool);

impl Serialize for WireValue<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Value::Object(object) => {
                let mut entries: Vec<_> = object.iter().collect();
                if self.1 {
                    const FIRST: &[&str] = &[
                        "jobId",
                        "requirementsSha256",
                        "requestId",
                        "kind",
                        "elementId",
                        "status",
                        "explanation",
                        "summary",
                        "rect",
                        "name",
                    ];
                    entries.sort_by_key(|(key, _)| {
                        if key.as_str() == "children" {
                            FIRST.len() + 2
                        } else if key.as_str() == "operations" {
                            FIRST.len() + 1
                        } else {
                            FIRST
                                .iter()
                                .position(|first| first == key)
                                .unwrap_or(FIRST.len())
                        }
                    });
                }
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for (key, value) in entries {
                    map.serialize_entry(key, &WireValue(value, key == "properties"))?;
                }
                map.end()
            }
            Value::Array(values) => {
                let mut seq = serializer.serialize_seq(Some(values.len()))?;
                for value in values {
                    seq.serialize_element(&WireValue(value, false))?;
                }
                seq.end()
            }
            value => value.serialize(serializer),
        }
    }
}
