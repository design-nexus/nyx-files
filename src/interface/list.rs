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
    last_sort: Rc<Cell<Option<(SortKey, SortDirection)>>>,
    sync_fn: Rc<dyn Fn()>,
}

pub(super) const TOGGLEABLE_COLUMNS: [ListColumnId; 4] = [
    ListColumnId::Mode,
    ListColumnId::Size,
    ListColumnId::Type,
    ListColumnId::Modified,
];

pub(super) fn column_title(id: ListColumnId) -> &'static str {
    match id {
        ListColumnId::Type => "Kind",
        other => other.title(),
    }
}

pub(super) fn is_column_visible(id: ListColumnId) -> bool {
    id == ListColumnId::Name
        || !PreferenceManager::shared()
            .list_hidden_columns()
            .contains(&id)
}

pub(super) fn toggle_column(id: ListColumnId) {
    if id == ListColumnId::Name {
        return;
    }
    let prefs = PreferenceManager::shared();
    let mut hidden = prefs.list_hidden_columns();
    if let Some(index) = hidden.iter().position(|hidden_id| *hidden_id == id) {
        hidden.remove(index);
    } else {
        hidden.push(id);
    }
    prefs.set_list_hidden_columns(hidden);
}

pub(super) fn reset_columns() {
    let prefs = PreferenceManager::shared();
    prefs.set_list_hidden_columns(Vec::new());
    prefs.set_list_column_order(crate::ui::preferences::default_list_column_order());
    prefs.set_browser_list_columns(None);
}

/// Opens a live-toggling Columns popover anchored to `anchor` at `(x, y)`.
pub(super) fn popup_columns(
    anchor: &impl IsA<gtk::Widget>,
    x: f64,
    y: f64,
    on_change: Option<Rc<dyn Fn()>>,
) {
    let anchor = anchor.as_ref().clone();
    let popover = gtk::Popover::new();
    popover.set_has_arrow(false);
    popover.add_css_class("menu-popover");
    popover.set_parent(&anchor);
    popover.set_position(gtk::PositionType::Bottom);
    popover.set_halign(gtk::Align::Start);

    let list = gtk::Box::new(gtk::Orientation::Vertical, 1);
    list.add_css_class("menu-list");

    let heading = gtk::Label::new(Some(&crate::i18n::tr("Columns")));
    heading.set_xalign(0.0);
    heading.add_css_class("menu-heading");
    list.append(&heading);

    let mut syncers: Vec<Rc<dyn Fn()>> = Vec::new();
    for id in TOGGLEABLE_COLUMNS {
        let button = gtk::Button::new();
        button.add_css_class("flat");
        button.add_css_class("menu-item");
        button.add_css_class("menu-check");
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let mark = gtk::Image::from_icon_name("object-select-symbolic");
        mark.add_css_class("menu-icon");
        mark.set_size_request(16, -1);
        let label = gtk::Label::new(Some(&crate::i18n::tr(column_title(id))));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        row.append(&mark);
        row.append(&label);
        button.set_child(Some(&row));

        let sync: Rc<dyn Fn()> = {
            let button = button.downgrade();
            let mark = mark.downgrade();
            Rc::new(move || {
                let (Some(button), Some(mark)) = (button.upgrade(), mark.upgrade()) else {
                    return;
                };
                let on = is_column_visible(id);
                mark.set_opacity(if on { 1.0 } else { 0.0 });
                if on {
                    button.add_css_class("checked");
                } else {
                    button.remove_css_class("checked");
                }
            })
        };
        sync();
        let s = sync.clone();
        let changed = on_change.clone();
        button.connect_clicked(move |_| {
            toggle_column(id);
            if let Some(changed) = &changed {
                changed();
            }
            s();
        });
        syncers.push(sync);
        list.append(&button);
    }

    list.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

    let reset = gtk::Button::new();
    reset.add_css_class("flat");
    reset.add_css_class("menu-item");
    let reset_row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.add_css_class("menu-icon");
    spacer.set_size_request(16, -1);
    let reset_label = gtk::Label::new(Some(&crate::i18n::tr("Reset to default columns")));
    reset_label.set_xalign(0.0);
    reset_label.set_hexpand(true);
    reset_row.append(&spacer);
    reset_row.append(&reset_label);
    reset.set_child(Some(&reset_row));
    let pop = popover.downgrade();
    let changed = on_change;
    reset.connect_clicked(move |_| {
        reset_columns();
        if let Some(changed) = &changed {
            changed();
        }
        for sync in &syncers {
            sync();
        }
        if let Some(pop) = pop.upgrade() {
            pop.popdown();
        }
    });
    list.append(&reset);

    popover.set_child(Some(&list));
    let (x, y) = clamp_to_window(&anchor, &popover, x, y);
    popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    let weak_anchor = anchor.downgrade();
    popover.connect_closed(move |pop| {
        let (pop, anchor) = (pop.clone(), weak_anchor.clone());
        glib::idle_add_local_once(move || {
            if anchor.upgrade().is_some() && pop.parent().is_some() {
                pop.unparent();
            }
        });
    });
    popover.popup();
}

