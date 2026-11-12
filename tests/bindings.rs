use std::time::Duration;

use mangania::bindings::{
    Action, DEFAULT_KEYBINDINGS, DEFAULT_MOUSE, KeyBindings, MouseBindings, MouseInput,
    WheelDirection,
};
use winit::event::MouseButton;
use winit::keyboard::{Key, ModifiersState, NamedKey, SmolStr};

fn character(key: &str) -> Key {
    Key::Character(SmolStr::new(key))
}

#[test]
fn default_key_bindings_work() {
    let bindings = KeyBindings::default();
    assert_eq!(
        bindings.action_for(&Key::Named(NamedKey::ArrowRight), ModifiersState::empty()),
        Some(Action::Next)
    );
    assert_eq!(
        bindings.action_for(&Key::Named(NamedKey::ArrowLeft), ModifiersState::empty()),
        Some(Action::Prev)
    );
    assert_eq!(
        bindings.action_for(&Key::Named(NamedKey::Home), ModifiersState::empty()),
        Some(Action::First)
    );
    assert_eq!(
        bindings.action_for(&Key::Named(NamedKey::End), ModifiersState::empty()),
        Some(Action::Last)
    );
    assert_eq!(
        bindings.action_for(&character("v"), ModifiersState::empty()),
        Some(Action::ToggleSpread)
    );
    assert_eq!(
        bindings.action_for(&Key::Named(NamedKey::F11), ModifiersState::empty()),
        Some(Action::ToggleFullscreen)
    );
    assert_eq!(
        bindings.action_for(&character("s"), ModifiersState::CONTROL),
        Some(Action::SavePage)
    );
    assert_eq!(
        bindings.action_for(&character("s"), ModifiersState::SUPER),
        Some(Action::SavePage)
    );
    assert_eq!(
        bindings.action_for(&Key::Named(NamedKey::Escape), ModifiersState::empty()),
        Some(Action::Quit)
    );
}

#[test]
fn default_files_match_defaults() {
    assert_eq!(
        KeyBindings::parse(DEFAULT_KEYBINDINGS),
        KeyBindings::default()
    );
    assert_eq!(
        MouseBindings::parse(DEFAULT_MOUSE),
        MouseBindings::default()
    );
}

#[test]
fn custom_key_bindings_replace_defaults() {
    let bindings = KeyBindings::parse(
        r#"
[keys]
next = ["n", "PageDown"]
save = ["Ctrl+Shift+S"]
quit = []
"#,
    );

    assert_eq!(
        bindings.action_for(&character("n"), ModifiersState::empty()),
        Some(Action::Next)
    );
    assert_eq!(
        bindings.action_for(&Key::Named(NamedKey::PageDown), ModifiersState::empty()),
        Some(Action::Next)
    );
    assert_eq!(
        bindings.action_for(&Key::Named(NamedKey::ArrowRight), ModifiersState::empty()),
        None
    );
    assert_eq!(
        bindings.action_for(
            &character("s"),
            ModifiersState::CONTROL | ModifiersState::SHIFT
        ),
        Some(Action::SavePage)
    );
    assert_eq!(
        bindings.action_for(&character("s"), ModifiersState::CONTROL),
        None
    );
    assert_eq!(
        bindings.action_for(&Key::Named(NamedKey::Escape), ModifiersState::empty()),
        None
    );
}

#[test]
fn modifiers_must_match_exactly() {
    let bindings = KeyBindings::default();
    assert_eq!(
        bindings.action_for(
            &character("s"),
            ModifiersState::CONTROL | ModifiersState::SHIFT
        ),
        None
    );
    assert_eq!(
        bindings.action_for(&character("v"), ModifiersState::SHIFT),
        None
    );
}

#[test]
fn keys_are_case_insensitive() {
    let bindings = KeyBindings::parse("[keys]\ntoggle_spread = [\"V\"]\n");
    assert_eq!(
        bindings.action_for(&character("v"), ModifiersState::empty()),
        Some(Action::ToggleSpread)
    );
}

#[test]
fn invalid_keys_and_actions_fall_back_to_defaults() {
    let bindings = KeyBindings::parse(
        r#"
[keys]
next = ["NotAKey", "Ctrl+Nope"]
unknown_action = ["x"]
"#,
    );
    assert_eq!(
        bindings.action_for(&Key::Named(NamedKey::ArrowRight), ModifiersState::empty()),
        None
    );
    assert_eq!(
        bindings.action_for(&Key::Named(NamedKey::ArrowLeft), ModifiersState::empty()),
        Some(Action::Prev)
    );
}

#[test]
fn default_mouse_bindings_work() {
    let bindings = MouseBindings::default();
    assert_eq!(
        bindings.action_for(MouseInput::Button(MouseButton::Left)),
        Some(Action::Next)
    );
    assert_eq!(
        bindings.action_for(MouseInput::Button(MouseButton::Right)),
        Some(Action::Prev)
    );
    assert_eq!(
        bindings.action_for(MouseInput::Wheel(WheelDirection::Down)),
        Some(Action::Next)
    );
    assert_eq!(
        bindings.action_for(MouseInput::Wheel(WheelDirection::Up)),
        Some(Action::Prev)
    );
    assert_eq!(
        bindings.double_action(MouseButton::Left),
        Some(Action::SavePage)
    );
    assert_eq!(
        bindings.single_action(MouseButton::Left),
        Some(Action::Next)
    );
    assert_eq!(bindings.wheel_threshold(), 50.0);
    assert_eq!(bindings.double_click_interval(), Duration::from_millis(400));
}

#[test]
fn custom_mouse_bindings_replace_defaults() {
    let bindings = MouseBindings::parse(
        r#"
wheel_threshold = 25.5
double_click_ms = 250

[bindings]
next = ["middle", "wheel_up"]
prev = []
save_page = ["double_left", "double_middle"]
"#,
    );

    assert_eq!(bindings.wheel_threshold(), 25.5);
    assert_eq!(bindings.double_click_interval(), Duration::from_millis(250));
    assert_eq!(
        bindings.action_for(MouseInput::Button(MouseButton::Middle)),
        Some(Action::Next)
    );
    assert_eq!(
        bindings.action_for(MouseInput::Wheel(WheelDirection::Up)),
        Some(Action::Next)
    );
    assert_eq!(
        bindings.action_for(MouseInput::Button(MouseButton::Left)),
        None
    );
    assert_eq!(
        bindings.action_for(MouseInput::Button(MouseButton::Right)),
        None
    );
    assert_eq!(
        bindings.action_for(MouseInput::DoubleClick(MouseButton::Left)),
        Some(Action::SavePage)
    );
    assert_eq!(
        bindings.action_for(MouseInput::DoubleClick(MouseButton::Middle)),
        Some(Action::SavePage)
    );
}

#[test]
fn invalid_mouse_options_use_defaults() {
    let bindings = MouseBindings::parse("wheel_threshold = 0\ndouble_click_ms = 0\n");
    assert_eq!(bindings.wheel_threshold(), 50.0);
    assert_eq!(bindings.double_click_interval(), Duration::from_millis(400));
}
