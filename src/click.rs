use std::time::Instant;

use winit::dpi::PhysicalPosition;
use winit::event::MouseButton;

use crate::bindings::{Action, MouseBindings};

const DOUBLE_CLICK_DISTANCE: f64 = 10.0;

struct PendingClick {
    button: MouseButton,
    action: Option<Action>,
    position: PhysicalPosition<f64>,
    deadline: Instant,
}

pub(crate) struct ClickTracker {
    cursor: PhysicalPosition<f64>,
    pending: Option<PendingClick>,
}

impl Default for ClickTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ClickTracker {
    pub(crate) fn new() -> Self {
        Self {
            cursor: PhysicalPosition::new(0.0, 0.0),
            pending: None,
        }
    }

    pub(crate) fn cursor_moved(&mut self, position: PhysicalPosition<f64>) {
        self.cursor = position;
    }

    /// Returns the actions to perform for this press. A click on a button with
    /// a double-click binding is deferred until `flush` or the deadline.
    pub(crate) fn press(
        &mut self,
        button: MouseButton,
        mouse: &MouseBindings,
        now: Instant,
    ) -> Vec<Action> {
        let mut actions = Vec::new();
        let double = mouse.double_action(button);
        if double.is_none() {
            if let Some(action) = self.flush() {
                actions.push(action);
            }
            if let Some(action) = mouse.single_action(button) {
                actions.push(action);
            }
            return actions;
        }

        let interval = mouse.double_click_interval();
        if let Some(pending) = self.pending.take() {
            if is_double_click(&pending, button, self.cursor, now) {
                if let Some(action) = double {
                    actions.push(action);
                }
                return actions;
            }
            if let Some(action) = pending.action {
                actions.push(action);
            }
        }

        self.pending = Some(PendingClick {
            button,
            action: mouse.single_action(button),
            position: self.cursor,
            deadline: now + interval,
        });
        actions
    }

    pub(crate) fn flush(&mut self) -> Option<Action> {
        self.pending.take().and_then(|pending| pending.action)
    }

    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.pending.as_ref().map(|pending| pending.deadline)
    }
}

fn is_double_click(
    pending: &PendingClick,
    button: MouseButton,
    position: PhysicalPosition<f64>,
    now: Instant,
) -> bool {
    pending.button == button
        && now <= pending.deadline
        && click_distance(pending.position, position) <= DOUBLE_CLICK_DISTANCE
}

fn click_distance(a: PhysicalPosition<f64>, b: PhysicalPosition<f64>) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn mouse_without_double_click() -> MouseBindings {
        MouseBindings::parse(
            r#"
[bindings]
next = ["left"]
prev = ["right"]
save_page = []
"#,
        )
    }

    #[test]
    fn defers_single_click_when_double_click_is_bound() {
        let mut clicks = ClickTracker::new();
        let mouse = MouseBindings::default();
        let now = Instant::now();

        assert!(clicks.press(MouseButton::Left, &mouse, now).is_empty());
        assert_eq!(clicks.deadline(), Some(now + Duration::from_millis(400)));
        assert_eq!(clicks.flush(), Some(Action::Next));
        assert_eq!(clicks.deadline(), None);
    }

    #[test]
    fn double_click_runs_only_the_double_action() {
        let mut clicks = ClickTracker::new();
        let mouse = MouseBindings::default();
        let now = Instant::now();
        clicks.cursor_moved(PhysicalPosition::new(10.0, 10.0));

        assert!(clicks.press(MouseButton::Left, &mouse, now).is_empty());
        let actions = clicks.press(MouseButton::Left, &mouse, now + Duration::from_millis(200));
        assert_eq!(actions, vec![Action::SavePage]);
        assert_eq!(clicks.deadline(), None);
    }

    #[test]
    fn click_without_double_click_binding_is_immediate() {
        let mut clicks = ClickTracker::new();
        let mouse = mouse_without_double_click();

        assert_eq!(
            clicks.press(MouseButton::Left, &mouse, Instant::now()),
            vec![Action::Next]
        );
        assert_eq!(clicks.deadline(), None);
    }

    #[test]
    fn click_after_deadline_flushes_then_starts_a_new_click() {
        let mut clicks = ClickTracker::new();
        let mouse = MouseBindings::default();
        let now = Instant::now();

        assert!(clicks.press(MouseButton::Left, &mouse, now).is_empty());
        let actions = clicks.press(MouseButton::Left, &mouse, now + Duration::from_millis(401));
        assert_eq!(actions, vec![Action::Next]);
        assert!(clicks.deadline().is_some());
    }

    #[test]
    fn distant_click_is_not_a_double_click() {
        let mut clicks = ClickTracker::new();
        let mouse = MouseBindings::default();
        let now = Instant::now();

        clicks.cursor_moved(PhysicalPosition::new(10.0, 10.0));
        assert!(clicks.press(MouseButton::Left, &mouse, now).is_empty());
        clicks.cursor_moved(PhysicalPosition::new(30.0, 10.0));
        let actions = clicks.press(MouseButton::Left, &mouse, now + Duration::from_millis(200));
        assert_eq!(actions, vec![Action::Next]);
        assert_eq!(clicks.flush(), Some(Action::Next));
    }

    #[test]
    fn another_button_flushes_the_pending_click() {
        let mut clicks = ClickTracker::new();
        let mouse = MouseBindings::default();
        let now = Instant::now();

        assert!(clicks.press(MouseButton::Left, &mouse, now).is_empty());
        assert_eq!(
            clicks.press(MouseButton::Right, &mouse, now + Duration::from_millis(100)),
            vec![Action::Next, Action::Prev]
        );
    }
}