fn clamp_to_window(anchor: &gtk::Widget, popover: &gtk::Popover, x: f64, y: f64) -> (f64, f64) {
    let Some(root) = anchor.root() else {
        return (x, y);
    };
    let root: gtk::Widget = root.upcast();
    let Some(at) = anchor.compute_point(&root, &gtk::graphene::Point::new(x as f32, y as f32))
    else {
        return (x, y);
    };
    let (_, width, _, _) = popover.measure(gtk::Orientation::Horizontal, -1);
    let (_, height, _, _) = popover.measure(gtk::Orientation::Vertical, width);
    let (at_x, at_y) = (f64::from(at.x()), f64::from(at.y()));
    let over_x = (at_x + f64::from(width) - f64::from(root.width())).clamp(0.0, at_x.max(0.0));
    let over_y = (at_y + f64::from(height) - f64::from(root.height())).clamp(0.0, at_y.max(0.0));
    (x - over_x, y - over_y)
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
        model::dnd_on(&label, item, &h);
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
    let column = gtk::ColumnViewColumn::new(Some(&crate::i18n::tr(column_title(id))), Some(factory));
    column.set_resizable(true);
    column.set_id(Some(id.css_class()));
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

fn group_header_factory() -> gtk::SignalListItemFactory {
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
    header
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
        let h = &model.hooks;
        let all_columns: Rc<Vec<gtk::ColumnViewColumn>> = Rc::new(vec![
            name_column(h),
            text_column(ListColumnId::Mode, h, |l, e| l.set_text(&mode_text(e))),
            text_column(ListColumnId::Size, h, |l, e| l.set_text(&size_text(e))),
            text_column(ListColumnId::Type, h, |l, e| l.set_text(&kind(e))),
            text_column(ListColumnId::Modified, h, |l, e| {
                crate::util::set_modified_date(l, Some(e), "")
            }),
        ]);
        let prefs = PreferenceManager::shared();
        let saved = prefs.browser_list_columns();
        for column in all_columns.iter() {
            if let Some(id) = id_of(column) {
                apply_column_width(column, id, saved.as_ref());
            }
            if id_of(column).and_then(sort_key).is_some() {
                column.set_sorter(Some(&gtk::CustomSorter::new(|_, _| gtk::Ordering::Equal)));
            }
        }
        model::on_context(&view, &model.hooks, || None);
        model::drop_on_view(&view, &model.hooks);
        let root = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .hexpand(true)
            .child(&view)
            .build();
        let showing = Rc::default();
        let last_sort = Rc::default();
        let sync_fn = Self::wire_preferences(&view, &root, &all_columns, &showing, &last_sort);
        if let Some(header) = view.first_child().filter(|w| w.css_name() == "header") {
            let click = gtk::GestureClick::new();
            click.set_button(gtk::gdk::BUTTON_SECONDARY);
            // Column title buttons handle all buttons in Bubble phase; Capture intercepts right-clicks first.
            click.set_propagation_phase(gtk::PropagationPhase::Capture);
            let anchor = view.downgrade();
            let sync = sync_fn.clone();
            click.connect_pressed(move |gesture, _, x, y| {
                let (Some(header), Some(anchor)) = (gesture.widget(), anchor.upgrade()) else {
                    return;
                };
                gesture.set_state(gtk::EventSequenceState::Claimed);
                let point = header
                    .compute_point(&anchor, &gtk::graphene::Point::new(x as f32, y as f32))
                    .unwrap_or_else(|| gtk::graphene::Point::new(x as f32, y as f32));
                popup_columns(
                    &anchor,
                    f64::from(point.x()),
                    f64::from(point.y()),
                    Some(sync.clone()),
                );
            });
            header.add_controller(click);
        }
        let list = Self {
            root,
            view,
            on_sort: Rc::default(),
            showing,
            last_sort,
            sync_fn,
        };
        list.sync_columns();
        list.follow_header_sort();
        list
    }

    pub(super) fn sync_columns(&self) {
        (self.sync_fn)();
    }

    /// Type headings only while the list is grouped by type; ungrouped, GTK
    /// would title the whole list after its first item.
    pub(super) fn set_grouped(&self, grouped: bool) {
        self.view
            .set_header_factory(grouped.then(group_header_factory).as_ref());
    }

    pub(super) fn connect_sort(&self, f: impl Fn(SortKey, SortDirection) + 'static) {
        *self.on_sort.borrow_mut() = Some(Box::new(f));
    }

    fn follow_header_sort(&self) {
        let Some(sorter) = self.view.sorter().and_downcast::<gtk::ColumnViewSorter>() else {
            return;
        };
        let (on_sort, showing, last_sort) = (
            self.on_sort.clone(),
            self.showing.clone(),
            self.last_sort.clone(),
        );
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
            last_sort.set(Some((key, direction)));
            if let Some(f) = on_sort.borrow().as_ref() {
                f(key, direction);
            }
        });
    }

    /// Marks the column the folder is sorted by, with its direction.
    pub(super) fn show_sort(&self, key: SortKey, direction: SortDirection) {
        self.last_sort.set(Some((key, direction)));
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

    /// Rebuilds visible columns from preferences and narrow-width rules, and saves
    /// column order and widths as the user drags them.
    fn wire_preferences(
        view: &gtk::ColumnView,
        root: &gtk::ScrolledWindow,
        all_columns: &Rc<Vec<gtk::ColumnViewColumn>>,
        showing: &Rc<Cell<bool>>,
        last_sort: &Rc<Cell<Option<(SortKey, SortDirection)>>>,
    ) -> Rc<dyn Fn()> {
        let prefs = PreferenceManager::shared();
        let syncing = Rc::new(Cell::new(false));
        let apply: Rc<dyn Fn()> = {
            let view = view.downgrade();
            let root = root.downgrade();
            let all_columns = all_columns.clone();
            let syncing = syncing.clone();
            let showing = showing.clone();
            let last_sort = last_sort.clone();
            Rc::new(move || {
                let (Some(view), Some(root)) = (view.upgrade(), root.upgrade()) else {
                    return;
                };
                let prefs = PreferenceManager::shared();
                let hidden = prefs.list_hidden_columns();
                let order = prefs.list_column_order();
                let width = root.width();
                let wanted: Vec<ListColumnId> = order
                    .into_iter()
                    .filter(|&id| {
                        let narrow = width > 0
                            && ((id == ListColumnId::Mode && width < 680)
                                || (id == ListColumnId::Type && width < 560)
                                || (id == ListColumnId::Modified && width < 440));
                        id == ListColumnId::Name || (!hidden.contains(&id) && !narrow)
                    })
                    .collect();
                let existing: Vec<gtk::ColumnViewColumn> = {
                    let columns = view.columns();
                    (0..columns.n_items())
                        .filter_map(|i| columns.item(i).and_downcast::<gtk::ColumnViewColumn>())
                        .collect()
                };
                let current: Vec<ListColumnId> = existing.iter().filter_map(id_of).collect();
                if current == wanted {
                    return;
                }
                syncing.set(true);
                for column in &existing {
                    view.remove_column(column);
                }
                for id in &wanted {
                    if let Some(column) = all_columns.iter().find(|c| id_of(c) == Some(*id)) {
                        column.set_visible(true);
                        view.append_column(column);
                    }
                }
                if let Some((key, direction)) = last_sort.get() {
                    let sort_column = all_columns
                        .iter()
                        .find(|c| id_of(c).is_some_and(|id| wanted.contains(&id) && sort_key(id) == Some(key)));
                    let sort_order = match direction {
                        SortDirection::Ascending => gtk::SortType::Ascending,
                        SortDirection::Descending => gtk::SortType::Descending,
                    };
                    showing.set(true);
                    view.sort_by_column(sort_column, sort_order);
                    showing.set(false);
                }
                syncing.set(false);
            })
        };
        let a = apply.clone();
        prefs.bind_preference(
            view,
            PreferenceManager::list_hidden_columns,
            move |_, _| a(),
        );
        let a = apply.clone();
        prefs.bind_preference(
            view,
            PreferenceManager::list_column_order,
            move |_, _| a(),
        );
        {
            let syncing = syncing.clone();
            let all_columns = all_columns.clone();
            prefs.bind_preference(
                view,
                PreferenceManager::browser_list_columns,
                move |_, saved| {
                    syncing.set(true);
                    for column in all_columns.iter() {
                        if let Some(id) = id_of(column) {
                            apply_column_width(column, id, saved.as_ref());
                        }
                    }
                    syncing.set(false);
                },
            );
        }
        let last = std::cell::Cell::new(-1);
        let a = apply.clone();
        crate::ui::frame::after_layout(root, move |root| {
            if root.width() != last.get() {
                last.set(root.width());
                a();
            }
        });
        let weak_view = view.downgrade();
        let s = syncing.clone();
        view.columns().connect_items_changed(move |_, _, _, _| {
            if s.get() {
                return;
            }
            let Some(view) = weak_view.upgrade() else {
                return;
            };
            let columns = view.columns();
            let visible_order: Vec<ListColumnId> = (0..columns.n_items())
                .filter_map(|i| columns.item(i).and_downcast::<gtk::ColumnViewColumn>())
                .filter_map(|c| id_of(&c))
                .collect();
            if visible_order.is_empty() {
                return;
            }
            let current = PreferenceManager::shared().list_column_order();
            let mut next = visible_order.iter().copied();
            let merged: Vec<ListColumnId> = current
                .iter()
                .map(|id| {
                    if visible_order.contains(id) {
                        next.next().unwrap_or(*id)
                    } else {
                        *id
                    }
                })
                .collect();
            if merged != current {
                PreferenceManager::shared().set_list_column_order(merged);
            }
        });
        for column in all_columns.iter() {
            let s = syncing.clone();
            let cols = all_columns.clone();
            column.connect_fixed_width_notify(move |column| {
                if s.get() {
                    return;
                }
                let width = column.fixed_width();
                match id_of(column) {
                    Some(ListColumnId::Name) if width > 0 => column.set_expand(false),
                    Some(id) if width > 0 && width < min_width(id) => {
                        column.set_fixed_width(min_width(id));
                        return;
                    }
                    _ => {}
                }
                save_widths(&cols);
            });
        }
        apply
    }
}

