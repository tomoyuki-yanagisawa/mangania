pub mod open;

use std::cmp::Ordering;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use zip::ZipArchive;

use crate::debug_log;

const MAX_ENTRIES: usize = 100_000;
const MAX_ENTRY_BYTES: u64 = 512 * 1024 * 1024;

enum Source {
    Zip(ZipArchive<File>),
    Directory { root: PathBuf },
}

pub struct Archive {
    source: Source,
    pub pages: Vec<Page>,
    pub(crate) path: PathBuf,
    pub(crate) size: u64,
    pub(crate) modified: Option<SystemTime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub name: String,
    pub entry: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntryInfo {
    pub size: u64,
    pub crc32: u32,
}

impl Archive {
    /// Opens a ZIP archive from a path. Format detection and shortcut
    /// resolution belong to [`crate::archive::open`].
    pub fn open(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
        Self::from_file(file, path)
    }

    /// Indexes a ZIP archive from an already-open file.
    pub fn from_file(file: File, path: &Path) -> Result<Self, String> {
        let metadata = file.metadata().map_err(|e| format!("stat archive: {e}"))?;
        let file_len = metadata.len();
        let modified = metadata.modified().ok();
        debug_log::log(format_args!("archive: size={file_len} bytes"));
        let mut zip =
            ZipArchive::new(file).map_err(|e| format!("read zip ({file_len} bytes): {e}"))?;
        if zip.len() > MAX_ENTRIES {
            return Err(format!("too many zip entries: {}", zip.len()));
        }

        let mut pages = Vec::new();
        for entry in 0..zip.len() {
            // Skip entries whose local header or name is unreadable instead of
            // failing the whole archive.
            let Ok(file) = zip.by_index(entry) else {
                continue;
            };
            if !file.is_file() {
                continue;
            }
            let Ok(name) = file.name() else {
                continue;
            };
            if !is_page_name(&name) || file.size() > MAX_ENTRY_BYTES {
                continue;
            }
            pages.push(Page {
                name: name.into_owned(),
                entry,
            });
        }

        pages.sort_by(|a, b| natural_cmp(&a.name, &b.name));
        debug_log::log(format_args!(
            "archive: entries={} pages={}",
            zip.len(),
            pages.len()
        ));
        Ok(Self {
            source: Source::Zip(zip),
            pages,
            path: path.to_path_buf(),
            size: file_len,
            modified,
        })
    }

    /// Indexes the image files directly inside `path`.
    ///
    /// Subdirectories are ignored; open the subdirectory itself to read it.
    pub fn open_directory(path: &Path) -> Result<Self, String> {
        let mut pages = Vec::new();
        let entries = std::fs::read_dir(path)
            .map_err(|error| format!("read directory {}: {error}", path.display()))?;
        for entry in entries {
            let entry =
                entry.map_err(|error| format!("read directory {}: {error}", path.display()))?;
            let file_type = entry.file_type().map_err(|error| {
                format!("stat directory entry {}: {error}", entry.path().display())
            })?;
            if file_type.is_symlink() || !file_type.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if !is_page_name(&name) {
                continue;
            }
            if pages.len() >= MAX_ENTRIES {
                return Err(format!("too many image files in {}", path.display()));
            }
            pages.push(Page { name, entry: 0 });
        }
        if pages.is_empty() {
            return Err(format!("no supported images in {}", path.display()));
        }

        pages.sort_by(|a, b| natural_cmp(&a.name, &b.name));
        for (index, page) in pages.iter_mut().enumerate() {
            page.entry = index;
        }
        debug_log::log(format_args!(
            "directory: path={} pages={}",
            path.display(),
            pages.len()
        ));
        Ok(Self {
            source: Source::Directory {
                root: path.to_path_buf(),
            },
            pages,
            path: path.to_path_buf(),
            size: 0,
            modified: None,
        })
    }

    /// The directory being read, if this archive is a plain image directory.
    pub(crate) fn directory_root(&self) -> Option<&Path> {
        match &self.source {
            Source::Directory { root } => Some(root),
            Source::Zip(_) => None,
        }
    }

