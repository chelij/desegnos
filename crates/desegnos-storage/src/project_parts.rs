//! Split a project into named, content-addressed parts and join them back.
//!
//! One part per boundary holds that boundary's sizes (each with its canvas placement) and its
//! element tree, including every element's per-size overrides. The
//! rest of the design is split into `reference`, `pointers`, `brief`, `identity`, `dossier`
//! and `settings` (everything a boundary does not own: grid, mode, decisions, revision).
//! Picture bytes are parts of their own. Every part is canonical JSON or raw picture bytes,
//! so equal content always hashes equally and unchanged parts are stored once.
use crate::ensure;
use base64::{Engine, engine::general_purpose::STANDARD};
use desegnos_core::{Error, Project, Result};
use serde_json::{Map, Value};
use std::{collections::BTreeMap, sync::Arc};
use uuid::Uuid;

pub const SETTINGS: &str = "settings";
pub const REFERENCE: &str = "reference";
pub const POINTERS: &str = "pointers";
pub const BRIEF: &str = "brief";
pub const IDENTITY: &str = "identity";
pub const DOSSIER: &str = "dossier";

const BOUNDARY: &str = "boundary";
const PICTURE: &str = "picture";

/// One part of a project, ready to be stored as a blob.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PartData {
    Json(Vec<u8>),
    Picture { mime: String, bytes: Vec<u8> },
}

impl PartData {
    /// The blob kind: JSON parts share one kind, a picture carries its media type.
    pub fn kind(&self) -> &str {
        match self {
            Self::Json(_) => "json",
            Self::Picture { mime, .. } => mime,
        }
    }

    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::Json(bytes) => bytes,
            Self::Picture { bytes, .. } => bytes,
        }
    }
}

/// `boundary/<index>/<uuid>`: the index keeps page order, and moving a page reuses its blob.
pub fn boundary_part(index: usize, id: Uuid) -> String {
    format!("{BOUNDARY}/{index:04}/{id}")
}

pub fn picture_part(id: Uuid) -> String {
    format!("{PICTURE}/{id}")
}

fn boundary_index(name: &str) -> Option<usize> {
    let rest = name.strip_prefix(BOUNDARY)?.strip_prefix('/')?;
    let (index, _) = rest.split_once('/')?;
    index.parse().ok()
}

fn picture_id(name: &str) -> Option<Uuid> {
    Uuid::parse_str(name.strip_prefix(PICTURE)?.strip_prefix('/')?).ok()
}

/// One place for the bytes a hash stands for. Sorted keys make equal content equal.
pub fn canonical_json(value: &Value) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_canonical(value, &mut bytes);
    bytes
}

fn write_canonical(value: &Value, out: &mut Vec<u8>) {
    match value {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(true) => out.extend_from_slice(b"true"),
        Value::Bool(false) => out.extend_from_slice(b"false"),
        Value::Number(number) => out.extend_from_slice(number.to_string().as_bytes()),
        Value::String(text) => out.extend_from_slice(
            serde_json::to_string(text)
                .expect("serializing a JSON string is infallible")
                .as_bytes(),
        ),
        Value::Array(items) => {
            out.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                write_canonical(item, out);
            }
            out.push(b']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push(b'{');
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                out.extend_from_slice(
                    serde_json::to_string(key)
                        .expect("serializing a JSON object key is infallible")
                        .as_bytes(),
                );
                out.push(b':');
                write_canonical(&map[key], out);
            }
            out.push(b'}');
        }
    }
}

/// Split a project into its parts. The same design always splits to the same bytes.
pub fn split_project(project: &Project) -> Result<BTreeMap<String, PartData>> {
    project.validate()?;
    let value = serde_json::to_value(project).map_err(json)?;
    let mut root = match value {
        Value::Object(map) => map,
        _ => return Err(Error("The project did not serialize as an object.".into())),
    };
    let mut parts = BTreeMap::new();

    let pages = root.remove("pages").unwrap_or_else(|| Value::Array(vec![]));
    let pages = match pages {
        Value::Array(pages) => pages,
        _ => return Err(Error("The project boundaries are not a list.".into())),
    };
    for (index, page) in pages.iter().enumerate() {
        let id = page
            .get("id")
            .and_then(Value::as_str)
            .and_then(|id| Uuid::parse_str(id).ok())
            .ok_or_else(|| Error("A boundary in the project has no valid id.".into()))?;
        parts.insert(
            boundary_part(index, id),
            PartData::Json(canonical_json(page)),
        );
    }

    // Optional fields that were skipped as empty still get a part with the right shape, so
    // joining always has something to put back.
    for (key, name, empty) in [
        ("reference", REFERENCE, Value::Array(vec![])),
        ("pointers", POINTERS, Value::Array(vec![])),
        ("brief", BRIEF, Value::Null),
        ("importedIdentity", IDENTITY, Value::Null),
        ("latestGeneration", DOSSIER, Value::Null),
    ] {
        let part = root.remove(key).unwrap_or(empty);
        parts.insert(name.to_owned(), PartData::Json(canonical_json(&part)));
    }

    root.remove("pictures");
    parts.insert(
        SETTINGS.to_owned(),
        PartData::Json(canonical_json(&Value::Object(root))),
    );
    for (id, uri) in &project.pictures {
        let (mime, bytes) = decode_picture(uri)?;
        parts.insert(picture_part(*id), PartData::Picture { mime, bytes });
    }
    Ok(parts)
}

