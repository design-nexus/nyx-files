// SPDX-License-Identifier: MIT

//! Browser composition and public commands. Feature modules share this view's state and the
//! application controller; they must not create independent navigation or operation state.

use crate::app::{Browser, FreshnessInput, Presence, VisibleDepths};
use crate::model::{FileEntry, Location};
use crate::services::{FileSource, LoadHandle, LocationValidationError, OperationProvider};
use crate::ui::browser::collection::cancel_source;
pub(super) use crate::ui::browser::columns::COLUMN_WIDTH;
use crate::ui::browser::columns::ColumnView;
use crate::ui::browser::inline_edit::{PendingEntryRename, PendingRename};
use crate::ui::browser::location::{
    MountCredentials, PendingConnect, UnlockProgressSlot, };
use crate::ui::browser::paths::{is_trash_location};
use crate::ui::browser::peek::{PeekAnchor, PeekView};
use crate::ui::browser::progress::{BackgroundProgress, FileProgressState};
use crate::ui::browser::transfer::FinishedSendToCompletion;
use crate::ui::browser::transfer::PendingSendToCompletion;
use crate::ui::browser::trash::TrashLoadingView;
use crate::ui::browser_modes::{BrowserDensity, BrowserMode, ClickActivation, ModeViews};
use crate::ui::keys;
use gtk::glib;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};
use std::time::Duration;


pub(in crate::ui) type PlaybackHandoff = Rc<dyn Fn(&Location) -> Option<Duration>>;

mod archive;
pub(super) mod camera_scroll;
mod clipboard;
mod collection;
mod columns;
pub(super) mod context_menu;
pub(crate) mod customization;
pub(crate) mod desktop;
mod destination;
mod dissolve_delete;
pub(crate) mod entry;
mod entry_animation;
mod events;
mod file_commands;
pub(in crate::ui) mod find;
pub(super) mod fly_to_trash;
mod inline_edit;
pub(crate) mod leftovers;
mod listing_filter;
mod listing_search;
mod location;
mod pane_header;
pub(in crate::ui) mod paths;
mod peek;
mod preferences;
mod presentation;
mod preview;
pub(crate) mod progress;
pub(crate) mod properties;
mod result_selection;
mod tab_location;
mod tab_stops;
mod transfer;
pub(crate) mod transfer_report;
mod trash;

pub(in crate::ui) use crate::ui::browser::clipboard::drag_preview_icon;
pub(super) use crate::ui::browser::clipboard::{
    ClipboardMark, ClipboardMarks, file_drag_content, };
pub(crate) use crate::ui::browser::clipboard::{
    clear_active_drag,
    };
pub(crate) use crate::ui::browser::collection::{
    scroll_collection_when_allocated, search_result_entry,
};
pub(in crate::ui) use crate::ui::browser::collection::{FilterQueryBinding};
pub(super) use crate::ui::browser::context_menu::{
    ContextMenuTrigger, };
pub(super) use crate::ui::browser::desktop::{launch_terminal, open_location_at};
#[cfg(test)]
pub(in crate::ui) use crate::ui::browser::entry::catppuccin;
pub(super) use crate::ui::browser::entry::{
    entry_icon, format_file_size, icon_for_name, metadata_needs_fill, };
pub(in crate::ui) use crate::ui::browser::listing_filter::{
    FilterStatus, results_step_target, scroll_results_to, selected_cursor,
};
pub(in crate::ui) use crate::ui::browser::location::ConnectOutcome;
pub use crate::ui::browser::peek::PeekBehavior;
pub use crate::ui::browser::properties::format_permissions;
pub(super) use crate::ui::modal::{
    animate_in, animate_out, dismiss_modal_layer, modal_layer, show_error_dialog, slide_out,
};

type PinHandler = Rc<dyn Fn(Location, String)>;
type UnpinHandler = Rc<dyn Fn(&Location)>;
type PinStatusHandler = Rc<dyn Fn(&Location) -> PinStatus>;
type PrintHandler = Rc<dyn Fn(FileEntry)>;
type PreparingTransfer = (
    transfer::Preparing,
    gio::Cancellable,
    Weak<FileProgressState>,
);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PinStatus {
    Available,
    Pinned,
    Unavailable,
}

