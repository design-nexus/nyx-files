// SPDX-License-Identifier: MIT

//! A directory pane's page stack doubles as its keyboard focus target while it shows a
//! status (empty or unreadable) or loading page, because the hidden collection view
//! would accept focus that nothing can see or move.

use std::{
    cell::Cell,
    rc::{Rc, Weak},
};

use gtk::{glib, prelude::*};

use super::{CONTENT_PAGE};
use crate::app::Browser;





/// Focuses the surface while it stands in for `view`, otherwise `view` itself.
pub(crate) fn focus_surface_or(stack: &gtk::Stack, view: &impl IsA<gtk::Widget>) -> bool {
    if surface_takes_focus(stack) {
        stack.grab_focus()
    } else {
        view.as_ref().grab_focus()
    }
}

/// Whether focus requests for this pane go to the surface rather than its view. On the
/// grace page the surface keeps whatever role the previous page gave it.
pub(crate) fn surface_takes_focus(stack: &gtk::Stack) -> bool {
    stack.is_focusable() && stack.visible_child_name().as_deref() != Some(CONTENT_PAGE)
}
