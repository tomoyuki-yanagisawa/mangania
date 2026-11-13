//! Key and mouse binding configuration.

use std::collections::BTreeMap;
use std::path::Path;

use crate::config;

mod key;
mod mouse;

pub use key::{DEFAULT_KEYBINDINGS, KEYBINDINGS_FILE, KeyBindings};
pub use mouse::{
    DEFAULT_DOUBLE_CLICK_MS, DEFAULT_MOUSE, DEFAULT_WHEEL_THRESHOLD, MOUSE_FILE, MouseBindings,
    MouseInput, WheelDirection,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Next,
    Prev,
    First,
    Last,
    ToggleSpread,
    SinglePage,
    SpreadPage,
    ToggleFullscreen,
    SavePage,
    Quit,
}

impl Action {
    const ALL: [Action; 10] = [
        Action::Next,
        Action::Prev,
        Action::First,
        Action::Last,
        Action::ToggleSpread,
        Action::SinglePage,
        Action::SpreadPage,
        Action::ToggleFullscreen,
        Action::SavePage,
        Action::Quit,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Action::Next => "next",
            Action::Prev => "prev",
            Action::First => "first",
            Action::Last => "last",
            Action::ToggleSpread => "toggle_spread",
            Action::SinglePage => "single_page",
            Action::SpreadPage => "spread_page",
            Action::ToggleFullscreen => "toggle_fullscreen",
            Action::SavePage => "save_page",
            Action::Quit => "quit",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "next" | "next_page" => Action::Next,
            "prev" | "previous" | "prev_page" | "previous_page" => Action::Prev,
            "first" | "first_page" => Action::First,
            "last" | "last_page" => Action::Last,
            "toggle_spread" => Action::ToggleSpread,
            "single_page" => Action::SinglePage,
            "spread_page" => Action::SpreadPage,
            "toggle_fullscreen" | "fullscreen" => Action::ToggleFullscreen,
            "save_page" | "save" => Action::SavePage,
            "quit" | "exit" => Action::Quit,
            _ => return None,
        })
    }
}

fn resolve_bindings<T: Copy>(
    map: BTreeMap<String, Vec<String>>,
    file: &str,
    what: &str,
    defaults: Option<&[(T, Action)]>,
    parse: impl Fn(&str) -> Option<T>,
) -> Vec<(T, Action)> {
    let mut canonical = canonical_actions(map, file);
    let mut pairs = Vec::new();
    for action in Action::ALL {
        match canonical.remove(action.as_str()) {
            Some(specs) => {
                for spec in specs {
                    match parse(&spec) {
                        Some(input) => pairs.push((input, action)),
                        None => eprintln!(
                            "mangania: {file}: invalid {what} {spec:?} for {}",
                            action.as_str()
                        ),
                    }
                }
            }
            None => {
                if let Some(defaults) = defaults {
                    pairs.extend(
                        defaults
                            .iter()
                            .filter(|(_, default)| *default == action)
                            .copied(),
                    );
                }
            }
        }
    }
    pairs
}

fn canonical_actions(
    map: BTreeMap<String, Vec<String>>,
    file: &str,
) -> BTreeMap<String, Vec<String>> {
    let mut canonical = BTreeMap::new();
    for (name, specs) in map {
        match Action::from_name(&name) {
            Some(action) => {
                canonical.insert(action.as_str().to_string(), specs);
            }
            None => eprintln!("mangania: {file}: unknown action {name:?}"),
        }
    }
    canonical
}

fn load_bindings<T: Default>(file: &str, default_text: &str, parse: impl Fn(&str) -> T) -> T {
    let Some(path) = config::config_path(file) else {
        return T::default();
    };
    ensure_default_file(&path, default_text);
    match std::fs::read_to_string(&path) {
        Ok(text) => parse(&text),
        Err(error) => {
            eprintln!("mangania: read {}: {error}", path.display());
            T::default()
        }
    }
}

fn ensure_default_file(path: &Path, contents: &str) {
    if path.exists() {
        return;
    }
    if let Some(parent) = path.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        eprintln!("mangania: create {}: {error}", parent.display());
        return;
    }
    if let Err(error) = std::fs::write(path, contents) {
        eprintln!("mangania: write {}: {error}", path.display());
    }
}