/// Join parts back into a project, rejecting any part that is missing or does not add up.
pub fn join_project(parts: &BTreeMap<String, PartData>) -> Result<Project> {
    let mut root = match read(parts, SETTINGS)? {
        Value::Object(map) => map,
        _ => return Err(Error("The project settings part is not an object.".into())),
    };

    let mut pages: Vec<(usize, Value)> = Vec::new();
    let mut pictures = Map::new();
    for (name, part) in parts {
        if let Some(index) = boundary_index(name) {
            let page = parse(name, part)?;
            pages.push((index, page));
        } else if let Some(id) = picture_id(name) {
            let PartData::Picture { mime, bytes } = part else {
                return Err(Error(format!("The picture part {name} holds JSON.")));
            };
            ensure(
                mime.starts_with("image/") && !bytes.is_empty(),
                format!("The picture part {name} is not a picture."),
            )?;
            pictures.insert(
                id.to_string(),
                Value::String(desegnos_core::limits::image_data_uri(&mime, &bytes)),
            );
        }
    }
    pages.sort_by_key(|(index, _)| *index);
    for (expected, (index, _)) in pages.iter().enumerate() {
        ensure(
            *index == expected,
            "The saved project is missing a boundary.".to_string(),
        )?;
    }
    root.insert(
        "pages".into(),
        Value::Array(pages.into_iter().map(|(_, page)| page).collect()),
    );
    root.insert("reference".into(), list(read(parts, REFERENCE)?)?);
    root.insert("pointers".into(), list(read(parts, POINTERS)?)?);
    root.insert("brief".into(), read(parts, BRIEF)?);
    let identity = read(parts, IDENTITY)?;
    if !identity.is_null() {
        root.insert("importedIdentity".into(), identity);
    }
    root.insert("latestGeneration".into(), read(parts, DOSSIER)?);
    if !pictures.is_empty() {
        root.insert("pictures".into(), Value::Object(pictures));
    }

    let project: Project = serde_json::from_value(Value::Object(root)).map_err(json)?;
    project.validate()?;
    Ok(project)
}

/// An empty optional list may have been written as null; it means the same thing.
fn list(value: Value) -> Result<Value> {
    match value {
        Value::Null => Ok(Value::Array(vec![])),
        Value::Array(items) => Ok(Value::Array(items)),
        _ => Err(Error("A stored project list is not a list.".into())),
    }
}

fn read(parts: &BTreeMap<String, PartData>, name: &str) -> Result<Value> {
    let part = parts
        .get(name)
        .ok_or_else(|| Error(format!("The saved project is missing its {name} part.")))?;
    parse(name, part)
}

fn parse(name: &str, part: &PartData) -> Result<Value> {
    match part {
        PartData::Json(bytes) => {
            serde_json::from_slice(bytes).map_err(|error| Error(format!("{name}: {error}")))
        }
        PartData::Picture { .. } => Err(Error(format!("The {name} part holds a picture."))),
    }
}

/// Pictures are stored as canonical base64 data URIs, so the bytes round-trip exactly.
fn decode_picture(uri: &Arc<str>) -> Result<(String, Vec<u8>)> {
    let rest = uri
        .strip_prefix("data:")
        .ok_or_else(|| Error("A project picture is not a data URI.".into()))?;
    let (mime, encoded) = rest
        .split_once(";base64,")
        .ok_or_else(|| Error("A project picture is not embedded base64.".into()))?;
    ensure(
        mime.starts_with("image/"),
        "A project picture has no image media type.".to_string(),
    )?;
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| Error("A project picture holds invalid base64.".into()))?;
    ensure(!bytes.is_empty(), "A project picture is empty.".to_string())?;
    Ok((mime.to_owned(), bytes))
}

fn json(error: serde_json::Error) -> Error {
    Error(error.to_string())
}
