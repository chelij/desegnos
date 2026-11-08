//! Read-only, bounded snapshots for identity analysis. Sources are untrusted data;
//! collecting a website never runs its scripts or extracts its archive.

use crate::source_files::{SourcePurpose, excluded as source_excluded};
use crate::source_files::{
    excessive_expansion, extension, is_image, is_text, open_unchanged, ordinary_zip_mode,
    portable_name, read_exact_bounded, safe_archive_name,
};
use crate::{io, load_project};

use base64::{Engine, engine::general_purpose::STANDARD};
use desegnos_core::{Error, ProjectPhoto, Result, digest, limits::MAX_PROJECT_PHOTO_BYTES};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, Metadata},
    path::Path,
};
use uuid::Uuid;
use zip::ZipArchive;

const MAX_ENTRIES: usize = 4096;
const MAX_FILES: usize = 128;
const MAX_TEXT_BYTES: usize = 256 * 1024;
const MAX_TOTAL_TEXT: usize = 2 * 1024 * 1024;
use desegnos_core::limits::MAX_IDENTITY_ASSETS as MAX_ASSETS;
use desegnos_core::limits::MAX_IDENTITY_LOGO_BYTES as MAX_ASSET_BYTES;
use desegnos_core::limits::MAX_IDENTITY_LOGO_URI_TOTAL_BYTES as MAX_ASSET_URI_BYTES;
use desegnos_core::limits::MAX_IDENTITY_SOURCE_BYTES as MAX_COLLECTED_BYTES;
const MAX_ARCHIVE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_EXPANDED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_SKIPPED: usize = 256;
const MAX_PHOTO_FILE_BYTES: usize = 30 * 1024 * 1024;
const MAX_PHOTO_PIXELS: u64 = 80_000_000;
const MIN_PHOTO_EDGE: u32 = 200;
const PHOTO_EDGE: u32 = 1280;
const THUMBNAIL_EDGE: u32 = 256;

/// A file the snapshot left out, and why. The editor words each reason for the user.
#[derive(ts_rs::TS, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkippedFile {
    /// Empty when the entry is a notice about the whole source rather than one file.
    pub path: String,
    pub reason: SkipReason,
}

#[derive(ts_rs::TS, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum SkipReason {
    /// A credential or private key, so it was not read.
    Private,
    /// A size, count or budget limit was reached.
    OverLimit,
    /// Not useful for reading the brand, or excluded by type or location.
    NotNeeded,
    /// A logo candidate that lost to better ones.
    NotSelected,
    /// The content could not be read as text or as a picture.
    Unreadable,
    Symlink,
    /// The same picture as another file.
    Duplicate,
    /// More entries than the report holds.
    Omitted {
        count: usize,
    },
    /// A note about the whole source, already worded for the user.
    Notice {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdentitySource {
    pub id: Uuid,
    pub name: String,
    pub sha256: String,
    pub files: Vec<IdentitySourceFile>,
    pub skipped: Vec<SkippedFile>,
    /// Pictures that are not logos (portfolio, products, people), resized for the project.
    #[serde(default)]
    pub photos: Vec<IdentitySourcePhoto>,
    #[serde(default)]
    pub pages: Vec<desegnos_core::SourcePage>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_page: Option<desegnos_core::SourcePageRef>,
}

#[derive(ts_rs::TS, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    inventory: BTreeSet<String>,
    photos: Vec<IdentitySourcePhoto>,
    photo_aliases: BTreeMap<String, Vec<String>>,
    page_overrides: BTreeMap<String, desegnos_core::SourcePage>,
    collected_bytes: usize,
    skipped: Vec<SkippedFile>,
    omitted_reports: usize,
}

impl Collector {
    fn add(&mut self, name: String, bytes: Vec<u8>) {
        if bytes.len() > MAX_TEXT_BYTES {
            self.skip(&name, SkipReason::OverLimit);
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
            self.skip(&removed.name, SkipReason::OverLimit);
        }
    }

