// SPDX-License-Identifier: MIT

//! The List view: Name, Mode, Size, Kind and Modified, in the order and with
//! the widths and hidden columns saved in preferences; grouped by file type
//! when that is on.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gtk::{gio, glib, prelude::*};

use crate::{
    model::{FileEntry, MetadataValue, SortDirection, SortKey},
    ui::{
        preferences::{ListColumnId, ListColumns, PreferenceManager},
        thumbnail::ThumbnailSlot,
    },
};

use super::model::{self, EntryModel, Hooks};

const ICON: i32 = 18;

type SortFn = Box<dyn Fn(SortKey, SortDirection)>;

pub(super) struct ListView {
    pub(super) root: gtk::ScrolledWindow,
    pub(super) view: gtk::ColumnView,
    on_sort: Rc<RefCell<Option<SortFn>>>,
    /// Set while the header shows the folder's sort, so that is not a click.
    showing: Rc<Cell<bool>>,
}

/// The browser's sort key a column header sorts by.
fn sort_key(id: ListColumnId) -> Option<SortKey> {
    match id {
        ListColumnId::Name => Some(SortKey::Name),
        ListColumnId::Size => Some(SortKey::Size),
        ListColumnId::Type => Some(SortKey::Type),
        ListColumnId::Modified => Some(SortKey::Modified),
        ListColumnId::Mode => None,
    }
}

/// Lets a row's child report its own position on a right-click.
fn context_on(child: &impl IsA<gtk::Widget>, item: &gtk::ListItem, hooks: &Rc<Hooks>) {
    let item = item.downgrade();
    model::on_context(child, hooks, move || item.upgrade().map(|i| i.position()));
}

fn text_column(
    id: ListColumnId,
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
    let column = gtk::ColumnViewColumn::new(Some(&crate::i18n::tr(title(id))), Some(factory));
    column.set_resizable(true);
    column.set_id(Some(id.css_class()));
    column
}

fn title(id: ListColumnId) -> &'static str {
    match id {
        ListColumnId::Type => "Kind",
        other => other.title(),
    }
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
    column.set_id(Some(ListColumnId::Name.css_class()));
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
    model::type_group(entry)
}

pub(super) fn size_text(entry: &FileEntry) -> String {
    match (&entry.size, entry.is_directory(), &entry.child_count) {
        (_, true, MetadataValue::Known(0)) => crate::i18n::tr("No items"),
        (_, true, MetadataValue::Known(n)) => crate::i18n::count_u64("items", *n),
        (MetadataValue::Known(size), false, _) => glib::format_size(*size).to_string(),
        _ => String::new(),
    }
}

fn mode_text(entry: &FileEntry) -> String {
    match entry.mode {
        MetadataValue::Known(mode) => crate::ui::browser::properties::format_permissions(mode),
        _ => String::new(),
    }
}

fn width_of(saved: &ListColumns, id: ListColumnId) -> Option<i32> {
    match id {
        ListColumnId::Name => saved.name,
        ListColumnId::Mode => Some(saved.mode),
        ListColumnId::Size => Some(saved.size),
        ListColumnId::Type => Some(saved.kind),
        ListColumnId::Modified => Some(saved.modified),
    }
}

fn id_of(column: &gtk::ColumnViewColumn) -> Option<ListColumnId> {
    let id = column.id()?;
    ListColumnId::ALL
        .into_iter()
        .find(|c| c.css_class() == id.as_str())
}

impl ListView {
    pub(super) fn new(model: &EntryModel) -> Self {
        let view = gtk::ColumnView::new(Some(model.selection.clone()));
        view.add_css_class("file-list");
        view.set_show_column_separators(false);
        view.set_show_row_separators(false);
        view.set_reorderable(true);
        let header = gtk::SignalListItemFactory::new();
        header.connect_setup(|_, item| {
            if let Some(header) = item.downcast_ref::<gtk::ListHeader>() {
                let label = gtk::Label::new(None);
                label.set_xalign(0.0);
                label.add_css_class("list-group");
                header.set_child(Some(&label));
            }
        });
        header.connect_bind(|_, item| {
            let Some(header) = item.downcast_ref::<gtk::ListHeader>() else {
                return;
            };
            let (Some(label), Some(object)) =
                (header.child().and_downcast::<gtk::Label>(), header.item())
            else {
                return;
            };
            if let Some(row) = model::row_of(&object) {
                label.set_text(&model::type_group(&row.entry).to_uppercase());
            }
        });
        view.set_header_factory(Some(&header));
        let h = &model.hooks;
        let columns: Vec<gtk::ColumnViewColumn> = vec![
            name_column(h),
            text_column(ListColumnId::Mode, h, |l, e| l.set_text(&mode_text(e))),
            text_column(ListColumnId::Size, h, |l, e| l.set_text(&size_text(e))),
            text_column(ListColumnId::Type, h, |l, e| l.set_text(&kind(e))),
            text_column(ListColumnId::Modified, h, |l, e| {
                crate::util::set_modified_date(l, Some(e), "")
            }),
        ];
        let prefs = PreferenceManager::shared();
        let saved = prefs.browser_list_columns();
        for id in prefs.list_column_order() {
            if let Some(column) = columns.iter().find(|c| id_of(c) == Some(id)) {
                if let Some(width) = saved
                    .as_ref()
                    .and_then(|s| width_of(s, id))
                    .filter(|w| *w > 0)
                {
                    column.set_fixed_width(width);
                }
                view.append_column(column);
            }
        }
        model::on_context(&view, &model.hooks, || None);
        model::drop_on_view(&view, &model.hooks);
        // A header click only asks the browser to sort; the rows keep its order.
        for column in &columns {
            if id_of(column).and_then(sort_key).is_some() {
                column.set_sorter(Some(&gtk::CustomSorter::new(|_, _| gtk::Ordering::Equal)));
            }
        }
        let root = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .hexpand(true)
            .child(&view)
            .build();
        let list = Self {
            root,
            view,
            on_sort: Rc::default(),
            showing: Rc::default(),
        };
        list.follow_preferences();
        list.follow_header_sort();
        list
    }

