use crate::generation_schema::candidate_schema;
/// Strict structured output requires every draft property, so an omitted optional kind field
/// arrives as null. The flattened element kind cannot tell a null member from an unknown one,
/// so remove those nulls before deserialising; every other optional field is an `Option` or has
/// a default, both of which already read null as absent.
pub(crate) fn drop_null_optional_draft_fields(value: &mut serde_json::Value) {
    const NULL_AS_ABSENT: [&str; 4] = ["inputType", "options", "language", "source"];
    match value {
        serde_json::Value::Object(object) => {
            object.retain(|key, member| {
                !(member.is_null() && NULL_AS_ABSENT.contains(&key.as_str()))
            });
            for member in object.values_mut() {
                drop_null_optional_draft_fields(member);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                drop_null_optional_draft_fields(item);
            }
        }
        _ => {}
    }
}

pub(crate) fn conform_to_candidate_schema(value: &mut serde_json::Value) {
    conform_to_schema(value, &candidate_schema());
}

/// Repair provider drift using the request schema: drop forbidden extra properties, replace
/// null strings where empty is allowed, and fill missing required properties only with an
/// allowed empty value. Other values remain for deserialisation and validation to reject.
pub(crate) fn conform_to_schema(value: &mut serde_json::Value, schema: &serde_json::Value) {
    conform_value(value, schema, schema);
}

fn conform_value(
    value: &mut serde_json::Value,
    schema: &serde_json::Value,
    root: &serde_json::Value,
) {
    let schema = dereference(schema, root);
    if let Some(branches) = union_branches(schema) {
        if let Some(branch) = matching_branch(value, branches, root) {
            conform_value(value, branch, root);
        }
        return;
    }
    if value.is_null() {
        if schema_accepts_null(schema) {
            return;
        }
        if matches!(schema.get("type"), Some(serde_json::Value::String(kind)) if kind == "string")
            && empty_string_is_allowed(schema)
        {
            *value = serde_json::Value::String(String::new());
        }
        return;
    }
    if !value_fits_type(value, schema) {
        return;
    }
    match value {
        serde_json::Value::Object(object) => {
            let properties = schema
                .get("properties")
                .and_then(serde_json::Value::as_object);
            if schema
                .get("additionalProperties")
                .and_then(serde_json::Value::as_bool)
                == Some(false)
                && let Some(properties) = properties
            {
                object.retain(|key, _| properties.contains_key(key));
            }
            if let Some(required) = schema.get("required").and_then(serde_json::Value::as_array) {
                for key in required.iter().filter_map(serde_json::Value::as_str) {
                    if !object.contains_key(key)
                        && let Some(property) =
                            properties.and_then(|properties| properties.get(key))
                        && let Some(empty) = empty_schema_value(property, root)
                    {
                        object.insert(key.into(), empty);
                    }
                }
            }
            for (key, member) in object.iter_mut() {
                if let Some(property) = properties.and_then(|properties| properties.get(key)) {
                    conform_value(member, property, root);
                }
            }
        }
        serde_json::Value::Array(items) => {
            if let Some(item_schema) = schema.get("items") {
                for item in items {
                    conform_value(item, item_schema, root);
                }
            }
        }
        _ => {}
    }
}

/// No objects, numbers, booleans or nonempty enum members can be inferred from omission.
fn empty_schema_value(
    schema: &serde_json::Value,
    root: &serde_json::Value,
) -> Option<serde_json::Value> {
    use serde_json::{Value, json};
    let schema = dereference(schema, root);
    // Prefer null for nullable properties, including nullable enums and union branches.
    let null = Value::Null;
    if schema_allows_empty(&null, schema, root) {
        return Some(null);
    }
    [json!([]), json!("")]
        .into_iter()
        .find(|empty| schema_allows_empty(empty, schema, root))
}

fn schema_allows_empty(
    value: &serde_json::Value,
    schema: &serde_json::Value,
    root: &serde_json::Value,
) -> bool {
    let schema = dereference(schema, root);
    if let Some(branches) = union_branches(schema) {
        let matches = branches
            .iter()
            .filter(|branch| schema_allows_empty(value, branch, root))
            .count();
        return if schema.get("oneOf").is_some() {
            matches == 1
        } else {
            matches > 0
        };
    }
    if schema
        .get("const")
        .is_some_and(|constant| constant != value)
        || schema
            .get("enum")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|values| !values.contains(value))
    {
        return false;
    }
    // An explicit type or literal constraint must establish that the value is safe.
    if (schema.get("type").is_none()
        && schema.get("enum").is_none()
        && schema.get("const").is_none())
        || !value_fits_type(value, schema)
    {
        return false;
    }
    if value.is_array() {
        return schema
            .get("minItems")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            == 0;
    }
    value.is_null()
        || (value.is_string() && empty_string_is_allowed(schema) && schema.get("format").is_none())
}