    fn add_photo(&mut self, name: String, bytes: &[u8]) {
        match prepare_photo(&name, bytes) {
            Ok(photo) => {
                self.photo_aliases
                    .entry(photo.photo.sha256.clone())
                    .or_default()
                    .push(name.clone());
                if self
                    .photos
                    .iter()
                    .any(|item| item.photo.sha256 == photo.photo.sha256)
                {
                    self.skip(&name, SkipReason::Duplicate);
                } else {
                    self.photos.push(photo);
                }
            }
            Err(reason) => self.skip(&name, reason),
        }
    }

    fn skip(&mut self, name: &str, reason: SkipReason) {
        self.skip_file(SkippedFile {
            path: display_name(name),
            reason,
        });
    }

    fn skip_file(&mut self, file: SkippedFile) {
        if self.skipped.len() < MAX_SKIPPED {
            self.skipped.push(file);
        } else {
            self.omitted_reports += 1;
        }
    }

    fn finish(mut self, name: String) -> Result<IdentitySource> {
        // A linked game is content on the site, not the site's own design. Keep a
        // script in an asset app only when a site page actually loads it as a script.
        let scripts = site_scripts(&self.candidates);
        let mut candidates = std::mem::take(&mut self.candidates);
        candidates.retain(|candidate| {
            if asset_app_code(&candidate.name) && !scripts.contains(&candidate.name) {
                self.skip(&candidate.name, SkipReason::NotNeeded);
                false
            } else {
                true
            }
        });
        self.candidates = candidates;
        self.candidates
            .sort_by_key(|candidate| (source_priority(&candidate.name), candidate.name.clone()));
        let routes = source_routes(&self.inventory, &self.candidates);
        let selected_assets = choose_assets(&self.candidates);
        let mut files = Vec::new();
        let mut total_text = 0;
        for candidate in std::mem::take(&mut self.candidates) {
            let mut text = None;
            let mut asset = None;
            if is_text(&candidate.name) {
                if candidate.bytes.len() > MAX_TEXT_BYTES {
                    self.skip(&candidate.name, SkipReason::OverLimit);
                } else if let Ok(value) = std::str::from_utf8(&candidate.bytes) {
                    if value.contains('\0') {
                        self.skip(&candidate.name, SkipReason::Unreadable);
                    } else if contains_secret(value) {
                        self.skip(&candidate.name, SkipReason::Private);
                    } else if value.trim().is_empty() {
                        self.skip(&candidate.name, SkipReason::NotNeeded);
                    } else if total_text + value.len() > MAX_TOTAL_TEXT {
                        self.skip(&candidate.name, SkipReason::OverLimit);
                    } else {
                        total_text += value.len();
                        text = Some(value.to_owned());
                    }
                } else {
                    self.skip(&candidate.name, SkipReason::Unreadable);
                }
            }
            if is_image(&candidate.name) {
                if let Some(value) = selected_assets.get(&candidate.name) {
                    asset = Some(value.clone());
                } else {
                    self.skip(&candidate.name, SkipReason::NotSelected);
                }
            }
            if text.is_none() && asset.is_none() {
                continue;
            }
            if files.len() == MAX_FILES {
                self.skip(&candidate.name, SkipReason::OverLimit);
                continue;
            }
            let fingerprint = digest(&candidate.bytes);
            files.push(IdentitySourceFile {
                id: stable_id(&format!("file:{}:{fingerprint}", candidate.name)),
                source_page: self
                    .page_overrides
                    .get(&candidate.name)
                    .cloned()
                    .or_else(|| {
                        text.as_deref().and_then(|text| {
                            routes.get(&candidate.name).and_then(|route| {
                                desegnos_core::source_page_at_route(&candidate.name, text, route)
                            })
                        })
                    })
                    .map(|page| page.reference()),
                name: candidate.name,
                text,
                asset,
            });
        }
        if files.is_empty() && self.photos.is_empty() {
            return Err(Error("No usable identity sources were found. Choose a Desegnos project, or a website folder or ZIP containing HTML, styles, identity notes, or logo files.".into()));
        }
        let mut pages: BTreeMap<_, _> = files
            .iter()
            .filter_map(|file| {
                self.page_overrides.get(&file.name).cloned().or_else(|| {
                    file.text.as_deref().and_then(|text| {
                        routes.get(&file.name).and_then(|route| {
                            desegnos_core::source_page_at_route(&file.name, text, route)
                        })
                    })
                })
            })
            .map(|page| (page.path.clone(), page))
            .collect();
        for item in &mut self.photos {
            if item.photo.source_pages.is_empty() {
                item.photo.source_pages = files
                    .iter()
                    .filter(|file| {
                        file.source_page.is_some()
                            && file.text.as_deref().is_some_and(|text| {
                                self.photo_aliases
                                    .get(&item.photo.sha256)
                                    .is_some_and(|aliases| {
                                        aliases.iter().any(|name| {
                                            desegnos_core::page_references_image(
                                                &file.name, text, name,
                                            )
                                        })
                                    })
                            })
                    })
                    .filter_map(|file| file.source_page.clone())
                    .collect();
            }
            for reference in &item.photo.source_pages {
                if let Some(page) = pages.get_mut(&reference.path) {
                    page.photo_count += 1;
                }
            }
        }
        self.skipped.sort();
        self.skipped.dedup();
        if self.omitted_reports > 0 {
            self.skipped.push(SkippedFile {
                path: String::new(),
                reason: SkipReason::Omitted {
                    count: self.omitted_reports,
                },
            });
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
            pages: pages.into_values().collect(),
        })
    }
}