    /// Returns ZIP metadata used by the disk cache key and extraction path.
    pub fn entry_info(&mut self, entry: usize) -> Result<EntryInfo, String> {
        let Source::Zip(zip) = &mut self.source else {
            return Err("not a ZIP archive".to_string());
        };
        let file = zip
            .by_index(entry)
            .map_err(|e| format!("entry {entry}: {e}"))?;
        if file.size() > MAX_ENTRY_BYTES {
            return Err(format!("entry {entry} is too large"));
        }
        Ok(EntryInfo {
            size: file.size(),
            crc32: file.crc32(),
        })
    }

    /// Streams a decompressed entry into `writer` without buffering it in memory.
    pub fn write_index<W: Write>(&mut self, entry: usize, writer: &mut W) -> Result<u64, String> {
        let Source::Zip(zip) = &mut self.source else {
            return Err("not a ZIP archive".to_string());
        };
        let file = zip
            .by_index(entry)
            .map_err(|e| format!("entry {entry}: {e}"))?;
        if file.size() > MAX_ENTRY_BYTES {
            return Err(format!("entry {entry} is too large"));
        }
        debug_log::log(format_args!(
            "archive: entry {entry} declared_size={}",
            file.size()
        ));

        // `size()` is metadata from the archive; never trust it as a bound.
        let mut limited = file.take(MAX_ENTRY_BYTES + 1);
        let copied = std::io::copy(&mut limited, writer)
            .map_err(|e| format!("extract entry {entry}: {e}"))?;
        if copied > MAX_ENTRY_BYTES {
            return Err(format!("entry {entry} exceeds the size limit"));
        }
        Ok(copied)
    }
}

fn is_page_name(name: &str) -> bool {
    if !is_safe_name(name) {
        return false;
    }
    let path = Path::new(name);
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(extension.as_str(), "jpg" | "jpeg" | "png" | "webp" | "bmp")
}

fn is_safe_name(name: &str) -> bool {
    if name.is_empty() || name.contains('\\') || name.contains('\0') {
        return false;
    }
    let path = Path::new(name);
    if path.is_absolute() {
        return false;
    }
    path.components().all(|component| match component {
        Component::Normal(part) => {
            let part = part.to_string_lossy();
            !part.starts_with('.') && part != "__MACOSX"
        }
        Component::CurDir => true,
        Component::ParentDir | Component::RootDir | Component::Prefix(_) => false,
    })
}

/// Orders names by their digit runs so `2.jpg` comes before `10.jpg`.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let a: Vec<char> = a.chars().flat_map(char::to_lowercase).collect();
    let b: Vec<char> = b.chars().flat_map(char::to_lowercase).collect();
    let (mut i, mut j) = (0, 0);

    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let end_a = digit_end(&a, i);
            let end_b = digit_end(&b, j);
            let start_a = skip_zeroes(&a, i, end_a);
            let start_b = skip_zeroes(&b, j, end_b);
            let len_a = end_a - start_a;
            let len_b = end_b - start_b;

            let ordering = len_a
                .cmp(&len_b)
                .then_with(|| a[start_a..end_a].cmp(&b[start_b..end_b]));
            if ordering != Ordering::Equal {
                return ordering;
            }
            i = end_a;
            j = end_b;
        } else {
            let ordering = a[i].cmp(&b[j]);
            if ordering != Ordering::Equal {
                return ordering;
            }
            i += 1;
            j += 1;
        }
    }

    a[i..].cmp(&b[j..])
}

fn digit_end(chars: &[char], start: usize) -> usize {
    let mut end = start;
    while end < chars.len() && chars[end].is_ascii_digit() {
        end += 1;
    }
    end
}

fn skip_zeroes(chars: &[char], start: usize, end: usize) -> usize {
    let mut start = start;
    while start < end && chars[start] == '0' {
        start += 1;
    }
    start
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_compares_numbers() {
        assert_eq!(natural_cmp("2.jpg", "10.jpg"), Ordering::Less);
        assert_eq!(natural_cmp("10.jpg", "2.jpg"), Ordering::Greater);
        assert_eq!(natural_cmp("page_009.png", "page_10.png"), Ordering::Less);
        assert_eq!(natural_cmp("a1", "a01"), Ordering::Equal);
        assert_eq!(natural_cmp("ch2/10.jpg", "ch10/2.jpg"), Ordering::Less);
    }
}
