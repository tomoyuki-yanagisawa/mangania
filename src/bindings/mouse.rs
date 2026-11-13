//! Mouse binding parsing and matching.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::Deserialize;
use winit::event::MouseButton;

use super::{Action, load_bindings, resolve_bindings};

pub const MOUSE_FILE: &str = "mouse.toml";
pub const DEFAULT_WHEEL_THRESHOLD: f32 = 50.0;
pub const DEFAULT_DOUBLE_CLICK_MS: u64 = 400;

pub const DEFAULT_MOUSE: &str = r#"# MangaNia mouse bindings.
# Buttons: left, right, middle, back, forward.
# Double clicks: double_left, double_right, double_middle, double_back, double_forward.
# Wheel: wheel_up, wheel_down.
# wheel_threshold is how much scroll is needed to turn a page.
# When a button has a double-click action, its single-click action is delayed
# by double_click_ms so the two can be distinguished.

wheel_threshold = 50.0
double_click_ms = 400

[bindings]
next = ["left", "wheel_down"]
prev = ["right", "wheel_up"]
save_page = ["double_left"]
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelDirection {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseInput {
    Button(MouseButton),
    DoubleClick(MouseButton),
    Wheel(WheelDirection),
}

impl MouseInput {
    fn parse(text: &str) -> Option<Self> {
        let normalized = text.trim().to_ascii_lowercase().replace(['-', ' '], "_");
        if let Some(name) = normalized.strip_prefix("double_") {
            return mouse_button(name).map(MouseInput::DoubleClick);
        }
        if let Some(name) = normalized.strip_prefix("wheel_") {
            return Some(MouseInput::Wheel(match name {
                "up" => WheelDirection::Up,
                "down" => WheelDirection::Down,
                _ => return None,
            }));
        }
        mouse_button(&normalized).map(MouseInput::Button)
    }
}

fn mouse_button(name: &str) -> Option<MouseButton> {
    Some(match name {
        "left" => MouseButton::Left,
        "right" => MouseButton::Right,
        "middle" => MouseButton::Middle,
        "back" => MouseButton::Back,
        "forward" => MouseButton::Forward,
        _ => return None,
    })
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawMouseBindings {
    #[serde(default)]
    bindings: BTreeMap<String, Vec<String>>,
    wheel_threshold: Option<f32>,
    double_click_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MouseBindings {
    pairs: Vec<(MouseInput, Action)>,
    threshold: f32,
    double_click: Duration,
}

impl Default for MouseBindings {
    fn default() -> Self {
        match toml::from_str::<RawMouseBindings>(DEFAULT_MOUSE) {
            Ok(raw) => Self::from_raw(raw, None),
            Err(error) => {
                eprintln!("mangania: built-in mouse bindings: {error}");
                Self {
                    pairs: Vec::new(),
                    threshold: DEFAULT_WHEEL_THRESHOLD,
                    double_click: Duration::from_millis(DEFAULT_DOUBLE_CLICK_MS),
                }
            }
        }
    }
}

impl MouseBindings {
    pub fn load() -> Self {
        load_bindings(MOUSE_FILE, DEFAULT_MOUSE, Self::parse)
    }

    pub fn parse(text: &str) -> Self {
        let defaults = Self::default();
        let raw: RawMouseBindings = match toml::from_str(text) {
            Ok(raw) => raw,
            Err(error) => {
                eprintln!("mangania: mouse.toml: {error}");
                return defaults;
            }
        };
        Self::from_raw(raw, Some(&defaults))
    }

    fn from_raw(raw: RawMouseBindings, defaults: Option<&Self>) -> Self {
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
        let double_click = match raw.double_click_ms {
            Some(value) if value > 0 => Duration::from_millis(value),
            Some(value) => {
                eprintln!(
                    "mangania: mouse.toml: invalid double_click_ms {value}; using {DEFAULT_DOUBLE_CLICK_MS}"
                );
                Duration::from_millis(DEFAULT_DOUBLE_CLICK_MS)
            }
            None => Duration::from_millis(DEFAULT_DOUBLE_CLICK_MS),
        };
        Self {
            pairs: resolve_bindings(
                raw.bindings,
                "mouse.toml",
                "input",
                defaults.map(|defaults| defaults.pairs.as_slice()),
                MouseInput::parse,
            ),
            threshold,
            double_click,
        }
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

    pub fn double_click_interval(&self) -> Duration {
        self.double_click
    }

    pub fn double_action(&self, button: MouseButton) -> Option<Action> {
        self.action_for(MouseInput::DoubleClick(button))
    }

    pub fn single_action(&self, button: MouseButton) -> Option<Action> {
        self.action_for(MouseInput::Button(button))
    }
}