/// Snapshot of files fetched from a website address. The same text, secret, size and
/// logo-selection rules apply as for a folder or ZIP.
pub fn identity_source_from_files(
    name: &str,
    files: Vec<(String, Vec<u8>)>,
    notices: Vec<String>,
) -> Result<IdentitySource> {
    let mut collector = Collector::default();
    for message in notices {
        collector.skip_file(SkippedFile {
            path: String::new(),
            reason: SkipReason::Notice { message },
        });
    }
    for (path, bytes) in files {
        collector.inventory.insert(path.clone());
        if source_excluded(&path, SourcePurpose::Identity) || !(relevant(&path) || is_photo(&path))
        {
            collector.skip(&path, SkipReason::NotNeeded);
        } else if is_photo(&path) {
            collector.add_photo(path, &bytes);
        } else {
            collector.add(path, bytes);
        }
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
        collect_folder(&root, &root, &mut 0, &mut collector, false)?;
        if source_routes(&collector.inventory, &collector.candidates).is_empty() {
            collector.photos.clear();
            collector.photo_aliases.clear();
            collector.candidates.clear();
            collector.collected_bytes = 0;
            collect_folder(&root, &root, &mut 0, &mut collector, true)?;
        }
    } else if metadata.is_file() {
        match extension(&name).as_str() {
            "desegnos" => collect_project(path, &metadata, &mut collector)?,
            "zip" => collect_zip(path, &metadata, &mut collector)?,
            _ => {
                return Err(Error(
                    "Choose a .desegnos project, a website folder, or a .zip website archive."
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
    include_build: bool,
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
            collector.skip(&relative.to_string_lossy(), SkipReason::NotNeeded);
            continue;
        };
        collector.inventory.insert(name.clone());
        let metadata = fs::symlink_metadata(&path).map_err(io)?;
        if metadata.file_type().is_symlink() {
            collector.skip(&name, SkipReason::Symlink);
        } else if source_excluded(&name, SourcePurpose::Identity)
            || !include_build && build_path(&name)
        {
            collector.skip(&name, SkipReason::NotNeeded);
        } else if metadata.is_dir() && separate_site(&path) {
            collector.skip(&name, SkipReason::NotNeeded);
        } else if metadata.is_dir() {
            collect_folder(root, &path, entries, collector, include_build)?;
        } else if metadata.is_file() && is_photo(&name) {
            if metadata.len() > MAX_PHOTO_FILE_BYTES as u64 {
                collector.skip(&name, SkipReason::OverLimit);
            } else {
                let canonical = path.canonicalize().map_err(io)?;
                if !canonical.starts_with(root) {
                    return Err(Error("A source path changed during collection. Choose the original folder again.".into()));
                }
                let mut file = open_unchanged(&path, &metadata)?;
                let bytes = read_exact_bounded(&mut file, metadata.len(), MAX_PHOTO_FILE_BYTES)?;
                collector.add_photo(name, &bytes);
            }
        } else if metadata.is_file() && relevant(&name) {
            if metadata.len() > MAX_TEXT_BYTES as u64 {
                collector.skip(&name, SkipReason::OverLimit);
            } else {
                let canonical = path.canonicalize().map_err(io)?;
                if !canonical.starts_with(root) {
                    return Err(Error("A source path changed during collection. Choose the original folder again.".into()));
                }
                let mut file = open_unchanged(&path, &metadata)?;
                let bytes = read_exact_bounded(&mut file, metadata.len(), MAX_TEXT_BYTES)?;
                collector.add(name, bytes);
            }
        } else if !metadata.is_dir() {
            collector.skip(&name, SkipReason::NotNeeded);
        }
    }
    Ok(())
}

fn collect_zip(path: &Path, metadata: &Metadata, collector: &mut Collector) -> Result<()> {
    if metadata.len() > MAX_ARCHIVE_BYTES {
        return Err(Error(
            "The website ZIP exceeds the 2 GiB archive limit.".into(),
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
    collector
        .inventory
        .extend(archive.file_names().map(str::to_owned));
    let source_names = archive
        .file_names()
        .filter(|name| {
            !source_excluded(name, SourcePurpose::Identity) && !build_path(name) && relevant(name)
        })
        .map(|name| Candidate {
            name: name.to_owned(),
            bytes: Vec::new(),
        })
        .collect::<Vec<_>>();
    let has_sources = !source_routes(&collector.inventory, &source_names).is_empty();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(io)?;
        let name = entry.name().to_owned();
        if safe_archive_name(&name).is_none()
            || entry.enclosed_name().is_none()
            || !names.insert(name.clone())
            || entry.encrypted()
            || !ordinary_zip_mode(entry.unix_mode())
        {
            return Err(Error("The website ZIP contains an unsafe, encrypted, symbolic-link, duplicate, or unsupported entry.".into()));
        }
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| Error("ZIP expanded size overflow.".into()))?;
        if total > MAX_EXPANDED_BYTES {
            return Err(Error(
                "The website ZIP exceeds the 2 GiB expanded size limit.".into(),
            ));
        }
        if excessive_expansion(entry.size(), entry.compressed_size()) {
            return Err(Error(
                "The website ZIP has an excessive expansion ratio.".into(),
            ));
        }
        if entry.is_dir() {
            continue;
        }
        let skip =
            source_excluded(&name, SourcePurpose::Identity) || has_sources && build_path(&name);
        if !skip && is_photo(&name) {
            if entry.size() > MAX_PHOTO_FILE_BYTES as u64 {
                collector.skip(&name, SkipReason::OverLimit);
            } else {
                let expected = entry.size();
                let bytes = read_exact_bounded(&mut entry, expected, MAX_PHOTO_FILE_BYTES)?;
                collector.add_photo(name, &bytes);
            }
        } else if skip || !relevant(&name) {
            collector.skip(&name, SkipReason::NotNeeded);
        } else if entry.size() > MAX_TEXT_BYTES as u64 {
            collector.skip(&name, SkipReason::OverLimit);
        } else {
            let expected = entry.size();
            let bytes = read_exact_bounded(&mut entry, expected, MAX_TEXT_BYTES)?;
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
        let source = desegnos_core::SourcePage {
            path: page.path.clone(),
            title: page.name.clone(),
            url: None,
            kind: desegnos_core::source_page_kind(&page.path, &page.name),
            photo_count: 0,
            item_count: 0,
        };
        collector
            .page_overrides
            .insert(format!("{prefix}/page.json"), source.clone());
        collector
            .page_overrides
            .insert(format!("{prefix}/index.html"), source);
        collector.add(format!("{prefix}/page.json"), serde_json::to_vec(&serde_json::json!({ "name": page.name, "path": page.path, "background": page.background, "viewport": page.viewport(), "sizes": page.sizes })).map_err(io)?);
    }
    if let Some(identity) = &project.imported_identity {
        for (index, asset) in identity.assets.iter().enumerate() {
            let uri = &asset.data_uri;
            if uri.len() > desegnos_core::limits::MAX_IDENTITY_LOGO_URI_BYTES {
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
            let bytes = STANDARD.decode(encoded).map_err(io)?;
            collector.add(format!("identity/logo-{index}.{suffix}"), bytes);
        }
    }
    Ok(())
}

/// PNG, JPEG and WebP pictures that are not logos or icons.
fn is_photo(name: &str) -> bool {
    matches!(extension(name).as_str(), "png" | "jpg" | "jpeg" | "webp") && !logo_name(name)
}

fn jpeg(image: &image::DynamicImage, quality: u8) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality)
        .encode_image(&image::DynamicImage::ImageRgb8(image.to_rgb8()))
        .ok()?;
    Some(bytes)
}

/// Decodes a photo within pixel limits, applies its camera orientation, and keeps a JPEG within
/// `PHOTO_EDGE` and `MAX_PROJECT_PHOTO_BYTES` plus a small thumbnail. Transparent areas become white.
fn prepare_photo(name: &str, bytes: &[u8]) -> std::result::Result<IdentitySourcePhoto, SkipReason> {
    use image::{ImageDecoder, ImageReader, imageops::FilterType};
    let reader = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| SkipReason::Unreadable)?;
    let mut decoder = reader.into_decoder().map_err(|_| SkipReason::Unreadable)?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > MAX_PHOTO_PIXELS {
        return Err(SkipReason::OverLimit);
    }
    if width.min(height) < MIN_PHOTO_EDGE {
        return Err(SkipReason::NotNeeded);
    }
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut picture =
        image::DynamicImage::from_decoder(decoder).map_err(|_| SkipReason::Unreadable)?;
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
            let bytes = jpeg(&resized, quality).ok_or(SkipReason::Unreadable)?;
            if bytes.len() <= MAX_PROJECT_PHOTO_BYTES {
                encoded = Some((bytes, resized.width(), resized.height()));
                break 'sizes;
            }
        }
        edge = edge * 3 / 4;
    }
    let (bytes, width, height) = encoded.ok_or(SkipReason::OverLimit)?;
    let thumbnail = jpeg(
        &picture.resize(THUMBNAIL_EDGE, THUMBNAIL_EDGE, FilterType::Triangle),
        70,
    )
    .ok_or(SkipReason::Unreadable)?;
    let sha256 = digest(&bytes);
    Ok(IdentitySourcePhoto {
        photo: ProjectPhoto {
            id: stable_id(&format!("photo:{sha256}")),
            name: display_name(name),
            data_uri: desegnos_core::limits::image_data_uri("image/jpeg", &bytes).into(),
            sha256,
            width,
            height,
            description: String::new(),
            source_pages: vec![],
        },
        thumbnail: desegnos_core::limits::image_data_uri("image/jpeg", &thumbnail),
    })
}

fn relevant(name: &str) -> bool {
    is_text(name) || is_image(name) && logo_name(name)
}

fn build_path(name: &str) -> bool {
    name.split('/').any(|part| {
        matches!(
            part.to_ascii_lowercase().as_str(),
            "dist" | "build" | "out" | "_site"
        )
    })
}

fn asset_app_path(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let parts: Vec<_> = lower.split('/').collect();
    parts.windows(2).any(|pair| {
        matches!(pair[0], "public" | "static" | "assets")
            && matches!(
                pair[1],
                "play" | "games" | "game" | "apps" | "app" | "demos" | "downloads" | "experiments"
            )
    })
}

fn asset_app_code(name: &str) -> bool {
    asset_app_path(name)
        && matches!(
            extension(name).as_str(),
            "js" | "mjs"
                | "cjs"
                | "ts"
                | "jsx"
                | "tsx"
                | "html"
                | "htm"
                | "css"
                | "scss"
                | "sass"
                | "less"
                | "astro"
                | "vue"
                | "svelte"
                | "json"
        )
}

fn site_scripts(candidates: &[Candidate]) -> BTreeSet<String> {
    let mut references = BTreeSet::new();
    for candidate in candidates
        .iter()
        .filter(|candidate| !asset_app_path(&candidate.name))
    {
        let Ok(text) = std::str::from_utf8(&candidate.bytes) else {
            continue;
        };
        for tag in text
            .split("<script")
            .skip(1)
            .filter_map(|part| part.split_once('>').map(|(tag, _)| tag))
        {
            let compact = tag.split_whitespace().collect::<String>();
            let Some((_, value)) = compact.split_once("src=") else {
                continue;
            };
            let value = value
                .trim_start_matches(['\'', '"'])
                .split(['\'', '"', '?', '#'])
                .next()
                .unwrap_or_default();
            for file in candidates {
                let public = file
                    .name
                    .strip_prefix("public/")
                    .or_else(|| file.name.strip_prefix("static/"))
                    .unwrap_or(&file.name);
                if value.trim_start_matches('/') == public || value == file.name {
                    references.insert(file.name.clone());
                }
            }
        }
    }
    references
}

fn logo_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ["logo", "brand", "wordmark", "favicon", "identity", "icon"]
        .iter()
        .any(|part| lower.contains(part))
}

