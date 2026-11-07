//! Bounded, in-memory ZIP imports. Never materialize archive paths on disk.
use base64::{Engine, engine::general_purpose::STANDARD};
use desegnos_core::{AttachmentKind, Error, RequestAttachment, Result};
use serde::Serialize;
use std::io::{Read, Seek, SeekFrom};
use zip::ZipArchive;

pub const MAX_ARCHIVE_BYTES: usize = 32 * 1024 * 1024;
const MAX_ENTRIES: usize = 64;
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Serialize)]
pub struct SkippedFile {
    pub name: String,
    pub reason: String,
}
#[derive(Serialize)]
pub struct ZipAttachments {
    pub attachments: Vec<RequestAttachment>,
    pub skipped: Vec<SkippedFile>,
}

pub fn unpack_request_zip<R: Read + Seek>(
    mut reader: R,
    name: &str,
    remaining: usize,
) -> Result<ZipAttachments> {
    if reader.seek(SeekFrom::End(0)).map_err(crate::io)? > MAX_ARCHIVE_BYTES as u64 {
        return Err(Error(
            "The ZIP exceeds the 32 MiB archive size limit.".into(),
        ));
    }
    reader.seek(SeekFrom::Start(0)).map_err(crate::io)?;
    let mut zip =
        ZipArchive::new(reader).map_err(|_| Error("The ZIP could not be opened.".into()))?;
    if zip.len() > MAX_ENTRIES {
        return Err(Error("The ZIP exceeds the 64 entry limit.".into()));
    }
    let mut result = ZipAttachments {
        attachments: vec![],
        skipped: vec![],
    };
    let mut total = 0_u64;
    let mut names = std::collections::BTreeSet::new();
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|_| Error("The ZIP contains an unreadable entry.".into()))?;
        let path = entry.name().to_owned();
        if crate::source_files::safe_archive_name(&path).is_none()
            || entry.enclosed_name().is_none()
        {
            return Err(Error("The ZIP contains an unsafe path.".into()));
        }
        let mut skip = |reason: &str| {
            result.skipped.push(SkippedFile {
                name: path.clone(),
                reason: reason.into(),
            })
        };
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| Error("ZIP size overflow.".into()))?;
        if total > MAX_TOTAL_BYTES {
            return Err(Error(
                "The ZIP exceeds the 64 MiB expanded size limit.".into(),
            ));
        }
        if !crate::source_files::ordinary_zip_mode(entry.unix_mode()) {
            skip("symlink or special file");
            continue;
        }
        if entry.is_dir() {
            skip("directory");
            continue;
        }
        if path
            .split('/')
            .any(|part| part.starts_with('.') || part == "__MACOSX")
        {
            skip("hidden file");
            continue;
        }
        if !names.insert(path.clone()) {
            skip("duplicate name");
            continue;
        }
        if entry.encrypted() {
            skip("encrypted");
            continue;
        }
        let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        let mime = match ext.as_str() {
            "png" => Some("image/png"),
            "jpg" | "jpeg" => Some("image/jpeg"),
            "webp" => Some("image/webp"),
            "gif" => Some("image/gif"),
            _ => None,
        };
        if ext == "zip" {
            skip("nested ZIP");
            continue;
        }
        if mime.is_none() && !matches!(ext.as_str(), "txt" | "md" | "csv" | "json" | "html" | "htm")
        {
            skip("unsupported");
            continue;
        }
        if entry.size() > MAX_FILE_BYTES {
            skip("exceeds 16 MiB file limit");
            continue;
        }
        if crate::source_files::excessive_expansion(entry.size(), entry.compressed_size()) {
            skip("excessive expansion ratio");
            continue;
        }
        let expected = entry.size();
        let bytes =
            crate::source_files::read_exact_bounded(&mut entry, expected, MAX_FILE_BYTES as usize)?;
        if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
            skip("nested ZIP");
            continue;
        }
        let full_name = format!("{name}/{path}");
        if full_name.chars().count() > 120 {
            skip("name exceeds 120 characters");
            continue;
        }
        let mut attachment = RequestAttachment {
            id: "new".into(),
            name: full_name,
            kind: AttachmentKind::Text,
            data_uri: None,
            text: None,
            url: None,
        };
        if let Some(mime) = mime {
            attachment.kind = AttachmentKind::Image;
            attachment.data_uri =
                Some(format!("data:{mime};base64,{}", STANDARD.encode(bytes)).into());
        } else {
            let Ok(text) = String::from_utf8(bytes) else {
                skip("invalid UTF-8 text");
                continue;
            };
            attachment.text = Some(text);
        }
        if attachment.validate().is_err() {
            skip("invalid or empty file");
            continue;
        }
        result.attachments.push(attachment);
    }
    result.attachments.sort_by(|a, b| {
        (a.kind != AttachmentKind::Image, &a.name).cmp(&(b.kind != AttachmentKind::Image, &b.name))
    });
    for attachment in result
        .attachments
        .drain(remaining.min(result.attachments.len())..)
    {
        result.skipped.push(SkippedFile {
            name: attachment
                .name
                .strip_prefix(&format!("{name}/"))
                .unwrap_or(&attachment.name)
                .into(),
            reason: "attachment limit".into(),
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::{ZipWriter, write::SimpleFileOptions};

    fn archive(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in files {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }
    fn unpack(bytes: Vec<u8>, remaining: usize) -> Result<ZipAttachments> {
        unpack_request_zip(Cursor::new(bytes), "site.zip", remaining)
    }
    #[test]
    fn traversal_and_absolute_paths_are_rejected() {
        for path in [
            "../bad.txt",
            "/bad.txt",
            "a/../../bad.txt",
            "C:/bad.txt",
            "a\\bad.txt",
        ] {
            assert!(unpack(archive(&[(path, b"bad")]), 16).is_err(), "{path}");
        }
    }
    #[test]
    fn symlinks_are_skipped() {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        zip.add_symlink("alias.txt", "target.txt", SimpleFileOptions::default())
            .unwrap();
        let result = unpack(zip.finish().unwrap().into_inner(), 16).unwrap();
        assert!(result.attachments.is_empty());
        assert_eq!(result.skipped[0].reason, "symlink or special file");
    }
    #[test]
    fn mixed_contents_are_images_first_then_sorted_text_and_count_capped() {
        let bytes = archive(&[
            ("z.txt", b"last"),
            ("b.png", b"\x89PNG\r\n\x1a\n"),
            ("a.txt", b"first"),
            ("a.gif", b"GIF89a"),
        ]);
        let result = unpack(bytes.clone(), 3).unwrap();
        assert_eq!(
            result
                .attachments
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>(),
            ["site.zip/a.gif", "site.zip/b.png", "site.zip/a.txt"]
        );
        assert_eq!(result.skipped[0].name, "z.txt");
        assert_eq!(result.skipped[0].reason, "attachment limit");
        let full = unpack(bytes, 0).unwrap();
        assert!(full.attachments.is_empty());
        assert_eq!(full.skipped.len(), 4);
    }
    #[test]
    fn unsupported_nested_hidden_and_directory_entries_are_skipped() {
        let result = unpack(
            archive(&[
                ("readme.md", b"ok"),
                ("program.exe", b"exe"),
                ("inner.zip", b"zip"),
                ("renamed.txt", b"PK\x03\x04rest"),
                (".DS_Store", b"x"),
                ("__MACOSX/a.txt", b"x"),
                (".hidden/a.txt", b"x"),
            ]),
            16,
        )
        .unwrap();
        assert_eq!(result.attachments.len(), 1);
        assert_eq!(result.skipped.len(), 6);
        assert_eq!(
            result
                .skipped
                .iter()
                .filter(|s| s.reason == "nested ZIP")
                .count(),
            2
        );
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        zip.add_directory("folder/", SimpleFileOptions::default())
            .unwrap();
        assert_eq!(
            unpack(zip.finish().unwrap().into_inner(), 16)
                .unwrap()
                .skipped[0]
                .reason,
            "directory"
        );
    }
    #[test]
    fn entry_file_total_and_bomb_limits_are_enforced() {
        let files: Vec<_> = (0..65)
            .map(|i| (format!("{i}.txt"), b"ok".as_slice()))
            .collect();
        assert!(
            unpack(
                archive(
                    &files
                        .iter()
                        .map(|(n, b)| (n.as_str(), *b))
                        .collect::<Vec<_>>()
                ),
                16
            )
            .is_err()
        );
        let large = vec![b'x'; MAX_FILE_BYTES as usize + 1];
        let result = unpack(archive(&[("big.txt", &large)]), 16).unwrap();
        assert_eq!(result.skipped[0].reason, "exceeds 16 MiB file limit");
        assert!(
            unpack(
                archive(&[
                    ("a.txt", &large),
                    ("b.txt", &large),
                    ("c.txt", &large),
                    ("d.txt", &large)
                ]),
                16
            )
            .is_err()
        );
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(
            "bomb.txt",
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
        zip.write_all(&vec![0; 2 * 1024 * 1024]).unwrap();
        assert_eq!(
            unpack(zip.finish().unwrap().into_inner(), 16)
                .unwrap()
                .skipped[0]
                .reason,
            "excessive expansion ratio"
        );
    }
    #[test]
    fn dishonest_declared_size_does_not_allow_unbounded_reads() {
        let mut bytes = archive(&[("a.txt", b"more than one byte")]);
        let central = bytes.windows(4).position(|s| s == b"PK\x01\x02").unwrap();
        bytes[central + 24..central + 28].copy_from_slice(&1u32.to_le_bytes());
        assert!(unpack(bytes, 16).is_err());
    }
}
