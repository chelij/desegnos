//! Read-only, bounded snapshots for identity analysis. Sources are untrusted data;
//! collecting a website never runs its scripts or extracts its archive.

use crate::{io, load_project};
use base64::{Engine, engine::general_purpose::STANDARD};
use desegni_core::{
    Error, MAX_PROJECT_PHOTO_BYTES, MAX_PROJECT_PHOTOS, ProjectPhoto, Result, digest,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, Metadata},
    io::Read,
    path::{Component, Path},
};
use uuid::Uuid;
use zip::ZipArchive;

const MAX_ENTRIES: usize = 4096;
const MAX_FILES: usize = 128;
const MAX_TEXT_BYTES: usize = 256 * 1024;
const MAX_TOTAL_TEXT: usize = 2 * 1024 * 1024;
const MAX_ASSET_BYTES: usize = 128 * 1024;
const MAX_ASSETS: usize = 4;
const MAX_ASSET_URI_BYTES: usize = 512 * 1024;
const MAX_COLLECTED_BYTES: usize = 4 * 1024 * 1024;
const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_EXPANDED_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SKIPPED: usize = 256;
const MAX_PHOTO_FILE_BYTES: usize = 30 * 1024 * 1024;
const MAX_PHOTO_PIXELS: u64 = 80_000_000;
const MIN_PHOTO_EDGE: u32 = 200;
const PHOTO_EDGE: u32 = 1280;
const THUMBNAIL_EDGE: u32 = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdentitySource {
    pub id: Uuid,
    pub name: String,
    pub sha256: String,
    pub files: Vec<IdentitySourceFile>,
    pub skipped: Vec<String>,
    /// Pictures that are not logos (portfolio, products, people), resized for the project.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub photos: Vec<IdentitySourcePhoto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdentitySourcePhoto {
    /// Description is empty until analysis describes the photo.
    pub photo: ProjectPhoto,
    /// A small JPEG the analysis looks at to describe the photo.
    pub thumbnail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdentitySourceFile {
    pub id: Uuid,
    pub name: String,
    pub text: Option<String>,
    pub asset: Option<IdentitySourceAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdentitySourceAsset {
    pub id: Uuid,
    pub name: String,
    pub mime: String,
    pub data_uri: String,
    pub sha256: String,
}

struct Candidate {
    name: String,
    bytes: Vec<u8>,
}

#[derive(Default)]
struct Collector {
    candidates: Vec<Candidate>,
    photos: Vec<IdentitySourcePhoto>,
    collected_bytes: usize,
    skipped: Vec<String>,
    omitted_reports: usize,
}

impl Collector {
    fn add(&mut self, name: String, bytes: Vec<u8>) {
        if bytes.len() > MAX_TEXT_BYTES {
            self.skip(&name, "exceeds the 256 KiB source file limit");
            return;
        }
        self.collected_bytes += bytes.len();
        self.candidates.push(Candidate { name, bytes });
        // Retain identity notes and design tokens ahead of incidental scripts
        // even when a large website fills the bounded collection snapshot.
        while self.collected_bytes > MAX_COLLECTED_BYTES {
            let index = self
                .candidates
                .iter()
                .enumerate()
                .max_by_key(|(_, candidate)| (source_priority(&candidate.name), &candidate.name))
                .map(|(index, _)| index)
                .expect("the nonempty snapshot exceeds its budget");
            let removed = self.candidates.swap_remove(index);
            self.collected_bytes -= removed.bytes.len();
            self.skip(
                &removed.name,
                "the 4 MiB collection snapshot budget is full",
            );
        }
    }

    fn add_photo(&mut self, name: String, bytes: &[u8]) {
        if self.photos.len() == MAX_PROJECT_PHOTOS {
            self.skip(&name, "only the first 16 photos are included");
            return;
        }
        match prepare_photo(&name, bytes) {
            Ok(photo)
                if self
                    .photos
                    .iter()
                    .any(|item| item.photo.sha256 == photo.photo.sha256) =>
            {
                self.skip(&name, "same photo as another file");
            }
            Ok(photo) => self.photos.push(photo),
            Err(reason) => self.skip(&name, reason),
        }
    }

    fn skip_reason(&mut self, reason: String) {
        if self.skipped.len() < MAX_SKIPPED {
            self.skipped.push(reason);
        } else {
            self.omitted_reports += 1;
        }
    }

    fn skip(&mut self, name: &str, reason: &str) {
        if self.skipped.len() < MAX_SKIPPED {
            self.skipped
                .push(format!("{}: {reason}", display_name(name)));
        } else {
            self.omitted_reports += 1;
        }
    }

    fn finish(mut self, name: String) -> Result<IdentitySource> {
        self.candidates
            .sort_by_key(|candidate| (source_priority(&candidate.name), candidate.name.clone()));
        let selected_assets = choose_assets(&self.candidates);
        let mut files = Vec::new();
        let mut total_text = 0;
        for candidate in std::mem::take(&mut self.candidates) {
            let mut text = None;
            let mut asset = None;
            if is_text(&candidate.name) {
                if candidate.bytes.len() > MAX_TEXT_BYTES {
                    self.skip(&candidate.name, "exceeds the 256 KiB text limit");
                } else if let Ok(value) = std::str::from_utf8(&candidate.bytes) {
                    if value.contains('\0') {
                        self.skip(&candidate.name, "binary content is not source text");
                    } else if contains_secret(value) {
                        self.skip(
                            &candidate.name,
                            "possible credential or private key; excluded",
                        );
                    } else if value.trim().is_empty() {
                        self.skip(&candidate.name, "empty source");
                    } else if total_text + value.len() > MAX_TOTAL_TEXT {
                        self.skip(&candidate.name, "the 2 MiB source text budget is full");
                    } else {
                        total_text += value.len();
                        text = Some(value.to_owned());
                    }
                } else {
                    self.skip(&candidate.name, "source is not UTF-8 text");
                }
            }
            if is_image(&candidate.name) {
                if let Some(value) = selected_assets.get(&candidate.name) {
                    asset = Some(value.clone());
                } else {
                    self.skip(
                        &candidate.name,
                        "not selected: only four safe, distinct logo assets are included",
                    );
                }
            }
            if text.is_none() && asset.is_none() {
                continue;
            }
            if files.len() == MAX_FILES {
                self.skip(&candidate.name, "the 128 source file limit is full");
                continue;
            }
            let fingerprint = digest(&candidate.bytes);
            files.push(IdentitySourceFile {
                id: stable_id(&format!("file:{}:{fingerprint}", candidate.name)),
                name: candidate.name,
                text,
                asset,
            });
        }
        if files.is_empty() && self.photos.is_empty() {
            return Err(Error("No usable identity sources were found. Choose a Desegnos project, or a website folder or ZIP containing HTML, styles, identity notes, or logo files.".into()));
        }
        files.sort_by(|a, b| a.name.cmp(&b.name));
        self.skipped.sort();
        self.skipped.dedup();
        if self.omitted_reports > 0 {
            self.skipped.push(format!(
                "{} additional excluded entries were omitted from this report.",
                self.omitted_reports
            ));
        }
        // Hash only the immutable, ordered snapshot, including image bytes via
        // their data URIs. It does not depend on timestamps, selection order,
        // the absolute local path, or fresh random IDs.
        self.photos.sort_by(|a, b| a.photo.name.cmp(&b.photo.name));
        let sha256 = if self.photos.is_empty() {
            digest(&serde_json::to_vec(&files).map_err(io)?)
        } else {
            digest(&serde_json::to_vec(&(&files, &self.photos)).map_err(io)?)
        };
        Ok(IdentitySource {
            id: stable_id(&format!("source:{sha256}")),
            name,
            sha256,
            files,
            skipped: self.skipped,
            photos: self.photos,
        })
    }
}

/// Snapshot of files fetched from a website address. The same text, secret, size and
/// logo-selection rules apply as for a folder or ZIP.
pub fn identity_source_from_files(
    name: &str,
    files: Vec<(String, Vec<u8>)>,
    skipped: Vec<String>,
) -> Result<IdentitySource> {
    let mut collector = Collector::default();
    for reason in skipped {
        collector.skip_reason(reason);
    }
    for (path, bytes) in files {
        collector.add(path, bytes);
    }
    collector.finish(name.to_owned())
}

/// Creates a deterministic snapshot without changing the source or acquiring a
/// project writer lock. Folder/ZIP limits apply before provider submission.
pub fn collect_identity_source(path: &Path) -> Result<IdentitySource> {
    let metadata = fs::symlink_metadata(path).map_err(io)?;
    if metadata.file_type().is_symlink() {
        return Err(Error("Choose the original project or folder; symbolic links are excluded from identity imports.".into()));
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Identity source")
        .to_owned();
    let mut collector = Collector::default();
    if metadata.is_dir() {
        let root = path.canonicalize().map_err(io)?;
        collect_folder(&root, &root, &mut 0, &mut collector)?;
    } else if metadata.is_file() {
        match extension(&name).as_str() {
            "desegnos" | "desegni" => collect_project(path, &metadata, &mut collector)?,
            "zip" => collect_zip(path, &metadata, &mut collector)?,
            _ => {
                return Err(Error(
                    "Choose a .desegnos or .desegni project, a website folder, or a .zip website archive."
                        .into(),
                ));
            }
        }
    } else {
        return Err(Error(
            "Choose a regular project file or website folder.".into(),
        ));
    }
    collector.finish(name)
}

fn collect_folder(
    root: &Path,
    directory: &Path,
    entries: &mut usize,
    collector: &mut Collector,
) -> Result<()> {
    let mut children = Vec::new();
    for child in fs::read_dir(directory).map_err(io)? {
        *entries += 1;
        if *entries > MAX_ENTRIES {
            return Err(Error("The website has more than 4,096 source entries. Choose the website or brand subfolder instead.".into()));
        }
        children.push(child.map_err(io)?);
    }
    children.sort_by_key(std::fs::DirEntry::file_name);
    for child in children {
        let path = child.path();
        let relative = path.strip_prefix(root).map_err(io)?;
        let Some(name) = portable_name(relative) else {
            collector.skip(
                &relative.to_string_lossy(),
                "unsupported or unsafe filename",
            );
            continue;
        };
        let metadata = fs::symlink_metadata(&path).map_err(io)?;
        if metadata.file_type().is_symlink() {
            collector.skip(&name, "symbolic link; excluded");
        } else if excluded(&name) {
            collector.skip(
                &name,
                "private, generated, dependency, or unsupported source",
            );
        } else if metadata.is_dir() {
            collect_folder(root, &path, entries, collector)?;
        } else if metadata.is_file() && is_photo(&name) {
            if metadata.len() > MAX_PHOTO_FILE_BYTES as u64 {
                collector.skip(&name, "photo exceeds the 30 MiB file limit");
            } else if collector.photos.len() == MAX_PROJECT_PHOTOS {
                collector.skip(&name, "only the first 16 photos are included");
            } else {
                let canonical = path.canonicalize().map_err(io)?;
                if !canonical.starts_with(root) {
                    return Err(Error("A source path changed during collection. Choose the original folder again.".into()));
                }
                let mut file = open_unchanged(&path, &metadata)?;
                let bytes = read_bounded(&mut file, MAX_PHOTO_FILE_BYTES)?;
                collector.add_photo(name, &bytes);
            }
        } else if metadata.is_file() && relevant(&name) {
            if metadata.len() > MAX_TEXT_BYTES as u64 {
                collector.skip(&name, "exceeds the 256 KiB source file limit");
            } else {
                let canonical = path.canonicalize().map_err(io)?;
                if !canonical.starts_with(root) {
                    return Err(Error("A source path changed during collection. Choose the original folder again.".into()));
                }
                let mut file = open_unchanged(&path, &metadata)?;
                let bytes = read_bounded(&mut file, MAX_TEXT_BYTES)?;
                collector.add(name, bytes);
            }
        } else if !metadata.is_dir() {
            collector.skip(&name, "unsupported file type");
        }
    }
    Ok(())
}

fn collect_zip(path: &Path, metadata: &Metadata, collector: &mut Collector) -> Result<()> {
    if metadata.len() > MAX_ARCHIVE_BYTES {
        return Err(Error(
            "The website ZIP exceeds the 64 MiB archive limit.".into(),
        ));
    }
    let mut archive = ZipArchive::new(open_unchanged(path, metadata)?).map_err(io)?;
    if archive.len() > MAX_ENTRIES {
        return Err(Error(
            "The website ZIP contains more than 4,096 entries.".into(),
        ));
    }
    let mut total = 0_u64;
    let mut names = BTreeSet::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(io)?;
        let name = entry.name().to_owned();
        // Validate every entry, even files that would otherwise be ignored.
        if safe_archive_name(&name).is_none()
            || entry.enclosed_name().is_none()
            || !names.insert(name.clone())
            || entry.encrypted()
            || entry.unix_mode().is_some_and(|mode| {
                let kind = mode & 0o170000;
                kind != 0 && kind != 0o100000 && kind != 0o040000
            })
        {
            return Err(Error("The website ZIP contains an unsafe, encrypted, symbolic-link, duplicate, or unsupported entry.".into()));
        }
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| Error("ZIP expanded size overflow.".into()))?;
        if total > MAX_EXPANDED_BYTES {
            return Err(Error(
                "The website ZIP exceeds the 64 MiB expanded size limit.".into(),
            ));
        }
        if entry.size() > 1024 * 1024
            && entry.size() > entry.compressed_size().max(1).saturating_mul(1000)
        {
            return Err(Error(
                "The website ZIP has an excessive expansion ratio.".into(),
            ));
        }
        if entry.is_dir() {
            continue;
        }
        if !excluded(&name) && is_photo(&name) {
            if entry.size() > MAX_PHOTO_FILE_BYTES as u64 {
                collector.skip(&name, "photo exceeds the 30 MiB file limit");
            } else if collector.photos.len() == MAX_PROJECT_PHOTOS {
                collector.skip(&name, "only the first 16 photos are included");
            } else {
                let expected = entry.size();
                let bytes = read_bounded(&mut entry, MAX_PHOTO_FILE_BYTES)?;
                if bytes.len() as u64 != expected {
                    return Err(Error(
                        "A ZIP source has an inconsistent expanded size.".into(),
                    ));
                }
                collector.add_photo(name, &bytes);
            }
        } else if excluded(&name) || !relevant(&name) {
            collector.skip(
                &name,
                "private, generated, dependency, or unsupported source",
            );
        } else if entry.size() > MAX_TEXT_BYTES as u64 {
            collector.skip(&name, "exceeds the 256 KiB source file limit");
        } else {
            let expected = entry.size();
            let bytes = read_bounded(&mut entry, MAX_TEXT_BYTES)?;
            if bytes.len() as u64 != expected {
                return Err(Error(
                    "A ZIP source has an inconsistent expanded size.".into(),
                ));
            }
            collector.add(name, bytes);
        }
    }
    Ok(())
}

fn collect_project(path: &Path, metadata: &Metadata, collector: &mut Collector) -> Result<()> {
    // Use the existing archive integrity and schema checks. The reader does not
    // create .lock/.previous files and works while the project is open.
    let file = open_unchanged(path, metadata)?;
    drop(file);
    let project = load_project(path)?;
    collector.add(
        "PROJECT.md".into(),
        project.project_markdown()?.into_bytes(),
    );
    for (index, page) in project.pages.iter().enumerate() {
        let prefix = format!("pages/{:03}-{}", index + 1, page.id);
        collector.add(format!("{prefix}/page.json"), serde_json::to_vec(&serde_json::json!({ "name": page.name, "path": page.path, "appearance": page.appearance, "viewport": page.viewport })).map_err(io)?);
        if let Some(content) = &page.content {
            for (name, value) in [
                ("index.html", &content.html),
                ("styles.css", &content.css),
                ("script.js", &content.javascript),
            ] {
                collector.add(format!("{prefix}/{name}"), value.as_bytes().to_vec());
            }
        }
    }
    // Newer schema models may carry approved identity assets. Reading through
    // serialization keeps this collector compatible with the current model
    // without accepting arbitrary extension objects as executable resources.
    let value = serde_json::to_value(&project).map_err(io)?;
    if let Some(assets) = value
        .pointer("/importedIdentity/assets")
        .and_then(serde_json::Value::as_array)
    {
        for (index, asset) in assets.iter().take(32).enumerate() {
            let Some(uri) = asset.get("dataUri").and_then(serde_json::Value::as_str) else {
                continue;
            };
            if uri.len() > MAX_ASSET_BYTES * 2 {
                continue;
            }
            let Some((header, encoded)) = uri.split_once(',') else {
                continue;
            };
            let suffix = match header {
                "data:image/png;base64" => "png",
                "data:image/jpeg;base64" => "jpg",
                "data:image/webp;base64" => "webp",
                "data:image/svg+xml;base64" => "svg",
                _ => continue,
            };
            if let Ok(bytes) = STANDARD.decode(encoded) {
                collector.add(format!("identity/logo-{index}.{suffix}"), bytes);
            }
        }
    }
    Ok(())
}

fn open_unchanged(path: &Path, expected: &Metadata) -> Result<File> {
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

fn read_bounded(reader: &mut impl Read, limit: usize) -> Result<Vec<u8>> {
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

fn portable_name(path: &Path) -> Option<String> {
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
    (name.len() <= 1024 && !name.is_empty()).then_some(name)
}

fn safe_archive_name(name: &str) -> Option<String> {
    if name.contains(['\\', ':']) || name.chars().any(char::is_control) {
        return None;
    }
    let name = name.strip_suffix('/').unwrap_or(name);
    if name
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return None;
    }
    portable_name(Path::new(name))
}

fn extension(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn is_text(name: &str) -> bool {
    matches!(
        extension(name).as_str(),
        "html"
            | "htm"
            | "css"
            | "scss"
            | "sass"
            | "less"
            | "js"
            | "jsx"
            | "mjs"
            | "cjs"
            | "ts"
            | "tsx"
            | "md"
            | "mdx"
            | "json"
            | "svg"
            | "txt"
    )
}

fn is_image(name: &str) -> bool {
    matches!(
        extension(name).as_str(),
        "png" | "jpg" | "jpeg" | "webp" | "svg"
    )
}

/// PNG and JPEG pictures that are not logos or icons.
fn is_photo(name: &str) -> bool {
    matches!(extension(name).as_str(), "png" | "jpg" | "jpeg") && !logo_name(name)
}

fn jpeg(image: &image::DynamicImage, quality: u8) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality)
        .encode_image(&image::DynamicImage::ImageRgb8(image.to_rgb8()))
        .ok()?;
    Some(bytes)
}

/// Decodes a photo within pixel limits, applies its camera orientation, and keeps a JPEG of at
/// most 1280 px and 200 KiB plus a small thumbnail. Transparent areas become white.
fn prepare_photo(
    name: &str,
    bytes: &[u8],
) -> std::result::Result<IdentitySourcePhoto, &'static str> {
    use image::{ImageDecoder, ImageReader, imageops::FilterType};
    let reader = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "the photo could not be read")?;
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| "the photo format is not supported")?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > MAX_PHOTO_PIXELS {
        return Err("the photo exceeds 80 million pixels");
    }
    if width.min(height) < MIN_PHOTO_EDGE {
        return Err("too small to be a photo");
    }
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut picture =
        image::DynamicImage::from_decoder(decoder).map_err(|_| "the photo could not be decoded")?;
    picture.apply_orientation(orientation);
    if picture.color().has_alpha() {
        let mut flat = image::RgbaImage::from_pixel(
            picture.width(),
            picture.height(),
            image::Rgba([255, 255, 255, 255]),
        );
        image::imageops::overlay(&mut flat, &picture.to_rgba8(), 0, 0);
        picture = image::DynamicImage::ImageRgba8(flat);
    }
    let mut edge = PHOTO_EDGE;
    let mut encoded = None;
    'sizes: while edge >= 480 {
        let resized = if picture.width().max(picture.height()) > edge {
            picture.resize(edge, edge, FilterType::Triangle)
        } else {
            picture.clone()
        };
        for quality in [82, 74, 66] {
            let bytes = jpeg(&resized, quality).ok_or("the photo could not be compressed")?;
            if bytes.len() <= MAX_PROJECT_PHOTO_BYTES {
                encoded = Some((bytes, resized.width(), resized.height()));
                break 'sizes;
            }
        }
        edge = edge * 3 / 4;
    }
    let (bytes, width, height) = encoded.ok_or("the photo could not be made small enough")?;
    let thumbnail = jpeg(
        &picture.resize(THUMBNAIL_EDGE, THUMBNAIL_EDGE, FilterType::Triangle),
        70,
    )
    .ok_or("the photo could not be compressed")?;
    let sha256 = digest(&bytes);
    Ok(IdentitySourcePhoto {
        photo: ProjectPhoto {
            id: stable_id(&format!("photo:{sha256}")),
            name: display_name(name),
            data_uri: format!("data:image/jpeg;base64,{}", STANDARD.encode(&bytes)).into(),
            sha256,
            width,
            height,
            description: String::new(),
        },
        thumbnail: format!("data:image/jpeg;base64,{}", STANDARD.encode(&thumbnail)),
    })
}

