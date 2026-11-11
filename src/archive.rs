use std::cmp::Ordering;
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path};

use zip::ZipArchive;

const MAX_ENTRIES: usize = 100_000;
const MAX_ENTRY_BYTES: u64 = 512 * 1024 * 1024;

pub struct Archive {
    zip: ZipArchive<File>,
    pub pages: Vec<usize>,
}

impl Archive {
    pub fn open(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
        let zip = ZipArchive::new(file).map_err(|e| format!("read zip: {e}"))?;
        if zip.len() > MAX_ENTRIES {
            return Err(format!("too many zip entries: {}", zip.len()));
        }

        let mut archive = Self {
            zip,
            pages: Vec::new(),
        };
        let mut pages = Vec::new();
        for entry in 0..archive.zip.len() {
            // Skip entries whose local header or name is unreadable instead of
            // failing the whole archive.
            let Ok(file) = archive.zip.by_index(entry) else {
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
            pages.push((name.into_owned(), entry));
        }

        pages.sort_by(|a, b| natural_cmp(&a.0, &b.0));
        archive.pages = pages.into_iter().map(|(_, entry)| entry).collect();
        Ok(archive)
    }

    pub fn read_index(&mut self, entry: usize) -> Result<Vec<u8>, String> {
        let file = self
            .zip
            .by_index(entry)
            .map_err(|e| format!("entry {entry}: {e}"))?;
        if file.size() > MAX_ENTRY_BYTES {
            return Err(format!("entry {entry} is too large"));
        }

        // `size()` is metadata from the archive; never trust it as an allocation bound.
        let mut data = Vec::new();
        file.take(MAX_ENTRY_BYTES + 1)
            .read_to_end(&mut data)
            .map_err(|e| format!("extract entry {entry}: {e}"))?;
        if data.len() as u64 > MAX_ENTRY_BYTES {
            return Err(format!("entry {entry} exceeds the size limit"));
        }
        Ok(data)
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
    matches!(extension.as_str(), "jpg" | "jpeg" | "png" | "webp")
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
