//! Choosing the next or previous archive in the same directory.

use std::fs::DirEntry;
use std::path::{Path, PathBuf};

use super::{is_page_name, natural_cmp};

const ARCHIVE_EXTENSIONS: [&str; 2] = ["zip", "cbz"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Archive,
    Directory,
}

/// Returns the entry `delta` positions away from `path` among the siblings of
/// the same kind, in natural order.
///
/// ZIP/CBZ files form one group and image directories another, so a ZIP never
/// jumps to a directory or the other way around. `delta` is 1 for the next
/// entry and -1 for the previous one. Hidden entries, directories without
/// direct page images, and the ends are ignored or do not wrap.
pub fn adjacent_archive(path: &Path, delta: isize) -> Option<PathBuf> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let current = path.file_name()?.to_str()?;
    let kind = if path.is_dir() {
        Kind::Directory
    } else {
        Kind::Archive
    };

    let mut entries: Vec<(String, PathBuf)> = std::fs::read_dir(parent)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| candidate(&entry, kind))
        .collect();
    entries.sort_by(|(a, _), (b, _)| natural_cmp(a, b));

    let index = entries.iter().position(|(name, _)| name == current)?;
    let target = index.checked_add_signed(delta)?;
    entries.get(target).map(|(_, path)| path.clone())
}

fn candidate(entry: &DirEntry, kind: Kind) -> Option<(String, PathBuf)> {
    let name = entry.file_name().into_string().ok()?;
    if name.starts_with('.') || name == "__MACOSX" {
        return None;
    }
    let file_type = entry.file_type().ok()?;
    let matches = match kind {
        Kind::Archive => file_type.is_file() && is_archive_file(&name),
        Kind::Directory => file_type.is_dir() && has_page_image(&entry.path()),
    };
    matches.then(|| (name, entry.path()))
}

fn is_archive_file(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ARCHIVE_EXTENSIONS
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}

/// True when the directory has a page image directly inside it, so opening it
/// can show something. Subdirectories are not searched.
fn has_page_image(directory: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry.file_type().is_ok_and(|kind| kind.is_file())
            && entry.file_name().to_str().is_some_and(is_page_name)
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
        std::fs::create_dir(dir.join("11")).expect("create directory");
        std::fs::write(dir.join("11").join("001.png"), b"").expect("write image");

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

    #[test]
    fn moves_through_image_directories_of_the_same_kind() {
        let dir = temp_dir();
        for name in ["book1", "book2", "notes", ".hidden"] {
            std::fs::create_dir(dir.join(name)).expect("create directory");
        }
        std::fs::write(dir.join("book1").join("001.png"), b"").expect("write image");
        std::fs::write(dir.join("book2").join("001.jpg"), b"").expect("write image");
        std::fs::write(dir.join("notes").join("readme.txt"), b"").expect("write text");
        std::fs::write(dir.join(".hidden").join("001.png"), b"").expect("write image");
        std::fs::write(dir.join("book3.zip"), b"").expect("write archive");

        assert_eq!(
            adjacent_archive(&dir.join("book1"), 1),
            Some(dir.join("book2"))
        );
        assert_eq!(adjacent_archive(&dir.join("book2"), 1), None);
        assert_eq!(
            adjacent_archive(&dir.join("book2"), -1),
            Some(dir.join("book1"))
        );

        std::fs::remove_dir_all(&dir).expect("remove temp dir");
    }
}
