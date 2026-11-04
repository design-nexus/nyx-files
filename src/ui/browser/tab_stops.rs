// SPDX-License-Identifier: MIT

//! Default key map Tab stops: each listing is one stop, and Tab into it lands on the
//! keyboard cursor. GTK's `ListTabBehavior::Item` makes List and Icons leave in one
//! press; the Columns strip spans several lists, so its Tab moves are routed here.

use std::rc::Rc;

use gtk::{glib, prelude::*};

use super::{BrowserView, ViewState};
use crate::ui::browser_modes::BrowserMode;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum TabCrossing {
    #[default]
    None,
    /// A Tab press from outside the item views is moving focus.
    Pending,
    /// That focus reached the listing; the cursor takes it once GTK finishes.
    Landing,
}

impl BrowserView {
}

impl ViewState {


}
