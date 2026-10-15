//! Window drag regions: `appRegion: "drag" | "no-drag"`, as Electron's `-webkit-app-region`.
//!
//! A frameless window (`titlebarTransparent`) has no titlebar of its own, so the page marks the
//! boxes that move it. Each host element with an effective value registers its box with GPUI as
//! a window control area (`drag`) or a hole in one (`no-drag`); GPUI answers
//! `Window::window_control_area_at` by rectangles in paint order, which is Electron's model and
//! not hit testing.
//!
//! The press is native because AppKit's `performWindowDragWithEvent:` needs the mouse-down that
//! is being dispatched, and Windows moves the window only from `WM_NCHITTEST` answering
//! `HTCAPTION`, before any page code could run. The page still does not hear of it there, but
//! gpuix only prevents the default and leaves the event unstopped for `DefWindowProc`. So the capture-phase press listener asks GPUI
//! first (`press_outcome`), and a press in a drag region never reaches the page. With no button
//! held, the pointer over one is outside the window as far as the page can tell
//! (`hides_pointer`), which is what Electron does on macOS.

use std::sync::{Arc, Mutex};

use gpui::{App, InteractiveElement, MouseButton, Pixels, Point, WindowControlArea};

use super::dispatcher::{PointerAction, PointerSample};
use super::state::PointerState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppRegion {
    Drag,
    NoDrag,
}

impl AppRegion {
    /// A value CSS would not accept is no declaration: the inherited one stays.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "drag" => Some(Self::Drag),
            "no-drag" => Some(Self::NoDrag),
            _ => None,
        }
    }
}

/// Registers `el`'s box with the window as `region` says. The element needs its host hitbox
/// already: GPUI records a control area against it.
pub fn mark<E: InteractiveElement>(el: E, region: Option<AppRegion>) -> E {
    match region {
        Some(AppRegion::Drag) => el.window_control_area(WindowControlArea::Drag),
        Some(AppRegion::NoDrag) => el.window_client_area(),
        None => el,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Linux,
    Windows,
}

impl Platform {
    pub const CURRENT: Platform = if cfg!(target_os = "macos") {
        Platform::MacOs
    } else if cfg!(target_os = "windows") {
        Platform::Windows
    } else {
        Platform::Linux
    };
}

/// What a press in a drag region asks of the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowAction {
    /// The platform moves the window. On macOS the call hands the drag to the WindowServer and
    /// returns at once, and the mouse-up may never reach the page.
    Move,
    /// What the titlebar does on a double click (macOS: zoom, minimise or nothing, as the
    /// user's setting says).
    TitlebarDoubleClick,
    Zoom,
    ShowMenu,
}

