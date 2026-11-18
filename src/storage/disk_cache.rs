use std::collections::hash_map::DefaultHasher;
use std::fs::{self, File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use crate::archive::{Archive, EntryInfo, Page};
use crate::debug_log;

const TEMP_PREFIX: &str = ".mangania-tmp-";

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// Persistent cache for decompressed ZIP entries.
///
/// The cache directory is the only page-read path: every page is extracted to
/// a file here and decoded from that file. Files are removed only by the
/// explicit `clear` action.
pub struct DiskCache {
    directory: PathBuf,
}

/// A page file on disk. `cached` is true when it lives in the disk cache.
pub struct PageFile {
    pub path: PathBuf,
    pub cached: bool,
}

impl Default for DiskCache {
    fn default() -> Self {
        Self::new(default_directory())
    }
}

impl DiskCache {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Returns the local file for a page.
    ///
    /// ZIP entries are extracted to the cache first; directory pages are used
    /// in place.
    pub fn page_path(&self, archive: &mut Archive, page: &Page) -> Result<PageFile, String> {
        if let Some(root) = archive.directory_root() {
            return Ok(PageFile {
                path: root.join(&page.name),
                cached: false,
            });
        }
        let path = self.source(archive, page.entry, &page.name)?;
        Ok(PageFile { path, cached: true })
    }

    /// Returns the cached file for an archive entry, extracting it on a miss.
    pub fn source(
        &self,
        archive: &mut Archive,
        entry: usize,
        name: &str,
    ) -> Result<PathBuf, String> {
        let info = archive.entry_info(entry)?;
        self.ensure_directory()?;

        let key = CacheKey {
            archive_path: &archive.path,
            archive_size: archive.size,
            archive_modified: archive.modified,
            entry,
            entry_name: name,
            info,
        };
        let path = self.directory.join(key.filename());

        if self.is_valid(&path, info.size)? {
            debug_log::log(format_args!(
                "disk cache: hit entry={entry} bytes={} path={}",
                info.size,
                path.display()
            ));
            return Ok(path);
        }

        self.store(archive, entry, &path, info.size)?;
        Ok(path)
    }

    /// Removes a cached file that could not be decoded.
    pub fn discard(&self, path: &Path) {
        match fs::remove_file(path) {
            Ok(()) => debug_log::log(format_args!(
                "disk cache: discarded unreadable {}",
                path.display()
            )),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => debug_log::log(format_args!(
                "disk cache: remove {}: {error}",
                path.display()
            )),
        }
    }

    fn ensure_directory(&self) -> Result<(), String> {
        fs::create_dir_all(&self.directory)
            .map_err(|error| format!("create disk cache {}: {error}", self.directory.display()))
    }

    fn is_valid(&self, path: &Path, size: u64) -> Result<bool, String> {
        match fs::metadata(path) {
            Ok(metadata) if metadata.is_file() && metadata.len() == size => Ok(true),
            Ok(metadata) => {
                debug_log::log(format_args!(
                    "disk cache: discard {} ({} bytes, expected {size})",
                    path.display(),
                    metadata.len()
                ));
                fs::remove_file(path).map_err(|error| {
                    format!("remove invalid disk cache {}: {error}", path.display())
                })?;
                Ok(false)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(format!("stat disk cache {}: {error}", path.display())),
        }
    }

    fn store(
        &self,
        archive: &mut Archive,
        entry: usize,
        path: &Path,
        size: u64,
    ) -> Result<(), String> {
        let (temp, mut file) = self.create_temp()?;
        let result = (|| -> Result<(), String> {
            let copied = archive
                .write_index(entry, &mut file)
                .map_err(|error| format!("extract entry {entry}: {error}"))?;
            if copied != size {
                return Err(format!(
                    "entry {entry} extracted {copied} bytes, expected {size}"
                ));
            }
            file.flush()
                .map_err(|error| format!("write {}: {error}", temp.display()))?;
            drop(file);

            match fs::rename(&temp, path) {
                Ok(()) => Ok(()),
                Err(error) => {
                    // Another instance may have finished the same entry first.
                    let existing = fs::metadata(path)
                        .map(|metadata| metadata.is_file() && metadata.len() == size)
                        .unwrap_or(false);
                    let _ = fs::remove_file(&temp);
                    if existing {
                        Ok(())
                    } else {
                        Err(format!(
                            "rename {} to {}: {error}",
                            temp.display(),
                            path.display()
                        ))
                    }
                }
            }
        })();

        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result?;

        debug_log::log(format_args!(
            "disk cache: stored entry={entry} bytes={} path={}",
            size,
            path.display()
        ));
        Ok(())
    }

    /// Deletes every cache file. The directory is recreated on the next read.
    pub fn clear(&self) -> Result<(), String> {
        match fs::remove_dir_all(&self.directory) {
            Ok(()) => {
                debug_log::log(format_args!(
                    "disk cache: cleared {}",
                    self.directory.display()
                ));
                Ok(())
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!(
                "clear disk cache {}: {error}",
                self.directory.display()
            )),
        }
    }

    fn create_temp(&self) -> Result<(PathBuf, File), String> {
        for _ in 0..16 {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = self
                .directory
                .join(format!("{TEMP_PREFIX}{}-{id}.tmp", std::process::id()));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => return Ok((path, file)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(format!("create {}: {error}", path.display())),
            }
        }
        Err("could not create a unique disk cache temp file".to_string())
    }
}

#[derive(Hash)]
struct CacheKey<'a> {
    archive_path: &'a Path,
    archive_size: u64,
    archive_modified: Option<SystemTime>,
    entry: usize,
    entry_name: &'a str,
    info: EntryInfo,
}

impl CacheKey<'_> {
    fn filename(&self) -> String {
        let mut hash = DefaultHasher::new();
        self.hash(&mut hash);
        format!("{:016x}.{}", hash.finish(), extension(self.entry_name))
    }
}

fn extension(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .filter(|extension| {
            !extension.is_empty()
                && extension.len() <= 8
                && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
        .unwrap_or_else(|| "img".to_string())
}

pub fn default_directory() -> PathBuf {
    platform_directory().unwrap_or_else(|| std::env::temp_dir().join("mangania").join("cache"))
}

#[cfg(windows)]
fn platform_directory() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join("AppData").join("Local"))
        })
        .map(|base| base.join("mangania").join("cache"))
}

#[cfg(target_os = "macos")]
fn platform_directory() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join("Library").join("Caches").join("mangania"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_directory() -> Option<PathBuf> {
    std::env::var_os("XDG_CACHE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".cache")))
        .map(|base| base.join("mangania"))
}

#[cfg(not(any(windows, unix)))]
fn platform_directory() -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::extension;

    #[test]
    fn keeps_supported_extensions_for_image_detection() {
        assert_eq!(extension("001.JPG"), "jpg");
        assert_eq!(extension("dir/page.png"), "png");
        assert_eq!(extension("no-extension"), "img");
    }
}
