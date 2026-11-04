// SPDX-License-Identifier: MIT

//! An edit is a lease on a bound entry, not on a row position. Presentations hand out typed
//! widgets and optional reveal/cleanup hooks. Rebinding to another identity, unbinding, dropping
//! the owner or cancelling revokes the lease. Submission is consumer policy; filesystem work
//! and its pending/error reconciliation remain in the browser's operation coordinator.

use crate::{
    model::{FileEntry, Location},
    services::validate_basename,
};
use gtk::{glib, prelude::*};
use std::{cell::RefCell, rc::Rc};

type Cancel = Rc<dyn Fn()>;

thread_local! {
    static ITEM_EDITS: RefCell<std::collections::HashMap<usize, (glib::WeakRef<glib::Object>, EditWidgets)>> =
        RefCell::new(std::collections::HashMap::new());
}



pub(super) type ActiveEdits = Rc<RefCell<Option<ActiveEdit>>>;
pub(super) type Reveal = Rc<dyn Fn(&gtk::Entry)>;

#[derive(Clone)]
pub(super) struct EditWidgets {
    pub(super) field: gtk::Entry,
    pub(super) display: gtk::Widget,
    identity: Rc<RefCell<Option<Location>>>,
    cancel: Rc<RefCell<Option<Cancel>>>,
}

impl EditWidgets {



}

pub(super) struct EditTarget {
    pub(super) widgets: EditWidgets,
    pub(super) reveal: Option<Reveal>,
    pub(super) finish: Option<Rc<dyn Fn()>>,
    pub(super) cancelled: Option<Rc<dyn Fn()>>,
}

impl From<EditWidgets> for EditTarget {
    fn from(widgets: EditWidgets) -> Self {
        Self {
            widgets,
            reveal: None,
            finish: None,
            cancelled: None,
        }
    }
}

pub(super) struct ActiveEdit {
    pub(super) entry: FileEntry,
    pub(super) field: gtk::Entry,
    target: EditTarget,
    handlers: Vec<glib::SignalHandlerId>,
    focus: gtk::EventControllerFocus,
    focus_handler: Option<glib::SignalHandlerId>,
    tick: Option<gtk::TickCallbackId>,
    submitted: bool,
}

impl Drop for ActiveEdit {
    fn drop(&mut self) {
        if !self.submitted
            && let Some(on_cancel) = &self.target.cancelled
        {
            on_cancel();
        }
        self.target.widgets.cancel.take();
        for handler in self.handlers.drain(..) {
            self.field.disconnect(handler);
        }
        if let Some(handler) = self.focus_handler.take() {
            self.focus.disconnect(handler);
        }
        // GTK walks the controller list during Tab focus crossing. Retire the handler now,
        // but detach its controller only after that walk has unwound.
        let field = self.field.downgrade();
        let focus = self.focus.clone();
        glib::idle_add_local_once(move || {
            if let Some(field) = field.upgrade() {
                field.remove_controller(&focus);
            }
        });
        if let Some(tick) = self.tick.take() {
            tick.remove();
        }
        self.field.set_visible(false);
        self.field.set_sensitive(true);
        self.field.remove_css_class("error");
        crate::ui::accessibility::set_description(&self.field, None);
        self.target.widgets.display.set_visible(true);
        if let Some(finish) = &self.target.finish {
            finish();
        }
    }
}






// Empty fields are an ordinary editing state, although they cannot be submitted.
pub(super) fn basename_field_error(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        None
    } else {
        validate_basename(name).err()
    }
}

