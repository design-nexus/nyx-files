// SPDX-License-Identifier: MIT

//! 10xer footer filter. It drives the focused pane's own filter field without
//! revealing the funnel, so that field's name rules, **Include subfolders**
//! scope, and stale-work cancellation stay the filter's rules.

use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::{Rc, Weak},
};

use gtk::prelude::*;

use super::{BrowserView, ViewState, columns::ColumnView};
use crate::{
    app::VisualKind,
    services::SearchItem,
    ui::{browser_modes::BrowserMode, inline_search::InlineSearch},
};

#[derive(Default)]
pub(super) struct FilterState {
    owner: RefCell<Weak<ViewState>>,
    handlers: RefCell<Vec<Rc<dyn Fn()>>>,
    /// A commit whose results were still loading; their arrival focuses the
    /// first result.
    focus_on_arrival: Cell<bool>,
    notify_scheduled: Cell<bool>,
    /// Whether a footer prompt that filters this browser, 10xer's **f** or **s**, has focus.
    footer_focus: RefCell<Option<Rc<dyn Fn() -> bool>>>,
    /// The search query and the hit the cursor last rested on in its results.
    cursor_hit: RefCell<Option<(String, PathBuf)>>,
}

impl FilterState {
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui) struct FilterStatus {
    pub query: String,
    pub search: bool,
    pub current: Option<PathBuf>,
    /// Whether results replace the listing. A typed query has not switched the
    /// view until its debounce fires, a search has nothing to count until its
    /// first batch, and a column on a non-local location narrows its own rows
    /// in place; the counts below describe none of these.
    pub displayed: bool,
    pub files: usize,
    pub folders: usize,
    pub visual: Option<VisualKind>,
}


impl FilterStatus {
    pub(in crate::ui) fn total(&self) -> usize {
        self.files + self.folders
    }
}

/// The result `steps` rows from `current`: zero stays (on the first result
/// without a cursor) and `usize::MAX` jumps to an end.
pub(in crate::ui) fn results_step_target(
    current: Option<u32>,
    count: u32,
    direction: i32,
    steps: usize,
) -> Option<u32> {
    let last = count.checked_sub(1)?;
    if steps == 0 {
        return Some(current.map_or(0, |current| current.min(last)));
    }
    if steps == usize::MAX {
        return Some(if direction < 0 { 0 } else { last });
    }
    let steps = u32::try_from(steps).unwrap_or(u32::MAX);
    Some(match current {
        Some(current) if direction < 0 => current.saturating_sub(steps),
        Some(current) => current.saturating_add(steps).min(last),
        None if direction < 0 => last,
        None => 0,
    })
}

pub(in crate::ui) fn scroll_results_to(
    view: &gtk::Widget,
    position: u32,
    flags: gtk::ListScrollFlags,
) {
    if let Some(list) = view.downcast_ref::<gtk::ListView>() {
        list.scroll_to(position, flags, None);
    } else if let Some(grid) = view.downcast_ref::<gtk::GridView>() {
        grid.scroll_to(position, flags, None);
    }
}

pub(super) struct Hits {
    pub(super) selection: gtk::MultiSelection,
    pub(super) view: gtk::Widget,
    pub(super) cursor: Option<u32>,
}

impl Hits {
    pub(super) fn count(&self) -> u32 {
        self.selection.n_items()
    }
}

pub(super) enum Target {
    Column(Box<ColumnView>),
    Pane {
        entry: gtk::Entry,
        button: gtk::ToggleButton,
        search: InlineSearch,
    },
}

impl Target {
    pub(super) fn entry(&self) -> &gtk::Entry {
        match self {
            Self::Column(column) => &column.filter_entry,
            Self::Pane { entry, .. } => entry,
        }
    }









    pub(super) fn results_view(&self) -> Option<gtk::Widget> {
        match self {
            Self::Column(column) => column
                .recursive_search_active
                .get()
                .then(|| column.list.clone().upcast()),
            Self::Pane { search, .. } => search.results_view(),
        }
    }


    pub(super) fn hits(&self) -> Option<Hits> {
        match self {
            Self::Column(column) => column.recursive_search_active.get().then(|| Hits {
                selection: column.selection.clone(),
                view: column.list.clone().upcast(),
                cursor: column_cursor(column),
            }),
            Self::Pane { search, .. } => search.hits().map(|(selection, view, cursor)| Hits {
                selection,
                view,
                cursor,
            }),
        }
    }

    pub(super) fn results(&self) -> Option<Vec<SearchItem>> {
        match self {
            Self::Column(column) => column
                .recursive_search_active
                .get()
                .then(|| column.search_results.borrow().clone()),
            Self::Pane { search, .. } => search.results(),
        }
    }