fn relevant(name: &str) -> bool {
    is_text(name) || is_image(name) && logo_name(name)
}

fn excluded(name: &str) -> bool {
    name.split('/').any(|part| {
        let part = part.to_ascii_lowercase();
        part.starts_with('.')
            || matches!(
                part.as_str(),
                "node_modules"
                    | "target"
                    | "dist"
                    | "build"
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
            || part.ends_with(".config.js")
            || part.ends_with(".config.ts")
            || part.ends_with(".config.json")
            || matches!(
                extension(&part).as_str(),
                "pem" | "key" | "p12" | "pfx" | "keystore"
            )
    })
}

fn logo_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ["logo", "brand", "wordmark", "favicon", "identity", "icon"]
        .iter()
        .any(|part| lower.contains(part))
}

fn source_priority(name: &str) -> u8 {
    let lower = name.to_ascii_lowercase();
    if [
        "identity",
        "brand",
        "token",
        "palette",
        "typograph",
        "readme",
        "project.md",
    ]
    .iter()
    .any(|part| lower.contains(part))
        && !is_image(name)
    {
        0
    } else {
        match extension(name).as_str() {
            "css" | "scss" | "sass" | "less" => 1,
            "html" | "htm" | "md" | "mdx" => 2,
            "svg" => 3,
            "png" | "jpg" | "jpeg" | "webp" => 4,
            _ => 5,
        }
    }
}

