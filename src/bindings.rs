use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use winit::event::MouseButton;
use winit::keyboard::{Key, ModifiersState, NamedKey};

use crate::config;

pub const KEYBINDINGS_FILE: &str = "keybindings.toml";
pub const MOUSE_FILE: &str = "mouse.toml";
pub const DEFAULT_WHEEL_THRESHOLD: f32 = 50.0;

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

pub const DEFAULT_MOUSE: &str = r#"# MangaNia mouse bindings.
# Buttons: left, right, middle, back, forward.
# Wheel: wheel_up, wheel_down.
# wheel_threshold is how much scroll is needed to turn a page.

wheel_threshold = 50.0

[bindings]
next = ["left", "wheel_down"]
prev = ["right", "wheel_up"]
"#;

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
    fn named(key: NamedKey) -> Self {
        Self {
            modifiers: ModifiersState::empty(),
            key: KeyCode::Named(key),
        }
    }

    fn character(key: char) -> Self {
        Self {
            modifiers: ModifiersState::empty(),
            key: KeyCode::Char(key),
        }
    }

    fn with(self, modifiers: ModifiersState) -> Self {
        Self {
            modifiers: self.modifiers | modifiers,
            key: self.key,
        }
    }

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
        Self {
            pairs: vec![
                (KeyChord::named(NamedKey::ArrowRight), Action::Next),
                (KeyChord::named(NamedKey::ArrowLeft), Action::Prev),
                (KeyChord::named(NamedKey::Home), Action::First),
                (KeyChord::named(NamedKey::End), Action::Last),
                (KeyChord::character('v'), Action::ToggleSpread),
                (KeyChord::named(NamedKey::F11), Action::ToggleFullscreen),
                (
                    KeyChord::character('s').with(ModifiersState::CONTROL),
                    Action::SavePage,
                ),
                (
                    KeyChord::character('s').with(ModifiersState::SUPER),
                    Action::SavePage,
                ),
                (KeyChord::named(NamedKey::Escape), Action::Quit),
            ],
        }
    }
}

impl KeyBindings {
    pub fn load() -> Self {
        let Some(path) = config::config_path(KEYBINDINGS_FILE) else {
            return Self::default();
        };
        ensure_default_file(&path, DEFAULT_KEYBINDINGS);
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(error) => {
                eprintln!("mangania: read {}: {error}", path.display());
                Self::default()
            }
        }
    }

    pub fn parse(text: &str) -> Self {
        let raw: RawKeyBindings = match toml::from_str(text) {
            Ok(raw) => raw,
            Err(error) => {
                eprintln!("mangania: keybindings.toml: {error}");
                return Self::default();
            }
        };
        Self::from_map(raw.keys)
    }

    fn from_map(map: BTreeMap<String, Vec<String>>) -> Self {
        let defaults = Self::default();
        let mut canonical = canonical_actions(map, "keybindings.toml");
        let mut pairs = Vec::new();
        for action in Action::ALL {
            match canonical.remove(action.as_str()) {
                Some(specs) => {
                    for spec in specs {
                        match KeyChord::parse(&spec) {
                            Some(chord) => pairs.push((chord, action)),
                            None => eprintln!(
                                "mangania: keybindings.toml: invalid key {spec:?} for {}",
                                action.as_str()
                            ),
                        }
                    }
                }
                None => pairs.extend(
                    defaults
                        .pairs
                        .iter()
                        .filter(|(_, default)| *default == action)
                        .copied(),
                ),
            }
        }
        Self { pairs }
    }

    pub fn action_for(&self, key: &Key, modifiers: ModifiersState) -> Option<Action> {
        self.pairs
            .iter()
            .find(|(chord, _)| chord.matches(key, modifiers))
            .map(|(_, action)| *action)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelDirection {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseInput {
    Button(MouseButton),
    Wheel(WheelDirection),
}

impl MouseInput {
    fn parse(text: &str) -> Option<Self> {
        let normalized = text.trim().to_ascii_lowercase().replace(['-', ' '], "_");
        Some(match normalized.as_str() {
            "left" => MouseInput::Button(MouseButton::Left),
            "right" => MouseInput::Button(MouseButton::Right),
            "middle" => MouseInput::Button(MouseButton::Middle),
            "back" => MouseInput::Button(MouseButton::Back),
            "forward" => MouseInput::Button(MouseButton::Forward),
            "wheel_up" | "wheelup" | "scroll_up" | "scrollup" => {
                MouseInput::Wheel(WheelDirection::Up)
            }
            "wheel_down" | "wheeldown" | "scroll_down" | "scrolldown" => {
                MouseInput::Wheel(WheelDirection::Down)
            }
            _ => return None,
        })
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawMouseBindings {
    #[serde(default)]
    bindings: BTreeMap<String, Vec<String>>,
    wheel_threshold: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MouseBindings {
    pairs: Vec<(MouseInput, Action)>,
    threshold: f32,
}

impl Default for MouseBindings {
    fn default() -> Self {
        Self {
            pairs: vec![
                (MouseInput::Button(MouseButton::Left), Action::Next),
                (MouseInput::Wheel(WheelDirection::Down), Action::Next),
                (MouseInput::Button(MouseButton::Right), Action::Prev),
                (MouseInput::Wheel(WheelDirection::Up), Action::Prev),
            ],
            threshold: DEFAULT_WHEEL_THRESHOLD,
        }
    }
}

impl MouseBindings {
    pub fn load() -> Self {
        let Some(path) = config::config_path(MOUSE_FILE) else {
            return Self::default();
        };
        ensure_default_file(&path, DEFAULT_MOUSE);
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(error) => {
                eprintln!("mangania: read {}: {error}", path.display());
                Self::default()
            }
        }
    }

    pub fn parse(text: &str) -> Self {
        let raw: RawMouseBindings = match toml::from_str(text) {
            Ok(raw) => raw,
            Err(error) => {
                eprintln!("mangania: mouse.toml: {error}");
                return Self::default();
            }
        };
        let threshold = match raw.wheel_threshold {
            Some(value) if value.is_finite() && value > 0.0 => value,
            Some(value) => {
                eprintln!(
                    "mangania: mouse.toml: invalid wheel_threshold {value}; using {DEFAULT_WHEEL_THRESHOLD}"
                );
                DEFAULT_WHEEL_THRESHOLD
            }
            None => DEFAULT_WHEEL_THRESHOLD,
        };
        Self::from_map(raw.bindings, threshold)
    }

    fn from_map(map: BTreeMap<String, Vec<String>>, threshold: f32) -> Self {
        let defaults = Self::default();
        let mut canonical = canonical_actions(map, "mouse.toml");
        let mut pairs = Vec::new();
        for action in Action::ALL {
            match canonical.remove(action.as_str()) {
                Some(specs) => {
                    for spec in specs {
                        match MouseInput::parse(&spec) {
                            Some(input) => pairs.push((input, action)),
                            None => eprintln!(
                                "mangania: mouse.toml: invalid input {spec:?} for {}",
                                action.as_str()
                            ),
                        }
                    }
                }
                None => pairs.extend(
                    defaults
                        .pairs
                        .iter()
                        .filter(|(_, default)| *default == action)
                        .copied(),
                ),
            }
        }
        Self { pairs, threshold }
    }

    pub fn action_for(&self, input: MouseInput) -> Option<Action> {
        self.pairs
            .iter()
            .find(|(bound, _)| *bound == input)
            .map(|(_, action)| *action)
    }

    pub fn wheel_threshold(&self) -> f32 {
        self.threshold
    }
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