fn apply_column_width(
    column: &gtk::ColumnViewColumn,
    id: ListColumnId,
    saved: Option<&ListColumns>,
) {
    let width = saved.and_then(|s| width_of(s, id)).filter(|w| *w > 0);
    if id == ListColumnId::Name {
        // Name takes the room the other columns leave until it is dragged.
        column.set_expand(width.is_none());
        column.set_fixed_width(width.unwrap_or(-1));
    } else {
        column.set_fixed_width(width.unwrap_or(default_width(id)).max(min_width(id)));
    }
}

/// The widths the columns open at, as before.
fn default_width(id: ListColumnId) -> i32 {
    match id {
        ListColumnId::Name => -1,
        ListColumnId::Mode => 160,
        ListColumnId::Size => 90,
        ListColumnId::Type => 120,
        ListColumnId::Modified => 150,
    }
}

/// How narrow a dragged column can get; Name has no minimum.
fn min_width(id: ListColumnId) -> i32 {
    match id {
        ListColumnId::Name => 0,
        ListColumnId::Mode => 80,
        ListColumnId::Size => 70,
        ListColumnId::Type => 80,
        ListColumnId::Modified => 110,
    }
}

fn save_widths(columns: &[gtk::ColumnViewColumn]) {
    let mut saved = PreferenceManager::shared()
        .browser_list_columns()
        .unwrap_or(ListColumns {
            name: None,
            mode: default_width(ListColumnId::Mode),
            size: default_width(ListColumnId::Size),
            kind: default_width(ListColumnId::Type),
            modified: default_width(ListColumnId::Modified),
        });
    for column in columns {
        let width = column.fixed_width();
        if width <= 0 {
            continue;
        }
        match id_of(column) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn column_order(list: &ListView) -> Vec<ListColumnId> {
        let columns = list.view.columns();
        (0..columns.n_items())
            .filter_map(|i| columns.item(i).and_downcast::<gtk::ColumnViewColumn>())
            .filter_map(|c| id_of(&c))
            .collect()
    }

    fn find_column(list: &ListView, id: ListColumnId) -> Option<gtk::ColumnViewColumn> {
        let columns = list.view.columns();
        (0..columns.n_items())
            .filter_map(|i| columns.item(i).and_downcast::<gtk::ColumnViewColumn>())
            .find(|c| id_of(c) == Some(id))
    }

    fn column_by_id(list: &ListView, id: ListColumnId) -> gtk::ColumnViewColumn {
        find_column(list, id).expect("column should exist")
    }

    fn sample_entry() -> crate::model::FileEntry {
        crate::model::FileEntry {
            location: crate::model::Location::local("/tmp/alpha.txt"),
            thumbnail_path: None,
            native_name: "alpha.txt".into(),
            display_name: "alpha.txt".into(),
            kind: crate::model::EntryKind::File,
            size: crate::model::MetadataValue::Known(1024),
            modified_unix_seconds: crate::model::MetadataValue::Known(1_700_000_000),
            recent_unix_seconds: crate::model::MetadataValue::Unknown,
            recent_uri: None,
            mode: crate::model::MetadataValue::Known(0o644),
            image_dimensions: crate::model::MetadataValue::Unknown,
            child_count: crate::model::MetadataValue::Unknown,
            duration_seconds: crate::model::MetadataValue::Unknown,
            is_hidden: false,
        }
    }

    #[test]
    fn toggle_and_reset_columns_update_open_list_view() {
        crate::test_support::gtk_test(
            "interface::list::tests::toggle_and_reset_columns_update_open_list_view",
            || {
                let _prefs = PreferenceManager::shared();
                let model = EntryModel::new();
                model.set_entries(&[sample_entry()], false);
                let list = ListView::new(&model);
                list.show_sort(SortKey::Size, SortDirection::Descending);

                assert_eq!(
                    list.view.first_child().map(|w| w.css_name()),
                    Some("header".into())
                );
                assert!(find_column(&list, ListColumnId::Size).is_some());

                toggle_column(ListColumnId::Name);
                assert!(is_column_visible(ListColumnId::Name));
                assert!(find_column(&list, ListColumnId::Name).is_some());

                toggle_column(ListColumnId::Size);
                list.sync_columns();
                assert!(!is_column_visible(ListColumnId::Size));
                assert!(find_column(&list, ListColumnId::Size).is_none());

                let custom_order = vec![
                    ListColumnId::Modified,
                    ListColumnId::Name,
                    ListColumnId::Size,
                    ListColumnId::Type,
                    ListColumnId::Mode,
                ];
                PreferenceManager::shared().set_list_column_order(custom_order.clone());
                assert_eq!(
                    column_order(&list),
                    vec![
                        ListColumnId::Modified,
                        ListColumnId::Name,
                        ListColumnId::Type,
                        ListColumnId::Mode,
                    ]
                );

                column_by_id(&list, ListColumnId::Mode).set_fixed_width(240);
                assert_eq!(
                    PreferenceManager::shared()
                        .browser_list_columns()
                        .map(|c| c.mode),
                    Some(240)
                );

                reset_columns();
                list.sync_columns();

                assert!(is_column_visible(ListColumnId::Size));
                assert!(find_column(&list, ListColumnId::Size).is_some());
                assert_eq!(
                    column_order(&list),
                    crate::ui::preferences::default_list_column_order()
                );
                assert_eq!(
                    column_by_id(&list, ListColumnId::Mode).fixed_width(),
                    default_width(ListColumnId::Mode)
                );
                assert!(column_by_id(&list, ListColumnId::Name).expands());
                assert_eq!(PreferenceManager::shared().browser_list_columns(), None);

                let sorter = list
                    .view
                    .sorter()
                    .and_downcast::<gtk::ColumnViewSorter>()
                    .expect("column view sorter");
                assert_eq!(
                    sorter.primary_sort_column().and_then(|c| id_of(&c)),
                    Some(ListColumnId::Size)
                );
                assert_eq!(sorter.primary_sort_order(), gtk::SortType::Descending);
            },
        );
    }

    #[test]
    fn header_has_capture_right_click_controller() {
        crate::test_support::gtk_test(
            "interface::list::tests::header_has_capture_right_click_controller",
            || {
                let _prefs = PreferenceManager::shared();
                let model = EntryModel::new();
                model.set_entries(&[sample_entry()], false);
                let list = ListView::new(&model);
                let header = list
                    .view
                    .first_child()
                    .filter(|w| w.css_name() == "header")
                    .expect("header widget");
                let controllers = header.observe_controllers();
                let has_capture_secondary = (0..controllers.n_items()).any(|i| {
                    controllers
                        .item(i)
                        .and_downcast::<gtk::GestureClick>()
                        .is_some_and(|g| {
                            g.button() == gtk::gdk::BUTTON_SECONDARY
                                && g.propagation_phase() == gtk::PropagationPhase::Capture
                        })
                });
                assert!(has_capture_secondary);
            },
        );
    }

    #[test]
    fn columns_popover_toggles_and_resets_columns() {
        crate::test_support::gtk_test(
            "interface::list::tests::columns_popover_toggles_and_resets_columns",
            || {
                let _prefs = PreferenceManager::shared();
                let model = EntryModel::new();
                model.set_entries(&[sample_entry()], false);
                let list = ListView::new(&model);
                let window = gtk::Window::new();
                window.set_size_request(900, 600);
                window.set_child(Some(&list.root));
                window.present();
                for _ in 0..10 {
                    while glib::MainContext::default().iteration(false) {}
                }

                popup_columns(&list.view, 40.0, 12.0, Some(list.sync_fn.clone()));
                for _ in 0..5 {
                    while glib::MainContext::default().iteration(false) {}
                }

                let mut popover = None;
                let mut child = list.view.first_child();
                while let Some(c) = child {
                    if let Ok(p) = c.clone().downcast::<gtk::Popover>() {
                        popover = Some(p);
                    }
                    child = c.next_sibling();
                }
                let popover = popover.expect("columns popover attached to view");
                assert!(popover.is_visible());

                let menu_list = popover.child().and_downcast::<gtk::Box>().expect("menu list");
                let mut buttons = Vec::new();
                let mut row = menu_list.first_child();
                while let Some(r) = row {
                    if let Ok(b) = r.clone().downcast::<gtk::Button>() {
                        buttons.push(b);
                    }
                    row = r.next_sibling();
                }
                assert_eq!(buttons.len(), 5);

                // Toggle Mode and Size off while popover stays open
                buttons[0].emit_clicked();
                buttons[1].emit_clicked();
                for _ in 0..10 {
                    while glib::MainContext::default().iteration(false) {}
                }
                assert!(popover.is_visible());
                assert!(!buttons[0].has_css_class("checked"));
                assert!(!buttons[1].has_css_class("checked"));
                assert_eq!(
                    column_order(&list),
                    vec![ListColumnId::Name, ListColumnId::Type, ListColumnId::Modified]
                );
                assert_eq!(
                    PreferenceManager::shared().list_hidden_columns(),
                    vec![ListColumnId::Mode, ListColumnId::Size]
                );

                // Reset restores all columns and closes popover
                buttons[4].emit_clicked();
                for _ in 0..10 {
                    while glib::MainContext::default().iteration(false) {}
                }
                assert_eq!(
                    column_order(&list),
                    crate::ui::preferences::default_list_column_order()
                );
                assert!(PreferenceManager::shared().list_hidden_columns().is_empty());
            },
        );
    }
}
