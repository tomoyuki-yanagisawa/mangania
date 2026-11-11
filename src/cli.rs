use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    Gui,
    List,
    Check,
    Render { page: usize, output: PathBuf },
    Help,
}

#[derive(Debug)]
pub struct Args {
    pub path: Option<PathBuf>,
    pub prefetch: usize,
    pub mode: Mode,
}

pub fn parse(args: impl Iterator<Item = OsString>) -> Result<Args, String> {
    let mut args = args;
    let mut path = None;
    let mut prefetch = 2;
    let mut mode = Mode::Gui;

    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--prefetch") => {
                prefetch = args
                    .next()
                    .and_then(|value| value.to_str().and_then(|value| value.parse().ok()))
                    .unwrap_or(2);
            }
            Some("--list") => mode = Mode::List,
            Some("--check") => mode = Mode::Check,
            Some("--render-page") => {
                let page = args
                    .next()
                    .and_then(|value| value.to_str().and_then(|value| value.parse().ok()))
                    .ok_or("--render-page requires PAGE")?;
                let output = args.next().ok_or("--render-page requires OUTPUT")?;
                mode = Mode::Render {
                    page,
                    output: PathBuf::from(output),
                };
            }
            Some("--help") | Some("-h") => mode = Mode::Help,
            _ if path.is_none() => path = Some(PathBuf::from(arg)),
            _ => {}
        }
    }

    Ok(Args {
        path,
        prefetch,
        mode,
    })
}
