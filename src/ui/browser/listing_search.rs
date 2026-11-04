// SPDX-License-Identifier: MIT

//! 10xer recursive **s** search. It borrows the focused listing's filter field
//! with its scope forced to include subfolders, so the filter's name rules, the
//! 100-hit cap, and stale-work cancellation stay the search's rules. The saved
//! **Include subfolders** preference is never written, and dismissal puts back
//! the **f** query the search replaced.

use std::cell::{Cell, RefCell};

use gtk::glib;

use super::BrowserView;

#[derive(Default)]
pub(super) struct SearchState {
    borrowed: RefCell<Option<(glib::WeakRef<gtk::Entry>, String)>>,
    /// A hit's Miller column is opening beside the hits, not replacing them.
    opening_hit_column: Cell<bool>,
}

impl SearchState {
}

impl BrowserView {











}

impl super::ViewState {



}