#[derive(Default)]
struct GlobalActivityState {
    next_id: u64,
    active: Vec<(u64, String)>,
}

impl GlobalActivityState {
    fn begin(&mut self, label: impl Into<String>) -> u64 {
        self.next_id = self.next_id.saturating_add(1);
        self.active.push((self.next_id, label.into()));
        self.next_id
    }

    fn finish(&mut self, id: u64) {
        self.active.retain(|(active_id, _)| *active_id != id);
    }

    fn current_label(&self) -> Option<&str> {
        self.active.last().map(|(_, label)| label.as_str())
    }
}

pub struct GlobalActivity {
    state: std::rc::Weak<ViewState>,
    id: u64,
}

impl Drop for GlobalActivity {
    fn drop(&mut self) {
        if let Some(state) = self.state.upgrade() {
            state.finish_global_activity(self.id);
        }
    }
}



pub(super) struct ViewState {
    overlay: gtk::Overlay,
    location_control: gtk::Box,
    location_stack: gtk::Stack,
    global_activity_spinner: gtk::Spinner,
    global_activity: RefCell<GlobalActivityState>,
    breadcrumbs: gtk::Box,
    breadcrumb_scroller: gtk::ScrolledWindow,
    location_entry: gtk::Entry,
    path_completion: Rc<location::completion::PathCompletion>,
    columns_widget: gtk::Box,
    scroller: gtk::ScrolledWindow,
    mode_views: RefCell<ModeViews>,
    mode: Cell<BrowserMode>,
    columns: RefCell<Vec<ColumnView>>,
    hovered_column: Cell<Option<usize>>,
    // The preview drawer holds the keys, so no column is the keyboard destination.
    preview_owns_keys: Cell<bool>,
    playback_handoff: RefCell<Option<PlaybackHandoff>>,
    context_menu_column: Cell<Option<usize>>,
    context_menu_generation: Cell<u64>,
    context_menu_focus: RefCell<Option<glib::WeakRef<gtk::Widget>>>,
    input_ownership: RefCell<super::input_ownership::InputOwnership>,
    /// True only while a column row gesture is writing the selection model.
    /// Focus echoes of the cursor are not pointer-owned.
    pointer_owns_selection: Cell<bool>,
    folder_press: RefCell<Option<columns::FolderPress>>,
    /// The depth whose column a sibling swap just removed, so its replacement fades in.
    swap_slot: Cell<Option<usize>>,
    tab_location: RefCell<tab_location::TabLocation>,
    column_resizing: Cell<bool>,
    /// Visible browser area published by the preview layout; bounds Miller column widths.
    column_width_limit: Cell<Option<i32>>,
    /// Set until the first limit, and after a text-size change, so the next
    /// published limit narrows wider columns once.
    clamp_to_next_column_limit: Cell<bool>,
    horizontal_scroll_generation: Rc<Cell<u64>>,
    suppress_focus_scroll: Cell<bool>,
    /// Set while a footer prompt moves the cursor; the prompt keeps the keys.
    cursor_keeps_focus: Cell<bool>,
    tab_crossing: Cell<tab_stops::TabCrossing>,
    pending_mirror: RefCell<Option<glib::SourceId>>,
    source_generation: Rc<Cell<u64>>,
    peek: RefCell<Option<PeekView>>,
    pending_peek: RefCell<Option<glib::SourceId>>,
    pending_close: RefCell<Option<glib::SourceId>>,
    peek_anchor: RefCell<Option<PeekAnchor>>,
    peek_behavior: PeekBehavior,
    peek_enabled: Cell<bool>,
    single_click_previews: Cell<bool>,
    columns_mirror_selection: Cell<bool>,
    multiple_selection: Rc<Cell<bool>>,
    interactive: bool,
    chooser_allows_create: Cell<bool>,
    columns_click_activation: Cell<ClickActivation>,
    active_rename: crate::ui::collection_edit::ActiveEdits,
    pending_rename: RefCell<Option<PendingRename>>,
    rename_generation: Cell<u64>,
    rename_reveal_generation: Cell<u64>,
    keyboard_refocus: Cell<Option<file_commands::KeyboardRefocus>>,
    pending_click_rename: RefCell<Option<glib::SourceId>>,
    click_rename_generation: Cell<u64>,
    pending_new_entry: RefCell<Option<Rc<PendingEntryRename>>>,
    progress_state: RefCell<Rc<FileProgressState>>,
    background_file_progress:
        RefCell<HashMap<crate::services::OperationRequestId, Rc<BackgroundProgress>>>,
    pending_send_to_completion: RefCell<Option<PendingSendToCompletion>>,
    finished_send_to_completion: RefCell<Option<FinishedSendToCompletion>>,

