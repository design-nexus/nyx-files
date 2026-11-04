// SPDX-License-Identifier: MIT

//! The List view: Name, Size, Modified and Kind.

use std::rc::Rc;

use gtk::{gio, glib, prelude::*};

use crate::model::{FileEntry, MetadataValue};

use super::model::{self, EntryModel, Hooks};
use crate::ui::thumbnail::ThumbnailSlot;

const ICON: i32 = 18;

pub(super) struct ListView {
    pub(super) root: gtk::ScrolledWindow,
    pub(super) view: gtk::ColumnView,
}

/// Lets a row's child report its own position on a right-click.
fn context_on(child: &impl IsA<gtk::Widget>, item: &gtk::ListItem, hooks: &Rc<Hooks>) {
    let item = item.downgrade();
    model::on_context(child, hooks, move || item.upgrade().map(|i| i.position()));
}

fn column(
    title: &str,
    hooks: &Rc<Hooks>,
    bind: fn(&gtk::Label, &FileEntry),
) -> gtk::ColumnViewColumn {
    let factory = gtk::SignalListItemFactory::new();
    let h = hooks.clone();
    factory.connect_setup(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.add_css_class("dim-cell");
        context_on(&label, item, &h);
        item.set_child(Some(&label));
    });
    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let (Some(label), Some(object)) = (item.child().and_downcast::<gtk::Label>(), item.item())
        else {
            return;
        };
        if let Some(row) = model::row_of(&object) {
            bind(&label, &row.entry);
        }
    });
    let column = gtk::ColumnViewColumn::new(Some(title), Some(factory));
    column.set_resizable(true);
    column
}

fn name_column(hooks: &Rc<Hooks>) -> gtk::ColumnViewColumn {
    let factory = gtk::SignalListItemFactory::new();
    let h = hooks.clone();
    factory.connect_setup(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let icon = model::icon_slot(ICON);
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        label.add_css_class("entry-name");
        row.append(&icon);
        row.append(&label);
        context_on(&row, item, &h);
        model::dnd_on(&row, item, &h);
        item.set_child(Some(&row));
    });
    let h = hooks.clone();
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
        h.bound(&r);
        if let Some(icon) = row.first_child().and_downcast::<ThumbnailSlot>() {
            model::show_icon(&icon, &r.entry, ICON);
        }
        if let Some(label) = row.last_child().and_downcast::<gtk::Label>() {
            label.set_text(&r.entry.display_name);
        }
        row.set_opacity(if r.entry.is_hidden { 0.6 } else { 1.0 });
    });
    model::cancel_on_unbind(&factory);
    let column = gtk::ColumnViewColumn::new(Some(&crate::i18n::tr("Name")), Some(factory));
    column.set_expand(true);
    column.set_resizable(true);
    column
}

/// The file's icon from the icon theme, by content type.
pub(super) fn entry_icon(entry: &FileEntry) -> gio::Icon {
    if entry.is_directory() {
        return gio::ThemedIcon::new("folder").upcast();
    }
    let (content_type, _) = gio::content_type_guess(Some(&entry.native_name), None::<&[u8]>);
    gio::content_type_get_icon(&content_type)
}

pub(super) fn kind(entry: &FileEntry) -> String {
    if entry.is_directory() {
        return crate::i18n::tr("Folder");
    }
    let (content_type, _) = gio::content_type_guess(Some(&entry.native_name), None::<&[u8]>);
    gio::content_type_get_description(&content_type).to_string()
}

pub(super) fn size_text(entry: &FileEntry) -> String {
    match (&entry.size, entry.is_directory(), &entry.child_count) {
        (_, true, MetadataValue::Known(0)) => crate::i18n::tr("No items"),
        (_, true, MetadataValue::Known(n)) => crate::i18n::count_u64("items", *n),
        (MetadataValue::Known(size), false, _) => glib::format_size(*size).to_string(),
        _ => String::new(),
    }
}

impl ListView {
    pub(super) fn new(model: &EntryModel) -> Self {
        let view = gtk::ColumnView::new(Some(model.selection.clone()));
        view.add_css_class("file-list");
        view.set_show_column_separators(false);
        view.set_show_row_separators(false);
        view.set_reorderable(false);
        view.append_column(&name_column(&model.hooks));
        view.append_column(&column(
            &crate::i18n::tr("Size"),
            &model.hooks,
            |label, entry| label.set_text(&size_text(entry)),
        ));
        view.append_column(&column(
            &crate::i18n::tr("Modified"),
            &model.hooks,
            |label, entry| {
                crate::util::set_modified_date(label, Some(entry), "");
            },
        ));
        view.append_column(&column(
            &crate::i18n::tr("Kind"),
            &model.hooks,
            |label, entry| label.set_text(&kind(entry)),
        ));
        model::on_context(&view, &model.hooks, || None);
        model::drop_on_view(&view, &model.hooks);
        let root = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .hexpand(true)
            .child(&view)
            .build();
        Self { root, view }
    }
}
