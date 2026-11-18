use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::archive::{Archive, Page};
use crate::debug_log;
use crate::platform::resolve_shortcut;

/// Enough bytes to recognize every supported container signature.
const PROBE_BYTES: usize = 1024;
const MIN_ZIP_BYTES: u64 = 22;
const MAX_SHORTCUT_DEPTH: usize = 8;

const ZIP_LOCAL_HEADER: &[u8] = b"PK\x03\x04";
const ZIP_END_OF_CENTRAL_DIRECTORY: &[u8] = b"PK\x05\x06";
const ZIP_SPANNING: &[u8] = b"PK\x07\x08";
const RAR_MAGIC: &[u8] = b"Rar!\x1A\x07";
const SEVEN_ZIP_MAGIC: &[u8] = b"7z\xBC\xAF\x27\x1C";
const SHELL_LINK_HEADER: &[u8] = &[
    0x4c, 0x00, 0x00, 0x00, 0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc0, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x46,
];

/// Container format detected from the file contents and, for `.lnk`, the name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Zip,
    Directory,
    Unsupported,
    Broken,
    ShellLink,
}

impl std::fmt::Display for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Zip => "ZIP",
            Self::Directory => "image directory",
            Self::Unsupported => "unsupported format",
            Self::Broken => "broken or unreadable file",
            Self::ShellLink => "Windows shortcut",
        })
    }
}

/// An opened archive with its path already resolved.
///
/// `Entry::open` follows Windows shortcuts, so callers can keep
/// `entry.path` and reopen it without resolving the shortcut again.
pub struct Entry {
    /// Path after following a Windows shortcut.
    pub path: PathBuf,
    pub archive: Archive,
}

impl Entry {
    /// Resolves a shortcut and opens a ZIP archive or image directory.
    ///
    /// RAR/CBR, 7z, HTML, empty, truncated, and unknown files get a
    /// format-specific error instead of a raw EOCD failure.
    pub fn open(path: &Path) -> Result<Self, String> {
        let mut path = path.to_path_buf();
        for _ in 0..MAX_SHORTCUT_DEPTH {
            if path.is_dir() {
                let archive = Archive::open_directory(&path)
                    .map_err(|error| format!("{}: {error}", path.display()))?;
                return Ok(Self { path, archive });
            }
            let probe = probe(&path)?;
            if probe.format != Format::ShellLink {
                return Self::from_probe(path, probe);
            }
            path = resolve_shortcut(&path)?;
        }
        Err(format!(
            "{}: too many nested Windows shortcuts",
            path.display()
        ))
    }

    fn from_probe(path: PathBuf, probe: Probe) -> Result<Self, String> {
        let Probe { file, len, format } = probe;
        debug_log::log(format_args!(
            "open: {} ({} bytes, detected {format})",
            path.display(),
            len
        ));
        match Archive::from_file(file, &path) {
            Ok(archive) => Ok(Self { path, archive }),
            Err(error) => Err(open_error(&path, format, len, &error)),
        }
    }

    pub fn page_count(&self) -> usize {
        self.archive.pages.len()
    }

    pub fn pages(&self) -> &[Page] {
        &self.archive.pages
    }

    pub fn page(&self, page: usize) -> Option<&Page> {
        self.archive.pages.get(page)
    }
}

struct Probe {
    file: File,
    len: u64,
    format: Format,
}

fn probe(path: &Path) -> Result<Probe, String> {
    let mut file = File::open(path).map_err(|error| format!("open {}: {error}", path.display()))?;
    let len = file
        .metadata()
        .map_err(|error| format!("stat {}: {error}", path.display()))?
        .len();
    let mut prefix = Vec::with_capacity(PROBE_BYTES);
    file.by_ref()
        .take(PROBE_BYTES as u64)
        .read_to_end(&mut prefix)
        .map_err(|error| format!("read {}: {error}", path.display()))?;
    let format = detect_prefix(path, &prefix, len);
    file.seek(SeekFrom::Start(0))
        .map_err(|error| format!("seek {}: {error}", path.display()))?;
    Ok(Probe { file, len, format })
}

/// Detects the container format without opening the archive.
///
/// `Unknown` can still be a ZIP with leading data; [`Entry::open`] tries the
/// ZIP parser before returning a format error.
pub fn detect(path: &Path) -> Format {
    if path.is_dir() {
        return Format::Directory;
    }
    probe(path)
        .map(|probe| probe.format)
        .unwrap_or(Format::Broken)
}

fn detect_prefix(path: &Path, prefix: &[u8], len: u64) -> Format {
    if is_shortcut(path) || looks_like_shell_link(prefix) {
        return Format::ShellLink;
    }
    if len == 0 {
        return Format::Broken;
    }
    if prefix.starts_with(RAR_MAGIC)
        || prefix.starts_with(SEVEN_ZIP_MAGIC)
        || looks_like_html(prefix)
    {
        return Format::Unsupported;
    }
    if starts_with_zip_signature(prefix) {
        return if len < MIN_ZIP_BYTES {
            Format::Broken
        } else {
            Format::Zip
        };
    }
    Format::Broken
}

fn starts_with_zip_signature(prefix: &[u8]) -> bool {
    [ZIP_LOCAL_HEADER, ZIP_END_OF_CENTRAL_DIRECTORY, ZIP_SPANNING]
        .iter()
        .any(|signature| prefix.starts_with(signature))
}

fn looks_like_shell_link(prefix: &[u8]) -> bool {
    prefix.starts_with(SHELL_LINK_HEADER)
}

fn looks_like_html(prefix: &[u8]) -> bool {
    let mut prefix = prefix;
    if prefix.starts_with(&[0xef, 0xbb, 0xbf]) {
        prefix = &prefix[3..];
    }
    let start = prefix
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(prefix.len());
    let prefix = &prefix[start..];
    [
        b"<!doctype html".as_slice(),
        b"<html".as_slice(),
        b"<head".as_slice(),
        b"<body".as_slice(),
    ]
    .iter()
    .any(|start| starts_with_ignore_ascii_case(prefix, start))
}

fn starts_with_ignore_ascii_case(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.len() >= needle.len() && haystack[..needle.len()].eq_ignore_ascii_case(needle)
}

fn is_shortcut(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"))
}

fn open_error(path: &Path, format: Format, len: u64, error: &str) -> String {
    let path = path.display();
    match format {
        Format::Unsupported => {
            format!(
                "{path}: unsupported file format; MangaNia reads ZIP/CBZ archives and image directories"
            )
        }
        Format::Directory => format!("{path}: cannot open image directory: {error}"),
        Format::Broken | Format::Zip => {
            format!("{path}: broken or unreadable archive ({len} bytes): {error}")
        }
        Format::ShellLink => {
            format!("{path}: Windows shortcut does not point to a readable archive: {error}")
        }
    }
}
