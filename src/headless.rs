use std::error::Error;
use std::io;
use std::path::Path;

use crate::archive::open::Entry;
use crate::render::decode::decode_page;
use crate::storage::disk_cache::DiskCache;

pub fn list(path: &Path) -> Result<(), Box<dyn Error>> {
    let entry = Entry::open(path).map_err(io::Error::other)?;
    for (index, page) in entry.pages().iter().enumerate() {
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
        match entry.page_file(cache, page) {
            Ok((page_info, file)) => {
                if let Err(error) = decode_page(cache, &file) {
                    failed += 1;
                    eprintln!("page {} ({}): {error}", page + 1, page_info.name);
                }
            }
            Err(error) => {
                failed += 1;
                eprintln!("page {}: {error}", page + 1);
            }
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
    let (_, file) = entry.page_file(cache, index).map_err(io::Error::other)?;
    let image = decode_page(cache, &file).map_err(io::Error::other)?;
    image::DynamicImage::ImageRgba8(image).save(output)?;
    println!("wrote {}", output.display());
    Ok(())
}
