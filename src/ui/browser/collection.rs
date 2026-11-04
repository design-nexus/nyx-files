// SPDX-License-Identifier: MIT

use crate::app::Browser;
use crate::model::Location;
use crate::services::{filter_query_allows_typos, fold_for_search};
use crate::ui::browser::entry::entry_matches;
use crate::ui::entry_list_model::EntryListModel;
use crate::ui::keys::{
    KeyPress,
    bindings::{ENTER, RIGHT},
};
use crate::ui::preferences::PreferenceManager;
use crate::ui::search_session::SearchScope;
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;


thread_local! {
    static PENDING_SCROLLS: RefCell<Vec<(glib::WeakRef<gtk::Widget>, gtk::TickCallbackId)>> = const { RefCell::new(Vec::new()) };
    static PENDING_REVEALS: RefCell<Vec<(glib::WeakRef<gtk::Widget>, gtk::TickCallbackId)>> = const { RefCell::new(Vec::new()) };
}

fn take_pending_scroll(view: &gtk::Widget) -> Option<gtk::TickCallbackId> {
    PENDING_SCROLLS.with_borrow_mut(|pending| {
        pending.retain(|(view, _)| view.upgrade().is_some());
        let index = pending
            .iter()
            .position(|(candidate, _)| candidate.upgrade().as_ref() == Some(view))?;
        Some(pending.swap_remove(index).1)
    })
}


/// `scroll_to` before the view has a real height leaves ListView/GridView with a
/// one-row widget pool, so scrolling after a mode switch stays janky.
pub(crate) fn scroll_collection_when_allocated(view: &gtk::Widget, position: u32) {
    scroll_collection_when_allocated_with(view, position, gtk::ListScrollFlags::FOCUS);
}


