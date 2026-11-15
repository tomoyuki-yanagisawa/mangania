use std::error::Error;
use std::io;
use std::path::Path;

use crate::decoder;
use crate::open::Entry;

pub fn list(path: &Path) -> Result<(), Box<dyn Error>> {
    let entry = Entry::open(path).map_err(io::Error::other)?;
    for (index, name) in entry.archive.names.iter().enumerate() {
        println!("{}\t{name}", index + 1);
    }
    println!("{} page(s)", entry.page_count());
    Ok(())
}

pub fn check(path: &Path) -> Result<(), Box<dyn Error>> {
    let mut entry = Entry::open(path).map_err(io::Error::other)?;
    let total = entry.page_count();
    let mut failed = 0;

    for page in 0..total {
        let name = entry.archive.names[page].clone();
        if let Err(error) = entry
            .read_page(page)
            .and_then(|bytes| decoder::decode(&bytes).map(|_| ()))
        {
            failed += 1;
            eprintln!("page {} ({}): {error}", page + 1, name);
        }
    }

    println!("checked {total} page(s), {failed} failed");
    if failed > 0 {
        return Err(io::Error::other(format!("{failed} page(s) failed")).into());
    }
    Ok(())
}

pub fn render_page(path: &Path, page: usize, output: &Path) -> Result<(), Box<dyn Error>> {
    let mut entry = Entry::open(path).map_err(io::Error::other)?;
    let index = page
        .checked_sub(1)
        .ok_or_else(|| io::Error::other("page must be >= 1"))?;
    let bytes = entry.read_page(index).map_err(io::Error::other)?;
    let image = decoder::decode(&bytes).map_err(io::Error::other)?;
    image::DynamicImage::ImageRgba8(image).save(output)?;
    println!("wrote {}", output.display());
    Ok(())
}
