//! Platform-independent menu commands, labels, and check states.

use crate::config::{DisplayMode, ScaleFilter};
use crate::input::bindings::Action;

/// Text shared by the Windows and macOS menu bars.
///
/// The application name is not here because only Windows uses it as a submenu
/// title.
pub(crate) mod labels {
    pub(crate) const FILE: &str = "ファイル";
    pub(crate) const NAVIGATION: &str = "移動";
    pub(crate) const VIEW: &str = "表示";
    pub(crate) const FILTERS: &str = "縮小フィルタ";
    pub(crate) const ALLOW_UPSCALE: &str = "拡大を許可";
    pub(crate) const SAVE_PAGE: &str = "表示中のページを保存";
    pub(crate) const CLEAR_CACHE: &str = "キャッシュを削除";
    pub(crate) const FULLSCREEN: &str = "全画面表示";
    pub(crate) const QUIT: &str = "終了";
    pub(crate) const NEXT: &str = "次のページ";
    pub(crate) const PREV: &str = "前のページ";
    pub(crate) const NEXT_SINGLE: &str = "1ページ進む";
    pub(crate) const PREV_SINGLE: &str = "1ページ戻る";
    pub(crate) const FIRST: &str = "先頭へ";
    pub(crate) const LAST: &str = "末尾へ";
    pub(crate) const NEXT_FILE: &str = "次のファイル";
    pub(crate) const PREV_FILE: &str = "前のファイル";
}

/// Navigation items in menu order.
pub(crate) const NAVIGATION_ITEMS: [(&str, Action); 8] = [
    (labels::NEXT, Action::Next),
    (labels::PREV, Action::Prev),
    (labels::NEXT_SINGLE, Action::NextSingle),
    (labels::PREV_SINGLE, Action::PrevSingle),
    (labels::FIRST, Action::First),
    (labels::LAST, Action::Last),
    (labels::NEXT_FILE, Action::NextFile),
    (labels::PREV_FILE, Action::PrevFile),
];

/// A command selected from the native menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuCommand {
    Action(Action),
    SetScaleFilter(ScaleFilter),
    ClearDiskCache,
    ExitApplication,
    ToggleAllowUpscale,
}

impl MenuCommand {
    /// Stable identifier shared by the Windows menu IDs and macOS item tags.
    pub(crate) fn id(self) -> u16 {
        match self {
            MenuCommand::Action(action) => {
                1 + Action::ALL
                    .iter()
                    .position(|candidate| *candidate == action)
                    .unwrap_or(0) as u16
            }
            MenuCommand::SetScaleFilter(filter) => {
                20 + ScaleFilter::ALL
                    .iter()
                    .position(|candidate| *candidate == filter)
                    .unwrap_or(0) as u16
            }
            MenuCommand::ClearDiskCache => 40,
            MenuCommand::ExitApplication => 41,
            MenuCommand::ToggleAllowUpscale => 42,
        }
    }

    pub(crate) fn from_id(id: u16) -> Option<Self> {
        let command = match id {
            1..=15 => MenuCommand::Action(Action::ALL[(id - 1) as usize]),
            20..=26 => MenuCommand::SetScaleFilter(ScaleFilter::ALL[(id - 20) as usize]),
            40 => MenuCommand::ClearDiskCache,
            41 => MenuCommand::ExitApplication,
            42 => MenuCommand::ToggleAllowUpscale,
            _ => return None,
        };
        Some(command)
    }
}

/// The action that switches to `mode`.
pub(crate) fn action_for_mode(mode: DisplayMode) -> Action {
    match mode {
        DisplayMode::Single => Action::SinglePage,
        DisplayMode::Spread => Action::SpreadPage,
        DisplayMode::Mixed => Action::MixedPage,
    }
}

/// Menu item check states kept in sync with the application.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MenuState {
    pub mode: DisplayMode,
    pub filter: ScaleFilter,
    pub fullscreen: bool,
    pub allow_upscale: bool,
}

