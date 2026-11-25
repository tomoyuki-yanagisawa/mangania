use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// Replaces `path` only after writing and syncing a file in the same directory.
pub(crate) fn atomic_write(
    path: &Path,
    write: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<()> {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let (temp, mut file) = loop {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!(".mangania-{}-{id}.tmp", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => break (temp, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = write(&mut file).and_then(|()| file.sync_all());
    drop(file);
    let result = result.and_then(|()| fs::rename(&temp, path));
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn failed_write_preserves_existing_file() {
        let dir =
            std::env::temp_dir().join(format!("mangania-atomic-write-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let output = dir.join("page.png");
        fs::write(&output, b"previous image").unwrap();

        assert!(
            atomic_write(&output, |file| {
                file.write_all(b"partial image")?;
                Err(io::Error::other("copy interrupted"))
            })
            .is_err()
        );
        assert_eq!(fs::read(&output).unwrap(), b"previous image");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);

        fs::remove_file(output).unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
