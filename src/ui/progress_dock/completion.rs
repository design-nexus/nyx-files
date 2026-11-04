// SPDX-License-Identifier: MIT

use gtk::{glib, prelude::*};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};

pub(super) struct Widgets {
    pub root: glib::WeakRef<gtk::Box>,
    pub overlay: glib::WeakRef<gtk::Overlay>,
    pub dock: glib::WeakRef<gtk::ScrolledWindow>,
    pub list: glib::WeakRef<gtk::Box>,
    pub meta: glib::WeakRef<gtk::Label>,
    pub status: glib::WeakRef<gtk::Label>,
    pub progress: glib::WeakRef<gtk::ProgressBar>,
}

pub(super) struct Completion {
    widgets: Widgets,
    completed: Cell<bool>,
    hovered: Cell<bool>,
    focused: Cell<bool>,
    pinned: Cell<bool>,
    duration: Cell<Duration>,
    remaining: Cell<Duration>,
    started: Cell<Option<Instant>>,
    exiting: Cell<Option<Instant>>,
    source: RefCell<Option<glib::SourceId>>,
    cancel: RefCell<Option<Rc<dyn Fn()>>>,
}

impl Completion {









    fn stop(&self) {
        if let Some(source) = self.source.borrow_mut().take() {
            source.remove();
        }
    }

    pub(super) fn dismiss(&self) {
        self.stop();
        self.cancel.take();
        if let Some(root) = self.widgets.root.upgrade()
            && let Some(list) = self.widgets.list.upgrade()
            && root.parent().as_ref() == Some(list.upcast_ref())
        {
            list.remove(&root);
        }
        if let Some(list) = self.widgets.list.upgrade()
            && list.first_child().is_none()
            && let Some(dock) = self.widgets.dock.upgrade()
            && dock.parent().is_some()
            && let Some(overlay) = self.widgets.overlay.upgrade()
        {
            overlay.remove_overlay(&dock);
        }
    }
}

impl Drop for Completion {
    fn drop(&mut self) {
        self.stop();
    }
}
