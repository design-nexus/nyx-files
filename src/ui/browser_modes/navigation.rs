// SPDX-License-Identifier: MIT

use std::collections::VecDeque;

use super::*;


#[derive(Clone, Copy)]
struct Viewport {
    mode: BrowserMode,
    vertical: f64,
    horizontal: f64,
    /// A grid of another width wraps into other rows, so its offsets do not carry over.
    page_width: f64,
}

impl Viewport {

}

#[derive(Clone)]
struct PanePosition {
    selected: Vec<Location>,
    focused: Option<Location>,
    anchor: Option<Location>,
    viewport: Viewport,
}

/// Where keyboard focus was when a reload started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FocusOwner {
    /// Elsewhere, or on filter results: the reload restores the viewport only.
    Outside,
    Items,
    FilterEntry,
}


#[derive(Clone)]
enum PendingRestore {
    History {
        position: PanePosition,
        /// An explicit navigation, or the listing being left held focus or nothing did.
        takes_focus: bool,
    },
    /// The model restores selection and cursor by identity across a reload; the view
    /// brings back the viewport and the focus.
    Reload {
        viewport: Viewport,
        focus: FocusOwner,
    },
}

impl PendingRestore {

}

#[derive(Default)]
pub(super) struct PaneNavigation {
    history: VecDeque<(Location, PanePosition)>,
    pending: Option<PendingRestore>,
    /// The restore being applied. While it settles, leaving or reloading saves its
    /// target rather than a half-restored viewport.
    applying: Option<PendingRestore>,
    restoring: Rc<Cell<bool>>,
    /// The pending restore was applied and its viewport is settling.
    applied: Rc<Cell<bool>>,
    input: Option<(glib::WeakRef<gtk::Widget>, gtk::EventControllerLegacy)>,
    leaving_with_focus: bool,
}

impl PaneNavigation {

    pub(super) fn cancel(&mut self) {
        self.restoring.set(false);
        self.pending = None;
        self.applying = None;
    }










}







