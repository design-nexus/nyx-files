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
    item.downcast_ref::<glib::BoxedAnyObject>()
        .map(glib::BoxedAnyObject::borrow::<Row>)
}

type BindHook = RefCell<Option<Box<dyn Fn(usize, &FileEntry)>>>;
type HoverHook = RefCell<Option<Box<dyn Fn(Option<u32>, &gtk::Widget)>>>;
type ContextHook = RefCell<Option<Box<dyn Fn(Option<u32>, &gtk::Widget, f64, f64)>>>;
type DragHook = RefCell<Option<Box<dyn Fn(u32) -> Vec<FileEntry>>>>;
type DropHook = RefCell<
    Option<
        Box<dyn Fn(Option<FileEntry>, Vec<crate::model::Location>, gtk::gdk::DragAction) -> bool>,
    >,
>;

#[derive(Default)]
pub(super) struct Hooks {
    pub(super) bind: BindHook,
    pub(super) context: ContextHook,
    /// The entries a drag starting on row `index` carries.
    pub(super) drag: DragHook,
    /// Files dropped on a row (`Some`, when it is a folder) or on the view.
    pub(super) drop: DropHook,
    /// The pointer entered row `Some(index)`, or left it (`None`).
    pub(super) hover: HoverHook,
    /// A plain single click on row `index`.
    pub(super) click: RefCell<Option<Box<dyn Fn(u32)>>>,
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
    /// The store, in groups by file type when grouping is on.
    sorted: gtk::SortListModel,
    pub(super) selection: gtk::MultiSelection,
    pub(super) hooks: Rc<Hooks>,
}

/// Folders first, then the other types alphabetically, "Other" last.
fn type_rank(entry: &FileEntry) -> (u8, String) {
    use crate::services::EntryType;
    match crate::services::entry_type(entry) {
        EntryType::Folder => (0, String::new()),
        EntryType::Other => (2, String::new()),
        other => (1, other.description().to_lowercase()),
    }
}

/// The heading for an entry's group.
pub(super) fn type_group(entry: &FileEntry) -> String {
    crate::i18n::tr(crate::services::entry_type(entry).description())
}

impl EntryModel {
    pub(super) fn new() -> Self {
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let sorted = gtk::SortListModel::new(Some(store.clone()), None::<gtk::Sorter>);
        let selection = gtk::MultiSelection::new(Some(sorted.clone()));
        Self {
            store,
            sorted,
            selection,
            hooks: Rc::default(),
        }
    }

    /// Groups rows by file type (as sections), keeping the order within each.
    pub(super) fn set_grouped(&self, grouped: bool) {
        if grouped == self.sorted.section_sorter().is_some() {
            return;
        }
        let selected = self.selected_locations();
        self.sorted.set_section_sorter(
            grouped
                .then(|| {
                    gtk::CustomSorter::new(|a, b| {
                        let (Some(a), Some(b)) = (row_of(a), row_of(b)) else {
                            return gtk::Ordering::Equal;
                        };
                        type_rank(&a.entry).cmp(&type_rank(&b.entry)).into()
                    })
                })
                .as_ref(),
        );
        self.reselect(&selected);
    }

    fn selected_locations(&self) -> Vec<crate::model::Location> {
        self.selected().into_iter().map(|e| e.location).collect()
    }

    fn reselect(&self, locations: &[crate::model::Location]) {
        let bits = gtk::Bitset::new_empty();
        if !locations.is_empty() {
            for i in 0..self.len() {
                if self
                    .entry(i)
                    .is_some_and(|e| locations.contains(&e.location))
                {
                    bits.add(i);
                }
            }
        }
        let current = self.selection.selection();
        let same = bits.size() == current.size()
            && (0..bits.size()).all(|i| bits.nth(i as u32) == current.nth(i as u32));
        if !same {
            self.selection
                .set_selection(&bits, &gtk::Bitset::new_range(0, self.len()));
        }
    }

