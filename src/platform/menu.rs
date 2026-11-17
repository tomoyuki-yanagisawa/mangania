//! Platform-independent menu commands and check states.

use crate::config::{DisplayMode, ScaleFilter};
use crate::input::bindings::Action;

/// A command selected from the native menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuCommand {
    Action(Action),
    SetScaleFilter(ScaleFilter),
    ClearDiskCache,
    ExitApplication,
}

impl MenuCommand {
    /// Stable identifier shared by the Windows menu IDs and macOS item tags.
    pub(crate) fn id(self) -> u16 {
        match self {
            MenuCommand::Action(action) => match action {
                Action::Next => 1,
                Action::Prev => 2,
                Action::NextSingle => 3,
                Action::PrevSingle => 4,
                Action::First => 5,
                Action::Last => 6,
                Action::ToggleSpread => 7,
                Action::SinglePage => 8,
                Action::SpreadPage => 9,
                Action::MixedPage => 10,
                Action::ToggleFullscreen => 11,
                Action::SavePage => 12,
                Action::Quit => 13,
            },
            MenuCommand::SetScaleFilter(filter) => {
                20 + ScaleFilter::ALL
                    .iter()
                    .position(|candidate| *candidate == filter)
                    .unwrap_or(0) as u16
            }
            MenuCommand::ClearDiskCache => 40,
            MenuCommand::ExitApplication => 41,
        }
    }

    pub(crate) fn from_id(id: u16) -> Option<Self> {
        let command = match id {
            1 => MenuCommand::Action(Action::Next),
            2 => MenuCommand::Action(Action::Prev),
            3 => MenuCommand::Action(Action::NextSingle),
            4 => MenuCommand::Action(Action::PrevSingle),
            5 => MenuCommand::Action(Action::First),
            6 => MenuCommand::Action(Action::Last),
            7 => MenuCommand::Action(Action::ToggleSpread),
            8 => MenuCommand::Action(Action::SinglePage),
            9 => MenuCommand::Action(Action::SpreadPage),
            10 => MenuCommand::Action(Action::MixedPage),
            11 => MenuCommand::Action(Action::ToggleFullscreen),
            12 => MenuCommand::Action(Action::SavePage),
            13 => MenuCommand::Action(Action::Quit),
            20..=26 => MenuCommand::SetScaleFilter(ScaleFilter::ALL[(id - 20) as usize]),
            40 => MenuCommand::ClearDiskCache,
            41 => MenuCommand::ExitApplication,
            _ => return None,
        };
        Some(command)
    }
}

/// Menu item check states kept in sync with the application.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MenuState {
    pub mode: DisplayMode,
    pub filter: ScaleFilter,
    pub fullscreen: bool,
}

#[cfg(test)]
mod tests {
    use super::MenuCommand;
    use crate::config::ScaleFilter;
    use crate::input::bindings::Action;

    #[test]
    fn ids_round_trip() {
        let commands = [
            MenuCommand::Action(Action::Next),
            MenuCommand::Action(Action::SavePage),
            MenuCommand::Action(Action::ToggleFullscreen),
            MenuCommand::SetScaleFilter(ScaleFilter::Nearest),
            MenuCommand::SetScaleFilter(ScaleFilter::Lanczos3),
            MenuCommand::ClearDiskCache,
            MenuCommand::ExitApplication,
        ];
        for command in commands {
            assert_eq!(MenuCommand::from_id(command.id()), Some(command));
        }
    }
}
