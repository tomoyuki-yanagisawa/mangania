use std::error::Error;
use std::io;
use std::path::Path;

use crate::archive::open::Entry;
use crate::storage::disk_cache::DiskCache;

pub fn list(path: &Path) -> Result<(), Box<dyn Error>> {
    let entry = Entry::open(path).map_err(io::Error::other)?;
    for (index, page) in entry.archive.pages.iter().enumerate() {
        println!("{}\t{}", index + 1, page.name);
    }
    println!("{} page(s)", entry.page_count());
    Ok(())
}

pub fn check(path: &Path, cache: &DiskCache) -> Result<(), Box<dyn Error>> {
    let mut entry = Entry::open(path).map_err(io::Error::other)?;
    let total = entry.page_count();
    let mut failed = 0;

    for page in 0..total {
        let page_info = entry.archive.pages[page].clone();
        if let Err(error) = cache.decode(&mut entry.archive, page_info.entry, &page_info.name) {
            failed += 1;
            eprintln!("page {} ({}): {error}", page + 1, page_info.name);
        }
    }

    println!("checked {total} page(s), {failed} failed");
    if failed > 0 {
        return Err(io::Error::other(format!("{failed} page(s) failed")).into());
    }
    Ok(())
}

pub fn render_page(
    path: &Path,
    page: usize,
    output: &Path,
    cache: &DiskCache,
) -> Result<(), Box<dyn Error>> {
    let mut entry = Entry::open(path).map_err(io::Error::other)?;
    let index = page
        .checked_sub(1)
        .ok_or_else(|| io::Error::other("page must be >= 1"))?;
    let page_info = entry.archive.pages[index].clone();
    let image = cache
        .decode(&mut entry.archive, page_info.entry, &page_info.name)
        .map_err(io::Error::other)?;
    image::DynamicImage::ImageRgba8(image).save(output)?;
    println!("wrote {}", output.display());
    Ok(())
}