    pub(super) fn current_result(&self) -> Option<SearchItem> {
        match self {
            Self::Column(column) => {
                if !column.recursive_search_active.get() {
                    return None;
                }
                let position = column_cursor(column)?;
                column
                    .search_results
                    .borrow()
                    .get(position as usize)
                    .cloned()
            }
            Self::Pane { search, .. } => search.current_result(),
        }
    }

    pub(super) fn step(&self, direction: i32, steps: usize, take_focus: bool) -> bool {
        match self {
            Self::Column(column) => step_column_results(column, direction, steps, take_focus),
            Self::Pane { search, .. } => search.step(direction, steps, take_focus),
        }
    }

}


pub(super) fn column_cursor(column: &ColumnView) -> Option<u32> {
    let focused = column.list.root().and_then(|root| root.focus());
    focused
        .and_then(|focused| {
            column.bound_rows.borrow().iter().find_map(|bound| {
                let row = bound.row.upgrade()?;
                let row: &gtk::Widget = row.upcast_ref();
                // GTK focuses the list item widget that holds the row.
                (focused == *row
                    || focused.is_ancestor(row)
                    || row.parent().as_ref() == Some(&focused))
                .then(|| bound.item.upgrade().map(|item| item.position()))
                .flatten()
            })
        })
        .or_else(|| {
            let selected = column.selection.selection();
            (!selected.is_empty()).then(|| selected.maximum())
        })
}

/// Outside a fill the selection is the cursor; a row GTK kept focused may
/// belong to a previous query.
pub(in crate::ui) fn selected_cursor(selection: &gtk::MultiSelection) -> Option<u32> {
    let selected = selection.selection();
    (!selected.is_empty()).then(|| selected.maximum())
}

fn step_column_results(
    column: &ColumnView,
    direction: i32,
    steps: usize,
    take_focus: bool,
) -> bool {
    if !column.recursive_search_active.get() {
        return false;
    }
    let current = if steps == 0 {
        selected_cursor(&column.selection)
    } else {
        column_cursor(column)
    };
    let Some(target) = results_step_target(current, column.selection.n_items(), direction, steps)
    else {
        return true;
    };
    column.selection.select_item(target, true);
    let flags = if take_focus {
        column.list.grab_focus();
        gtk::ListScrollFlags::FOCUS
    } else {
        gtk::ListScrollFlags::NONE
    };
    column.list.scroll_to(target, flags, None);
    true
}





impl ViewState {
    pub(super) fn filter_target(&self) -> Option<Target> {
        self.try_filter_target().flatten()
    }

    /// `None` while a rebuild holds the views, which browser events can
    /// interrupt.
    fn try_filter_target(&self) -> Option<Option<Target>> {
        if self.mode.get() == BrowserMode::Columns {
            let columns = self.columns.try_borrow().ok()?;
            return Some(self.filter_depth().and_then(|depth| {
                columns
                    .get(depth)
                    .cloned()
                    .map(|column| Target::Column(Box::new(column)))
            }));
        }
        let panes = self.mode_views.try_borrow().ok()?;
        Some(
            panes
                .active_filter()
                .map(|(entry, button, search)| Target::Pane {
                    entry,
                    button,
                    search,
                }),
        )
    }





    fn filter_depth(&self) -> Option<usize> {
        if self.mode.get() == BrowserMode::Columns {
            self.focused_column_depth()
                .or_else(|| self.browser.active_depth())
        } else {
            self.browser.active_depth()
        }
    }
}

impl BrowserView {
    pub(super) fn filter_target(&self) -> Option<Target> {
        self.state.filter_target()
    }













    pub(in crate::ui) fn results_replace_listing(&self) -> bool {
        self.filter_target()
            .is_some_and(|target| target.results_view().is_some())
    }


    /// Moves among filter results instead of the hidden directory cursor.
    /// Returns whether results are showing.
    pub(in crate::ui) fn step_filter_results(
        &self,
        direction: i32,
        steps: usize,
        take_focus: bool,
    ) -> bool {
        if take_focus && self.step_filled_results(direction, steps) {
            return true;
        }
        self.filter_target()
            .is_some_and(|target| target.step(direction, steps, take_focus))
    }


    #[cfg(test)]
    pub(in crate::ui) fn filter_result_names(&self) -> Vec<String> {
        self.filter_target()
            .and_then(|target| target.results())
            .unwrap_or_default()
            .into_iter()
            .map(|item| item.name)
            .collect()
    }

    #[cfg(test)]
    pub(in crate::ui) fn extend_result_selection(&self, position: u32) -> bool {
        self.filter_target()
            .and_then(|target| target.hits())
            .is_some_and(|hits| hits.selection.select_item(position, false))
    }


}

impl Target {
}

impl ViewState {

}
