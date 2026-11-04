// SPDX-License-Identifier: MIT

//! The Columns view: one list per open folder, left to right. Selecting a
//! folder shows its contents in the next column.

use std::{cell::RefCell, rc::Rc};

use gtk::{glib, prelude::*};

use super::model::{self, EntryModel};

const WIDTH: i32 = 248;
const ICON: i32 = 17;

pub(super) struct Column {
    pub(super) model: Rc<EntryModel>,
    pub(super) view: gtk::ListView,
    scroller: gtk::ScrolledWindow,
}

pub(super) struct ColumnsView {
    pub(super) root: gtk::ScrolledWindow,
    strip: gtk::Box,
    pub(super) columns: RefCell<Vec<Rc<Column>>>,
}

fn column_list(model: &EntryModel) -> gtk::ListView {
    let factory = gtk::SignalListItemFactory::new();
    let hooks = model.hooks.clone();
    factory.connect_setup(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let icon = model::icon_slot(ICON);
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        let more = gtk::Image::from_icon_name("go-next-symbolic");
        more.add_css_class("column-more");
        row.append(&icon);
        row.append(&label);
        row.append(&more);
        let weak = item.downgrade();
        model::on_context(&row, &hooks, move || weak.upgrade().map(|i| i.position()));
        model::dnd_on(&row, item, &hooks);
        item.set_child(Some(&row));
    });
    let hooks = model.hooks.clone();
    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
        let (Some(row), Some(object)) = (item.child(), item.item()) else { return };
        let Some(r) = model::row_of(&object) else { return };
        hooks.bound(&r);
        let mut child = row.first_child();
        if let Some(icon) = child.as_ref().and_then(|c| c.downcast_ref::<crate::ui::thumbnail::ThumbnailSlot>()) {
            model::show_icon(icon, &r.entry, ICON);
        }
        child = child.and_then(|c| c.next_sibling());
        if let Some(label) = child.as_ref().and_then(|c| c.downcast_ref::<gtk::Label>()) {
            label.set_text(&r.entry.display_name);
        }
        if let Some(more) = row.last_child() {
            more.set_visible(r.entry.is_directory());
        }
        row.set_opacity(if r.entry.is_hidden { 0.6 } else { 1.0 });
    });
    model::cancel_on_unbind(&factory);
    let view = gtk::ListView::new(Some(model.selection.clone()), Some(factory));
    view.add_css_class("file-column");
    view.set_enable_rubberband(true);
    model::on_context(&view, &model.hooks, || None);
    model::drop_on_view(&view, &model.hooks);
    view
}

impl ColumnsView {
    pub(super) fn new() -> Self {
        let strip = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        strip.add_css_class("column-strip");
        let root = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .hexpand(true)
            .vexpand(true)
            .child(&strip)
            .build();
        Self { root, strip, columns: RefCell::new(Vec::new()) }
    }

    /// Makes exactly `count` columns; `make` wires each new one (given its depth).
    pub(super) fn ensure(&self, count: usize, make: impl Fn(usize, &Rc<Column>)) {
        let mut columns = self.columns.borrow_mut();
        let grew = columns.len() < count;
        while columns.len() > count {
            if let Some(column) = columns.pop() {
                self.strip.remove(&column.scroller);
            }
        }
        while columns.len() < count {
            let model = Rc::new(EntryModel::new());
            let view = column_list(&model);
            let scroller = gtk::ScrolledWindow::builder()
                .hscrollbar_policy(gtk::PolicyType::Never)
                .vexpand(true)
                .child(&view)
                .build();
            scroller.set_size_request(WIDTH, -1);
            scroller.add_css_class("column");
            self.strip.append(&scroller);
            let column = Rc::new(Column { model, view, scroller });
            make(columns.len(), &column);
            columns.push(column);
        }
        drop(columns);
        if grew {
            let root = self.root.clone();
            glib::idle_add_local_once(move || {
                let adj = root.hadjustment();
                adj.set_value(adj.upper());
            });
        }
    }

    pub(super) fn column(&self, depth: usize) -> Option<Rc<Column>> {
        self.columns.borrow().get(depth).cloned()
    }
}