    /// Shows `entries`. When the same items are showing in the same order,
    /// only rows whose details changed are refreshed and the selection stays.
    pub(super) fn set_entries(&self, entries: &[FileEntry], show_hidden: bool) {
        let shown: Vec<(usize, &FileEntry)> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| show_hidden || !e.is_hidden)
            .collect();
        let selected = self.selected_locations();
        let same_items = shown.len() as u32 == self.store.n_items()
            && shown.iter().enumerate().all(|(i, (_, entry))| {
                self.store.item(i as u32).is_some_and(|item| {
                    row_of(&item).is_some_and(|r| r.entry.location == entry.location)
                })
            });
        if same_items {
            for (i, (position, entry)) in shown.iter().enumerate() {
                let Some(item) = self
                    .store
                    .item(i as u32)
                    .and_downcast::<glib::BoxedAnyObject>()
                else {
                    continue;
                };
                let changed = {
                    let row = item.borrow::<Row>();
                    row.position != *position || row.entry != **entry
                };
                if changed {
                    let fresh = glib::BoxedAnyObject::new(Row {
                        position: *position,
                        entry: (*entry).clone(),
                    });
                    self.store.splice(i as u32, 1, &[fresh]);
                }
            }
        } else {
            let items: Vec<glib::BoxedAnyObject> = shown
                .iter()
                .map(|(position, entry)| {
                    glib::BoxedAnyObject::new(Row {
                        position: *position,
                        entry: (*entry).clone(),
                    })
                })
                .collect();
            self.store.splice(0, self.store.n_items(), &items);
        }
        self.reselect(&selected);
    }

    pub(super) fn len(&self) -> u32 {
        self.sorted.n_items()
    }

    pub(super) fn entry(&self, index: u32) -> Option<FileEntry> {
        let item = self.sorted.item(index)?;
        row_of(&item).map(|r| r.entry.clone())
    }

    /// The browser column positions of the selected rows, and the rows' indexes.
    pub(super) fn selected_positions(&self) -> Vec<(u32, usize)> {
        let bits = self.selection.selection();
        (0..bits.size())
            .filter_map(|i| {
                let index = bits.nth(i as u32);
                let item = self.sorted.item(index)?;
                row_of(&item).map(|r| (index, r.position))
            })
            .collect()
    }

    pub(super) fn selected(&self) -> Vec<FileEntry> {
        let bits = self.selection.selection();
        (0..bits.size())
            .filter_map(|i| self.entry(bits.nth(i as u32)))
            .collect()
    }

    /// Selects the rows at these browser column positions.
    pub(super) fn select_positions(&self, positions: &[usize]) {
        let bits = gtk::Bitset::new_empty();
        for i in 0..self.len() {
            if let Some(item) = self.sorted.item(i)
                && row_of(&item).is_some_and(|r| positions.contains(&r.position))
            {
                bits.add(i);
            }
        }
        if !bits.is_empty() {
            self.selection
                .set_selection(&bits, &gtk::Bitset::new_range(0, self.len()));
        }
    }

    /// Selects `index` alone unless it is already part of the selection.
    pub(super) fn select_for_menu(&self, index: u32) {
        if !self.selection.is_selected(index) {
            self.selection.select_item(index, true);
        }
    }

    pub(super) fn select_location(&self, location: &crate::model::Location) -> Option<u32> {
        (0..self.len())
            .find(|&i| self.entry(i).is_some_and(|e| &e.location == location))
            .inspect(|&i| {
                self.selection.select_item(i, true);
            })
    }
}

/// Right-click on `widget` reports the row at `position` (or none, for the background).
pub(super) fn on_context(
    widget: &impl IsA<gtk::Widget>,
    hooks: &Rc<Hooks>,
    position: impl Fn() -> Option<u32> + 'static,
) {
    let hooks = hooks.clone();
    nyx_ui::menu::on_right_click(widget, move |w, x, y| hooks.context(position(), w, x, y));
}

