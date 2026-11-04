// SPDX-License-Identifier: MIT

//! The Columns view: one list per open folder, left to right. Selecting a
//! folder shows its contents in the next column.

use std::{cell::RefCell, rc::Rc};

use gtk::{glib, prelude::*};

use crate::ui::preferences::PreferenceManager;

use super::model::{self, EntryModel};

const WIDTH: i32 = 300;
const MIN_WIDTH: i32 = 160;
const ICON: i32 = 17;

pub(super) struct Column {
    pub(super) model: Rc<EntryModel>,
    pub(super) view: gtk::ListView,
    scroller: gtk::ScrolledWindow,
    /// The column and the handle at its right edge that resizes it.
    frame: gtk::Box,
}

/// The width new columns open at: the last one dragged, else the standard.
fn saved_width() -> i32 {
    PreferenceManager::shared()
        .browser_column_width()
        .unwrap_or(WIDTH)
        .max(MIN_WIDTH)
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
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let icon = model::icon_slot(ICON);
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        let count = gtk::Label::new(None);
        count.add_css_class("column-count");
        let more = gtk::Image::from_icon_name("go-next-symbolic");
        more.add_css_class("column-more");
        row.append(&icon);
        row.append(&label);
        row.append(&count);
        row.append(&more);
        let weak = item.downgrade();
        model::on_context(&row, &hooks, move || weak.upgrade().map(|i| i.position()));
        model::dnd_on(&row, item, &hooks);
        item.set_child(Some(&row));
    });
    let hooks = model.hooks.clone();
    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let (Some(row), Some(object)) = (item.child(), item.item()) else {
            return;
        };
        let Some(r) = model::row_of(&object) else {
            return;
        };
        hooks.bound(&r);
        let mut child = row.first_child();
        if let Some(icon) = child
            .as_ref()
            .and_then(|c| c.downcast_ref::<crate::ui::thumbnail::ThumbnailSlot>())
        {
            model::show_icon(icon, &r.entry, ICON);
        }
        child = child.and_then(|c| c.next_sibling());
        if let Some(label) = child.as_ref().and_then(|c| c.downcast_ref::<gtk::Label>()) {
            label.set_text(&r.entry.display_name);
        }
        child = child.and_then(|c| c.next_sibling());
        if let Some(count) = child.as_ref().and_then(|c| c.downcast_ref::<gtk::Label>()) {
            let text = if PreferenceManager::shared().columns_show_sizes() {
                super::list::size_text(&r.entry)
            } else {
                String::new()
            };
            count.set_visible(!text.is_empty());
            count.set_text(&text);
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
        Self {
            root,
            strip,
            columns: RefCell::new(Vec::new()),
        }
    }

    /// Makes exactly `count` columns; `make` wires each new one (given its depth).
    pub(super) fn ensure(&self, count: usize, make: impl Fn(usize, &Rc<Column>)) {
        let mut columns = self.columns.borrow_mut();
        let grew = columns.len() < count;
        while columns.len() > count {
            if let Some(column) = columns.pop() {
                self.strip.remove(&column.frame);
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
            scroller.set_size_request(saved_width(), -1);
            scroller.add_css_class("column");
            let frame = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            frame.append(&scroller);
            frame.append(&self.resize_handle(&scroller));
            self.strip.append(&frame);
            let column = Rc::new(Column {
                model,
                view,
                scroller,
                frame,
            });
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

    /// Dragging the edge resizes the column; when the drag ends the other
    /// columns take the same width, which new columns open at. A double-click
    /// returns to the standard width.
    fn resize_handle(&self, scroller: &gtk::ScrolledWindow) -> gtk::Box {
        let handle = gtk::Box::new(gtk::Orientation::Vertical, 0);
        handle.add_css_class("column-resize");
        handle.set_size_request(6, -1);
        handle.set_cursor_from_name(Some("col-resize"));
        handle.set_tooltip_text(Some(&crate::i18n::tr("Column width")));
        let drag = gtk::GestureDrag::new();
        let start = Rc::new(std::cell::Cell::new(0));
        let (s, sc) = (start.clone(), scroller.downgrade());
        drag.connect_drag_begin(move |_, _, _| {
            if let Some(sc) = sc.upgrade() {
                s.set(sc.width());
            }
        });
        let (s, sc) = (start.clone(), scroller.downgrade());
        drag.connect_drag_update(move |_, dx, _| {
            if let Some(sc) = sc.upgrade() {
                sc.set_size_request((s.get() + dx.round() as i32).max(MIN_WIDTH), -1);
            }
        });
        let (strip, sc) = (self.strip.downgrade(), scroller.downgrade());
        drag.connect_drag_end(move |_, _, _| {
            let (Some(strip), Some(sc)) = (strip.upgrade(), sc.upgrade()) else {
                return;
            };
            let width = sc.size_request().0.max(MIN_WIDTH);
            set_all_widths(&strip, width);
            PreferenceManager::shared().set_browser_column_width(Some(width));
        });
        handle.add_controller(drag);
        let click = gtk::GestureClick::new();
        let strip = self.strip.downgrade();
        click.connect_released(move |_, presses, _, _| {
            if presses == 2
                && let Some(strip) = strip.upgrade()
            {
                set_all_widths(&strip, WIDTH);
                PreferenceManager::shared().set_browser_column_width(None);
            }
        });
        handle.add_controller(click);
        handle
    }

    pub(super) fn column(&self, depth: usize) -> Option<Rc<Column>> {
        self.columns.borrow().get(depth).cloned()
    }
}

fn set_all_widths(strip: &gtk::Box, width: i32) {
    let mut frame = strip.first_child();
    while let Some(f) = frame {
        if let Some(scroller) = f.first_child() {
            scroller.set_size_request(width, -1);
        }
        frame = f.next_sibling();
    }
}
