use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DEFAULT_EXPORT_NAME: &str = "{book}_{page:03}";
pub const DEFAULT_PREFETCH: usize = 2;
pub const DEFAULT_CACHE_MB: usize = 128;

const CONFIG_FILE: &str = "config.toml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub spread: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub export_dir: Option<PathBuf>,
    pub export_name: String,
    pub prefetch: usize,
    pub cache_mb: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            spread: false,
            export_dir: None,
            export_name: DEFAULT_EXPORT_NAME.to_string(),
            prefetch: DEFAULT_PREFETCH,
            cache_mb: DEFAULT_CACHE_MB,
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
        if !path.exists()
            && let Err(error) = write_default_config(path)
        {
            eprintln!("mangania: write default config {}: {error}", path.display());
        }
        Self::load_from(path)
    }

    pub fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
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
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, self.to_toml())
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
            "# prefetch: 先読み枚数\n",
            "# cache_mb: メモリキャッシュ上限（MiB）。0 は既定値\n",
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
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, Config::default().to_toml())
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
