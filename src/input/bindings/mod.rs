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
    NextSingle,
    PrevSingle,
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
    const ALL: [Action; 12] = [
        Action::Next,
        Action::Prev,
        Action::NextSingle,
        Action::PrevSingle,
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
            Action::NextSingle => "next_single",
            Action::PrevSingle => "prev_single",
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
}

fn resolve_bindings<T: Copy>(
    mut map: BTreeMap<String, Vec<String>>,
    file: &str,
    what: &str,
    defaults: Option<&[(T, Action)]>,
    parse: impl Fn(&str) -> Option<T>,
) -> Vec<(T, Action)> {
    let mut pairs = Vec::new();
    for action in Action::ALL {
        match map.remove(action.as_str()) {
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
    for name in map.keys() {
        eprintln!("mangania: {file}: unknown action {name:?}");
    }
    pairs
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_names_are_unique_and_ordered() {
        let names: Vec<_> = Action::ALL.iter().map(|action| action.as_str()).collect();
        assert_eq!(
            names,
            [
                "next",
                "prev",
                "next_single",
                "prev_single",
                "first",
                "last",
                "toggle_spread",
                "single_page",
                "spread_page",
                "toggle_fullscreen",
                "save_page",
                "quit",
            ]
        );
    }

    #[test]
    fn resolution_keeps_defaults_and_action_priority() {
        let defaults = [
            ('x', Action::Next),
            ('y', Action::Prev),
            ('z', Action::Quit),
        ];
        let map = BTreeMap::from([
            ("prev".into(), vec!["x".into()]),
            ("quit".into(), vec![]),
            ("next_page".into(), vec!["a".into()]),
            ("unknown".into(), vec!["b".into()]),
        ]);
        let pairs = resolve_bindings(map, "test", "input", Some(&defaults), |s| s.chars().next());
        assert_eq!(pairs, [('x', Action::Next), ('x', Action::Prev)]);
    }

    #[test]
    fn explicit_invalid_inputs_replace_defaults() {
        let defaults = [('x', Action::Next)];
        let map = BTreeMap::from([("next".into(), vec!["invalid".into()])]);
        let pairs = resolve_bindings(map, "test", "input", Some(&defaults), |_| None);
        assert!(pairs.is_empty());
    }
}