/// Makes a row draggable and, when it is a folder, a drop target.
pub(super) fn dnd_on(child: &impl IsA<gtk::Widget>, item: &gtk::ListItem, hooks: &Rc<Hooks>) {
    let click = gtk::GestureClick::new();
    click.set_button(gtk::gdk::BUTTON_PRIMARY);
    let (weak, h) = (item.downgrade(), hooks.clone());
    click.connect_released(move |gesture, presses, _, _| {
        let plain = !gesture
            .current_event_state()
            .intersects(gtk::gdk::ModifierType::CONTROL_MASK | gtk::gdk::ModifierType::SHIFT_MASK);
        if presses == 1
            && plain
            && let Some(item) = weak.upgrade()
            && let Some(f) = h.click.borrow().as_ref()
        {
            f(item.position());
        }
    });
    child.add_controller(click);
    let motion = gtk::EventControllerMotion::new();
    let (weak, h) = (item.downgrade(), hooks.clone());
    motion.connect_enter(move |m, _, _| {
        if let (Some(item), Some(widget)) = (weak.upgrade(), m.widget())
            && let Some(f) = h.hover.borrow().as_ref()
        {
            f(Some(item.position()), &widget);
        }
    });
    let h = hooks.clone();
    motion.connect_leave(move |m| {
        if let (Some(widget), Some(f)) = (m.widget(), h.hover.borrow().as_ref()) {
            f(None, &widget);
        }
    });
    child.add_controller(motion);
    let source = gtk::DragSource::new();
    source.set_actions(gtk::gdk::DragAction::COPY | gtk::gdk::DragAction::MOVE);
    let (weak, h) = (item.downgrade(), hooks.clone());
    source.connect_prepare(move |_, _, _| {
        let index = weak.upgrade()?.position();
        let entries = h
            .drag
            .borrow()
            .as_ref()
            .map(|f| f(index))
            .unwrap_or_default();
        super::actions::file_content(&entries)
    });
    child.add_controller(source);
    let target = gtk::DropTarget::new(
        gtk::gdk::FileList::static_type(),
        gtk::gdk::DragAction::COPY | gtk::gdk::DragAction::MOVE,
    );
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
        let Some(folder) = weak
            .upgrade()
            .and_then(|i| i.item())
            .and_then(|o| row_of(&o).map(|r| r.entry.clone()))
        else {
            return false;
        };
        let Some(sources) = super::actions::dropped(value) else {
            return false;
        };
        h.drop
            .borrow()
            .as_ref()
            .is_some_and(|f| f(Some(folder), sources, super::actions::forced_action(t)))
    });
    child.add_controller(target);
}

/// Makes a view's empty space take drops into the folder showing.
pub(super) fn drop_on_view(view: &impl IsA<gtk::Widget>, hooks: &Rc<Hooks>) {
    let target = gtk::DropTarget::new(
        gtk::gdk::FileList::static_type(),
        gtk::gdk::DragAction::COPY | gtk::gdk::DragAction::MOVE,
    );
    target.connect_enter(|t, _, _| super::actions::drop_action(t));
    target.connect_motion(|t, _, _| super::actions::drop_action(t));
    let h = hooks.clone();
    target.connect_drop(move |t, value, _, _| {
        let Some(sources) = super::actions::dropped(value) else {
            return false;
        };
        h.drop
            .borrow()
            .as_ref()
            .is_some_and(|f| f(None, sources, super::actions::forced_action(t)))
    });
    view.add_controller(target);
}

/// An icon slot for a row: the thumbnail when there is one, else the item's icon.
pub(super) fn icon_slot(size: i32) -> crate::ui::thumbnail::ThumbnailSlot {
    let slot = crate::ui::thumbnail::ThumbnailSlot::new(size);
    slot.add_css_class("entry-icon");
    slot
}

pub(super) fn show_icon(slot: &crate::ui::thumbnail::ThumbnailSlot, entry: &FileEntry, size: i32) {
    let icon = crate::ui::thumbnail::theme_icon(
        &super::icons_theme::name_for(entry),
        crate::ui::browser::entry::entry_icon(entry),
    );
    crate::ui::thumbnail::set_thumbnail_or_icon(slot, entry, &icon, size, size);
    slot.set_hidden(entry.is_hidden);
}

/// Stops a recycled row's pending thumbnail.
pub(super) fn cancel_on_unbind(factory: &gtk::SignalListItemFactory) {
    factory.connect_unbind(|_, item| {
        crate::ui::thumbnail::cancel_list_item_thumbnails(item.upcast_ref())
    });
}