fn dereference<'a>(
    mut schema: &'a serde_json::Value,
    root: &'a serde_json::Value,
) -> &'a serde_json::Value {
    // The candidate schema only uses flat root-relative definitions, and element drafts refer to
    // themselves through them, so one lookup per level is enough.
    while let Some(name) = schema
        .get("$ref")
        .and_then(serde_json::Value::as_str)
        .and_then(|reference| reference.strip_prefix("#/$defs/"))
    {
        let Some(target) = root.get("$defs").and_then(|defs| defs.get(name)) else {
            break;
        };
        schema = target;
    }
    schema
}

fn union_branches(schema: &serde_json::Value) -> Option<&[serde_json::Value]> {
    ["anyOf", "oneOf"].iter().find_map(|keyword| {
        schema
            .get(*keyword)
            .and_then(serde_json::Value::as_array)
            .map(Vec::as_slice)
    })
}

/// Choose the union branch a value belongs to, or none when the choice is ambiguous so the value
/// is left untouched. A null belongs to a null branch, a value that fits exactly one branch type
/// belongs to that branch, and object branches are told apart by their single-valued enum or const
/// discriminator, such as `kind`.
fn matching_branch<'a>(
    value: &serde_json::Value,
    branches: &'a [serde_json::Value],
    root: &serde_json::Value,
) -> Option<&'a serde_json::Value> {
    if value.is_null() {
        if branches
            .iter()
            .any(|branch| schema_accepts_null(dereference(branch, root)))
        {
            return None;
        }
        return single(branches);
    }
    let typed: Vec<&serde_json::Value> = branches
        .iter()
        .filter(|branch| value_fits_type(value, dereference(branch, root)))
        .collect();
    if let [only] = typed.as_slice() {
        return Some(only);
    }
    if value.is_object() {
        let discriminated: Vec<&serde_json::Value> = typed
            .iter()
            .copied()
            .filter(|branch| discriminator_matches(value, dereference(branch, root)))
            .collect();
        if let [only] = discriminated.as_slice() {
            return Some(only);
        }
    }
    None
}

fn single(branches: &[serde_json::Value]) -> Option<&serde_json::Value> {
    match branches {
        [only] => Some(only),
        _ => None,
    }
}

fn schema_accepts_null(schema: &serde_json::Value) -> bool {
    match schema.get("type") {
        None => true,
        Some(serde_json::Value::String(kind)) => kind == "null",
        Some(serde_json::Value::Array(kinds)) => {
            kinds.iter().any(|kind| kind.as_str() == Some("null"))
        }
        _ => false,
    }
}

fn value_fits_type(value: &serde_json::Value, schema: &serde_json::Value) -> bool {
    fn fits(value: &serde_json::Value, kind: &str) -> bool {
        match kind {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "number" => value.is_number(),
            "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
            "boolean" => value.is_boolean(),
            "null" => value.is_null(),
            _ => true,
        }
    }
    match schema.get("type") {
        None => true,
        Some(serde_json::Value::String(kind)) => fits(value, kind),
        Some(serde_json::Value::Array(kinds)) => kinds
            .iter()
            .filter_map(serde_json::Value::as_str)
            .any(|kind| fits(value, kind)),
        _ => true,
    }
}

/// An empty string is only substituted where the schema does not rule it out: a const, an enum
/// without `""`, a positive minimum length or any pattern can all forbid it.
fn empty_string_is_allowed(schema: &serde_json::Value) -> bool {
    if schema
        .get("const")
        .is_some_and(|value| value.as_str() != Some(""))
    {
        return false;
    }
    if let Some(values) = schema.get("enum").and_then(serde_json::Value::as_array)
        && !values.iter().any(|value| value.as_str() == Some(""))
    {
        return false;
    }
    if schema.get("minLength").and_then(serde_json::Value::as_u64) > Some(0) {
        return false;
    }
    schema.get("pattern").is_none()
}

fn discriminator_matches(value: &serde_json::Value, branch: &serde_json::Value) -> bool {
    let (Some(object), Some(properties)) = (
        value.as_object(),
        branch
            .get("properties")
            .and_then(serde_json::Value::as_object),
    ) else {
        return false;
    };
    let mut declared = 0;
    let mut matched = 0;
    for (key, property) in properties {
        let Some(expected) = discriminator_value(property) else {
            continue;
        };
        declared += 1;
        if object.get(key) == Some(expected) {
            matched += 1;
        }
    }
    declared > 0 && declared == matched
}

fn discriminator_value(schema: &serde_json::Value) -> Option<&serde_json::Value> {
    if let Some(constant) = schema.get("const") {
        return Some(constant);
    }
    match schema
        .get("enum")
        .and_then(serde_json::Value::as_array)?
        .as_slice()
    {
        [only] => Some(only),
        _ => None,
    }
}

#[cfg(test)]
mod conformance_tests {
    use super::*;
    use crate::{generation::GenerationCandidate, generation_schema::uuid_schema};
    use serde_json::{Value, json};

