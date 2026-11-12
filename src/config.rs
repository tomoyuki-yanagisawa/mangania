use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DEFAULT_EXPORT_NAME: &str = "{book}_{page:03}";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub spread: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub export_dir: Option<PathBuf>,
    pub export_name: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            spread: false,
            export_dir: None,
            export_name: DEFAULT_EXPORT_NAME.to_string(),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        config_path()
            .map(|path| Self::load_from(&path))
            .unwrap_or_default()
    }

    pub fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = config_path() else {
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
        toml::to_string(self).unwrap_or_default()
    }

    pub fn resolved_export_dir(&self) -> PathBuf {
        self.export_dir
            .as_deref()
            .map(expand_tilde)
            .unwrap_or_else(|| {
                dirs::picture_dir()
                    .or_else(dirs::home_dir)
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join("MangaNia")
            })
    }
}

fn expand_tilde(path: &Path) -> PathBuf {
    if let Ok(rest) = path.strip_prefix("~")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    path.to_path_buf()
}

fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|base| base.join("mangania").join("config.toml"))
}
