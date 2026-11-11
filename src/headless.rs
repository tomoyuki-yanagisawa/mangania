use std::error::Error;
use std::io;
use std::path::Path;

use crate::archive::Archive;
use crate::decoder;

pub fn list(path: &Path) -> Result<(), Box<dyn Error>> {
    let archive = Archive::open(path).map_err(io::Error::other)?;
    for (index, name) in archive.names.iter().enumerate() {
        println!("{}\t{name}", index + 1);
    }
    println!("{} page(s)", archive.pages.len());
    Ok(())
}

pub fn check(path: &Path) -> Result<(), Box<dyn Error>> {
    let mut archive = Archive::open(path).map_err(io::Error::other)?;
    let total = archive.pages.len();
    let mut failed = 0;

    for page in 0..total {
        let entry = archive.pages[page];
        if let Err(error) = archive
            .read_index(entry)
            .and_then(|bytes| decoder::decode(&bytes).map(|_| ()))
        {
            failed += 1;
            eprintln!("page {} ({}): {error}", page + 1, archive.names[page]);
        }
    }

    println!("checked {total} page(s), {failed} failed");
    if failed > 0 {
        return Err(io::Error::other(format!("{failed} page(s) failed")).into());
    }
    Ok(())
}

pub fn render_page(path: &Path, page: usize, output: &Path) -> Result<(), Box<dyn Error>> {
    let mut archive = Archive::open(path).map_err(io::Error::other)?;
    let index = page
        .checked_sub(1)
        .ok_or_else(|| io::Error::other("page must be >= 1"))?;
    let entry = *archive
        .pages
        .get(index)
        .ok_or_else(|| io::Error::other("page out of range"))?;
    let bytes = archive.read_index(entry).map_err(io::Error::other)?;
    let image = decoder::decode(&bytes).map_err(io::Error::other)?;
    image::DynamicImage::ImageRgba8(image).save(output)?;
    println!("wrote {}", output.display());
    Ok(())
}