    const JOB: &str = "3d1f9c56-0000-4000-8000-000000000000";
    const SHA: &str = "0000000000000000000000000000000000000000000000000000000000000000";
    const PAGE: &str = "3d1f9c56-0000-4000-8000-000000000001";

    fn candidate(operations: Value) -> Value {
        json!({
            "jobId": JOB,
            "requirementsSha256": SHA,
            "summary": "Built the page",
            "requests": [{
                "requestId": null,
                "elementId": null,
                "status": "implemented",
                "explanation": "Built the page",
                "operations": operations
            }],
            "images": [],
            "pointerClassifications": [],
            "decisionProposals": [],
            "criticism": []
        })
    }

    fn conformed(mut value: Value) -> Value {
        conform_to_candidate_schema(&mut value);
        value
    }

    fn element() -> Value {
        json!({
            "kind": "text",
            "rect": {"x": 0.0, "y": 0.0, "width": 400.0, "height": 40.0},
            "text": "Hero"
        })
    }

    #[test]
    fn missing_properties_only_receive_schema_allowed_empty_values() {
        let schema = json!({
            "type":"object", "required":["array","nullable","text","number","id","choice","nonempty"],
            "properties":{
                "array":{"type":"array"},
                "nullable":{"enum":["known",null]},
                "text":{"type":"string"},
                "number":{"type":"number"},
                "id":uuid_schema(),
                "choice":{"type":"string","enum":["known"]},
                "nonempty":{"type":"array","minItems":1}
            }
        });
        let mut value = json!({});
        conform_to_schema(&mut value, &schema);
        assert_eq!(value, json!({"array":[],"nullable":null,"text":""}));
    }

    #[test]
    fn unknown_keys_are_dropped_at_depth() {
        let mut operation = json!({
            "kind": "insert",
            "parentId": PAGE,
            "index": null,
            "element": element()
        });
        operation["element"]["style"] = json!({"opacity": 1.0, "borderWidth": 2});
        operation["position"] = json!(3);
        let value = conformed(candidate(json!([operation])));
        let operation = &value["requests"][0]["operations"][0];
        assert_eq!(operation["kind"], json!("insert"), "{operation}");
        assert!(operation.get("position").is_none(), "{operation}");
        assert!(
            operation["element"]["style"].get("borderWidth").is_none(),
            "{operation}"
        );
        assert_eq!(operation["element"]["style"]["opacity"], json!(1.0));
        assert_eq!(operation["element"]["text"], json!("Hero"));
    }

    #[test]
    fn a_boundary_insert_with_a_stray_index_loses_it() {
        let value = conformed(candidate(json!([{
            "kind": "insertInBoundary",
            "pageId": PAGE,
            "sizeId": null,
            "index": 0,
            "element": element()
        }])));
        let operation = &value["requests"][0]["operations"][0];
        assert!(operation.get("index").is_none(), "{operation}");
        assert_eq!(operation["element"]["text"], json!("Hero"));
    }

    #[test]
    fn a_null_string_becomes_empty_where_the_schema_allows_it() {
        let mut value = candidate(json!([]));
        value["summary"] = Value::Null;
        value["requests"][0]["explanation"] = Value::Null;
        let value = conformed(value);
        assert_eq!(value["summary"], json!(""));
        assert_eq!(value["requests"][0]["explanation"], json!(""));
    }

    #[test]
    fn a_null_enum_member_is_left_for_validation() {
        let mut value = candidate(json!([]));
        value["requests"][0]["status"] = Value::Null;
        let value = conformed(value);
        assert_eq!(value["requests"][0]["status"], Value::Null);
        let error = serde_json::from_value::<GenerationCandidate>(value)
            .unwrap_err()
            .to_string();
        assert!(error.contains("invalid type: null"), "{error}");
    }

    #[test]
    fn schema_repair_is_idempotent() {
        let mut drafted = element();
        drafted["style"] = json!({"opacity": 1.0, "textStyle": {"size": 32.0, "align": "center"}});
        drafted["children"] = json!([]);
        let value = candidate(json!([
            {"kind": "insertInBoundary", "pageId": PAGE, "sizeId": null, "element": drafted},
            {"kind": "remove", "elementId": PAGE}
        ]));
        let value = conformed(value);
        let before = serde_json::to_string(&value).unwrap();
        let after = conformed(value);
        assert_eq!(serde_json::to_string(&after).unwrap(), before);
    }

    #[test]
    fn a_type_mismatch_other_than_null_still_fails() {
        let mut value = candidate(json!([]));
        value["summary"] = json!(42);
        let value = conformed(value);
        assert_eq!(value["summary"], json!(42));
        let error = serde_json::from_value::<GenerationCandidate>(value)
            .unwrap_err()
            .to_string();
        assert!(error.contains("invalid type: integer"), "{error}");
    }
}