    pin_handler: RefCell<Option<PinHandler>>,
    unpin_handler: RefCell<Option<UnpinHandler>>,
    pin_status_handler: RefCell<Option<PinStatusHandler>>,
    print_handler: RefCell<Option<PrintHandler>>,
    search_selection_handlers: RefCell<Vec<Rc<dyn Fn()>>>,
    pending_select: RefCell<Vec<String>>,
    pending_properties: RefCell<Option<Location>>,
    pending_location_selection: RefCell<Option<(Location, Vec<Location>)>>,
    pending_extract_retry: RefCell<Option<(FileEntry, Location)>>,
    extract_destination: RefCell<Option<Location>>,
    pending_archive_destination: RefCell<Option<Location>>,
    /// The entries a just-dispatched, non-permanent delete requested,
    /// snapshotted so a `CompletedWithErrors` response naming entries that
    /// failed only because the location doesn't support Trash can offer a
    /// permanent-delete retry for exactly those entries.
    pending_delete_entries: RefCell<Vec<FileEntry>>,
    pending_file_operation_animation: RefCell<Option<fly_to_trash::PreparedFlight>>,
    /// Visible permanent-delete rows captured before the operation mutates the model.
    pending_delete_dissolve: RefCell<Option<(usize, dissolve_delete::PreparedDissolve)>>,
    delete_dissolve_request: Cell<Option<crate::services::OperationRequestId>>,
    deferred_delete_empty_depth: Cell<Option<usize>>,
    pending_navigate: RefCell<Option<Location>>,
    pending_location_credentials: RefCell<Option<MountCredentials>>,
    pending_connect: RefCell<Option<Rc<PendingConnect>>>,
    pending_trash_lookup: RefCell<Option<LoadHandle>>,
    leftovers: leftovers::LeftoverState,
    pending_empty_trash: RefCell<Option<LoadHandle>>,
    trash_loading: RefCell<Option<TrashLoadingView>>,
    unlock_slots: RefCell<Vec<UnlockProgressSlot>>,
    trash_button: glib::WeakRef<gtk::Button>,
    recent_removal: crate::adapters::RecentRemovalState,
    drag_autoscroll: RefCell<Option<Rc<columns::drag_scroll::DragAutoscroll>>>,
    drag_source_depth: Cell<Option<usize>>,
    suppress_scroll_after_drop: Cell<bool>,
    transfer_replaces_cursor: Cell<bool>,
    transfer_preparation: RefCell<Option<Rc<FileProgressState>>>,
    preparing_transfers: RefCell<Vec<PreparingTransfer>>,
    drop_active_depths: Cell<Option<(usize, usize)>>,
    find: RefCell<find::FindState>,
    listing_filter: listing_filter::FilterState,
    listing_search: listing_search::SearchState,
    result_selection: result_selection::ResultSelection,
    browser: Rc<Browser>,
}


#[derive(Clone)]
pub struct BrowserView {
    state: Rc<ViewState>,
}

#[derive(Clone)]
pub(crate) struct WeakBrowserView(Weak<ViewState>);


impl WeakBrowserView {
    pub(crate) fn upgrade(&self) -> Option<BrowserView> {
        self.0.upgrade().map(|state| BrowserView { state })
    }
}

