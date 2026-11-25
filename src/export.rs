use std::io;
use std::path::{Path, PathBuf};

use same_file::Handle;

use crate::archive::open::Entry;
use crate::config::{Config, DEFAULT_EXPORT_NAME};
use crate::storage::atomic_write::atomic_write;
use crate::storage::disk_cache::DiskCache;

pub fn save_current_page(
    archive_path: &Path,
    page: usize,
    config: &Config,
    cache: &DiskCache,
) -> Result<PathBuf, String> {
    let mut entry = Entry::open(archive_path)?;
    let page_info = entry
        .page(page)
        .cloned()
        .ok_or_else(|| "page out of range".to_string())?;
    let entry_name = page_info.name.as_str();

    let book = if entry.path.is_dir() {
        entry.path.file_name()
    } else {
        entry.path.file_stem()
    }
    .and_then(|value| value.to_str())
    .unwrap_or("book")
    .to_string();
    let name = Path::new(entry_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("page");
    let extension = Path::new(entry_name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("img");
    let template = if config.export_name.trim().is_empty() {
        DEFAULT_EXPORT_NAME
    } else {
        &config.export_name
    };
    let stem = sanitize_filename(&expand_name(template, &book, page + 1, name));

    let output = config
        .resolved_export_dir()
        .join(format!("{stem}.{extension}"));
    let source = cache.page_path(&mut entry.archive, &page_info)?.path;
    copy_page(&source, &output)
        .map_err(|error| format!("copy {} to {}: {error}", source.display(), output.display()))?;
    Ok(output)
}

fn copy_page(source: &Path, output: &Path) -> io::Result<()> {
    let mut source = Handle::from_path(source)?;
    match Handle::from_path(output) {
        Ok(existing) if existing == source => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "source and destination are the same file",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    atomic_write(output, |file| {
        io::copy(source.as_file_mut(), file)?;
        Ok(())
    })
}

pub fn expand_name(template: &str, book: &str, page: usize, name: &str) -> String {
    let mut output = String::new();
    let mut rest = template;

    while let Some(start) = rest.find('{') {
        output.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            output.push('{');
            rest = after;
            continue;
        };
        let key = &after[..end];
        match key {
            "book" => output.push_str(book),
            "page" => output.push_str(&page.to_string()),
            "name" => output.push_str(name),
            _ if key.starts_with("page:") => {
                if let Some(width) = page_width(&key[5..]) {
                    output.push_str(&format!("{page:0width$}"));
                } else {
                    output.push('{');
                    output.push_str(key);
                    output.push('}');
                }
            }
            _ => {
                output.push('{');
                output.push_str(key);
                output.push('}');
            }
        }
        rest = &after[end + 1..];
    }
    output.push_str(rest);
    output
}

fn page_width(spec: &str) -> Option<usize> {
    if spec.is_empty() {
        return None;
    }
    let width = if spec.chars().all(|c| c == '0') {
        spec.len()
    } else {
        spec.parse().ok()?
    };
    Some(width.min(20))
}

pub fn sanitize_filename(name: &str) -> String {
    let mut sanitized: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();

    sanitized = sanitized.trim_end_matches([' ', '.']).to_string();
    if sanitized.is_empty() {
        sanitized.push_str("page");
    }

    let stem = sanitized
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.contains(&stem.as_str()) {
        sanitized.insert(0, '_');
    }

    sanitized.chars().take(200).collect()
}