impl WindowAction {
    pub fn name(self) -> &'static str {
        match self {
            WindowAction::Move => "move",
            WindowAction::TitlebarDoubleClick => "titlebarDoubleClick",
            WindowAction::Zoom => "zoom",
            WindowAction::ShowMenu => "showMenu",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Press {
    pub button: MouseButton,
    pub ctrl: bool,
    pub click_count: usize,
    /// A button that reached the page is still down.
    pub other_button_down: bool,
}

/// What becomes of a press in a drag region that the page does not get.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PressOutcome {
    /// gpuix asks the window to do this, and nothing below it hears of the press.
    Perform(WindowAction),
    /// The OS acts on its own after GPUI has seen the press, so gpuix must neither stop it
    /// nor ask for an action (it only prevents the default): on Windows GPUI gets the caption's `WM_NC*BUTTONDOWN` as a mouse
    /// down, and a handled one makes `DefWindowProc` skip the drag, the double-click maximise
    /// and the system menu.
    OsOwned,
}

/// What a press in a drag region does, or `None` when it goes to the page.
///
/// Only the primary button moves the window, and on macOS not with ctrl, which is a context
/// click. On Windows the caption takes every button before the page could, so a press there
/// is always the window's, whatever else is held.
pub fn press_outcome(platform: Platform, press: &Press) -> Option<PressOutcome> {
    if platform == Platform::Windows {
        return Some(PressOutcome::OsOwned);
    }
    if press.other_button_down {
        return None;
    }
    let action = match press.button {
        MouseButton::Left if platform == Platform::MacOs && press.ctrl => return None,
        MouseButton::Left if press.click_count == 2 => match platform {
            Platform::MacOs => WindowAction::TitlebarDoubleClick,
            _ => WindowAction::Zoom,
        },
        MouseButton::Left => WindowAction::Move,
        MouseButton::Right if platform == Platform::Linux => WindowAction::ShowMenu,
        _ => return None,
    };
    Some(PressOutcome::Perform(action))
}

/// What the window needs to do for a swallowed press. GPUI's `Window` is the real one; the test
/// renderer's offscreen window has no current mouse event to hand to AppKit's drag, so it
/// records instead.
pub trait WindowActions {
    fn perform(&mut self, action: WindowAction, position: Point<Pixels>, cx: &mut App);
}

impl WindowActions for gpui::Window {
    fn perform(&mut self, action: WindowAction, position: Point<Pixels>, cx: &mut App) {
        match action {
            WindowAction::Move => self.start_window_move(cx),
            WindowAction::TitlebarDoubleClick => self.titlebar_double_click(),
            WindowAction::Zoom => self.zoom_window(),
            WindowAction::ShowMenu => self.show_window_menu(position),
        }
    }
}

/// The window actions of the test renderer, in the order they were asked for.
#[derive(Clone, Default)]
pub struct SimulatedWindowActions(Arc<Mutex<Vec<WindowAction>>>);

impl SimulatedWindowActions {
    /// What was asked since the last call.
    pub fn take(&self) -> Vec<WindowAction> {
        std::mem::take(&mut self.0.lock().unwrap())
    }
}

impl WindowActions for SimulatedWindowActions {
    fn perform(&mut self, action: WindowAction, _position: Point<Pixels>, _cx: &mut App) {
        self.0.lock().unwrap().push(action);
    }
}

/// Whether the page sees `sample` as the pointer being outside the window: it is over a drag
/// region and no button is held, or the window owns the pointer because it took a press. The
/// leave events fire, no moves do, and when the pointer comes out the page gets its enter
/// events. A button that went down outside the region keeps the pointer, so a drag started
/// elsewhere carries on through the region. A press, release or wheel is never hidden.
pub(super) fn hides_pointer(state: &PointerState, sample: &PointerSample) -> bool {
    if state.file_drag.is_some() {
        return false;
    }
    let owned = state.window_presses != 0;
    let over_region = sample.over_drag_region;
    match sample.action {
        PointerAction::Move {
            pressed_button: None,
        } => over_region,
        // GPUI reports one pressed button, so only the dispatcher knows what reached the page.
        PointerAction::Move { .. } | PointerAction::Resync => {
            owned || (over_region && state.buttons == 0)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(button: MouseButton, click_count: usize) -> Press {
        Press {
            button,
            ctrl: false,
            click_count,
            other_button_down: false,
        }
    }

    #[test]
    fn only_drag_and_no_drag_are_values() {
        assert_eq!(AppRegion::parse("drag"), Some(AppRegion::Drag));
        assert_eq!(AppRegion::parse("no-drag"), Some(AppRegion::NoDrag));
        for invalid in ["", "none", "Drag", "inherit"] {
            assert_eq!(AppRegion::parse(invalid), None);
        }
    }

    fn perform(platform: Platform, press: &Press) -> Option<WindowAction> {
        match press_outcome(platform, press) {
            Some(PressOutcome::Perform(action)) => Some(action),
            _ => None,
        }
    }

    #[test]
    fn a_primary_press_moves_the_window_and_a_second_one_acts_as_the_titlebar() {
        for platform in [Platform::MacOs, Platform::Linux] {
            assert_eq!(
                perform(platform, &press(MouseButton::Left, 1)),
                Some(WindowAction::Move)
            );
        }
        assert_eq!(
            perform(Platform::MacOs, &press(MouseButton::Left, 2)),
            Some(WindowAction::TitlebarDoubleClick)
        );
        assert_eq!(
            perform(Platform::Linux, &press(MouseButton::Left, 2)),
            Some(WindowAction::Zoom)
        );
    }

    #[test]
    fn on_windows_the_os_owns_every_press_and_gpuix_asks_for_no_action() {
        for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
            for click_count in [1, 2] {
                assert_eq!(
                    press_outcome(Platform::Windows, &press(button, click_count)),
                    Some(PressOutcome::OsOwned)
                );
            }
        }
        let chord = Press {
            other_button_down: true,
            ..press(MouseButton::Left, 1)
        };
        assert_eq!(
            press_outcome(Platform::Windows, &chord),
            Some(PressOutcome::OsOwned)
        );
    }

    #[test]
    fn ctrl_click_is_a_context_click_on_macos_only() {
        let ctrl = |platform| {
            press_outcome(
                platform,
                &Press {
                    ctrl: true,
                    ..press(MouseButton::Left, 1)
                },
            )
        };
        assert_eq!(ctrl(Platform::MacOs), None);
        assert_eq!(
            ctrl(Platform::Linux),
            Some(PressOutcome::Perform(WindowAction::Move))
        );
    }

    #[test]
    fn a_right_press_opens_the_window_menu_on_linux_and_reaches_the_page_on_macos() {
        assert_eq!(
            perform(Platform::Linux, &press(MouseButton::Right, 1)),
            Some(WindowAction::ShowMenu)
        );
        assert_eq!(press_outcome(Platform::MacOs, &press(MouseButton::Right, 1)), None);
        assert_eq!(press_outcome(Platform::Linux, &press(MouseButton::Middle, 1)), None);
    }

    #[test]
    fn a_press_while_another_button_is_down_reaches_the_page() {
        let chord = Press {
            other_button_down: true,
            ..press(MouseButton::Left, 1)
        };
        for platform in [Platform::MacOs, Platform::Linux] {
            assert_eq!(press_outcome(platform, &chord), None);
        }
    }

    fn sample(action: PointerAction, over_drag_region: bool) -> PointerSample {
        PointerSample {
            action,
            position: (0.0, 0.0),
            movement: (0.0, 0.0),
            modifiers: Default::default(),
            hits: Vec::new(),
            over_drag_region,
        }
    }

    fn moving(pressed_button: Option<u32>) -> PointerAction {
        PointerAction::Move { pressed_button }
    }

    #[test]
    fn a_pointer_over_a_drag_region_with_no_button_held_is_hidden() {
        let state = PointerState::default();
        assert!(hides_pointer(&state, &sample(moving(None), true)));
        assert!(hides_pointer(&state, &sample(PointerAction::Resync, true)));
        assert!(!hides_pointer(&state, &sample(moving(None), false)));
    }

    #[test]
    fn a_held_button_keeps_the_pointer_through_the_region() {
        let mut state = PointerState::default();
        state.buttons = 1;
        assert!(!hides_pointer(&state, &sample(moving(Some(0)), true)));
        assert!(!hides_pointer(&state, &sample(PointerAction::Resync, true)));
        // The platform says nothing is held: the release was lost, so it is hidden again.
        assert!(hides_pointer(&state, &sample(moving(None), true)));
    }

    #[test]
    fn a_press_the_window_took_owns_the_pointer_until_it_is_released() {
        let mut state = PointerState::default();
        state.window_presses = 1;
        assert!(hides_pointer(&state, &sample(moving(Some(0)), false)));
        assert!(hides_pointer(&state, &sample(PointerAction::Resync, false)));
        // A move that reports no button held means the release was lost, and it ends it.
        assert!(!hides_pointer(&state, &sample(moving(None), false)));
    }

    #[test]
    fn presses_releases_and_wheel_ticks_are_never_hidden() {
        let state = PointerState::default();
        for action in [
            PointerAction::Down {
                button: 2,
                click_count: 1,
            },
            PointerAction::Up {
                button: 2,
                click_count: 1,
            },
            PointerAction::Exit,
        ] {
            assert!(!hides_pointer(&state, &sample(action, true)));
        }
    }
}