impl BrowserView {
















    pub fn browser(&self) -> Rc<Browser> {
        self.state.browser.clone()
    }














    pub fn view_mode(&self) -> BrowserMode {
        self.state.mode.get()
    }





    // Columns grabs the collection view itself; item-level focus lands on
    // editable cells that would swallow navigation keys.
    pub(in crate::ui) fn focus_file_view(&self) {
        if self.view_mode() == BrowserMode::Columns {
            self.state.focus_rebuilt_active_column();
        } else {
            self.state.browser.focus_active();
        }
    }





























    /// Shows the shared header spinner until the returned activity guard is dropped.
    pub fn begin_global_activity(&self, label: impl Into<String>) -> GlobalActivity {
        self.state.begin_global_activity(label)
    }













    #[cfg(test)]
    pub(in crate::ui) fn columns_mirror_selection_enabled(&self) -> bool {
        self.state.columns_mirror_selection.get()
    }

    /// Runs the cursor mirror at once instead of after its delay.
    #[cfg(test)]
    pub(in crate::ui) fn mirror_focused_folder_now(&self) {
        if let Some((depth, position, _)) = self.state.browser.focused_item() {
            self.state.apply_child_mirror(depth, position, false);
        }
    }





    pub(in crate::ui) fn set_preview_owns_keys(&self, owned: bool) {
        if self.state.preview_owns_keys.replace(owned) != owned {
            self.state.refresh_destination_style();
        }
    }






    pub fn keyboard_navigation(&self) {
        self.state
            .input_ownership
            .borrow_mut()
            .keyboard_navigation();
        self.state.overlay.add_css_class("keyboard-navigation");
        if let Some(window) = self.state.overlay.root().and_downcast::<gtk::Window>() {
            window.set_focus_visible(true);
        }
        cancel_source(&self.state.pending_peek);
        self.state.browser.close_peek();
        // Capture-phase keys run before the pane sees the event that would cancel
        // an in-progress restore, so the command has to cancel it first.
        self.state.mode_views.borrow_mut().cancel_pending_restore();
        self.state.sync_mode_selection();
        self.state.cancel_tab_location_hold();
        self.state.refresh_destination_style();
    }
















    /// Source positions of one pane in the order it displays them, including
    /// type groups and a Columns filter. `None` falls back to source order.
    fn displayed_order(&self, depth: usize) -> Option<Vec<usize>> {
        let order: Vec<usize> = if self.view_mode() == BrowserMode::Columns {
            let columns = self.state.columns.borrow();
            let column = columns.get(depth)?;
            (0..column.selection.n_items())
                .filter_map(|position| column.map.source_position(position))
                .collect()
        } else {
            self.state.mode_views.borrow().visual_order(depth)
        };
        (!order.is_empty()).then_some(order)
    }

    pub(crate) fn displayed_entries_matching(
        &self,
        depth: usize,
        keep: impl Fn(&FileEntry) -> bool,
    ) -> Vec<FileEntry> {
        if let Some(results) = self.filter_target().and_then(|target| target.results()) {
            return results
                .iter()
                .map(search_result_entry)
                .filter(keep)
                .collect();
        }
        let order = self.displayed_order(depth);
        self.state
            .browser
            .with_column_entries(depth, |entries| match order {
                Some(order) => order
                    .iter()
                    .filter_map(|position| entries.get(*position))
                    .filter(|entry| keep(entry))
                    .cloned()
                    .collect(),
                None => entries
                    .iter()
                    .filter(|entry| keep(entry))
                    .cloned()
                    .collect(),
            })
            .unwrap_or_default()
    }

    pub(crate) fn displayed_cursor_entry(&self, depth: usize) -> Option<FileEntry> {
        if let Some(target) = self
            .filter_target()
            .filter(|target| target.results_view().is_some())
        {
            return target.current_result().as_ref().map(search_result_entry);
        }
        self.state.browser.cursor_entry(depth)
    }

