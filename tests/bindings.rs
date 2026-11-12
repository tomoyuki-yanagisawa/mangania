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

fn assert_key(
    bindings: &KeyBindings,
    key: Key,
    modifiers: ModifiersState,
    expected: Option<Action>,
) {
    assert_eq!(
        bindings.action_for(&key, modifiers),
        expected,
        "key {key:?} with {modifiers:?}"
    );
}

fn assert_mouse(bindings: &MouseBindings, input: MouseInput, expected: Option<Action>) {
    assert_eq!(bindings.action_for(input), expected, "input {input:?}");
}

#[test]
fn default_key_bindings_work() {
    let bindings = KeyBindings::default();
    let empty = ModifiersState::empty();
    let cases = [
        (Key::Named(NamedKey::ArrowRight), empty, Action::Next),
        (Key::Named(NamedKey::ArrowLeft), empty, Action::Prev),
        (Key::Named(NamedKey::Home), empty, Action::First),
        (Key::Named(NamedKey::End), empty, Action::Last),
        (character("v"), empty, Action::ToggleSpread),
        (Key::Named(NamedKey::F11), empty, Action::ToggleFullscreen),
        (character("s"), ModifiersState::CONTROL, Action::SavePage),
        (character("s"), ModifiersState::SUPER, Action::SavePage),
        (Key::Named(NamedKey::Escape), empty, Action::Quit),
    ];
    for (key, modifiers, expected) in cases {
        assert_key(&bindings, key, modifiers, Some(expected));
    }
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
    let empty = ModifiersState::empty();

    assert_key(&bindings, character("n"), empty, Some(Action::Next));
    assert_key(
        &bindings,
        Key::Named(NamedKey::PageDown),
        empty,
        Some(Action::Next),
    );
    assert_key(&bindings, Key::Named(NamedKey::ArrowRight), empty, None);
    assert_key(
        &bindings,
        character("s"),
        ModifiersState::CONTROL | ModifiersState::SHIFT,
        Some(Action::SavePage),
    );
    assert_key(&bindings, character("s"), ModifiersState::CONTROL, None);
    assert_key(&bindings, Key::Named(NamedKey::Escape), empty, None);
}

#[test]
fn modifiers_must_match_exactly() {
    let bindings = KeyBindings::default();
    assert_key(
        &bindings,
        character("s"),
        ModifiersState::CONTROL | ModifiersState::SHIFT,
        None,
    );
    assert_key(&bindings, character("v"), ModifiersState::SHIFT, None);
}

#[test]
fn keys_are_case_insensitive() {
    let bindings = KeyBindings::parse("[keys]\ntoggle_spread = [\"V\"]\n");
    assert_key(
        &bindings,
        character("v"),
        ModifiersState::empty(),
        Some(Action::ToggleSpread),
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
    let empty = ModifiersState::empty();

    assert_key(&bindings, Key::Named(NamedKey::ArrowRight), empty, None);
    assert_key(
        &bindings,
        Key::Named(NamedKey::ArrowLeft),
        empty,
        Some(Action::Prev),
    );
}

#[test]
fn default_mouse_bindings_work() {
    let bindings = MouseBindings::default();
    let cases = [
        (MouseInput::Button(MouseButton::Left), Action::Next),
        (MouseInput::Button(MouseButton::Right), Action::Prev),
        (MouseInput::Wheel(WheelDirection::Down), Action::Next),
        (MouseInput::Wheel(WheelDirection::Up), Action::Prev),
        (MouseInput::DoubleClick(MouseButton::Left), Action::SavePage),
    ];
    for (input, expected) in cases {
        assert_mouse(&bindings, input, Some(expected));
    }

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
    let cases = [
        (MouseInput::Button(MouseButton::Middle), Some(Action::Next)),
        (MouseInput::Wheel(WheelDirection::Up), Some(Action::Next)),
        (MouseInput::Button(MouseButton::Left), None),
        (MouseInput::Button(MouseButton::Right), None),
        (
            MouseInput::DoubleClick(MouseButton::Left),
            Some(Action::SavePage),
        ),
        (
            MouseInput::DoubleClick(MouseButton::Middle),
            Some(Action::SavePage),
        ),
    ];
    for (input, expected) in cases {
        assert_mouse(&bindings, input, expected);
    }

    assert_eq!(bindings.wheel_threshold(), 25.5);
    assert_eq!(bindings.double_click_interval(), Duration::from_millis(250));
}

#[test]
fn invalid_mouse_options_use_defaults() {
    let bindings = MouseBindings::parse("wheel_threshold = 0\ndouble_click_ms = 0\n");
    assert_eq!(bindings.wheel_threshold(), 50.0);
    assert_eq!(bindings.double_click_interval(), Duration::from_millis(400));
}