#[cfg(test)]
mod tests {
    use super::{MenuCommand, MenuState, NAVIGATION_ITEMS, action_for_mode, labels};
    use crate::config::{DisplayMode, ScaleFilter};
    use crate::input::bindings::Action;

    #[test]
    fn ids_round_trip() {
        let commands = [
            MenuCommand::Action(Action::Next),
            MenuCommand::Action(Action::SavePage),
            MenuCommand::Action(Action::ToggleFullscreen),
            MenuCommand::Action(Action::NextFile),
            MenuCommand::Action(Action::PrevFile),
            MenuCommand::SetScaleFilter(ScaleFilter::Nearest),
            MenuCommand::SetScaleFilter(ScaleFilter::Lanczos3),
            MenuCommand::ClearDiskCache,
            MenuCommand::ExitApplication,
            MenuCommand::ToggleAllowUpscale,
        ];
        for command in commands {
            assert_eq!(MenuCommand::from_id(command.id()), Some(command));
        }
    }

    #[test]
    fn ids_match_the_os_menu_ids() {
        assert_eq!(MenuCommand::Action(Action::Next).id(), 1);
        assert_eq!(MenuCommand::Action(Action::Quit).id(), 13);
        assert_eq!(MenuCommand::Action(Action::NextFile).id(), 14);
        assert_eq!(MenuCommand::Action(Action::PrevFile).id(), 15);
        assert_eq!(MenuCommand::SetScaleFilter(ScaleFilter::Nearest).id(), 20);
        assert_eq!(MenuCommand::SetScaleFilter(ScaleFilter::Lanczos3).id(), 26);
        assert_eq!(MenuCommand::ClearDiskCache.id(), 40);
        assert_eq!(MenuCommand::ExitApplication.id(), 41);
        assert_eq!(MenuCommand::ToggleAllowUpscale.id(), 42);
        assert_eq!(MenuCommand::from_id(16), None);
        assert_eq!(MenuCommand::from_id(19), None);
        assert_eq!(MenuCommand::from_id(43), None);
    }

    #[test]
    fn mode_actions_match_display_modes() {
        assert_eq!(action_for_mode(DisplayMode::Single), Action::SinglePage);
        assert_eq!(action_for_mode(DisplayMode::Spread), Action::SpreadPage);
        assert_eq!(action_for_mode(DisplayMode::Mixed), Action::MixedPage);
    }

    #[test]
    fn menu_state_keeps_the_selected_values() {
        let state = MenuState {
            mode: DisplayMode::Mixed,
            filter: ScaleFilter::Gaussian,
            fullscreen: true,
            allow_upscale: false,
        };
        assert_eq!(state.mode, DisplayMode::Mixed);
        assert_eq!(state.filter, ScaleFilter::Gaussian);
        assert!(state.fullscreen);
        assert!(!state.allow_upscale);
    }

    #[test]
    fn all_labels_are_non_empty() {
        for label in [
            labels::FILE,
            labels::NAVIGATION,
            labels::VIEW,
            labels::FILTERS,
            labels::ALLOW_UPSCALE,
            labels::SAVE_PAGE,
            labels::CLEAR_CACHE,
            labels::FULLSCREEN,
            labels::QUIT,
            labels::NEXT,
            labels::PREV,
            labels::NEXT_SINGLE,
            labels::PREV_SINGLE,
            labels::FIRST,
            labels::LAST,
            labels::NEXT_FILE,
            labels::PREV_FILE,
        ] {
            assert!(!label.is_empty());
        }
    }

    #[test]
    fn navigation_items_are_in_menu_order() {
        assert_eq!(
            NAVIGATION_ITEMS,
            [
                (labels::NEXT, Action::Next),
                (labels::PREV, Action::Prev),
                (labels::NEXT_SINGLE, Action::NextSingle),
                (labels::PREV_SINGLE, Action::PrevSingle),
                (labels::FIRST, Action::First),
                (labels::LAST, Action::Last),
                (labels::NEXT_FILE, Action::NextFile),
                (labels::PREV_FILE, Action::PrevFile),
            ]
        );
    }
}
