//! Decode the current project format without changing its contents.
use crate::{Error, Project, Result, SCHEMA_VERSION, digest, ensure};

pub struct DecodedProject {
    pub project: Project,
    pub source_schema_version: u32,
    pub source_sha256: String,
    pub source_project_markdown: String,
}

pub fn decode_project(bytes: &[u8]) -> Result<DecodedProject> {
    decode_source(bytes, None, None)
}

pub fn decode_project_with_notes(bytes: &[u8], notes: &[u8]) -> Result<DecodedProject> {
    decode_source(bytes, None, Some(notes))
}

pub fn decode_project_with_source_hash(bytes: &[u8], hash: &str) -> Result<DecodedProject> {
    decode_source(bytes, Some(hash), None)
}

fn decode_source(
    bytes: &[u8],
    expected_hash: Option<&str>,
    expected_notes: Option<&[u8]>,
) -> Result<DecodedProject> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Header {
        schema_version: u32,
    }
    let header: Header = serde_json::from_slice(bytes).map_err(|error| Error(error.to_string()))?;
    ensure(
        header.schema_version == SCHEMA_VERSION,
        "This project format is unsupported.",
    )?;
    let project: Project =
        serde_json::from_slice(bytes).map_err(|error| Error(error.to_string()))?;
    project.validate()?;
    project.verify_picture_bytes()?;
    let source_sha256 =
        digest(&serde_json::to_vec(&project).map_err(|error| Error(error.to_string()))?);
    ensure(
        expected_hash.is_none_or(|hash| hash == source_sha256),
        "The project source failed its integrity check.",
    )?;
    let source_project_markdown = project.project_markdown()?;
    ensure(
        expected_notes.is_none_or(|notes| notes == source_project_markdown.as_bytes()),
        "The saved project notes disagree with the accepted project data.",
    )?;
    Ok(DecodedProject {
        source_schema_version: project.schema_version,
        project,
        source_sha256,
        source_project_markdown,
    })
}
