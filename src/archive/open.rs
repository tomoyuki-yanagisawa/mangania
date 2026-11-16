use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::archive::Archive;
use crate::debug_log;
use crate::platform::resolve_shortcut;

/// Enough bytes to recognize every supported container signature.
const PROBE_BYTES: usize = 1024;
/// An empty ZIP (End of Central Directory only) is 22 bytes.
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
    Rar,
    SevenZip,
    ShellLink,
    Html,
    Empty,
    Truncated,
    Unknown,
}

impl Format {
    pub fn label(self) -> &'static str {
        match self {
            Self::Zip => "ZIP",
            Self::Rar => "RAR/CBR",
            Self::SevenZip => "7z",
            Self::ShellLink => "Windows shortcut",
            Self::Html => "HTML",
            Self::Empty => "empty",
            Self::Truncated => "truncated ZIP",
            Self::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
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
    /// Resolves a shortcut, detects the file format, and opens a ZIP archive.
    ///
    /// ZIP is the only supported container. RAR/CBR, 7z, HTML, empty, truncated,
    /// and unknown files get a format-specific error instead of a raw EOCD
    /// failure.
    pub fn open(path: &Path) -> Result<Self, String> {
        let mut path = path.to_path_buf();
        for _ in 0..MAX_SHORTCUT_DEPTH {
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
        if format == Format::Empty {
            return Err(format!("{}: file is empty", path.display()));
        }
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

    pub fn is_empty(&self) -> bool {
        self.archive.pages.is_empty()
    }

    pub fn page_name(&self, page: usize) -> Option<&str> {
        self.archive.pages.get(page).map(|page| page.name.as_str())
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
    if is_shortcut(path) {
        return Format::ShellLink;
    }
    let Ok(metadata) = std::fs::metadata(path) else {
        return Format::Unknown;
    };
    let Ok(mut file) = File::open(path) else {
        return Format::Unknown;
    };
    let mut prefix = Vec::with_capacity(PROBE_BYTES);
    if file
        .by_ref()
        .take(PROBE_BYTES as u64)
        .read_to_end(&mut prefix)
        .is_err()
    {
        return Format::Unknown;
    }
    detect_prefix(path, &prefix, metadata.len())
}

fn detect_prefix(path: &Path, prefix: &[u8], len: u64) -> Format {
    if is_shortcut(path) || looks_like_shell_link(prefix) {
        return Format::ShellLink;
    }
    if len == 0 {
        return Format::Empty;
    }
    if prefix.starts_with(RAR_MAGIC) {
        return Format::Rar;
    }
    if prefix.starts_with(SEVEN_ZIP_MAGIC) {
        return Format::SevenZip;
    }
    if looks_like_html(prefix) {
        return Format::Html;
    }
    if starts_with_zip_signature(prefix) {
        return if len < MIN_ZIP_BYTES {
            Format::Truncated
        } else {
            Format::Zip
        };
    }
    if prefix.starts_with(b"PK") {
        return Format::Truncated;
    }
    Format::Unknown
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
        Format::Rar => {
            format!("{path}: RAR/CBR archives are not supported; convert to ZIP/CBZ")
        }
        Format::SevenZip => format!("{path}: 7z archives are not supported; convert to ZIP/CBZ"),
        Format::Html => format!("{path}: looks like an HTML file, not a ZIP archive"),
        Format::Empty => format!("{path}: file is empty"),
        Format::Truncated => format!("{path}: truncated ZIP archive ({len} bytes): {error}"),
        Format::Zip => format!("{path}: invalid or truncated ZIP archive ({len} bytes): {error}"),
        Format::ShellLink => {
            format!("{path}: Windows shortcut does not point to a readable archive: {error}")
        }
        Format::Unknown => format!(
            "{path}: unsupported or unrecognized file format ({len} bytes); MangaNia reads ZIP/CBZ archives"
        ),
    }
}
