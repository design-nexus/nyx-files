// SPDX-License-Identifier: MIT

//! The rows a view shows: the column's entries (hidden ones left out when
//! hidden files are off) and their selection, shared by the List and Icons views.

use std::{cell::RefCell, rc::Rc};

use gtk::{gio, glib, prelude::*};

use crate::model::FileEntry;

/// A row: the entry and its position in the browser's column.
pub(super) struct Row {
    pub(super) position: usize,
    pub(super) entry: FileEntry,
}

pub(super) fn row_of(item: &glib::Object) -> Option<std::cell::Ref<'_, Row>> {
    item.downcast_ref::<glib::BoxedAnyObject>().map(glib::BoxedAnyObject::borrow::<Row>)
}

type BindHook = RefCell<Option<Box<dyn Fn(usize, &FileEntry)>>>;
type ContextHook = RefCell<Option<Box<dyn Fn(Option<u32>, &gtk::Widget, f64, f64)>>>;
type DragHook = RefCell<Option<Box<dyn Fn(u32) -> Vec<FileEntry>>>>;
type DropHook = RefCell<Option<Box<dyn Fn(Option<FileEntry>, Vec<crate::model::Location>, gtk::gdk::DragAction) -> bool>>>;

#[derive(Default)]
pub(super) struct Hooks {
    pub(super) bind: BindHook,
    pub(super) context: ContextHook,
    /// The entries a drag starting on row `index` carries.
    pub(super) drag: DragHook,
    /// Files dropped on a row (`Some`, when it is a folder) or on the view.
    pub(super) drop: DropHook,
}

impl Hooks {
    pub(super) fn bound(&self, row: &Row) {
        if let Some(f) = self.bind.borrow().as_ref() {
            f(row.position, &row.entry);
        }
    }

    pub(super) fn context(&self, position: Option<u32>, widget: &gtk::Widget, x: f64, y: f64) {
        if let Some(f) = self.context.borrow().as_ref() {
            f(position, widget, x, y);
        }
    }
}

pub(super) struct EntryModel {
    pub(super) store: gio::ListStore,
    pub(super) selection: gtk::MultiSelection,
    pub(super) hooks: Rc<Hooks>,
}

impl EntryModel {
    pub(super) fn new() -> Self {
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let selection = gtk::MultiSelection::new(Some(store.clone()));
        Self { store, selection, hooks: Rc::default() }
    }

    pub(super) fn set_entries(&self, entries: &[FileEntry], show_hidden: bool) {
        let selected: Vec<_> = self.selected().into_iter().map(|e| e.location).collect();
        let items: Vec<glib::BoxedAnyObject> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| show_hidden || !e.is_hidden)
            .map(|(position, entry)| glib::BoxedAnyObject::new(Row { position, entry: entry.clone() }))
            .collect();
        self.store.splice(0, self.store.n_items(), &items);
        if !selected.is_empty() {
            let bits = gtk::Bitset::new_empty();
            for (i, item) in items.iter().enumerate() {
                if selected.contains(&item.borrow::<Row>().entry.location) {
                    bits.add(i as u32);
                }
            }
            self.selection.set_selection(&bits, &bits);
        }
    }

    pub(super) fn len(&self) -> u32 {
        self.store.n_items()
    }

    pub(super) fn entry(&self, index: u32) -> Option<FileEntry> {
        let item = self.store.item(index)?;
        row_of(&item).map(|r| r.entry.clone())
    }

    /// The browser column positions of the selected rows, and the rows' indexes.
    pub(super) fn selected_positions(&self) -> Vec<(u32, usize)> {
        let bits = self.selection.selection();
        (0..bits.size())
            .filter_map(|i| {
                let index = bits.nth(i as u32);
                let item = self.store.item(index)?;
                row_of(&item).map(|r| (index, r.position))
            })
            .collect()
    }

    pub(super) fn selected(&self) -> Vec<FileEntry> {
        let bits = self.selection.selection();
        (0..bits.size()).filter_map(|i| self.entry(bits.nth(i as u32))).collect()
    }

    /// Selects `index` alone unless it is already part of the selection.
    pub(super) fn select_for_menu(&self, index: u32) {
        if !self.selection.is_selected(index) {
            self.selection.select_item(index, true);
        }
    }

    pub(super) fn select_location(&self, location: &crate::model::Location) -> Option<u32> {
        (0..self.len()).find(|&i| self.entry(i).is_some_and(|e| &e.location == location)).inspect(|&i| {
            self.selection.select_item(i, true);
        })
    }
}