fn scroll_collection_when_allocated_with(
    view: &gtk::Widget,
    position: u32,
    flags: gtk::ListScrollFlags,
) {
    if let Some(pending) = take_pending_scroll(view) {
        pending.remove();
    }
    if view.height() > 1 {
        apply_collection_scroll(view, position, flags);
        return;
    }
    let frames = Cell::new(0u8);
    let callback = view.add_tick_callback(move |view, _| {
        if view.height() > 1 {
            take_pending_scroll(view);
            if flags.intersects(gtk::ListScrollFlags::FOCUS | gtk::ListScrollFlags::SELECT)
                && !collection_view_holds_focus(view)
            {
                return glib::ControlFlow::Break;
            }
            apply_collection_scroll(view, position, flags);
            return glib::ControlFlow::Break;
        }
        let waited = frames.get().saturating_add(1);
        frames.set(waited);
        if waited >= 8 {
            take_pending_scroll(view);
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
    PENDING_SCROLLS.with_borrow_mut(|pending| pending.push((view.downgrade(), callback)));
}



fn collection_view_holds_focus(view: &gtk::Widget) -> bool {
    let Some(focused) = view.root().and_then(|root| root.focus()) else {
        return false;
    };
    view.has_focus() || focused == *view || view.is_ancestor(&focused) || focused.is_ancestor(view)
}

pub(super) fn apply_collection_scroll(
    view: &gtk::Widget,
    position: u32,
    flags: gtk::ListScrollFlags,
) {
    if let Ok(list) = view.clone().downcast::<gtk::ListView>() {
        if position < list.model().map_or(0, |model| model.n_items()) {
            list.scroll_to(position, flags, None);
        }
        return;
    }
    if let Ok(grid) = view.clone().downcast::<gtk::GridView>()
        && position < grid.model().map_or(0, |model| model.n_items())
    {
        grid.scroll_to(position, flags, None);
    }
}








// Don't filter a replaced listing: doing so would discard its hidden selection.

type FilterQueryCallback = dyn Fn(String, SearchScope, bool);

pub(in crate::ui) struct FilterQueryBinding {
    entry: glib::WeakRef<gtk::Entry>,
    changed: Option<glib::SignalHandlerId>,
    pending: Rc<RefCell<Option<glib::SourceId>>>,
    callback: Weak<FilterQueryCallback>,
    scope: FilterScope,
}

// 10xer overrides scope without overwriting the saved subfolder preference.
#[derive(Clone, Default)]
struct FilterScope {
    include_subfolders: Rc<Cell<bool>>,
    tenxer: Rc<Cell<bool>>,
    forced: Rc<Cell<bool>>,
}

impl FilterScope {
}

impl FilterQueryBinding {





}

impl Drop for FilterQueryBinding {
    fn drop(&mut self) {
        cancel_source(&self.pending);
        if let Some(entry) = self.entry.upgrade()
            && let Some(changed) = self.changed.take()
        {
            entry.disconnect(changed);
        }
    }
}







#[derive(Default)]
pub(crate) struct PositionMap {
    query: String,
    generation: u64,
    forward: Vec<usize>,
    reverse: Vec<u32>,
}

pub(crate) const NO_FILTERED_POSITION: u32 = u32::MAX;

fn rebuild_position_map(
    source: &EntryListModel,
    query: &str,
    show_hidden: bool,
    generation: u64,
) -> PositionMap {
    let n_source = source.n_items() as usize;
    let mut forward = Vec::new();
    let mut reverse = vec![NO_FILTERED_POSITION; n_source];
    for (source_position, filtered_position) in reverse.iter_mut().enumerate() {
        let Some(text) = source.value(source_position as u32) else {
            continue;
        };
        if entry_matches(&text, show_hidden, query) {
            *filtered_position = forward.len() as u32;
            forward.push(source_position);
        }
    }
    PositionMap {
        query: query.to_owned(),
        generation,
        forward,
        reverse,
    }
}

#[derive(Clone)]
pub(crate) struct ViewMap {
    cache: Rc<RefCell<PositionMap>>,
    query: Rc<RefCell<String>>,
    show_hidden: Rc<Cell<bool>>,
    generation: Rc<Cell<u64>>,
    source: EntryListModel,
    filter: gtk::FilterListModel,
    placeholder: Option<gtk::StringList>,
}

impl ViewMap {

    fn placeholder_count(&self) -> u32 {
        self.placeholder
            .as_ref()
            .map_or(0, |placeholder| placeholder.n_items())
    }

    pub(crate) fn has_query(&self) -> bool {
        !self.query.borrow().trim().is_empty()
    }

    pub(crate) fn query(&self) -> std::cell::Ref<'_, String> {
        self.query.borrow()
    }

    pub(crate) fn source_position(&self, visible_position: u32) -> Option<usize> {
        let filter_position = visible_position.checked_sub(self.placeholder_count())?;
        let query = self.query.borrow();
        if query.is_empty() && self.show_hidden.get() {
            if filter_position < self.source.n_items() && filter_position < self.filter.n_items() {
                return Some(filter_position as usize);
            }
            return None;
        }
        let mut cache = self.cache.borrow_mut();
        let generation = self.generation.get();
        if cache.query != *query || cache.generation != generation {
            *cache = rebuild_position_map(&self.source, &query, self.show_hidden.get(), generation);
        }
        cache.forward.get(filter_position as usize).copied()
    }

    pub(crate) fn view_position(&self, source_position: usize) -> Option<u32> {
        let query = self.query.borrow();
        if query.is_empty() && self.show_hidden.get() {
            let position = source_position as u64;
            if position < self.source.n_items() as u64 && position < self.filter.n_items() as u64 {
                return Some(position as u32 + self.placeholder_count());
            }
            return None;
        }
        let mut cache = self.cache.borrow_mut();
        let generation = self.generation.get();
        if cache.query != *query || cache.generation != generation {
            *cache = rebuild_position_map(&self.source, &query, self.show_hidden.get(), generation);
        }
        let position = *cache.reverse.get(source_position)?;
        (position != NO_FILTERED_POSITION).then_some(position + self.placeholder_count())
    }

}

pub(crate) fn search_result_entry(item: &crate::services::SearchItem) -> crate::model::FileEntry {
    use crate::model::{FileEntry, MetadataValue};
    FileEntry {
        location: Location::local(item.path.clone()),
        native_name: item.path.file_name().unwrap_or_default().to_os_string(),
        thumbnail_path: None,
        display_name: item.name.clone(),
        kind: item.kind,
        size: MetadataValue::Unknown,
        modified_unix_seconds: MetadataValue::Unknown,
        recent_unix_seconds: MetadataValue::Unknown,
        is_hidden: false,
        mode: item.mode.clone(),
        image_dimensions: MetadataValue::Unknown,
        child_count: MetadataValue::Unknown,
        duration_seconds: MetadataValue::Unknown,
        recent_uri: None,
    }
}








pub(crate) fn cancel_source(source: &RefCell<Option<glib::SourceId>>) {
    if let Some(source) = source.take() {
        source.remove();
    }
}


