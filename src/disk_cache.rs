use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use image::RgbaImage;

use crate::archive::{Archive, EntryInfo};
use crate::debug_log;
use crate::decoder;

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

    /// Returns the cached file for a page, extracting it from the ZIP on a miss.
    pub fn source(&self, archive: &mut Archive, page: usize) -> Result<PathBuf, String> {
        let (entry, name) = page_entry(archive, page)?;
        let info = archive.entry_info(entry)?;
        self.ensure_directory()?;

        let key = CacheKey {
            archive_path: &archive.path,
            archive_size: archive.size,
            archive_modified: archive.modified,
            entry,
            entry_name: &name,
            info,
        };
        let path = self.directory.join(key.filename());

        if self.is_valid(&path, info.size)? {
            debug_log::log(format_args!(
                "disk cache: hit page={} bytes={} path={}",
                page + 1,
                info.size,
                path.display()
            ));
            return Ok(path);
        }

        self.store(archive, page, entry, &path, info.size)?;
        Ok(path)
    }

    /// Decodes a page from the cached file. A decode failure is retried once
    /// after deleting the file and extracting it again.
    pub fn decode_page(&self, archive: &mut Archive, page: usize) -> Result<RgbaImage, String> {
        let path = self.source(archive, page)?;
        match decoder::decode_file(&path) {
            Ok(image) => Ok(image),
            Err(first) => {
                debug_log::log(format_args!(
                    "disk cache: decode failed page={} path={} error={first}; re-extracting",
                    page + 1,
                    path.display()
                ));
                let _ = fs::remove_file(&path);
                let path = self.source(archive, page)?;
                match decoder::decode_file(&path) {
                    Ok(image) => Ok(image),
                    Err(second) => {
                        let _ = fs::remove_file(&path);
                        let message = format!(
                            "page {}: cached image is unreadable after re-extracting ({}): {second}",
                            page + 1,
                            path.display()
                        );
                        debug_log::log(format_args!("disk cache: {message}"));
                        Err(message)
                    }
                }
            }
        }
    }

    /// Writes the original decompressed entry bytes to `output` by copying the
    /// cached file.
    pub fn write_entry_to(
        &self,
        archive: &mut Archive,
        page: usize,
        output: &Path,
    ) -> Result<(), String> {
        let source = self.source(archive, page)?;
        let result = (|| -> Result<(), String> {
            let input = File::open(&source)
                .map_err(|error| format!("open disk cache {}: {error}", source.display()))?;
            let mut reader = BufReader::new(input);
            let file = File::create(output)
                .map_err(|error| format!("create {}: {error}", output.display()))?;
            let mut writer = BufWriter::new(file);
            io::copy(&mut reader, &mut writer)
                .map_err(|error| format!("copy {}: {error}", source.display()))?;
            writer
                .flush()
                .map_err(|error| format!("write {}: {error}", output.display()))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(output);
        }
        result
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
        page: usize,
        entry: usize,
        path: &Path,
        size: u64,
    ) -> Result<(), String> {
        let (temp, mut file) = self.create_temp()?;
        let result = (|| -> Result<(), String> {
            let copied = archive
                .write_index(entry, &mut file)
                .map_err(|error| format!("extract page {}: {error}", page + 1))?;
            if copied != size {
                return Err(format!(
                    "page {} extracted {copied} bytes, expected {size}",
                    page + 1
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
            "disk cache: stored page={} bytes={} path={}",
            page + 1,
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
        let mut hash = StableHash::new();
        hash.write(self.archive_path.as_os_str().as_encoded_bytes());
        hash.u64(self.archive_size);
        match self.archive_modified {
            Some(modified) => {
                hash.u8(1);
                match modified.duration_since(UNIX_EPOCH) {
                    Ok(duration) => {
                        hash.u64(duration.as_secs());
                        hash.u32(duration.subsec_nanos());
                    }
                    Err(_) => {
                        hash.u64(0);
                        hash.u32(0);
                    }
                }
            }
            None => hash.u8(0),
        }
        hash.u64(self.entry as u64);
        hash.write(self.entry_name.as_bytes());
        hash.u64(self.info.size);
        hash.u32(self.info.crc32);
        format!("{:016x}.{}", hash.finish(), extension(self.entry_name))
    }
}

struct StableHash(u64);

impl StableHash {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn u8(&mut self, value: u8) {
        self.write(&[value]);
    }

    fn u32(&mut self, value: u32) {
        self.write(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.write(&value.to_le_bytes());
    }

    fn finish(self) -> u64 {
        self.0
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

fn page_entry(archive: &Archive, page: usize) -> Result<(usize, String), String> {
    let page = archive
        .pages
        .get(page)
        .ok_or_else(|| format!("page {} is out of range", page + 1))?;
    Ok((page.entry, page.name.clone()))
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
    use super::{StableHash, extension};

    #[test]
    fn keeps_supported_extensions_for_image_detection() {
        assert_eq!(extension("001.JPG"), "jpg");
        assert_eq!(extension("dir/page.png"), "png");
        assert_eq!(extension("no-extension"), "img");
    }

    #[test]
    fn stable_hash_changes_with_input() {
        let mut a = StableHash::new();
        a.write(b"archive");
        a.u64(7);
        let mut b = StableHash::new();
        b.write(b"archive");
        b.u64(8);
        assert_ne!(a.finish(), b.finish());
    }
}