fn source_priority(name: &str) -> u8 {
    if asset_app_path(name) {
        return 9;
    }
    let lower = name.to_ascii_lowercase();
    let stem = Path::new(&lower)
        .file_stem()
        .and_then(|part| part.to_str())
        .unwrap_or_default();
    let style = matches!(extension(name).as_str(), "css" | "scss" | "sass" | "less");
    if (style
        && ["global", "style", "theme", "token", "base", "main", "app"]
            .iter()
            .any(|part| stem.contains(part)))
        || ["tailwind", "theme", "token", "palette", "typograph"]
            .iter()
            .any(|part| lower.contains(part))
            && !is_image(name)
    {
        0
    } else if lower.contains("layout") {
        1
    } else if matches!(stem, "index" | "home" | "about" | "about-us") {
        2
    } else if ["header", "footer", "navbar", "navigation"]
        .iter()
        .any(|part| stem.contains(part))
    {
        3
    } else if ["readme", "design", "brand", "identity", "project.md"]
        .iter()
        .any(|part| lower.contains(part))
        && !is_image(name)
    {
        4
    } else if is_image(name) && logo_name(name) {
        5
    } else if style {
        6
    } else {
        match extension(name).as_str() {
            "html" | "htm" | "astro" | "vue" | "svelte" | "jsx" | "tsx" | "md" | "mdx" | "njk"
            | "hbs" | "liquid" | "php" => 7,
            _ => 8,
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
        let Some(mime) =
            desegnos_core::limits::image_mime_for_extension(&extension(&candidate.name))
        else {
            continue;
        };
        let data_uri = desegnos_core::limits::image_data_uri(mime, &candidate.bytes);
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

fn checked_asset(candidate: &Candidate) -> Option<desegnos_core::IdentityAsset> {
    let mime = desegnos_core::limits::image_mime_for_extension(&extension(&candidate.name))?;
    let sha256 = digest(&candidate.bytes);
    let asset = desegnos_core::IdentityAsset {
        id: stable_id(&format!("asset:{sha256}")),
        name: candidate.name.clone(),
        mime: mime.into(),
        data_uri: desegnos_core::limits::image_data_uri(mime, &candidate.bytes),
        sha256,
    };
    asset.validate().ok()?;
    Some(asset)
}

fn raster_size(name: &str, bytes: &[u8]) -> Option<(u32, u32)> {
    let mime = desegnos_core::limits::image_mime_for_extension(&extension(name))?;
    desegnos_core::raster_dimensions(mime, bytes)
}

fn separate_site(path: &Path) -> bool {
    path.join(".desegnos").exists()
        || fs::read_dir(path).is_ok_and(|mut entries| {
            entries.any(|entry| {
                entry.is_ok_and(|entry| {
                    extension(&entry.file_name().to_string_lossy()) == "desegnos"
                })
            })
        })
        || path.join("index.html").is_file() && path.join("package.json").is_file()
}

/// Routing evidence is computed before the bounded analysis snapshot is ranked.
/// Configuration is inspected by name only; no project code is ever executed.
fn source_routes(
    inventory: &BTreeSet<String>,
    candidates: &[Candidate],
) -> BTreeMap<String, String> {
    let has = |prefix: &str| inventory.iter().any(|name| name.starts_with(prefix));
    let config = |prefix: &str| {
        inventory
            .iter()
            .any(|name| !name.contains('/') && name.starts_with(prefix))
    };
    let astro = config("astro.config.");
    let next = config("next.config.");
    let nuxt = config("nuxt.config.");
    let svelte = config("svelte.config.");
    let hugo = config("hugo.")
        || (has("content/")
            && (inventory.contains("config.toml") || inventory.contains("config.yaml")));
    let eleventy = config(".eleventy.") || config("eleventy.config.");
    let framework = astro
        || next
        || nuxt
        || svelte
        || hugo
        || eleventy
        || config("gatsby-config.")
        || config("vite.config.") && has("src/pages/");
    let roots: Vec<&str> = if hugo {
        vec!["content/"]
    } else if svelte {
        vec!["src/routes/"]
    } else if next {
        vec!["src/app/", "app/", "src/pages/", "pages/"]
    } else if astro {
        vec!["src/pages/"]
    } else if nuxt {
        vec!["app/pages/", "pages/"]
    } else if eleventy && !has("src/pages/") && !has("pages/") {
        vec![""]
    } else {
        vec!["src/pages/", "pages/"]
    };
    let framework_sources = candidates
        .iter()
        .any(|c| roots.iter().any(|root| c.name.starts_with(root)));
    let source_html = candidates.iter().any(|c| {
        matches!(extension(&c.name).as_str(), "html" | "htm")
            && !build_path(&c.name)
            && !c.name.starts_with("public/")
    });
    let mut routes = BTreeMap::new();
    for candidate in candidates {
        let name = &candidate.name;
        let ext = extension(name);
        let root = roots.iter().find(|root| name.starts_with(**root));
        let relative = if let Some(root) = root {
            &name[root.len()..]
        } else if (!framework || !framework_sources && build_path(name))
            && matches!(ext.as_str(), "html" | "htm")
        {
            name.as_str()
        } else {
            continue;
        };
        if name
            .split('/')
            .any(|p| p.starts_with('.') || p == "node_modules")
        {
            continue;
        }
        // Independent exports inside ZIPs have the same boundary as folders.
        if name.match_indices('/').any(|(i, _)| {
            let prefix = &name[..i];
            inventory.contains(&format!("{prefix}/.desegnos"))
                || inventory
                    .iter()
                    .any(|n| n.starts_with(&format!("{prefix}/")) && extension(n) == "desegnos")
                || inventory.contains(&format!("{prefix}/index.html"))
                    && inventory.contains(&format!("{prefix}/package.json"))
        }) {
            continue;
        }
        if (root.is_none() || eleventy)
            && (source_html && (build_path(name) || name.starts_with("public/"))
                || relative
                    .split('/')
                    .any(|p| ["docs", "notes", "anti-slop", "audits", "audit"].contains(&p)))
        {
            continue;
        }
        let mut route = relative
            .rsplit_once('.')
            .map_or(relative, |(stem, _)| stem)
            .to_owned();
        if next && (name.starts_with("app/") || name.starts_with("src/app/")) {
            if !["js", "jsx", "ts", "tsx", "mdx"].contains(&ext.as_str())
                || route.rsplit('/').next() != Some("page")
            {
                continue;
            }
            route = route
                .strip_suffix("page")
                .unwrap()
                .trim_end_matches('/')
                .split('/')
                .filter(|part| !part.starts_with('(') && !part.starts_with('@'))
                .collect::<Vec<_>>()
                .join("/");
            if relative
                .split('/')
                .any(|p| p.starts_with('@') || p.starts_with('_'))
            {
                continue;
            }
        } else if svelte {
            if !relative.ends_with("+page.svelte") {
                continue;
            }
            route = route
                .trim_end_matches("+page")
                .trim_end_matches('/')
                .split('/')
                .filter(|part| !part.starts_with('('))
                .collect::<Vec<_>>()
                .join("/");
        } else if ![
            "html", "htm", "astro", "md", "mdx", "vue", "js", "jsx", "ts", "tsx", "njk", "liquid",
        ]
        .contains(&ext.as_str())
            || ["js", "jsx", "ts", "tsx"].contains(&ext.as_str())
                && !next
                && !config("gatsby-config.")
        {
            continue;
        }
        // API handlers and local administration tools are not visitor pages.
        if route
            .split('/')
            .any(|part| ["api", "admin"].contains(&part))
        {
            continue;
        }
        if root.is_some()
            && route
                .split('/')
                .any(|p| p.starts_with('_') && !(hugo && p == "_index"))
        {
            continue;
        }
        if hugo && route.ends_with("_index") {
            route.truncate(route.len() - 6);
        }
        if route.contains('[') {
            let prefix = route.split('[').next().unwrap().trim_end_matches('/');
            let Ok(text) = std::str::from_utf8(&candidate.bytes) else {
                continue;
            };
            // Only expand the collection named by this route, with literal evidence
            // that the route renders that collection.
            let collection = text
                .split_once("getCollection(")
                .and_then(|(_, rest)| {
                    let rest = rest.trim_start();
                    let quote = rest.chars().next()?;
                    if !['\'', '"'].contains(&quote) {
                        return None;
                    }
                    rest[1..]
                        .split_once(quote)
                        .map(|(collection, _)| collection)
                })
                .unwrap_or_else(|| prefix.rsplit('/').next().unwrap_or_default());
            let content_prefix = format!("src/content/{collection}/");
            let items: Vec<_> = candidates
                .iter()
                .filter(|item| {
                    item.name.starts_with(&content_prefix)
                        && ["md", "mdx"].contains(&extension(&item.name).as_str())
                })
                .collect();
            if !items.is_empty()
                && (text.contains(&format!("'{collection}'"))
                    || text.contains(&format!("\"{collection}\"")))
            {
                for item in items {
                    let slug = &item.name[content_prefix.len()..];
                    let slug = slug.rsplit_once('.').unwrap().0;
                    routes.insert(
                        item.name.clone(),
                        desegnos_core::source_page_path(&format!("{prefix}/{slug}")),
                    );
                }
            } else if items.is_empty() && !prefix.is_empty() {
                routes.insert(name.clone(), desegnos_core::source_page_path(prefix));
            }
            continue;
        }
        routes.insert(name.clone(), desegnos_core::source_page_path(&route));
    }
    routes
}
