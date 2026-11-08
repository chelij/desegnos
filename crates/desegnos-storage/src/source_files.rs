//! Path, archive and bounded-read policy shared by source import and export.
use crate::io;
use desegnos_core::{Error, Result};
use std::{
    fs::{self, File, Metadata},
    io::Read,
    path::{Component, Path},
};
#[derive(Clone, Copy)]
pub(crate) enum SourcePurpose {
    Identity,
    Render,
}
pub(crate) fn ordinary_zip_mode(mode: Option<u32>) -> bool {
    mode.is_none_or(|mode| matches!(mode & 0o170000, 0 | 0o100000 | 0o040000))
}
pub(crate) fn excessive_expansion(size: u64, compressed: u64) -> bool {
    size > 1024 * 1024 && size > compressed.max(1).saturating_mul(1000)
}
pub(crate) fn read_exact_bounded(
    reader: &mut impl Read,
    expected: u64,
    limit: usize,
) -> Result<Vec<u8>> {
    if expected > limit as u64 {
        return Err(Error("A source exceeds its file size limit.".into()));
    }
    let bytes = read_bounded(reader, expected as usize)?;
    if bytes.len() as u64 != expected {
        return Err(Error("A source changed size during collection.".into()));
    }
    Ok(bytes)
}
pub(crate) fn open_unchanged(path: &Path, expected: &Metadata) -> Result<File> {
    if fs::symlink_metadata(path)
        .map_err(io)?
        .file_type()
        .is_symlink()
    {
        return Err(Error(
            "A source became a symbolic link during collection.".into(),
        ));
    }
    let file = File::open(path).map_err(io)?;
    let current = file.metadata().map_err(io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if current.dev() != expected.dev() || current.ino() != expected.ino() {
            return Err(Error(
                "A source changed during collection. Try the import again.".into(),
            ));
        }
    }
    if !current.is_file() || current.len() != expected.len() {
        return Err(Error(
            "A source changed during collection. Try the import again.".into(),
        ));
    }
    Ok(file)
}

pub(crate) fn read_bounded(reader: &mut impl Read, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() > limit {
        return Err(Error(
            "A source grew beyond its file size limit during collection.".into(),
        ));
    }
    Ok(bytes)
}

pub(crate) fn portable_name(path: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        if let Component::Normal(part) = component {
            let part = part.to_str()?;
            if part.is_empty() || part.contains(['\\', ':']) || part.chars().any(char::is_control) {
                return None;
            }
            parts.push(part);
        } else {
            return None;
        }
    }
    let name = parts.join("/");
    safe_archive_name(&name)
}

pub(crate) fn safe_archive_name(name: &str) -> Option<String> {
    let name = name.strip_suffix('/').unwrap_or(name);
    if name.is_empty()
        || name.len() > 1024
        || name.contains(['\\', ':'])
        || name.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || part.ends_with(['.', ' '])
                || part
                    .chars()
                    .any(|c| c.is_control() || "<>\"|*?".contains(c))
        })
    {
        return None;
    }
    Some(name.to_owned())
}

pub(crate) fn extension(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

pub(crate) fn excluded(name: &str, purpose: SourcePurpose) -> bool {
    name.split('/').any(|part| {
        let part = part.to_ascii_lowercase();
        if part.starts_with('.')
            || matches!(
                part.as_str(),
                "__macosx" | "$recycle.bin" | "system volume information" | "node_modules"
            )
        {
            return true;
        }
        if matches!(purpose, SourcePurpose::Render) {
            return false;
        }
        part.starts_with('.')
            || matches!(
                part.as_str(),
                "node_modules"
                    | "target"
                    | "coverage"
                    | "vendor"
                    | "__pycache__"
                    | "cache"
                    | "tmp"
                    | "temp"
                    | "credentials"
                    | "secrets"
                    | "auth"
                    | "config"
                    | "configuration"
                    | "tests"
                    | "test-results"
            )
            || part.ends_with(".lock")
            || part.ends_with(".map")
            || part.ends_with(".min.js")
            || part.ends_with(".min.css")
            || part.contains(".min.")
            || part.starts_with("vendor.")
            || part.contains(".bundle.")
            || part.split(['.', '-', '_']).any(|word| {
                matches!(
                    word,
                    "license" | "licence" | "licenses" | "licences" | "ofl" | "copying"
                )
            })
            || [
                "package-lock.json",
                "pnpm-lock.yaml",
                "yarn.lock",
                "cargo.lock",
                "composer.lock",
                "bun.lock",
                "bun.lockb",
                "auth.json",
                "authentication.json",
                "passwords.json",
                "service-account.json",
            ]
            .contains(&part.as_str())
            || part.contains("credential")
            || part.contains("secret")
            || part.contains("private-key")
            || part.starts_with("config.")
            || (part.contains(".config.")
                && !["tailwind", "theme", "unocss"]
                    .iter()
                    .any(|prefix| part.starts_with(prefix)))
            || matches!(
                extension(&part).as_str(),
                "pem" | "key" | "p12" | "pfx" | "keystore"
            )
    })
}

struct FileClass {
    priority: Option<u8>,
    text: bool,
    image: bool,
    download: bool,
}
fn classify(name: &str) -> FileClass {
    let ext = extension(name);
    let (priority, text, image) = match ext.as_str() {
        "html" | "htm" => (Some(0), true, false),
        "css" => (Some(1), true, false),
        "svg" => (Some(2), true, true),
        "png" | "jpg" | "jpeg" | "webp" => (Some(2), false, true),
        "gif" | "avif" | "ico" | "bmp" => (Some(2), false, false),
        "woff" | "woff2" | "ttf" | "otf" | "eot" => (Some(3), false, false),
        "js" | "mjs" | "cjs" => (Some(4), true, false),
        "md" | "mdx" | "txt" | "json" | "scss" | "sass" | "less" | "ts" | "tsx" | "jsx"
        | "astro" | "vue" | "svelte" | "njk" | "hbs" | "liquid" | "php" | "toml" | "yaml"
        | "yml" => (Some(5), true, false),
        _ => (None, false, false),
    };
    FileClass {
        priority,
        text,
        image,
        download: matches!(
            ext.as_str(),
            "mp3" | "mp4" | "webm" | "ogg" | "wav" | "pdf" | "txt" | "json" | "xml"
        ),
    }
}
pub(crate) fn priority(name: &str) -> Option<u8> {
    classify(name).priority
}
pub(crate) fn is_text(name: &str) -> bool {
    classify(name).text
}
pub(crate) fn is_image(name: &str) -> bool {
    classify(name).image
}
pub(crate) fn is_download(name: &str) -> bool {
    classify(name).download
}
