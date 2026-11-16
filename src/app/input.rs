use std::time::Instant;

use winit::event::{MouseButton, MouseScrollDelta};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::Key;

use crate::input::bindings::{Action, MouseInput, WheelDirection};
use crate::input::navigation;

use super::App;

impl App {
    fn navigate(&mut self, delta: isize) {
        let Some(viewer) = &self.viewer else {
            return;
        };
        let Some(page) = navigation::step(
            viewer.current,
            viewer.pages.len(),
            self.config.spread,
            delta,
        ) else {
            return;
        };
        self.go_to_unaligned(page);
    }

    fn navigate_single(&mut self, delta: isize) {
        let Some(viewer) = &self.viewer else {
            return;
        };
        let Some(page) = navigation::step_single(viewer.current, viewer.pages.len(), delta) else {
            return;
        };
        self.go_to_unaligned(page);
    }

    fn go_to(&mut self, page: usize) {
        self.go_to_inner(page, true);
    }

    fn go_to_unaligned(&mut self, page: usize) {
        self.go_to_inner(page, false);
    }

    fn go_to_inner(&mut self, page: usize, align: bool) {
        let spread = self.config.spread;
        self.scroll = 0.0;
        let Some(viewer) = self.viewer.as_mut() else {
            return;
        };
        let moved = if align {
            viewer.go_to(page, spread)
        } else {
            viewer.go_to_unaligned(page)
        };
        if !moved {
            return;
        }
        self.error = None;
        self.status = None;
        self.refresh();
    }

    pub(super) fn handle_click(&mut self, button: MouseButton, event_loop: &ActiveEventLoop) {
        let actions = self.clicks.press(button, &self.mouse, Instant::now());
        for action in actions {
            self.perform(action, event_loop);
        }
        self.update_click_timer(event_loop);
    }

    pub(super) fn flush_pending_click(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(action) = self.clicks.flush() {
            self.perform(action, event_loop);
        }
    }

    pub(super) fn update_click_timer(&self, event_loop: &ActiveEventLoop) {
        let control_flow = match self.clicks.deadline() {
            Some(deadline) => ControlFlow::WaitUntil(deadline),
            None => ControlFlow::Wait,
        };
        event_loop.set_control_flow(control_flow);
    }

    pub(super) fn handle_scroll(&mut self, delta: MouseScrollDelta, event_loop: &ActiveEventLoop) {
        self.flush_pending_click(event_loop);
        self.update_click_timer(event_loop);
        if self.viewer.is_none() {
            return;
        }
        self.scroll += match delta {
            MouseScrollDelta::LineDelta(_, y) => y * 50.0,
            MouseScrollDelta::PixelDelta(position) => position.y as f32,
        };
        let threshold = self.mouse.wheel_threshold();
        let direction = if self.scroll <= -threshold {
            Some(WheelDirection::Down)
        } else if self.scroll >= threshold {
            Some(WheelDirection::Up)
        } else {
            None
        };
        let Some(direction) = direction else {
            return;
        };
        self.scroll = 0.0;
        if let Some(action) = self.mouse.action_for(MouseInput::Wheel(direction)) {
            self.perform(action, event_loop);
        }
    }

    pub(super) fn handle_key(&mut self, key: Key, event_loop: &ActiveEventLoop) {
        if let Some(action) = self.keys.action_for(&key, self.modifiers) {
            self.perform(action, event_loop);
        }
    }

    pub(super) fn perform(&mut self, action: Action, event_loop: &ActiveEventLoop) {
        match action {
            Action::Next => self.navigate(1),
            Action::Prev => self.navigate(-1),
            Action::NextSingle => self.navigate_single(1),
            Action::PrevSingle => self.navigate_single(-1),
            Action::First => self.go_to(0),
            Action::Last => self.go_to(usize::MAX),
            Action::ToggleSpread => self.set_spread(!self.config.spread),
            Action::SinglePage => self.set_spread(false),
            Action::SpreadPage => self.set_spread(true),
            Action::ToggleFullscreen => {
                let fullscreen = self.is_fullscreen();
                self.set_fullscreen(!fullscreen);
            }
            Action::SavePage => self.save_current_page(),
            Action::Quit => {
                if self.is_fullscreen() {
                    self.set_fullscreen(false);
                } else {
                    event_loop.exit();
                }
            }
        }
    }

    pub(super) fn is_fullscreen(&self) -> bool {
        self.window
            .as_ref()
            .and_then(|window| window.fullscreen())
            .is_some()
    }
}
