// SPDX-License-Identifier: MIT

//! The list view: Name, Size, Modified and Kind for the folder showing.

use gtk::{gio, glib, prelude::*};

use crate::model::{FileEntry, MetadataValue};

pub(super) struct ListView {
    pub(super) root: gtk::ScrolledWindow,
    pub(super) view: gtk::ColumnView,
    store: gio::ListStore,
    bind_hook: BindHook,
}

/// A row: the entry and its position in the browser's column.
pub(super) struct Row {
    pub(super) position: usize,
    pub(super) entry: FileEntry,
}

fn row_of(item: &glib::Object) -> Option<std::cell::Ref<'_, Row>> {
    item.downcast_ref::<glib::BoxedAnyObject>().map(glib::BoxedAnyObject::borrow::<Row>)
}

fn entry_of(item: &glib::Object) -> Option<std::cell::Ref<'_, FileEntry>> {
    row_of(item).map(|row| std::cell::Ref::map(row, |r| &r.entry))
}

type BindHook = std::rc::Rc<std::cell::RefCell<Option<Box<dyn Fn(usize, &FileEntry)>>>>;

fn column(title: &str, expand: bool, bind: fn(&gtk::Label, &FileEntry)) -> gtk::ColumnViewColumn {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.add_css_class("list-cell");
        if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
            item.set_child(Some(&label));
        }
    });
    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
        let (Some(label), Some(object)) = (item.child().and_downcast::<gtk::Label>(), item.item()) else { return };
        if let Some(entry) = entry_of(&object) {
            bind(&label, &entry);
        }
    });
    let column = gtk::ColumnViewColumn::new(Some(title), Some(factory));
    column.set_expand(expand);
    column.set_resizable(true);
    column
}

fn name_column(hook: BindHook) -> gtk::ColumnViewColumn {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let icon = gtk::Image::new();
        icon.add_css_class("entry-icon");
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        label.add_css_class("entry-name");
        row.append(&icon);
        row.append(&label);
        if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
            item.set_child(Some(&row));
        }
    });
    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
        let (Some(row), Some(object)) = (item.child(), item.item()) else { return };
        if let (Some(r), Some(hook)) = (row_of(&object), hook.borrow().as_ref()) {
            hook(r.position, &r.entry);
        }
        let Some(entry) = entry_of(&object) else { return };
        if let Some(icon) = row.first_child().and_downcast::<gtk::Image>() {
            icon.set_from_gicon(&entry_icon(&entry));
        }
        if let Some(label) = row.last_child().and_downcast::<gtk::Label>() {
            label.set_text(&entry.display_name);
        }
        row.set_opacity(if entry.is_hidden { 0.6 } else { 1.0 });
    });
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

fn kind(entry: &FileEntry) -> String {
    if entry.is_directory() {
        return crate::i18n::tr("Folder");
    }
    let (content_type, _) = gio::content_type_guess(Some(&entry.native_name), None::<&[u8]>);
    gio::content_type_get_description(&content_type).to_string()
}

impl ListView {
    pub(super) fn new() -> Self {
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let selection = gtk::MultiSelection::new(Some(store.clone()));
        let view = gtk::ColumnView::new(Some(selection.clone()));
        view.add_css_class("file-list");
        view.set_show_column_separators(false);
        view.set_show_row_separators(false);
        view.set_reorderable(false);
        let bind_hook = BindHook::default();
        view.append_column(&name_column(bind_hook.clone()));
        view.append_column(&column(&crate::i18n::tr("Size"), false, |label, entry| {
            label.set_text(&match (&entry.size, entry.is_directory(), &entry.child_count) {
                (_, true, MetadataValue::Known(0)) => crate::i18n::tr("No items"),
                (_, true, MetadataValue::Known(n)) => crate::i18n::count_u64("items", *n),
                (MetadataValue::Known(size), false, _) => glib::format_size(*size).to_string(),
                _ => String::new(),
            });
            label.add_css_class("dim-cell");
        }));
        view.append_column(&column(&crate::i18n::tr("Modified"), false, |label, entry| {
            crate::util::set_modified_date(label, Some(entry), "");
            label.add_css_class("dim-cell");
        }));
        view.append_column(&column(&crate::i18n::tr("Kind"), false, |label, entry| {
            label.set_text(&kind(entry));
            label.add_css_class("dim-cell");
        }));
        let root = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .hexpand(true)
            .child(&view)
            .build();
        Self { root, view, store, bind_hook }
    }

    /// Called as each row is shown, with its column position (to fill metadata).
    pub(super) fn on_bind(&self, f: impl Fn(usize, &FileEntry) + 'static) {
        *self.bind_hook.borrow_mut() = Some(Box::new(f));
    }

    pub(super) fn set_entries(&self, entries: &[FileEntry], show_hidden: bool) {
        let items: Vec<glib::BoxedAnyObject> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| show_hidden || !e.is_hidden)
            .map(|(position, entry)| glib::BoxedAnyObject::new(Row { position, entry: entry.clone() }))
            .collect();
        self.store.splice(0, self.store.n_items(), &items);
    }

    pub(super) fn entry(&self, position: u32) -> Option<FileEntry> {
        let item = self.store.item(position)?;
        entry_of(&item).map(|e| e.clone())
    }
}
