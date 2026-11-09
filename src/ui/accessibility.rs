// SPDX-License-Identifier: MIT

//! Accessible semantics for browser widgets whose nested content has no useful derived name.

use std::cell::RefCell;

use gtk::{glib, prelude::*};

use crate::model::{EntryKind, FileEntry};

use super::{
    browser_modes::BrowserMode,
    keys::{Keys, bindings},
    shortcut_reference::with_keys,
};

#[cfg(feature = "qa")]
pub(crate) mod qa;

pub(super) fn set_label(widget: &impl IsA<gtk::Accessible>, label: &str) {
    widget.update_property(&[gtk::accessible::Property::Label(label)]);
    #[cfg(feature = "qa")]
    qa::record(widget, |naming| naming.label = Some(label.to_owned()));
}

thread_local! {
    static OWN_LABELS: RefCell<Vec<glib::WeakRef<gtk::Widget>>> = RefCell::default();
    static STAND_INS: RefCell<Vec<glib::WeakRef<gtk::Widget>>> = RefCell::default();
}

fn holds(list: &RefCell<Vec<glib::WeakRef<gtk::Widget>>>, widget: &gtk::Widget) -> bool {
    let mut list = list.borrow_mut();
    list.retain(|kept| kept.upgrade().is_some());
    list.iter()
        .any(|kept| kept.upgrade().as_ref() == Some(widget))
}

pub(super) fn set_own_label(widget: &impl IsA<gtk::Widget>, label: &str) {
    let widget = widget.upcast_ref::<gtk::Widget>();
    OWN_LABELS.with(|own| {
        if !holds(own, widget) {
            own.borrow_mut().push(widget.downgrade());
        }
    });
    STAND_INS.with(|stand_ins| {
        stand_ins
            .borrow_mut()
            .retain(|kept| kept.upgrade().as_ref() != Some(widget));
    });
    set_label(widget, label);
}

pub(super) fn set_description(widget: &impl IsA<gtk::Accessible>, description: Option<&str>) {
    widget.update_property(&[gtk::accessible::Property::Description(
        description.unwrap_or(""),
    )]);
    #[cfg(feature = "qa")]
    qa::record(widget, |naming| {
        naming.description = description.map(str::to_owned);
    });
}

pub(super) fn set_busy(widget: &impl IsA<gtk::Accessible>, busy: bool) {
    widget.update_state(&[gtk::accessible::State::Busy(busy)]);
    #[cfg(feature = "qa")]
    qa::record(widget, |naming| naming.busy = Some(busy));
}

// Location controls require explicit accessible names.

/// A plain `GtkBox` has the `generic` accessible role, and ARIA forbids naming
/// those, so GTK silently drops any label set on one.
pub(super) fn pane_box() -> gtk::Box {
    gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .accessible_role(gtk::AccessibleRole::Group)
        .build()
}

pub(super) fn dialog_box(title: &str) -> gtk::Box {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .accessible_role(gtk::AccessibleRole::Dialog)
        .build();
    content.update_property(&[gtk::accessible::Property::Label(title)]);
    content.update_state(&[gtk::accessible::State::Hidden(false)]);
    #[cfg(feature = "qa")]
    qa::record(&content, |naming| {
        naming.label = Some(title.to_owned());
        naming.mark = Some(qa::Mark::Dialog);
    });
    content
}

#[cfg(test)]
mod tests;
