use std::ffi::OsString;
use std::path::PathBuf;

use mangania::cli::{Mode, parse};

fn parse_args(values: &[&str]) -> Result<mangania::cli::Args, String> {
    parse(values.iter().map(|value| OsString::from(*value)))
}

#[test]
fn parses_list_and_prefetch() {
    let args = parse_args(&["--prefetch", "4", "--list", "book.zip"]).unwrap();
    assert_eq!(args.prefetch, 4);
    assert_eq!(args.path, Some(PathBuf::from("book.zip")));
    assert_eq!(args.mode, Mode::List);
}

#[test]
fn parses_render_page() {
    let args = parse_args(&["--render-page", "2", "out.png", "book.zip"]).unwrap();
    assert_eq!(
        args.mode,
        Mode::Render {
            page: 2,
            output: PathBuf::from("out.png")
        }
    );
}

#[test]
fn requires_render_output() {
    assert!(parse_args(&["--render-page", "2"]).is_err());
}
