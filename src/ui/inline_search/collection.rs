// SPDX-License-Identifier: MIT

//! Stable identity and displayed-order lookup. The rank map is only a sorter input; all
//! behavior resolves entries through the sorted GTK model, never a parallel result vector.
use crate::{
    model::{FileEntry, Location},
    services::SearchItem,
    ui::browser::find::{HitRanges, highlight_hit_name},
};
use gtk::{gio, glib, prelude::*};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    rc::Rc,
};

#[derive(Clone)]
pub(super) struct CollectionBehavior {
    pub(super) multiple_selection: Rc<Cell<bool>>,
    pub(super) activate: Rc<dyn Fn(FileEntry)>,
    pub(super) single_click: Rc<dyn Fn(FileEntry)>,
    pub(super) focus_items: Rc<dyn Fn()>,
    pub(super) focus_entry: Rc<dyn Fn() -> bool>,
}


#[derive(Clone)]
pub(super) enum ResultKind {
    Rows,
    Icons { thumbnail_size: Rc<Cell<i32>> },
}

#[derive(Default)]
pub(super) struct NameHighlights {
    pub(super) find: Option<String>,
    pub(super) hits: HitRanges,
}

impl NameHighlights {
    pub(super) fn apply(&self, label: &gtk::Widget, path: &Path) {
        highlight_hit_name(
            label,
            self.find.as_deref(),
            self.hits.get(path).map(Vec::as_slice),
        );
    }
}

pub(super) struct BoundResult {
    pub(super) item: glib::WeakRef<gtk::ListItem>,
    pub(super) widget: glib::WeakRef<gtk::Widget>,
    pub(super) rename_label: glib::WeakRef<gtk::Widget>,
    pub(super) presentation: crate::ui::inline_search::presentation::ResultWidgets,
}

pub(super) struct ResultCollection {
    pub(super) view: gtk::Widget,
    pub(super) model: gio::ListStore,
    pub(super) sorted: gtk::SortListModel,
    pub(super) sorter: gtk::CustomSorter,
    pub(super) positions: Rc<RefCell<HashMap<PathBuf, usize>>>,
    pub(super) selection: gtk::MultiSelection,
    pub(super) bound: Rc<RefCell<Vec<BoundResult>>>,
    pub(super) anchor: Rc<Cell<Option<u32>>>,
    // GTK selects the pointer target before item gestures run; retain the intended
    // modified-click group so drag and context-menu focus can preserve it.
    pub(super) gesture_selection: Rc<RefCell<Option<gtk::Bitset>>>,
    pub(super) kind: ResultKind,
    pub(super) root: PathBuf,
    pub(super) reconciling: Rc<Cell<bool>>,
    pub(super) highlights: Rc<RefCell<NameHighlights>>,
    pub(super) behavior: CollectionBehavior,
}

impl ResultCollection {
    pub(super) fn item(&self, position: u32) -> Option<SearchItem> {
        let object = self
            .sorted
            .item(position)?
            .downcast::<glib::BoxedAnyObject>()
            .ok()?;
        Some(object.borrow::<SearchItem>().clone())
    }

    pub(super) fn items(&self) -> Vec<SearchItem> {
        (0..self.sorted.n_items())
            .filter_map(|position| self.item(position))
            .collect()
    }

    pub(super) fn position_at(&self, picked: &gtk::Widget) -> Option<u32> {
        self.bound
            .borrow_mut()
            .retain(|bound| bound.item.upgrade().is_some() && bound.widget.upgrade().is_some());
        self.bound.borrow().iter().find_map(|bound| {
            let item = bound.item.upgrade()?;
            let widget = bound.widget.upgrade()?;
            (picked == &widget
                || picked.is_ancestor(&widget)
                || widget.parent().as_ref() == Some(picked))
            .then(|| item.position())
        })
    }

    pub(super) fn current_position(&self) -> Option<u32> {
        self.view
            .root()
            .and_then(|root| root.focus())
            .and_then(|focus| self.position_at(&focus))
            .or_else(|| {
                (0..self.selection.n_items()).find(|position| self.selection.is_selected(*position))
            })
    }

    pub(super) fn selected_positions(&self) -> Vec<usize> {
        (0..self.selection.n_items())
            .filter(|position| self.selection.is_selected(*position))
            .map(|position| position as usize)
            .collect()
    }


    pub(super) fn focus(&self, position: u32, exclusive: bool) -> bool {
        if position >= self.selection.n_items() {
            return false;
        }
        if exclusive {
            self.gesture_selection.borrow_mut().take();
        }
        self.selection.select_item(position, exclusive);
        self.gesture_selection
            .replace(Some(self.selection.selection().copy()));
        self.restore_focus(position);
        true
    }

    pub(super) fn restore_focus(&self, position: u32) {
        if let Some(window) = self.view.root().and_downcast::<gtk::Window>() {
            window.set_focus_visible(true);
        }
        self.view.grab_focus();
        if let Some(list) = self.view.downcast_ref::<gtk::ListView>() {
            list.scroll_to(position, gtk::ListScrollFlags::FOCUS, None);
        } else if let Some(grid) = self.view.downcast_ref::<gtk::GridView>() {
            grid.scroll_to(position, gtk::ListScrollFlags::FOCUS, None);
        }
    }






    pub(super) fn refresh_highlights(&self) {
        let highlights = self.highlights.borrow();
        for bound in self.bound.borrow().iter() {
            let (Some(item), Some(label)) = (bound.item.upgrade(), bound.rename_label.upgrade())
            else {
                continue;
            };
            if let Some(result) = self.item(item.position()) {
                highlights.apply(&label, &result.path);
            }
        }
    }



}

pub(super) fn collection_entry(
    model: &impl IsA<gio::ListModel>,
    position: u32,
) -> Option<FileEntry> {
    let object = model
        .item(position)?
        .downcast::<glib::BoxedAnyObject>()
        .ok()?;
    Some(crate::ui::browser::search_result_entry(
        &object.borrow::<SearchItem>(),
    ))
}


