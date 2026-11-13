//! Keyboard binding parsing and matching.

use std::collections::BTreeMap;

use serde::Deserialize;
use winit::keyboard::{Key, ModifiersState, NamedKey};

use super::{Action, load_bindings, resolve_bindings};

pub const KEYBINDINGS_FILE: &str = "keybindings.toml";

pub const DEFAULT_KEYBINDINGS: &str = r#"# MangaNia key bindings.
# Format: "Modifier+Key" (e.g. "Ctrl+S", "Shift+Left").
# Modifiers: Ctrl, Shift, Alt, Super (Super = Cmd on macOS).
# An action can have multiple keys. Use [] to disable an action.

[keys]
next = ["Right"]
prev = ["Left"]
first = ["Home"]
last = ["End"]
toggle_spread = ["v"]
single_page = []
spread_page = []
toggle_fullscreen = ["F11"]
save_page = ["Ctrl+S", "Cmd+S"]
quit = ["Escape"]
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyCode {
    Named(NamedKey),
    Char(char),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct KeyChord {
    modifiers: ModifiersState,
    key: KeyCode,
}

impl KeyChord {
    fn parse(text: &str) -> Option<Self> {
        let parts = split_chord(text)?;
        let (key, modifier_tokens) = parts.split_last()?;
        let mut modifiers = ModifiersState::empty();
        for token in modifier_tokens {
            let modifier = match token.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => ModifiersState::CONTROL,
                "shift" => ModifiersState::SHIFT,
                "alt" | "option" | "opt" => ModifiersState::ALT,
                "super" | "cmd" | "command" | "meta" | "win" => ModifiersState::SUPER,
                _ => return None,
            };
            modifiers |= modifier;
        }
        Some(Self {
            modifiers,
            key: parse_key(key)?,
        })
    }

    fn matches(self, key: &Key, modifiers: ModifiersState) -> bool {
        if self.modifiers != modifiers & modifier_mask() {
            return false;
        }
        match (self.key, key) {
            (KeyCode::Named(expected), Key::Named(actual)) => expected == *actual,
            (KeyCode::Char(expected), Key::Character(actual)) => {
                let mut chars = actual.chars();
                matches!((chars.next(), chars.next()), (Some(actual), None) if expected.eq_ignore_ascii_case(&actual))
            }
            _ => false,
        }
    }
}

fn modifier_mask() -> ModifiersState {
    ModifiersState::CONTROL | ModifiersState::SHIFT | ModifiersState::ALT | ModifiersState::SUPER
}

fn split_chord(text: &str) -> Option<Vec<&str>> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if text == "+" {
        return Some(vec!["+"]);
    }
    if let Some(prefix) = text.strip_suffix("++") {
        let mut parts: Vec<&str> = prefix
            .split('+')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect();
        parts.push("+");
        return Some(parts);
    }
    let parts: Vec<&str> = text
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    (!parts.is_empty()).then_some(parts)
}

fn parse_key(token: &str) -> Option<KeyCode> {
    let lower = token.to_ascii_lowercase();
    if lower == "plus" {
        return Some(KeyCode::Char('+'));
    }
    let mut chars = token.chars();
    if let (Some(key), None) = (chars.next(), chars.next()) {
        return Some(KeyCode::Char(key.to_ascii_lowercase()));
    }
    named_key(&lower).map(KeyCode::Named)
}

fn named_key(name: &str) -> Option<NamedKey> {
    Some(match name {
        "left" | "arrowleft" => NamedKey::ArrowLeft,
        "right" | "arrowright" => NamedKey::ArrowRight,
        "up" | "arrowup" => NamedKey::ArrowUp,
        "down" | "arrowdown" => NamedKey::ArrowDown,
        "home" => NamedKey::Home,
        "end" => NamedKey::End,
        "pageup" | "pgup" => NamedKey::PageUp,
        "pagedown" | "pgdn" => NamedKey::PageDown,
        "escape" | "esc" => NamedKey::Escape,
        "space" | "spacebar" => NamedKey::Space,
        "enter" | "return" => NamedKey::Enter,
        "tab" => NamedKey::Tab,
        "backspace" => NamedKey::Backspace,
        "delete" | "del" => NamedKey::Delete,
        "insert" | "ins" => NamedKey::Insert,
        "f1" => NamedKey::F1,
        "f2" => NamedKey::F2,
        "f3" => NamedKey::F3,
        "f4" => NamedKey::F4,
        "f5" => NamedKey::F5,
        "f6" => NamedKey::F6,
        "f7" => NamedKey::F7,
        "f8" => NamedKey::F8,
        "f9" => NamedKey::F9,
        "f10" => NamedKey::F10,
        "f11" => NamedKey::F11,
        "f12" => NamedKey::F12,
        "f13" => NamedKey::F13,
        "f14" => NamedKey::F14,
        "f15" => NamedKey::F15,
        "f16" => NamedKey::F16,
        "f17" => NamedKey::F17,
        "f18" => NamedKey::F18,
        "f19" => NamedKey::F19,
        "f20" => NamedKey::F20,
        "f21" => NamedKey::F21,
        "f22" => NamedKey::F22,
        "f23" => NamedKey::F23,
        "f24" => NamedKey::F24,
        "f25" => NamedKey::F25,
        "f26" => NamedKey::F26,
        "f27" => NamedKey::F27,
        "f28" => NamedKey::F28,
        "f29" => NamedKey::F29,
        "f30" => NamedKey::F30,
        "f31" => NamedKey::F31,
        "f32" => NamedKey::F32,
        "f33" => NamedKey::F33,
        "f34" => NamedKey::F34,
        "f35" => NamedKey::F35,
        _ => return None,
    })
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawKeyBindings {
    #[serde(default)]
    keys: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBindings {
    pairs: Vec<(KeyChord, Action)>,
}

impl Default for KeyBindings {
    fn default() -> Self {
        match toml::from_str::<RawKeyBindings>(DEFAULT_KEYBINDINGS) {
            Ok(raw) => Self {
                pairs: resolve_bindings(raw.keys, "keybindings.toml", "key", None, KeyChord::parse),
            },
            Err(error) => {
                eprintln!("mangania: built-in keybindings: {error}");
                Self { pairs: Vec::new() }
            }
        }
    }
}

impl KeyBindings {
    pub fn load() -> Self {
        load_bindings(KEYBINDINGS_FILE, DEFAULT_KEYBINDINGS, Self::parse)
    }

    pub fn parse(text: &str) -> Self {
        let defaults = Self::default();
        let raw: RawKeyBindings = match toml::from_str(text) {
            Ok(raw) => raw,
            Err(error) => {
                eprintln!("mangania: keybindings.toml: {error}");
                return defaults;
            }
        };
        Self {
            pairs: resolve_bindings(
                raw.keys,
                "keybindings.toml",
                "key",
                Some(&defaults.pairs),
                KeyChord::parse,
            ),
        }
    }

    pub fn action_for(&self, key: &Key, modifiers: ModifiersState) -> Option<Action> {
        self.pairs
            .iter()
            .find(|(chord, _)| chord.matches(key, modifiers))
            .map(|(_, action)| *action)
    }
}
