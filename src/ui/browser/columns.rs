// SPDX-License-Identifier: MIT

use crate::model::{FileEntry, Location};
use crate::services::fold_for_search;
use crate::ui::browser::ViewState;
use crate::ui::browser::collection::{
    ViewMap, cancel_source,
    scroll_collection_when_allocated,
    };
use crate::ui::browser::find::{
    HitRanges, highlight_hit_name, highlight_listing_name, };
use crate::ui::browser::presentation::LoadPresentation;
use crate::ui::browser_modes::{ClickActivation, ClickCount};
use crate::ui::entry_list_model::EntryListModel;
use crate::ui::keys::{
    self,
    bindings::{DOWN, UP},
};
use crate::ui::motion::{animations_enabled, emphasized_deceleration};
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

pub(in crate::ui) const COLUMN_WIDTH: i32 = 300;











/// A plain press on a folder row, kept so the next press can complete a
/// double-click on it even after the strip slid another row under the pointer.
pub(super) struct FolderPress {
    pub(super) at: Instant,
    /// Window coordinates, which stay put while the strip scrolls.
    pub(super) point: (f64, f64),
    pub(super) depth: usize,
    pub(super) position: usize,
    pub(super) location: Location,
}

impl FolderPress {
}

#[derive(Clone)]
pub(super) struct BoundRow {
    pub(super) item: glib::WeakRef<gtk::ListItem>,
    pub(super) row: glib::WeakRef<gtk::Box>,
    pub(super) rename_label: glib::WeakRef<gtk::Label>,
    pub(super) edit: crate::ui::collection_edit::EditWidgets,
    pub(super) spacer: gtk::Box,
    pub(super) size: gtk::Label,
}




#[derive(Clone)]
pub(super) struct ColumnView {
    pub(super) shell: gtk::Box,
    pub(super) resize_handle: gtk::Box,
    pub(super) reveal_button: gtk::Button,
    pub(super) animation_generation: Rc<Cell<u64>>,
    /// The intended width in unscaled pixels: the saved default, then the last
    /// drag or autofit. A text-size change scales this, not the capped width.
    pub(super) unscaled_width: Rc<Cell<f64>>,
    pub(super) presentation: LoadPresentation,
    pub(super) model: EntryListModel,
    pub(super) filtered_model: gtk::FilterListModel,
    pub(super) map: ViewMap,
    pub(super) model_generation: Rc<Cell<u64>>,
    pub(super) header_actions: gtk::Box,
    pub(super) sort_direction_button: gtk::Button,
    pub(super) header_actions_stack: gtk::Stack,
    pub(super) filter_entry: gtk::Entry,
    pub(super) filter_button: gtk::ToggleButton,
    pub(super) selection: gtk::MultiSelection,
    pub(super) syncing_selection: Rc<Cell<bool>>,
    pub(super) list: gtk::ListView,
    pub(super) listing_scroll: gtk::ScrolledWindow,
    pub(super) marquee: crate::ui::marquee::Marquee,
    pub(super) bound_rows: Rc<RefCell<Vec<BoundRow>>>,
    /// Bumped by each cursor restore so an older pending restore cannot land late.
    pub(super) cursor_restore_generation: Rc<Cell<u64>>,
    pub(super) folder_context_trigger: Rc<dyn Fn(f64, f64)>,
    pub(super) item_context_trigger: Rc<dyn Fn(f64, f64)>,
    pub(super) entry_count: Rc<Cell<usize>>,
    pub(super) spinner: gtk::Spinner,
    pub(super) spinner_delay: Rc<RefCell<Option<glib::SourceId>>>,
    pub(super) truncated_hint: gtk::Image,
    pub(super) not_responding: gtk::Box,
    pub(super) empty_trash_button: Option<gtk::Button>,
    pub(super) leftovers_button: gtk::Button,
    pub(super) show_hidden: Rc<Cell<bool>>,
    pub(super) filter: gtk::CustomFilter,
    pub(super) search_results: Rc<RefCell<Vec<crate::services::SearchItem>>>,
    pub(super) search_session: crate::ui::search_session::SearchSession,
    query_binding: Rc<RefCell<Option<super::collection::FilterQueryBinding>>>,
    pub(super) search_model: gtk::StringList,
    pub(super) recursive_search_active: Rc<Cell<bool>>,
    hits: Rc<RefCell<ColumnHits>>,
}

#[derive(Default)]
pub(super) struct ColumnHits {
    pub(super) ranges: HitRanges,
    pub(super) recursive: bool,
}

impl ColumnView {

    /// Focuses the list, or the column surface while it shows an empty, error or
    /// loading page.
    pub(super) fn focus_surface(&self) -> bool {
        crate::ui::loading_skeleton::focus_surface_or(&self.presentation.stack, &self.list)
    }


    pub(super) fn refresh_name_highlights(&self, find: Option<&str>) {
        let searching = self.recursive_search_active.get();
        let results = self.search_results.borrow();
        let hits = self.hits.borrow();
        let filter = self.map.query();
        for bound in self.bound_rows.borrow().iter() {
            let (Some(label), Some(item)) = (bound.rename_label.upgrade(), bound.item.upgrade())
            else {
                continue;
            };
            if searching {
                let hit = results
                    .get(item.position() as usize)
                    .and_then(|hit| hits.ranges.get(&hit.path));
                highlight_hit_name(label.upcast_ref(), find, hit.map(Vec::as_slice));
            } else {
                highlight_listing_name(label.upcast_ref(), find, &filter);
            }
        }
    }



}











pub(super) fn scroll_column_to(column: &ColumnView, position: u32) {
    if position >= column.selection.n_items() {
        return;
    }
    scroll_collection_when_allocated(column.list.upcast_ref(), position);
}
















/// The standard column width at the current text size, the narrowest the
/// cap may make a column.
pub(super) fn column_floor() -> i32 {
    let scale = crate::ui::preferences::PreferenceManager::shared().interface_scale();
    (f64::from(COLUMN_WIDTH) * scale).round() as i32
}








impl ViewState {






    pub(super) fn focus_rebuilt_active_column(&self) {
        let Some(depth) = self.browser.active_depth() else {
            return;
        };
        let columns = self.columns.borrow();
        let Some(column) = columns.get(depth) else {
            return;
        };
        let position = self
            .browser
            .focused_item()
            .and_then(|(focused_depth, position, _)| {
                (focused_depth == depth)
                    .then(|| column.map.view_position(position))
                    .flatten()
            });
        if let Some(position) = position {
            scroll_column_to(column, position);
        }
        column.focus_surface();
        let column = column.clone();
        glib::idle_add_local_once(move || {
            // A Ctrl+F, popover or dialog opened since the rebuild keeps focus.
            let root = column.list.root();
            let focused = root.as_ref().and_then(|root| root.focus());
            if focused.is_some_and(|focused| {
                crate::ui::focus_navigation::editable(&focused)
                    || crate::ui::focus_navigation::in_popover(&focused)
            }) || root
                .and_downcast::<gtk::Window>()
                .is_some_and(|window| crate::ui::window::visible_modal_layer(&window).is_some())
            {
                return;
            }
            column.focus_surface();
        });
    }











}

pub(super) mod drag_scroll;
mod reveal;
mod rows;
mod search;



pub(super) use reveal::ColumnSpan;