    /// `key` supplies the keyboard scroll direction; `None` preserves pointer navigation.
    pub(crate) fn step_to(&self, depth: usize, location: &Location, key: Option<i32>) -> bool {
        if let Some(target) = self.filter_target()
            && let Some(results) = target.results()
        {
            let Some(position) = results
                .iter()
                .position(|item| search_result_entry(item).location == *location)
            else {
                return false;
            };
            let cursor = target.hits().and_then(|hits| hits.cursor);
            let delta = position as i32 - cursor.unwrap_or(0) as i32;
            if key.is_some() {
                self.keyboard_navigation();
            }
            if cursor.is_none() {
                target.step(1, 0, key.is_some());
            }
            return self.step_filter_results(
                delta.signum(),
                delta.unsigned_abs() as usize,
                key.is_some(),
            );
        }
        let Some(position) = self
            .state
            .browser
            .with_column_entries(depth, |entries| {
                entries.iter().position(|entry| entry.location == *location)
            })
            .flatten()
        else {
            return false;
        };
        if key.is_some() {
            self.keyboard_navigation();
        }
        let collection = self
            .state
            .overlay
            .root()
            .and_then(|root| root.focus())
            .as_ref()
            .and_then(super::scrolling::focused_collection);
        if crate::ui::preferences::PreferenceManager::shared().tenxer_mode() {
            let order = self.displayed_order(depth);
            self.state
                .browser
                .place_cursor(depth, position, order.as_deref());
        } else {
            self.state.browser.select(depth, position);
        }
        self.state
            .mirror_focused_folder(depth, Some(position), false);
        if let (Some(direction), Some((view, scroll))) = (key, collection) {
            let position = self.cursor_view_position(&view);
            super::scrolling::reveal_cursor(
                &view,
                &scroll,
                direction,
                super::scrolling::CursorMotion::Step,
                position,
            );
        }
        true
    }

















    /// Recursive results have their own selection, independent of the directory's selection.
    pub fn selected_search_result(&self) -> Option<FileEntry> {
        if self.view_mode() != BrowserMode::Columns {
            return self.state.mode_views.borrow().selected_search_result();
        }
        let focused = self.state.overlay.root()?.focus()?;
        self.state
            .columns
            .borrow()
            .iter()
            .enumerate()
            .find_map(|(depth, column)| {
                if !(focused.is_ancestor(&column.filter_entry)
                    || focused == column.filter_entry.clone().upcast::<gtk::Widget>()
                    || focused.is_ancestor(&column.list)
                    || focused == column.list.clone().upcast::<gtk::Widget>())
                {
                    return None;
                }
                if column.recursive_search_active.get() {
                    let cursor = listing_filter::column_cursor(column)?;
                    return column
                        .search_results
                        .borrow()
                        .get(cursor as usize)
                        .map(search_result_entry);
                }
                if column.map.has_query() {
                    let selected = column.selection.selection();
                    if selected.is_empty() {
                        return None;
                    }
                    let source_position = column.map.source_position(selected.maximum())?;
                    return self.state.browser.entry_at(depth, source_position);
                }
                None
            })
    }








    fn cursor_view_position(&self, view: &gtk::Widget) -> Option<u32> {
        let (depth, source, _) = self.state.browser.focused_item()?;
        let columns = self.state.columns.borrow();
        if let Some(column) = columns.get(depth) {
            let list = column.list.clone().upcast::<gtk::Widget>();
            if &list == view {
                return column.map.view_position(source);
            }
        }
        drop(columns);
        self.state
            .mode_views
            .borrow()
            .view_position_in(view, depth, source)
    }








}

impl ViewState {




    fn begin_global_activity(self: &Rc<Self>, label: impl Into<String>) -> GlobalActivity {
        let label = label.into();
        let id = self.global_activity.borrow_mut().begin(label.clone());
        crate::ui::accessibility::set_description(&self.global_activity_spinner, Some(&label));
        self.global_activity_spinner.set_visible(true);
        self.global_activity_spinner.start();
        GlobalActivity {
            state: Rc::downgrade(self),
            id,
        }
    }

