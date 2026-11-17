use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg;

const SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

// The title bar draws the small window icon at 16 logical pixels and scales it
// for high-DPI monitors, so 32 px matches winit's recommendation. The taskbar
// uses ICON_BIG, where 256 px is the ceiling winit documents.
const TITLEBAR_ICON_SIZE: u32 = 32;
const TASKBAR_ICON_SIZE: u32 = 256;

// The Windows icon is rendered from assets/icon.svg so no binary file needs to be tracked.
fn main() {
    println!("cargo:rerun-if-changed=assets/icon.svg");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let svg = fs::read_to_string("assets/icon.svg").expect("read assets/icon.svg");
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default()).expect("parse icon SVG");
    let source = tree.size();
    assert_eq!(source.width(), source.height(), "icon must be square");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set"));
    let icon_path = out_dir.join("mangania.ico");
    fs::write(&icon_path, render_ico(&tree)).expect("write generated icon");

    // The icon embedded in the exe only covers Explorer. The live window needs
    // its own RGBA icon, which winit converts to an HICON.
    write_rgba(&tree, &out_dir, "titlebar", TITLEBAR_ICON_SIZE);
    write_rgba(&tree, &out_dir, "taskbar", TASKBAR_ICON_SIZE);

    winresource::WindowsResource::new()
        .set_icon(icon_path.to_str().expect("icon path is UTF-8"))
        .compile()
        .expect("failed to embed the Windows icon");
}

fn write_rgba(tree: &usvg::Tree, out_dir: &Path, name: &str, size: u32) {
    let pixmap = render_pixmap(tree, size);
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for pixel in pixmap.pixels() {
        let color = pixel.demultiply();
        rgba.extend_from_slice(&[color.red(), color.green(), color.blue(), color.alpha()]);
    }
    fs::write(out_dir.join(format!("mangania-{name}.rgba")), rgba).expect("write window icon");
}

fn render_pixmap(tree: &usvg::Tree, size: u32) -> Pixmap {
    let mut pixmap = Pixmap::new(size, size).expect("allocate icon frame");
    let scale = size as f32 / tree.size().width();
    resvg::render(
        tree,
        Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap
}

fn render_ico(tree: &usvg::Tree) -> Vec<u8> {
    let frames = SIZES
        .into_iter()
        .map(|size| {
            let pixmap = render_pixmap(tree, size);
            let data = if size == 256 {
                pixmap.encode_png().expect("encode 256 px icon")
            } else {
                bmp_frame(&pixmap)
            };
            (size, data)
        })
        .collect::<Vec<_>>();

    ico_file(&frames)
}

fn bmp_frame(pixmap: &Pixmap) -> Vec<u8> {
    let size = pixmap.width();
    let row_len = size as usize;
    let mask_stride = size.div_ceil(32) * 4;
    let mut xor = Vec::with_capacity(row_len * row_len * 4);
    let mut mask = vec![0u8; mask_stride as usize * row_len];

    for (y, row) in pixmap.pixels().chunks_exact(row_len).rev().enumerate() {
        for (x, pixel) in row.iter().enumerate() {
            let color = pixel.demultiply();
            xor.extend_from_slice(&[color.blue(), color.green(), color.red(), color.alpha()]);
            if pixel.alpha() == 0 {
                mask[y * mask_stride as usize + x / 8] |= 0x80 >> (x % 8);
            }
        }
    }

    let image_size = (xor.len() + mask.len()) as u32;
    let mut frame = Vec::with_capacity(40 + image_size as usize);
    frame.extend_from_slice(&40u32.to_le_bytes());
    frame.extend_from_slice(&(size as i32).to_le_bytes());
    frame.extend_from_slice(&((size * 2) as i32).to_le_bytes());
    frame.extend_from_slice(&1u16.to_le_bytes());
    frame.extend_from_slice(&32u16.to_le_bytes());
    frame.extend_from_slice(&0u32.to_le_bytes());
    frame.extend_from_slice(&image_size.to_le_bytes());
    frame.extend_from_slice(&[0u8; 16]);
    frame.extend_from_slice(&xor);
    frame.extend_from_slice(&mask);
    frame
}

fn ico_file(frames: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut file = Vec::new();
    file.extend_from_slice(&0u16.to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&(frames.len() as u16).to_le_bytes());

    let mut offset = 6 + frames.len() * 16;
    let mut offsets = Vec::with_capacity(frames.len());
    for (_, data) in frames {
        offset = (offset + 3) & !3;
        offsets.push(offset);
        offset += data.len();
    }

    for ((size, data), offset) in frames.iter().zip(&offsets) {
        let edge = if *size == 256 { 0 } else { *size as u8 };
        file.extend_from_slice(&[edge, edge, 0, 0]);
        file.extend_from_slice(&1u16.to_le_bytes());
        file.extend_from_slice(&32u16.to_le_bytes());
        file.extend_from_slice(&(data.len() as u32).to_le_bytes());
        file.extend_from_slice(&(*offset as u32).to_le_bytes());
    }

    for ((_, data), offset) in frames.iter().zip(&offsets) {
        file.resize(*offset, 0);
        file.extend_from_slice(data);
    }
    file
}