fn contains_secret(text: &str) -> bool {
    fn credential_key(key: &str) -> bool {
        matches!(
            key.to_ascii_lowercase()
                .replace(['-', '_', ' '], "")
                .as_str(),
            "apikey"
                | "accesstoken"
                | "refreshtoken"
                | "clientsecret"
                | "password"
                | "passwd"
                | "authorization"
                | "privatekey"
                | "awsaccesskeyid"
                | "awssecretaccesskey"
                | "token"
                | "secret"
        )
    }
    fn literal_secret(value: &str) -> bool {
        !value.is_empty()
            && !value.starts_with("process.env")
            && !value.starts_with("import.meta.env")
            && !value.starts_with("std::env")
            && !value.starts_with("${")
            && !matches!(value, "null" | "undefined" | "true" | "false")
    }
    fn json_secret(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Object(fields) => fields.iter().any(|(key, value)| {
                credential_key(key) && value.as_str().is_some_and(literal_secret)
                    || json_secret(value)
            }),
            serde_json::Value::Array(values) => values.iter().any(json_secret),
            _ => false,
        }
    }
    if serde_json::from_str::<serde_json::Value>(text).is_ok_and(|value| json_secret(&value)) {
        return true;
    }
    let lower = text.to_ascii_lowercase();
    if lower.contains("-----begin") && lower.contains("private key-----")
        || lower.contains("authorization: bearer ")
    {
        return true;
    }
    for token in text.split(|ch: char| {
        ch.is_whitespace() || matches!(ch, '\'' | '"' | '`' | ',' | ';' | '<' | '>')
    }) {
        if ((token.starts_with("sk-") && token.len() >= 24)
            || (token.starts_with("ghp_")
                || token.starts_with("github_pat_")
                || token.starts_with("xoxb-")
                || token.starts_with("xoxp-"))
                && token.len() >= 24)
            || (token.starts_with("AKIA") && token.len() == 20)
            || (token.starts_with("AIza") && token.len() >= 35)
        {
            return true;
        }
    }
    for line in lower.lines() {
        let Some((key, value)) = line.split_once(['=', ':']) else {
            continue;
        };
        let key = key
            .trim()
            .trim_matches(['\'', '"', '`', '{', ' '])
            .replace(['-', '_', ' '], "");
        if [
            "apikey",
            "accesstoken",
            "refreshtoken",
            "clientsecret",
            "password",
            "passwd",
            "authorization",
            "privatekey",
            "awsaccesskeyid",
            "awssecretaccesskey",
        ]
        .iter()
        .any(|name| key.ends_with(name))
            || [
                "token",
                "secret",
                "consttoken",
                "lettoken",
                "vartoken",
                "constsecret",
                "letsecret",
                "varsecret",
            ]
            .contains(&key.as_str())
        {
            let value = value
                .trim()
                .trim_matches(['\'', '"', '`', ',', ';', '}', ' ']);
            if literal_secret(value) {
                return true;
            }
        }
    }
    false
}