/// Right-click on `widget` reports the row at `position` (or none, for the background).
pub(super) fn on_context(widget: &impl IsA<gtk::Widget>, hooks: &Rc<Hooks>, position: impl Fn() -> Option<u32> + 'static) {
    let hooks = hooks.clone();
    nyx_ui::menu::on_right_click(widget, move |w, x, y| hooks.context(position(), w, x, y));
}

/// Makes a row draggable and, when it is a folder, a drop target.
pub(super) fn dnd_on(child: &impl IsA<gtk::Widget>, item: &gtk::ListItem, hooks: &Rc<Hooks>) {
    let source = gtk::DragSource::new();
    source.set_actions(gtk::gdk::DragAction::COPY | gtk::gdk::DragAction::MOVE);
    let (weak, h) = (item.downgrade(), hooks.clone());
    source.connect_prepare(move |_, _, _| {
        let index = weak.upgrade()?.position();
        let entries = h.drag.borrow().as_ref().map(|f| f(index)).unwrap_or_default();
        super::actions::file_content(&entries)
    });
    child.add_controller(source);
    let target = gtk::DropTarget::new(gtk::gdk::FileList::static_type(), gtk::gdk::DragAction::COPY | gtk::gdk::DragAction::MOVE);
    target.set_preload(true);
    let weak = item.downgrade();
    target.connect_accept(move |_, _| {
        weak.upgrade()
            .and_then(|i| i.item())
            .and_then(|o| row_of(&o).map(|r| r.entry.is_directory()))
            .unwrap_or(false)
    });
    target.connect_enter(|t, _, _| {
        if let Some(w) = t.widget() {
            w.add_css_class("drop-target");
        }
        super::actions::drop_action(t)
    });
    target.connect_motion(|t, _, _| super::actions::drop_action(t));
    target.connect_leave(|t| {
        if let Some(w) = t.widget() {
            w.remove_css_class("drop-target");
        }
    });
    let (weak, h) = (item.downgrade(), hooks.clone());
    target.connect_drop(move |t, value, _, _| {
        if let Some(w) = t.widget() {
            w.remove_css_class("drop-target");
        }
        let Some(folder) = weak.upgrade().and_then(|i| i.item()).and_then(|o| row_of(&o).map(|r| r.entry.clone())) else {
            return false;
        };
        let Some(sources) = super::actions::dropped(value) else { return false };
        h.drop.borrow().as_ref().is_some_and(|f| f(Some(folder), sources, super::actions::forced_action(t)))
    });
    child.add_controller(target);
}

/// Makes a view's empty space take drops into the folder showing.
pub(super) fn drop_on_view(view: &impl IsA<gtk::Widget>, hooks: &Rc<Hooks>) {
    let target = gtk::DropTarget::new(gtk::gdk::FileList::static_type(), gtk::gdk::DragAction::COPY | gtk::gdk::DragAction::MOVE);
    target.connect_enter(|t, _, _| super::actions::drop_action(t));
    target.connect_motion(|t, _, _| super::actions::drop_action(t));
    let h = hooks.clone();
    target.connect_drop(move |t, value, _, _| {
        let Some(sources) = super::actions::dropped(value) else { return false };
        h.drop.borrow().as_ref().is_some_and(|f| f(None, sources, super::actions::forced_action(t)))
    });
    view.add_controller(target);
}
