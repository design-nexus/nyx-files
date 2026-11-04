// SPDX-License-Identifier: MIT

//! Folder peeking: resting the pointer on a folder in Icons or List shows
//! what it holds, when the setting is on.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    time::Duration,
};

use gtk::{glib, prelude::*};
use nyx_ui::widgets;

use crate::{app::BrowserEvent, model::FileEntry, ui::preferences::PreferenceManager};

use super::tab::Tab;

const DELAY: Duration = Duration::from_millis(700);
const SHOWN: usize = 12;

#[derive(Default)]
pub(super) struct Peek {
    popover: RefCell<Option<gtk::Popover>>,
    list: RefCell<Option<gtk::Box>>,
    status: RefCell<Option<gtk::Label>>,
    count: Cell<usize>,
    timer: RefCell<Option<glib::SourceId>>,
    /// The row the pointer rests on.
    hovered: Cell<Option<u32>>,
}

impl Peek {
    fn cancel_timer(&self) {
        if let Some(timer) = self.timer.take() {
            timer.remove();
        }
    }

    fn close(&self, tab: &Tab) {
        self.cancel_timer();
        if let Some(popover) = self.popover.take() {
            popover.popdown();
            popover.unparent();
        }
        self.list.take();
        self.status.take();
        tab.browser.close_peek();
    }

    /// The pointer entered row `index` (on `widget`) or left (`None`).
    pub(super) fn hover(&self, tab: &Rc<Tab>, index: Option<u32>, widget: &gtk::Widget) {
        self.hovered.set(index);
        self.cancel_timer();
        let Some(index) = index else {
            if self.popover.borrow().is_some() {
                self.close(tab);
            }
            return;
        };
        if !PreferenceManager::shared().folder_peeking()
            || tab.mode() == crate::ui::browser_modes::BrowserMode::Columns
        {
            return;
        }
        let Some(entry) = tab.model.entry(index).filter(FileEntry::is_directory) else {
            return;
        };
        let (weak, anchor) = (Rc::downgrade(tab), widget.downgrade());
        let timer = glib::timeout_add_local_once(DELAY, move || {
            let (Some(tab), Some(anchor)) = (weak.upgrade(), anchor.upgrade()) else {
                return;
            };
            tab.peek.timer.take();
            if tab.peek.hovered.get() == Some(index) {
                tab.peek.open(&tab, &anchor, entry);
            }
        });
        self.timer.replace(Some(timer));
    }

    fn open(&self, tab: &Rc<Tab>, anchor: &gtk::Widget, entry: FileEntry) {
        self.close(tab);
        let Some(depth) = tab.browser.active_depth() else {
            return;
        };
        let body = widgets::vbox(4);
        body.add_css_class("folder-peek");
        body.set_size_request(256, -1);
        body.append(&widgets::label(&entry.display_name, "search-name"));
        let list = widgets::vbox(2);
        body.append(&list);
        let status = widgets::label(&crate::i18n::tr("Loading…"), "dim");
        body.append(&status);
        let popover = gtk::Popover::builder()
            .child(&body)
            .autohide(false)
            .has_arrow(false)
            .position(gtk::PositionType::Right)
            .build();
        popover.add_css_class("menu-popover");
        popover.update_property(&[gtk::accessible::Property::Label(&crate::i18n::tr(
            "Folder peek",
        ))]);
        popover.set_parent(anchor);
        popover.popup();
        self.count.set(0);
        self.popover.replace(Some(popover));
        self.list.replace(Some(list));
        self.status.replace(Some(status));
        tab.browser.begin_peek(depth, entry.location);
    }

    /// Fills the open peek from the browser's peek events.
    pub(super) fn on_event(&self, event: &BrowserEvent) {
        let status = self.status.borrow().clone();
        match event {
            BrowserEvent::PeekEntriesAdded { entries } => {
                let Some(list) = self.list.borrow().clone() else {
                    return;
                };
                for entry in entries {
                    let shown = self.count.get();
                    self.count.set(shown + 1);
                    if shown >= SHOWN {
                        continue;
                    }
                    let row = widgets::hbox(8);
                    let icon = gtk::Image::from_icon_name(&super::icons_theme::name_for(entry));
                    icon.set_pixel_size(16);
                    row.append(&icon);
                    let name = widgets::label(&entry.display_name, "");
                    name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
                    row.append(&name);
                    list.append(&row);
                }
                if let Some(status) = status {
                    let more = self.count.get().saturating_sub(SHOWN);
                    status.set_text(&if more > 0 {
                        crate::i18n::count("more_items", more)
                    } else {
                        String::new()
                    });
                    status.set_visible(more > 0);
                }
            }
            BrowserEvent::PeekFinished => {
                if let Some(status) = status
                    && self.count.get() == 0
                {
                    status.set_text(&crate::i18n::tr("Empty folder"));
                    status.set_visible(true);
                }
            }
            BrowserEvent::PeekFailed { message } => {
                if let Some(status) = status {
                    status.set_text(message);
                    status.set_visible(true);
                }
            }
            _ => {}
        }
    }
}

/// Wires `tab`'s Icons and List rows to peek.
pub(super) fn install(tab: &Rc<Tab>) {
    let weak: Weak<Tab> = Rc::downgrade(tab);
    *tab.model.hooks.hover.borrow_mut() = Some(Box::new(move |index, widget| {
        if let Some(tab) = weak.upgrade() {
            tab.peek.hover(&tab, index, widget);
        }
    }));
}