fn stable_id(value: &str) -> Uuid {
    let hash = digest(value.as_bytes());
    let mut bytes = [0_u8; 16];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte =
            u8::from_str_radix(&hash[index * 2..index * 2 + 2], 16).expect("digest is hexadecimal");
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn display_name(value: &str) -> String {
    value
        .chars()
        .take(240)
        .map(|ch| if ch.is_control() { '�' } else { ch })
        .collect()
}

fn choose_assets(candidates: &[Candidate]) -> BTreeMap<String, IdentitySourceAsset> {
    let mut groups: BTreeMap<(bool, String), Vec<(&Candidate, u32)>> = BTreeMap::new();
    for candidate in candidates {
        if !is_image(&candidate.name)
            || !logo_name(&candidate.name)
            || candidate.bytes.len() > MAX_ASSET_BYTES
        {
            continue;
        }
        let svg = extension(&candidate.name) == "svg";
        let size = if svg {
            let Ok(text) = std::str::from_utf8(&candidate.bytes) else {
                continue;
            };
            if contains_secret(text) {
                continue;
            }
            0
        } else {
            let Some((width, height)) = raster_size(&candidate.name, &candidate.bytes) else {
                continue;
            };
            if width == 0 || height == 0 || width > 4096 || height > 4096 {
                continue;
            }
            width.max(height)
        };
        if checked_asset(candidate).is_none() {
            continue;
        }
        groups
            .entry((svg, asset_group(&candidate.name, svg)))
            .or_default()
            .push((candidate, size));
    }
    let mut chosen = Vec::new();
    for ((svg, _), mut variants) in groups {
        variants.sort_by_key(|(candidate, size)| {
            if svg {
                let name = candidate.name.to_ascii_lowercase();
                (
                    if name.contains("auto") {
                        0
                    } else if name.contains("light") {
                        1
                    } else {
                        2
                    },
                    candidate.bytes.len(),
                    candidate.name.clone(),
                )
            } else {
                (
                    if *size >= 128 {
                        *size - 128
                    } else {
                        10000 + 128 - *size
                    },
                    candidate.bytes.len(),
                    candidate.name.clone(),
                )
            }
        });
        chosen.push((svg, variants[0].0));
    }
    chosen.sort_by_key(|(svg, candidate)| {
        (
            std::cmp::Reverse(*svg),
            candidate.name.matches('/').count(),
            candidate.name.clone(),
        )
    });
    let mut result = BTreeMap::new();
    let mut svg_count = 0;
    let mut hashes = BTreeSet::new();
    let mut uri_bytes = 0;
    for (svg, candidate) in chosen {
        if result.len() == MAX_ASSETS {
            break;
        }
        if svg && svg_count == 2 {
            continue;
        }
        let sha256 = digest(&candidate.bytes);
        if !hashes.insert(sha256.clone()) {
            continue;
        }
        let mime = match extension(&candidate.name).as_str() {
            "svg" => "image/svg+xml",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "webp" => "image/webp",
            _ => continue,
        };
        let data_uri = format!("data:{mime};base64,{}", STANDARD.encode(&candidate.bytes));
        if uri_bytes + data_uri.len() > MAX_ASSET_URI_BYTES {
            continue;
        }
        uri_bytes += data_uri.len();
        if svg {
            svg_count += 1;
        }
        result.insert(
            candidate.name.clone(),
            IdentitySourceAsset {
                id: stable_id(&format!("asset:{sha256}")),
                name: candidate.name.clone(),
                mime: mime.into(),
                data_uri,
                sha256,
            },
        );
    }
    result
}

fn asset_group(name: &str, svg: bool) -> String {
    let stem = Path::new(name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(name)
        .to_ascii_lowercase();
    stem.split(['-', '_', ' '])
        .filter(|part| {
            !part.is_empty()
                && !(part.chars().all(|ch| ch.is_ascii_digit())
                    || part.contains('x')
                        && part
                            .split('x')
                            .all(|value| value.chars().all(|ch| ch.is_ascii_digit())))
                && !(svg && ["auto", "light", "dark"].contains(part))
        })
        .collect::<Vec<_>>()
        .join("-")
}

fn checked_asset(candidate: &Candidate) -> Option<desegni_core::IdentityAsset> {
    let mime = match extension(&candidate.name).as_str() {
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => return None,
    };
    let sha256 = digest(&candidate.bytes);
    let asset = desegni_core::IdentityAsset {
        id: stable_id(&format!("asset:{sha256}")),
        name: candidate.name.clone(),
        mime: mime.into(),
        data_uri: format!("data:{mime};base64,{}", STANDARD.encode(&candidate.bytes)),
        sha256,
    };
    asset.validate().ok()?;
    Some(asset)
}

fn raster_size(name: &str, bytes: &[u8]) -> Option<(u32, u32)> {
    match extension(name).as_str() {
        "png" if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.get(12..16) == Some(b"IHDR") => {
            Some((
                u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?),
                u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?),
            ))
        }
        "jpg" | "jpeg" if bytes.starts_with(&[0xff, 0xd8]) && bytes.ends_with(&[0xff, 0xd9]) => {
            let mut cursor = 2;
            while cursor + 4 <= bytes.len() {
                if bytes[cursor] != 0xff {
                    return None;
                }
                let marker = bytes[cursor + 1];
                cursor += 2;
                if marker == 0xda || marker == 0xd9 {
                    return None;
                }
                if marker == 0xff || marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
                    continue;
                }
                let length =
                    u16::from_be_bytes(bytes.get(cursor..cursor + 2)?.try_into().ok()?) as usize;
                if length < 2 {
                    return None;
                }
                if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
                    return Some((
                        u16::from_be_bytes(bytes.get(cursor + 5..cursor + 7)?.try_into().ok()?)
                            as u32,
                        u16::from_be_bytes(bytes.get(cursor + 3..cursor + 5)?.try_into().ok()?)
                            as u32,
                    ));
                }
                cursor = cursor.checked_add(length)?;
            }
            None
        }
        "webp" if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") => {
            match bytes.get(12..16)? {
                b"VP8X" => Some((
                    1 + little_24(bytes.get(24..27)?),
                    1 + little_24(bytes.get(27..30)?),
                )),
                b"VP8L" if bytes.get(20) == Some(&0x2f) => {
                    let bits = u32::from_le_bytes(bytes.get(21..25)?.try_into().ok()?);
                    Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
                }
                b"VP8 " if bytes.get(23..26) == Some(&[0x9d, 0x01, 0x2a]) => Some((
                    (u16::from_le_bytes(bytes.get(26..28)?.try_into().ok()?) & 0x3fff) as u32,
                    (u16::from_le_bytes(bytes.get(28..30)?.try_into().ok()?) & 0x3fff) as u32,
                )),
                _ => None,
            }
        }
        _ => None,
    }
}

fn little_24(bytes: &[u8]) -> u32 {
    bytes[0] as u32 | (bytes[1] as u32) << 8 | (bytes[2] as u32) << 16
}
