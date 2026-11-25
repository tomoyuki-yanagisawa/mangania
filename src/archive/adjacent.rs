//! Choosing the next or previous archive in the same directory.

use std::path::{Path, PathBuf};

use super::natural_cmp;

const ARCHIVE_EXTENSIONS: [&str; 2] = ["zip", "cbz"];

/// Returns the archive `delta` positions away from `path` among the ZIP/CBZ
/// files in its directory, in natural order.
///
/// `delta` is 1 for the next file and -1 for the previous one. Directories,
/// hidden files, and other formats are ignored, and the ends do not wrap.
pub fn adjacent_archive(path: &Path, delta: isize) -> Option<PathBuf> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let current = path.file_name()?.to_str()?;

    let mut archives: Vec<(String, PathBuf)> = std::fs::read_dir(parent)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            is_archive_file(&name).then(|| (name, entry.path()))
        })
        .collect();
    archives.sort_by(|(a, _), (b, _)| natural_cmp(a, b));

    let index = archives.iter().position(|(name, _)| name == current)?;
    let target = index.checked_add_signed(delta)?;
    archives.get(target).map(|(_, path)| path.clone())
}

fn is_archive_file(name: &str) -> bool {
    if name.starts_with('.') {
        return false;
    }
    Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ARCHIVE_EXTENSIONS
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::adjacent_archive;

    fn temp_dir() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "mangania-adjacent-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn moves_through_archives_in_natural_order() {
        let dir = temp_dir();
        for name in ["1.zip", "2.zip", "10.cbz", "notes.txt", ".hidden.zip"] {
            std::fs::write(dir.join(name), b"").expect("write file");
        }
        std::fs::create_dir(dir.join("folder.zip")).expect("create directory");

        assert_eq!(
            adjacent_archive(&dir.join("1.zip"), 1),
            Some(dir.join("2.zip"))
        );
        assert_eq!(
            adjacent_archive(&dir.join("2.zip"), 1),
            Some(dir.join("10.cbz"))
        );
        assert_eq!(adjacent_archive(&dir.join("10.cbz"), 1), None);
        assert_eq!(adjacent_archive(&dir.join("1.zip"), -1), None);
        assert_eq!(
            adjacent_archive(&dir.join("10.cbz"), -1),
            Some(dir.join("2.zip"))
        );

        assert_eq!(adjacent_archive(&dir.join("notes.txt"), 1), None);

        std::fs::remove_dir_all(&dir).expect("remove temp dir");
    }
}
