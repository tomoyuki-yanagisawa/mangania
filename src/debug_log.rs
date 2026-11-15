use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static FILE: OnceLock<Mutex<File>> = OnceLock::new();
static START: OnceLock<Instant> = OnceLock::new();

pub(crate) fn init(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)?;
    let _ = START.set(Instant::now());
    let _ = FILE.set(Mutex::new(file));

    log(format_args!(
        "MangaNia {} debug log",
        env!("CARGO_PKG_VERSION")
    ));
    log(format_args!(
        "os={} arch={}",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    Ok(())
}

pub(crate) fn log(args: std::fmt::Arguments<'_>) {
    let Some(file) = FILE.get() else {
        return;
    };
    let elapsed = START
        .get()
        .map(|start| start.elapsed().as_secs_f64())
        .unwrap_or(0.0);
    if let Ok(mut file) = file.lock() {
        let _ = writeln!(file, "[{elapsed:>9.3}s] {args}");
        let _ = file.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_debug_lines() {
        let dir = std::env::temp_dir().join(format!("mangania-debug-log-{}", std::process::id()));
        let path = dir.join("debug.log");

        init(&path).unwrap();
        log(format_args!("probe"));

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("MangaNia"));
        assert!(text.contains("probe"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