    fn finish_global_activity(&self, id: u64) {
        let current = {
            let mut activity = self.global_activity.borrow_mut();
            activity.finish(id);
            activity.current_label().map(str::to_owned)
        };
        if let Some(label) = current {
            crate::ui::accessibility::set_description(&self.global_activity_spinner, Some(&label));
        } else {
            self.global_activity_spinner.stop();
            self.global_activity_spinner.set_visible(false);
            crate::ui::accessibility::set_description(
                &self.global_activity_spinner,
                Some(&crate::i18n::tr("Working…")),
            );
        }
    }




    fn sync_mode_selection(&self) {
        if self.mode.get() == BrowserMode::Columns {
            if let Some(depth) = self.focused_column_depth() {
                self.browser.set_active_column(depth);
            }
            return;
        }
        let Some((depth, positions)) = self.mode_views.borrow().selected_positions() else {
            return;
        };
        let focused = self
            .mode_views
            .borrow()
            .focused_position()
            .filter(|(focused_depth, _)| *focused_depth == depth)
            .map(|(_, position)| position)
            .or_else(|| positions.last().copied());
        self.browser.set_selection(depth, &positions, focused);
    }





    fn destination_depth(&self) -> Option<usize> {
        if self.mode.get() != BrowserMode::Columns {
            return self.browser.active_depth();
        }
        if let Some(depth) = self.context_menu_column.get()
            && depth < self.columns.borrow().len()
        {
            return Some(depth);
        }
        self.input_ownership.borrow().destination(
            self.hovered_column.get(),
            self.focused_column_depth(),
            self.browser.active_depth(),
            self.columns.borrow().len(),
        )
    }

    fn refresh_destination_style(&self) {
        let destination = if self.preview_owns_keys.get() {
            None
        } else {
            self.destination_depth()
        };
        let focused_column = self.focused_column_depth();
        let focused_item = self
            .browser
            .focused_item()
            .map(|(depth, position, _)| (depth, position));
        for (depth, column) in self.columns.borrow().iter().enumerate() {
            let show_actions = destination == Some(depth);
            if show_actions {
                column.shell.add_css_class("active-column");
            } else {
                column.shell.remove_css_class("active-column");
            }
            if !show_actions
                && self
                    .overlay
                    .root()
                    .and_then(|root| root.focus())
                    .is_some_and(|focused| {
                        focused == *column.header_actions.upcast_ref::<gtk::Widget>()
                            || focused.is_ancestor(&column.header_actions)
                    })
            {
                // Do not leave keyboard focus inside controls hidden by pointer navigation.
                column.focus_surface();
            }
            column
                .header_actions_stack
                .set_visible_child_name(if show_actions { "actions" } else { "hidden" });
            let cursor = focused_item
                .filter(|(item_depth, _)| *item_depth == depth)
                .filter(|_| focused_column == Some(depth))
                .and_then(|(_, position)| column.map.view_position(position));
            column.bound_rows.borrow_mut().retain(|bound| {
                let (Some(item), Some(row)) = (bound.item.upgrade(), bound.row.upgrade()) else {
                    return false;
                };
                if cursor == Some(item.position()) {
                    row.add_css_class("keyboard-cursor");
                } else {
                    row.remove_css_class("keyboard-cursor");
                }
                true
            });
            let active = destination == Some(depth)
                && self.browser.location_at(depth).is_some_and(|location| {
                    !is_trash_location(&location) && !location.is_recent_location()
                });
            if active {
                column.shell.add_css_class("destination-column");
            } else {
                column.shell.remove_css_class("destination-column");
            }
        }
    }

    fn focused_column_depth(&self) -> Option<usize> {
        let focused = self.overlay.root()?.focus()?;
        self.columns.borrow().iter().position(|column| {
            focused == column.shell.clone().upcast::<gtk::Widget>()
                || focused.is_ancestor(&column.shell)
        })
    }


}



fn single_pane_preview_reservation(width: i32) -> i32 {
    width.max(0) / 2
}

mod chooser_context;
