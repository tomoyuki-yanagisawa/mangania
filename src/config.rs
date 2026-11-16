use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use image::imageops::FilterType;
use serde::{Deserialize, Serialize};

pub const DEFAULT_EXPORT_NAME: &str = "{book}_{page:03}";
pub const DEFAULT_PREFETCH: usize = 2;
pub const DEFAULT_CACHE_PAGES: usize = 8;
pub const DEFAULT_CACHE_MB: usize = 128;

const CONFIG_FILE: &str = "config.toml";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleFilter {
    Nearest,
    #[default]
    Triangle,
    CatmullRom,
    Gaussian,
    Lanczos3,
}

impl ScaleFilter {
    pub const ALL: [ScaleFilter; 5] = [
        ScaleFilter::Nearest,
        ScaleFilter::Triangle,
        ScaleFilter::CatmullRom,
        ScaleFilter::Gaussian,
        ScaleFilter::Lanczos3,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ScaleFilter::Nearest => "nearest",
            ScaleFilter::Triangle => "triangle",
            ScaleFilter::CatmullRom => "catmull_rom",
            ScaleFilter::Gaussian => "gaussian",
            ScaleFilter::Lanczos3 => "lanczos3",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ScaleFilter::Nearest => "Nearest",
            ScaleFilter::Triangle => "Triangle",
            ScaleFilter::CatmullRom => "Catmull-Rom",
            ScaleFilter::Gaussian => "Gaussian",
            ScaleFilter::Lanczos3 => "Lanczos3",
        }
    }

    pub fn to_filter_type(self) -> FilterType {
        match self {
            ScaleFilter::Nearest => FilterType::Nearest,
            ScaleFilter::Triangle => FilterType::Triangle,
            ScaleFilter::CatmullRom => FilterType::CatmullRom,
            ScaleFilter::Gaussian => FilterType::Gaussian,
            ScaleFilter::Lanczos3 => FilterType::Lanczos3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub spread: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub export_dir: Option<PathBuf>,
    pub export_name: String,
    pub prefetch: usize,
    pub cache_pages: usize,
    pub cache_mb: usize,
    pub scale_filter: ScaleFilter,
    pub debug: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            spread: false,
            export_dir: None,
            export_name: DEFAULT_EXPORT_NAME.to_string(),
            prefetch: DEFAULT_PREFETCH,
            cache_pages: DEFAULT_CACHE_PAGES,
            cache_mb: DEFAULT_CACHE_MB,
            scale_filter: ScaleFilter::default(),
            debug: false,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        config_path(CONFIG_FILE)
            .map(|path| Self::load_or_create_from(&path))
            .unwrap_or_default()
    }

    pub fn load_or_create_from(path: &Path) -> Self {
        if let Err(error) = write_default_config(path)
            && error.kind() != io::ErrorKind::AlreadyExists
        {
            eprintln!("mangania: write default config {}: {error}", path.display());
        }
        Self::load_from(path)
    }

    pub fn load_from(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => match toml::from_str(&text) {
                Ok(config) => config,
                Err(error) => {
                    eprintln!("mangania: parse config {}: {error}", path.display());
                    Self::default()
                }
            },
            Err(error) => {
                eprintln!("mangania: read config {}: {error}", path.display());
                Self::default()
            }
        }
    }

    pub fn save(&self) {
        let Some(path) = config_path(CONFIG_FILE) else {
            return;
        };
        if let Err(error) = self.save_to(&path) {
            eprintln!("mangania: save config {}: {error}", path.display());
        }
    }

    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        validate_existing::<Config>(path)?;
        atomic_write(path, self.to_toml().as_bytes())
    }

    pub fn parse(text: &str) -> Self {
        toml::from_str(text).unwrap_or_default()
    }

    pub fn to_toml(&self) -> String {
        let mut text = String::from(concat!(
            "# MangaNia settings\n",
            "# spread: false = 1ページ表示, true = 見開き表示\n",
            "# export_dir: 省略時は OS のピクチャフォルダ/MangaNia\n",
            "# export_name: {book}, {page}, {page:03}, {name} が使えます\n",
            "# prefetch: 先読みする見開き数（単ページ表示ではページ数）\n",
            "# cache_pages: メモリに保持するページ数。0 は既定値\n",
            "# cache_mb: メモリキャッシュ上限（MiB）。0 は既定値\n",
            "# scale_filter: nearest / triangle / catmull_rom / gaussian / lanczos3\n",
            "# debug: true で debug.log にログを出力\n",
            "\n",
        ));
        text.push_str(&toml::to_string(self).unwrap_or_default());
        if self.export_dir.is_none() {
            text.push_str("\n# export_dir = \"~/Pictures/MangaNia\"\n");
        }
        text
    }

    pub fn resolved_export_dir(&self) -> PathBuf {
        self.export_dir
            .as_deref()
            .map(expand_tilde)
            .unwrap_or_else(|| {
                picture_dir()
                    .or_else(home_dir)
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join("MangaNia")
            })
    }

    /// Cache capacity in images. One page can occupy two images: its window
    /// preview and the exact size used for spread layout.
    pub fn cache_entries(&self) -> usize {
        let pages = if self.cache_pages == 0 {
            DEFAULT_CACHE_PAGES
        } else {
            self.cache_pages
        };
        pages.saturating_mul(2)
    }

    pub fn cache_bytes(&self) -> usize {
        let cache_mb = if self.cache_mb == 0 {
            DEFAULT_CACHE_MB
        } else {
            self.cache_mb
        };
        cache_mb.saturating_mul(1024 * 1024)
    }
}

fn write_default_config(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(Config::default().to_toml().as_bytes())
}

fn validate_existing<T: for<'de> Deserialize<'de>>(path: &Path) -> io::Result<()> {
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str::<T>(&text)
            .map(|_| ())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let (temp, mut file) = loop {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!(".mangania-{}-{id}.tmp", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
        {
            Ok(file) => break (temp, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        if let Ok(metadata) = std::fs::metadata(path) {
            file.set_permissions(metadata.permissions())?;
        }
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}

fn expand_tilde(path: &Path) -> PathBuf {
    if let Ok(rest) = path.strip_prefix("~")
        && let Some(home) = home_dir()
    {
        return home.join(rest);
    }
    path.to_path_buf()
}

#[cfg(windows)]
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(PathBuf::from)
}

#[cfg(not(windows))]
fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

#[cfg(windows)]
fn config_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join("AppData").join("Roaming")))
}

#[cfg(not(windows))]
fn config_dir() -> Option<PathBuf> {
    dirs::config_dir()
}

#[cfg(windows)]
fn picture_dir() -> Option<PathBuf> {
    home_dir().map(|home| home.join("Pictures"))
}

#[cfg(not(windows))]
fn picture_dir() -> Option<PathBuf> {
    dirs::picture_dir()
}

pub(crate) fn config_path(name: &str) -> Option<PathBuf> {
    config_dir().map(|base| base.join("mangania").join(name))
}