    pub(super) fn connect_sort(&self, f: impl Fn(SortKey, SortDirection) + 'static) {
        *self.on_sort.borrow_mut() = Some(Box::new(f));
    }

    fn follow_header_sort(&self) {
        let Some(sorter) = self.view.sorter().and_downcast::<gtk::ColumnViewSorter>() else {
            return;
        };
        let (on_sort, showing) = (self.on_sort.clone(), self.showing.clone());
        sorter.connect_changed(move |sorter, _| {
            if showing.get() {
                return;
            }
            let Some(key) = sorter
                .primary_sort_column()
                .and_then(|c| id_of(&c))
                .and_then(sort_key)
            else {
                return;
            };
            let direction = match sorter.primary_sort_order() {
                gtk::SortType::Descending => SortDirection::Descending,
                _ => SortDirection::Ascending,
            };
            if let Some(f) = on_sort.borrow().as_ref() {
                f(key, direction);
            }
        });
    }

    /// Marks the column the folder is sorted by, with its direction.
    pub(super) fn show_sort(&self, key: SortKey, direction: SortDirection) {
        let columns = self.view.columns();
        let column = (0..columns.n_items())
            .filter_map(|i| columns.item(i).and_downcast::<gtk::ColumnViewColumn>())
            .find(|c| id_of(c).and_then(sort_key) == Some(key));
        let order = match direction {
            SortDirection::Ascending => gtk::SortType::Ascending,
            SortDirection::Descending => gtk::SortType::Descending,
        };
        if let Some(sorter) = self.view.sorter().and_downcast::<gtk::ColumnViewSorter>()
            && sorter.primary_sort_column().as_ref() == column.as_ref()
            && (column.is_none() || sorter.primary_sort_order() == order)
        {
            return;
        }
        self.showing.set(true);
        self.view.sort_by_column(column.as_ref(), order);
        self.showing.set(false);
    }

    /// Hidden columns follow the preference; narrow lists also drop Mode, Kind,
    /// then Modified, so names keep their room; order and widths are saved.
    fn follow_preferences(&self) {
        let prefs = PreferenceManager::shared();
        let apply = {
            let view = self.view.downgrade();
            let root = self.root.downgrade();
            move || {
                let (Some(view), Some(root)) = (view.upgrade(), root.upgrade()) else {
                    return;
                };
                let hidden = PreferenceManager::shared().list_hidden_columns();
                let width = root.width();
                let columns = view.columns();
                for i in 0..columns.n_items() {
                    let Some(column) = columns.item(i).and_downcast::<gtk::ColumnViewColumn>()
                    else {
                        continue;
                    };
                    let Some(id) = id_of(&column) else { continue };
                    let narrow = width > 0
                        && ((id == ListColumnId::Mode && width < 680)
                            || (id == ListColumnId::Type && width < 560)
                            || (id == ListColumnId::Modified && width < 440));
                    column.set_visible(
                        id == ListColumnId::Name || (!hidden.contains(&id) && !narrow),
                    );
                }
            }
        };
        let a = apply.clone();
        prefs.bind_preference(
            &self.view,
            PreferenceManager::list_hidden_columns,
            move |_, _| a(),
        );
        let last = std::cell::Cell::new(-1);
        self.root.add_tick_callback(move |root, _| {
            if root.width() != last.get() {
                last.set(root.width());
                apply();
            }
            glib::ControlFlow::Continue
        });
        let view = self.view.downgrade();
        self.view
            .columns()
            .connect_items_changed(move |_, _, _, _| {
                let Some(view) = view.upgrade() else { return };
                let columns = view.columns();
                let order: Vec<ListColumnId> = (0..columns.n_items())
                    .filter_map(|i| columns.item(i).and_downcast::<gtk::ColumnViewColumn>())
                    .filter_map(|c| id_of(&c))
                    .collect();
                if order.len() == ListColumnId::ALL.len()
                    && order != PreferenceManager::shared().list_column_order()
                {
                    PreferenceManager::shared().set_list_column_order(order);
                }
            });
        let columns = self.view.columns();
        for i in 0..columns.n_items() {
            let Some(column) = columns.item(i).and_downcast::<gtk::ColumnViewColumn>() else {
                continue;
            };
            let view = self.view.downgrade();
            column.connect_fixed_width_notify(move |_| {
                if let Some(view) = view.upgrade() {
                    save_widths(&view);
                }
            });
        }
    }
}

fn save_widths(view: &gtk::ColumnView) {
    let columns = view.columns();
    let mut saved = PreferenceManager::shared()
        .browser_list_columns()
        .unwrap_or(ListColumns {
            name: None,
            mode: 110,
            size: 90,
            kind: 140,
            modified: 140,
        });
    for i in 0..columns.n_items() {
        let Some(column) = columns.item(i).and_downcast::<gtk::ColumnViewColumn>() else {
            continue;
        };
        let width = column.fixed_width();
        if width <= 0 {
            continue;
        }
        match id_of(&column) {
            Some(ListColumnId::Name) => saved.name = Some(width),
            Some(ListColumnId::Mode) => saved.mode = width,
            Some(ListColumnId::Size) => saved.size = width,
            Some(ListColumnId::Type) => saved.kind = width,
            Some(ListColumnId::Modified) => saved.modified = width,
            None => {}
        }
    }
    PreferenceManager::shared().set_browser_list_columns(Some(saved));
}
