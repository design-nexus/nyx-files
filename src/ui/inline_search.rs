// SPDX-License-Identifier: MIT

//! Recursive filtering for single-pane presentations.
//!
//! Miller Columns already swaps recursive results into its existing row collection. Icons and
//! List share `ResultCollection`: one stable model, selection/navigation/context/rename surface,
//! and presentation-specific factories. The browser and file chooser supply activation,
//! selection-mode, and selection-change policy through `SearchCollectionOptions`.

use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::Rc,
};

use gtk::{glib, prelude::*};

use super::keys::{
    self,
    bindings::{DOWN, ENTER, UP},
};
use crate::{
    app::Browser,
    model::{FileEntry, Location},
    services::SearchItem,
};

mod collection;
mod presentation;
use collection::{
    CollectionBehavior, ResultCollection, ResultKind, collection_entry,
};

pub(super) type SearchSelectionChanged = Rc<dyn Fn(Vec<FileEntry>)>;
pub(super) type SearchSelectionHandlers = Rc<RefCell<Vec<SearchSelectionChanged>>>;



enum Publication {
    Results(Vec<SearchItem>, bool),
    Directory,
}

struct State {
    entry: glib::WeakRef<gtk::Entry>,
    stack: gtk::Stack,
    status: gtk::Label,
    collection: ResultCollection,
    session: super::search_session::SearchSession,
    query_binding: RefCell<Option<super::browser::FilterQueryBinding>>,
    updating: Cell<bool>,
    publishing: Cell<bool>,
    selection_dirty: Cell<bool>,
    published_selection: RefCell<Vec<FileEntry>>,
    pending_update: RefCell<Option<Publication>>,
    detached: Cell<bool>,
    recursive: Cell<bool>,
    context_menu_trigger: RefCell<Option<super::browser::ContextMenuTrigger>>,
    activate: Rc<dyn Fn(FileEntry)>,
    selection_callbacks: RefCell<Vec<SearchSelectionChanged>>,
    results_changed: Rc<dyn Fn()>,
}

impl State {

    fn selected_entries(&self) -> Vec<FileEntry> {
        self.collection
            .selected_positions()
            .into_iter()
            .filter_map(|position| collection_entry(&self.collection.sorted, position as u32))
            .collect()
    }


}

#[derive(Clone)]
pub(super) struct InlineSearch {
    pub widget: gtk::Widget,
    state: Option<Rc<State>>,
    marquee: Option<super::marquee::Marquee>,
}

impl InlineSearch {






    pub fn selected_entry(&self) -> Option<FileEntry> {
        let state = self.state.as_ref()?;
        if state.stack.visible_child_name().as_deref() != Some("search") {
            return None;
        }
        state
            .collection
            .current_position()
            .and_then(|position| collection_entry(&state.collection.sorted, position))
            .or_else(|| state.selected_entries().into_iter().next())
    }





    pub(in crate::ui) fn refresh_result_highlights(&self, find: Option<&str>) {
        if let Some(state) = self.state.as_ref() {
            state.collection.highlights.borrow_mut().find = find.map(str::to_owned);
            state.collection.refresh_highlights();
        }
    }














    fn showing_results(&self) -> Option<&Rc<State>> {
        self.state
            .as_ref()
            .filter(|state| state.stack.visible_child_name().as_deref() == Some("search"))
    }



    pub(in crate::ui) fn results(&self) -> Option<Vec<SearchItem>> {
        Some(self.showing_results()?.collection.items())
    }

    pub(in crate::ui) fn current_result(&self) -> Option<SearchItem> {
        let collection = &self.showing_results()?.collection;
        collection.item(collection.current_position()?)
    }


    pub(in crate::ui) fn results_view(&self) -> Option<gtk::Widget> {
        Some(self.showing_results()?.collection.view.clone())
    }

    pub(in crate::ui) fn hits(&self) -> Option<(gtk::MultiSelection, gtk::Widget, Option<u32>)> {
        let collection = &self.showing_results()?.collection;
        Some((
            collection.selection.clone(),
            collection.view.clone(),
            collection.current_position(),
        ))
    }


    /// Moves the result cursor `steps` rows; `usize::MAX` jumps to an end. The
    /// prompt keeps keyboard focus unless `take_focus`.
    pub(in crate::ui) fn step(&self, direction: i32, steps: usize, take_focus: bool) -> bool {
        let Some(state) = self.showing_results() else {
            return false;
        };
        let collection = &state.collection;
        let current = if steps == 0 {
            super::browser::selected_cursor(&collection.selection)
        } else {
            collection.current_position()
        };
        let Some(target) = super::browser::results_step_target(
            current,
            collection.selection.n_items(),
            direction,
            steps,
        ) else {
            return true;
        };
        if take_focus {
            collection.focus(target, true);
        } else {
            collection.selection.select_item(target, true);
            super::browser::scroll_results_to(&collection.view, target, gtk::ListScrollFlags::NONE);
        }
        true
    }
}





