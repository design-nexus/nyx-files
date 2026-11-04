// SPDX-License-Identifier: MIT

use std::cell::RefCell;

use gtk::{glib, prelude::*};





#[derive(Default)]
pub(super) struct SidebarDropFeedback {
    active: RefCell<Option<ActiveSite>>,
    sidebar: glib::WeakRef<gtk::Box>,
}

struct ActiveSite {
    owner: glib::WeakRef<gtk::Widget>,
    indicator: Option<(glib::WeakRef<gtk::Widget>, &'static str)>,
    cancel: Option<Box<dyn FnOnce()>>,
}

impl ActiveSite {
}

impl SidebarDropFeedback {











}
