// SPDX-License-Identifier: MIT

//! One tab: an `app::Browser` and the view showing its folder.

use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};

use gtk::{glib, prelude::*};

use crate::{
    adapters::{LocalFileSource, LocalOperationProvider},
    app::{Browser, BrowserEvent},
    model::{FileEntry, Location, MetadataValue},
};

use super::{
    columns::{Column, ColumnsView},
    icons::IconsView,
    list::ListView,
    model::EntryModel,
};
use crate::ui::{browser_modes::BrowserMode, preferences::PreferenceManager};

type Changed = Box<dyn Fn()>;
type ContextFn = Box<dyn Fn(Option<FileEntry>, &gtk::Widget, f64, f64)>;
type RejectedFn = Box<dyn Fn(crate::services::LocationValidationError)>;
type EventFn = Box<dyn Fn(&BrowserEvent)>;
type ResultFn = Box<dyn Fn(crate::services::SearchItem)>;
type OpenFileFn = Box<dyn Fn(&FileEntry)>;
type DropFn = Box<dyn Fn(Location, Vec<Location>, gtk::gdk::DragAction)>;

pub(super) struct Tab {
    pub(super) browser: Rc<Browser>,
    pub(super) model: Rc<EntryModel>,
    columns: ColumnsView,
    list: ListView,
    icons: IconsView,
    pub(super) page: gtk::Stack,
    /// The views and the preview beside them.

    /// The paned views and preview, with the shortcut footer under them.
    pub(super) container: gtk::Box,
    pub(super) footer: crate::ui::shortcut_footer::ShortcutFooter,
    pub(super) preview: crate::ui::preview::PreviewDrawer,
    filter: crate::ui::search_session::SearchSession,
    results: gtk::ListBox,
    results_items: RefCell<Vec<crate::services::SearchItem>>,
    filter_query: RefCell<String>,
    result_open: RefCell<Option<ResultFn>>,
    events: RefCell<Option<EventFn>>,
    status: gtk::Label,
    mode: Cell<BrowserMode>,
    refresh_queued: Cell<bool>,
    syncing: Cell<bool>,
    shown: RefCell<Option<(Location, usize)>>,
    changed: RefCell<Vec<Changed>>,
    context: RefCell<Option<ContextFn>>,
    dropped: RefCell<Option<DropFn>>,
    rejected: RefCell<Option<RejectedFn>>,
    select_next: RefCell<Option<Location>>,
    /// Items whose metadata has been asked for, so a rebind doesn't ask again.
    requested: RefCell<std::collections::HashSet<Location>>,
    /// Replaces launching a file, as the chooser does to pick it.
    open_file: RefCell<Option<OpenFileFn>>,
    /// Folders opened per depth whose listing has not finished yet; a finished
    /// one is recorded for Jump to Folder.
    visits: RefCell<Vec<Option<Location>>>,
    /// When a pointer last pressed in a column, to tell clicks from keyboard moves.
    last_pointer: Cell<Option<std::time::Instant>>,
    pub(super) peek: super::peek::Peek,
}

/// With "Keep arrows in file list" (and in 10xer mode), arrow keys at a
/// view's edge don't move focus out of it.
fn keep_arrows(view: &gtk::Widget) {
    view.connect_keynav_failed(|_, _| {
        if PreferenceManager::shared().arrow_navigation_scoped_active() {
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
}

/// Startup metrics the field harness reads: listings published and the first rows.
fn published(rows: usize, started: std::time::Instant) {
    crate::metrics::mark_batch_rendered(rows, started);
    if rows > 0 {
        crate::metrics::mark_first_visible_row(rows);
    }
}

pub(super) fn home() -> PathBuf {
    glib::home_dir()
}

impl Tab {
    pub(super) fn new(location: Location) -> Rc<Tab> {
        Self::with_source(location, Rc::new(LocalFileSource))
    }

    /// A tab whose listings come from `source`, as the chooser's filtered one.
    pub(super) fn with_source(
        location: Location,
        source: Rc<dyn crate::services::FileSource>,
    ) -> Rc<Tab> {
        let browser =
            Browser::with_preferences(source, PreferenceManager::shared().sort_preferences());
        browser.set_operation_provider(Rc::new(LocalOperationProvider));
        let model = Rc::new(EntryModel::new());
        let columns = ColumnsView::new();
        let list = ListView::new(&model);
        list.view.set_enable_rubberband(true);
        let icons = IconsView::new(&model);
        let page = gtk::Stack::new();
        page.add_named(&list.root, Some("list"));
        page.add_named(&icons.root, Some("icons"));
        page.add_named(&columns.root, Some("columns"));
        let status = gtk::Label::new(None);
        status.add_css_class("dim-label");
        status.set_wrap(true);
        status.set_justify(gtk::Justification::Center);
        page.add_named(&status, Some("status"));
        let results = gtk::ListBox::new();
        results.add_css_class("search-results");
        results.add_css_class("filter-results");
        results.update_property(&[gtk::accessible::Property::Label(&crate::i18n::tr(
            crate::ui::SEARCH_RESULTS_LABEL,
        ))]);
        let results_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&results)
            .build();
        page.add_named(&results_scroll, Some("results"));
        let backend = PreferenceManager::shared();
        let preview = crate::ui::preview::PreviewDrawer::new(
            Rc::new(crate::adapters::LocalPreviewProvider::new(Rc::new(
                move || backend.media_preview_backend(),
            ))),
            true,
        );
        preview.bind_find_shortcut(&PreferenceManager::shared());
        let root = gtk::Paned::new(gtk::Orientation::Horizontal);
        root.add_css_class("preview-split");
        root.set_wide_handle(true);
        root.set_resize_start_child(true);
        root.set_resize_end_child(false);
        root.set_shrink_start_child(false);
        root.set_shrink_end_child(true);
        root.set_start_child(Some(&page));
        root.set_position(i32::MAX);
        root.set_vexpand(true);
        preview.attach_split_only(&root);
        let footer = crate::ui::shortcut_footer::ShortcutFooter::new(
            PreferenceManager::shared().browser_mode(),
        );
        footer.bind_preferences(&PreferenceManager::shared());
        let jobs = crate::ui::jobs::JobsIndicator::new();
        footer.set_activity(jobs.widget());
        let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
        container.append(&root);
        container.append(footer.widget());
        jobs.bind_window(&container);
        if let Some(display) = gtk::gdk::Display::default() {
            let _ = footer.connect_clipboard(&display.clipboard());
        }
        let tab = Rc::new(Tab {
            browser,
            model,
            columns,
            list,
            icons,
            page,
            container,
            footer,
            preview,
            filter: Default::default(),
            results,
            results_items: RefCell::default(),
            filter_query: RefCell::default(),
            result_open: RefCell::new(None),
            events: RefCell::new(None),
            status,
            mode: Cell::new(PreferenceManager::shared().browser_mode()),
            refresh_queued: Cell::new(false),
            syncing: Cell::new(false),
            shown: RefCell::new(None),
            changed: Default::default(),
            context: RefCell::new(None),
            dropped: RefCell::new(None),
            rejected: RefCell::new(None),
            select_next: RefCell::new(None),
            requested: RefCell::default(),
            open_file: RefCell::new(None),
            visits: RefCell::default(),
            last_pointer: Cell::new(None),
            peek: Default::default(),
        });
        super::peek::install(&tab);
        tab.bind_preferences();
        let weak = Rc::downgrade(&tab);
        tab.browser.observe(move |event| {
            if let Some(tab) = weak.upgrade() {
                tab.on_event(event);
            }
        });
        let weak = Rc::downgrade(&tab);
        tab.list.view.connect_activate(move |_, index| {
            if let Some(tab) = weak.upgrade() {
                tab.activate(index);
            }
        });
        let weak = Rc::downgrade(&tab);
        tab.icons.view.connect_activate(move |_, index| {
            if let Some(tab) = weak.upgrade() {
                tab.activate(index);
            }
        });
        let weak = Rc::downgrade(&tab);
        tab.model
            .selection
            .connect_selection_changed(move |_, _, _| {
                if let Some(tab) = weak.upgrade() {
                    tab.push_selection();
                }
            });
        let weak = Rc::downgrade(&tab);
        *tab.model.hooks.context.borrow_mut() = Some(Box::new(move |index, widget, x, y| {
            let Some(tab) = weak.upgrade() else { return };
            let entry = index.and_then(|i| {
                tab.model.select_for_menu(i);
                tab.model.entry(i)
            });
            if let Some(f) = tab.context.borrow().as_ref() {
                f(entry, widget, x, y);
            }
        }));
        let weak = Rc::downgrade(&tab);
        *tab.model.hooks.bind.borrow_mut() = Some(Box::new(move |position, entry| {
            let Some(tab) = weak.upgrade() else { return };
            if entry.modified_unix_seconds == MetadataValue::Unknown
                && let Some(depth) = tab.depth()
                && tab.requested.borrow_mut().insert(entry.location.clone())
            {
                tab.browser.request_visible_metadata_fill(
                    depth,
                    position,
                    entry.location.clone(),
                    false,
                );
            }
        }));
        tab.wire_dnd(&tab.model.clone());
        keep_arrows(tab.list.view.upcast_ref());
        keep_arrows(tab.icons.view.upcast_ref());
        tab.apply_grouping();
        let weak = Rc::downgrade(&tab);
        PreferenceManager::shared().bind_preference(
            &tab.page,
            PreferenceManager::group_by_type,
            move |_, _| {
                if let Some(tab) = weak.upgrade() {
                    tab.apply_grouping();
                }
            },
        );
        tab.preview.observe_browser(&tab.browser);
        tab.footer.observe_browser(&tab.browser);
        let weak = Rc::downgrade(&tab);
        tab.results.connect_row_activated(move |_, row| {
            let Some(tab) = weak.upgrade() else { return };
            let item = usize::try_from(row.index())
                .ok()
                .and_then(|i| tab.results_items.borrow().get(i).cloned());
            if let (Some(item), Some(open)) = (item, tab.result_open.borrow().as_ref()) {
                open(item);
            }
        });
        let weak = Rc::downgrade(&tab);
        tab.preview.set_stepper(move |delta| {
            let tab = weak.upgrade()?;
            let model = tab.active_model();
            let current = model
                .selected_positions()
                .first()
                .map(|(i, _)| *i as i64)
                .unwrap_or(-1);
            let next = (current + i64::from(delta)).clamp(0, i64::from(model.len()) - 1);
            let next = u32::try_from(next).ok()?;
            model.selection.select_item(next, true);
            model.entry(next)
        });
        tab.browser.navigate(location);
        tab
    }

    /// Every browser event, for operation progress and results.
    pub(super) fn connect_open_file(&self, f: impl Fn(&FileEntry) + 'static) {
        *self.open_file.borrow_mut() = Some(Box::new(f));
    }

    pub(super) fn connect_events(&self, f: impl Fn(&BrowserEvent) + 'static) {
        *self.events.borrow_mut() = Some(Box::new(f));
    }

    /// The row the keyboard is on in the active view: the last selected.
    pub(super) fn cursor(&self) -> Option<u32> {
        self.active_model()
            .selected_positions()
            .last()
            .map(|(i, _)| *i)
    }

    /// Moves the keyboard row to `to` (clamped), selecting from `anchor` when
    /// visual mode is on, and scrolls it into view.
    pub(super) fn move_cursor(&self, to: i64, anchor: Option<u32>) {
        let model = self.active_model();
        let len = model.len();
        if len == 0 {
            return;
        }
        let to = to.clamp(0, i64::from(len) - 1) as u32;
        match anchor {
            Some(anchor) => {
                let (start, end) = (anchor.min(to), anchor.max(to));
                model.selection.select_range(start, end - start + 1, true);
            }
            None => {
                model.selection.select_item(to, true);
            }
        }
        self.scroll_to(to);
    }

    fn scroll_to(&self, index: u32) {
        match self.mode.get() {
            BrowserMode::List => {
                self.list
                    .view
                    .scroll_to(index, None, gtk::ListScrollFlags::FOCUS, None)
            }
            BrowserMode::Icons => {
                self.icons
                    .view
                    .scroll_to(index, gtk::ListScrollFlags::FOCUS, None)
            }
            BrowserMode::Columns => {
                if let Some(column) = self.depth().and_then(|d| self.columns.column(d)) {
                    column
                        .view
                        .scroll_to(index, gtk::ListScrollFlags::FOCUS, None);
                }
            }
        }
    }

    /// About how many rows fit in the view.
    pub(super) fn page_rows(&self) -> i64 {
        i64::from((self.page.height() / 34).max(1))
    }

    /// Steps left: the column before in Columns, else the enclosing folder.
    pub(super) fn step_out(&self) {
        if self.mode.get() == BrowserMode::Columns
            && let Some(depth) = self.depth().filter(|d| *d > 0)
        {
            self.focus_column(depth - 1);
        } else {
            self.browser.parent();
        }
    }

    /// Steps right: into the folder on the cursor, or opens a file.
    pub(super) fn step_in(&self) {
        let Some(entry) = self.cursor().and_then(|i| self.active_model().entry(i)) else {
            return;
        };
        if !entry.is_directory() {
            self.open(&entry);
        } else if self.mode.get() == BrowserMode::Columns
            && let Some(depth) = self.depth()
        {
            self.browser.descend(depth, entry.location.clone());
        } else {
            self.browser.navigate(entry.location.clone());
        }
    }

    /// The next row (forward or back from the cursor, wrapping) whose name
    /// contains `text`, ignoring case.
    pub(super) fn find(&self, text: &str, backward: bool) -> bool {
        let model = self.active_model();
        let len = model.len();
        if len == 0 || text.is_empty() {
            return false;
        }
        let needle = text.to_lowercase();
        let start = self.cursor().unwrap_or(if backward { 0 } else { len - 1 });
        for step in 1..=len {
            let i = if backward {
                (start + len - step % len) % len
            } else {
                (start + step) % len
            };
            if model
                .entry(i)
                .is_some_and(|e| e.display_name.to_lowercase().contains(&needle))
            {
                self.move_cursor(i64::from(i), None);
                return true;
            }
        }
        false
    }

    /// Keys for the shortcut footer (F1 and the reference while it is open).
    pub(super) fn footer_key(&self, press: &crate::ui::keys::KeyPress) -> bool {
        self.footer.handle_key(press).is_some()
    }

    /// Where a filter result goes when activated.
    pub(super) fn connect_result_open(&self, f: impl Fn(crate::services::SearchItem) + 'static) {
        *self.result_open.borrow_mut() = Some(Box::new(f));
    }

    pub(super) fn filter_text(&self) -> String {
        self.filter_query.borrow().clone()
    }

    pub(super) fn filtering(&self) -> bool {
        !self.filter_query.borrow().is_empty()
    }

    /// Narrows the folder showing to `query` (into subfolders when the setting
    /// says so); an empty query shows the folder again.
    pub(super) fn set_filter(self: &Rc<Self>, query: &str) {
        self.set_search(
            query,
            PreferenceManager::shared().filter_include_subfolders(),
        );
    }

    /// Narrows to `query` in this folder, or in its subfolders too.
    pub(super) fn set_search(self: &Rc<Self>, query: &str, subfolders: bool) {
        use crate::ui::search_session::{SearchInput, SearchScope};
        *self.filter_query.borrow_mut() = query.trim().to_string();
        let Some(root) = self
            .location()
            .and_then(|l| l.native_path().map(std::path::PathBuf::from))
        else {
            return;
        };
        if query.trim().is_empty() {
            self.filter.cancel();
            self.results_items.borrow_mut().clear();
            self.show_page();
            return;
        }
        let input = SearchInput {
            root,
            show_hidden: self.shows_hidden(),
            scope: if subfolders {
                SearchScope::Subfolders
            } else {
                SearchScope::Folder
            },
            refused: Default::default(),
            within: None,
        };
        let weak = Rc::downgrade(self);
        self.filter.update(
            input,
            query,
            false,
            Rc::new(move |batch| {
                if let Some(tab) = weak.upgrade()
                    && batch.query.trim() == tab.filter_query.borrow().as_str()
                {
                    tab.show_results(batch.items);
                }
            }),
        );
        self.page.set_visible_child_name("results");
    }

    fn show_results(&self, items: Vec<crate::services::SearchItem>) {
        while let Some(child) = self.results.first_child() {
            self.results.remove(&child);
        }
        for item in items.iter().take(500) {
            self.results.append(&super::search::row(item));
        }
        if items.is_empty() {
            let empty = nyx_ui::widgets::label(&crate::i18n::tr("No matches"), "dim");
            empty.set_margin_top(24);
            empty.set_halign(gtk::Align::Center);
            self.results.append(&empty);
        } else if let Some(first) = self.results.row_at_index(0) {
            self.results.select_row(Some(&first));
        }
        *self.results_items.borrow_mut() = items;
        self.page.set_visible_child_name("results");
    }

    /// Navigation the browser refused (not mounted, missing, not allowed).
    pub(super) fn connect_rejected(
        &self,
        f: impl Fn(crate::services::LocationValidationError) + 'static,
    ) {
        *self.rejected.borrow_mut() = Some(Box::new(f));
    }

    /// Files dropped: the destination folder, the files, and Shift/Ctrl's choice.
    pub(super) fn connect_drop(
        &self,
        f: impl Fn(Location, Vec<Location>, gtk::gdk::DragAction) + 'static,
    ) {
        *self.dropped.borrow_mut() = Some(Box::new(f));
    }

    fn wire_dnd(self: &Rc<Self>, model: &Rc<EntryModel>) {
        let m = Rc::downgrade(model);
        *model.hooks.drag.borrow_mut() = Some(Box::new(move |index| {
            let Some(model) = m.upgrade() else {
                return Vec::new();
            };
            model.select_for_menu(index);
            model.selected()
        }));
        let weak = Rc::downgrade(self);
        *model.hooks.drop.borrow_mut() = Some(Box::new(move |folder, sources, action| {
            let Some(tab) = weak.upgrade() else {
                return false;
            };
            let Some(destination) = folder.map(|f| f.location).or_else(|| tab.location()) else {
                return false;
            };
            if let Some(f) = tab.dropped.borrow().as_ref() {
                f(destination, sources, action);
            }
            true
        }));
    }

    /// Right-clicks: the item under the pointer (selected for the menu), or none.
    pub(super) fn connect_context(
        &self,
        f: impl Fn(Option<FileEntry>, &gtk::Widget, f64, f64) + 'static,
    ) {
        *self.context.borrow_mut() = Some(Box::new(f));
    }

    pub(super) fn mode(&self) -> BrowserMode {
        self.mode.get()
    }

    pub(super) fn set_mode(self: &Rc<Self>, mode: BrowserMode) {
        self.mode.set(mode);
        self.set_scope(mode);
        self.footer.set_mode(mode);
        self.apply_grouping();
        self.refresh();
        self.show_page();
    }

    /// Columns keeps every visible column fresh; the other views only the folder showing.
    fn set_scope(&self, mode: BrowserMode) {
        let scope = match mode {
            BrowserMode::Columns => crate::app::VisibleDepths::All,
            BrowserMode::Icons | BrowserMode::List => crate::app::VisibleDepths::Active,
        };
        self.browser.set_freshness(crate::app::FreshnessInput {
            scope,
            ..self.browser.freshness_input()
        });
    }

    /// Whether the window is focused, visible or hidden; a tab not showing
    /// counts as hidden. Listings refresh by it.
    pub(super) fn set_presence(&self, presence: crate::app::Presence, power_saver: bool) {
        self.browser.set_freshness(crate::app::FreshnessInput {
            presence,
            power_saver,
            ..self.browser.freshness_input()
        });
    }

    fn bind_preferences(self: &Rc<Self>) {
        let prefs = PreferenceManager::shared();
        self.set_scope(self.mode.get());
        let weak = Rc::downgrade(self);
        prefs.bind_preference(
            &self.page,
            PreferenceManager::watch_network_folders,
            move |_, watch| {
                if let Some(tab) = weak.upgrade() {
                    tab.browser.set_freshness(crate::app::FreshnessInput {
                        watch_network_folders: watch,
                        ..tab.browser.freshness_input()
                    });
                }
            },
        );
        let weak = Rc::downgrade(self);
        prefs.bind_preference(
            &self.page,
            PreferenceManager::sort_preferences,
            move |_, value| {
                if let Some(tab) = weak.upgrade() {
                    tab.browser.apply_default_preferences(value);
                }
            },
        );
        let weak = Rc::downgrade(self);
        prefs.bind_preference(
            &self.page,
            PreferenceManager::remember_folder_views,
            move |_, remember| {
                if let Some(tab) = weak.upgrade() {
                    tab.browser.set_folder_sorts(remember.then(|| {
                        Rc::new(|location: &Location, opened| {
                            PreferenceManager::shared().resolve_folder_sort(location, opened)
                        }) as crate::app::FolderSortResolver
                    }));
                }
            },
        );
        let weak = Rc::downgrade(self);
        prefs.bind_preference(
            &self.page,
            PreferenceManager::folder_sorts_revision,
            move |_, _| {
                if let Some(tab) = weak.upgrade() {
                    tab.browser.resync_column_sorts();
                }
            },
        );
        let weak = Rc::downgrade(self);
        prefs.bind_preference(
            &self.page,
            PreferenceManager::tenxer_mode,
            move |_, enabled| {
                if let Some(tab) = weak.upgrade() {
                    tab.browser.set_preserve_fill_on_removal(enabled);
                }
            },
        );
    }

    fn record_visit(&self, event: &BrowserEvent) {
        let mut visits = self.visits.borrow_mut();
        let finished = match event {
            BrowserEvent::Reset => {
                visits.clear();
                None
            }
            BrowserEvent::ColumnsTruncated { len, .. } => {
                visits.truncate(*len);
                None
            }
            BrowserEvent::ColumnAdded { depth, location } => {
                if visits.len() <= *depth {
                    visits.resize(depth + 1, None);
                }
                visits[*depth] = Some(location.clone());
                None
            }
            BrowserEvent::LoadFinished { depth, .. } => {
                visits.get_mut(*depth).and_then(Option::take)
            }
            _ => None,
        };
        drop(visits);
        if let Some(path) = finished.as_ref().and_then(Location::native_path) {
            crate::services::NavigationHistory::shared().record(path);
        }
    }

    /// Steps the Icons size (positive is larger), or resets it with 0, and saves
    /// it for this folder or as the default, as the preferences say.
    pub(super) fn resize_icons(&self, steps: i32) {
        let prefs = PreferenceManager::shared();
        let location = self.location();
        let size = if steps == 0 {
            match location.as_ref().filter(|l| prefs.remembers(l)) {
                Some(l) => {
                    prefs.reset_folder_icons_size(l);
                    prefs.icons_size_for(Some(l))
                }
                None => crate::ui::browser_modes::DEFAULT_ICONS_THUMBNAIL_SIZE,
            }
        } else {
            crate::ui::browser_modes::thumbnail_size::stepped(self.icons.size(), steps)
        };
        prefs.set_folder_icons_size(location.as_ref(), size);
        self.icons.set_size(size);
    }

    pub(super) fn toggle_preview(&self) {
        let entry = self.selected().into_iter().next();
        self.preview.toggle(
            crate::ui::preview::preview_target(entry),
            self.browser.active_depth(),
        );
    }

    pub(super) fn icons_size(&self) -> i32 {
        self.icons.size()
    }

    pub(super) fn set_icons_size(&self, size: i32) {
        PreferenceManager::shared().set_folder_icons_size(self.location().as_ref(), size);
        self.icons.set_size(size);
    }

    fn apply_grouping(&self) {
        self.model.set_grouped(
            self.mode.get() == BrowserMode::List && PreferenceManager::shared().group_by_type(),
        );
    }

    /// The model the selection and menus act on: the active column's in Columns.
    pub(super) fn active_model(&self) -> Rc<EntryModel> {
        if self.mode.get() == BrowserMode::Columns
            && let Some(column) = self.depth().and_then(|d| self.columns.column(d))
        {
            return column.model.clone();
        }
        self.model.clone()
    }

    fn column_count(&self) -> usize {
        (0..)
            .take_while(|&d| self.browser.location_at(d).is_some())
            .count()
    }

    fn wire_column(self: &Rc<Self>, depth: usize, column: &Rc<Column>) {
        self.wire_dnd(&column.model);
        let click = gtk::GestureClick::new();
        click.set_button(0);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        click.connect_pressed(move |_, _, _, _| {
            if let Some(tab) = weak.upgrade() {
                tab.last_pointer.set(Some(std::time::Instant::now()));
            }
        });
        column.view.add_controller(click);
        keep_arrows(column.view.upcast_ref());
        let weak = Rc::downgrade(self);
        *column.model.hooks.bind.borrow_mut() = Some(Box::new(move |position, entry| {
            if let Some(tab) = weak.upgrade()
                && entry.modified_unix_seconds == MetadataValue::Unknown
                && tab.requested.borrow_mut().insert(entry.location.clone())
            {
                tab.browser.request_visible_metadata_fill(
                    depth,
                    position,
                    entry.location.clone(),
                    false,
                );
            }
        }));
        let (weak, model) = (Rc::downgrade(self), Rc::downgrade(&column.model));
        *column.model.hooks.context.borrow_mut() = Some(Box::new(move |index, widget, x, y| {
            let (Some(tab), Some(model)) = (weak.upgrade(), model.upgrade()) else {
                return;
            };
            tab.browser.set_active_column(depth);
            let entry = index.and_then(|i| {
                model.select_for_menu(i);
                model.entry(i)
            });
            if let Some(f) = tab.context.borrow().as_ref() {
                f(entry, widget, x, y);
            }
        }));
        let weak = Rc::downgrade(self);
        column
            .model
            .selection
            .connect_selection_changed(move |_, _, _| {
                if let Some(tab) = weak.upgrade() {
                    tab.column_selected(depth);
                }
            });
        let (weak, model) = (Rc::downgrade(self), Rc::downgrade(&column.model));
        column.view.connect_activate(move |_, index| {
            let (Some(tab), Some(model)) = (weak.upgrade(), model.upgrade()) else {
                return;
            };
            let Some(entry) = model.entry(index) else {
                return;
            };
            if entry.is_directory() {
                tab.browser.descend(depth, entry.location.clone());
            } else {
                tab.open(&entry);
            }
        });
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(self);
        crate::ui::keys::on_press(&keys, move |_, press| {
            let Some(tab) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            use crate::ui::keys::bindings::{LEFT, RIGHT};
            if press.is(LEFT) && depth > 0 {
                tab.focus_column(depth - 1);
            } else if press.is(RIGHT) && tab.columns.column(depth + 1).is_some() {
                tab.focus_column(depth + 1);
            } else {
                return glib::Propagation::Proceed;
            }
            glib::Propagation::Stop
        });
        column.view.add_controller(keys);
        let weak = Rc::downgrade(self);
        let focus = gtk::EventControllerFocus::new();
        focus.connect_enter(move |_| {
            if let Some(tab) = weak.upgrade() {
                tab.browser.set_active_column(depth);
            }
        });
        column.view.add_controller(focus);
    }

    fn focus_column(&self, depth: usize) {
        let Some(column) = self.columns.column(depth) else {
            return;
        };
        self.browser.set_active_column(depth);
        if column.model.selection.selection().is_empty() && column.model.len() > 0 {
            column.model.selection.select_item(0, true);
        }
        column.view.grab_focus();
        for f in self.changed.borrow().iter() {
            f();
        }
    }

    fn column_selected(&self, depth: usize) {
        if self.syncing.get() {
            return;
        }
        let Some(column) = self.columns.column(depth) else {
            return;
        };
        let selected = column.model.selected_positions();
        let positions: Vec<usize> = selected.iter().map(|(_, p)| *p).collect();
        self.browser.set_active_column(depth);
        self.browser
            .set_selection(depth, &positions, positions.last().copied());
        let entries = column.model.selected();
        self.preview_single_click(&entries);
        let keyboard = self
            .last_pointer
            .get()
            .is_none_or(|at| at.elapsed() > std::time::Duration::from_millis(400));
        let mirror = !keyboard || PreferenceManager::shared().columns_mirror_selection();
        match entries.as_slice() {
            [entry] if entry.is_directory() && mirror => {
                self.browser.show_child(depth, entry.location.clone())
            }
            [entry] if entry.is_directory() => {}
            _ if self.columns.column(depth + 1).is_some() => self.browser.close_column(depth + 1),
            _ => {}
        }
    }

    fn refresh_columns(self: &Rc<Self>) {
        let count = self.column_count();
        self.columns
            .ensure(count, |depth, column| self.wire_column(depth, column));
        self.syncing.set(true);
        for depth in 0..count {
            let Some(column) = self.columns.column(depth) else {
                continue;
            };
            let show_hidden = self
                .browser
                .column_preferences(depth)
                .is_some_and(|p| p.show_hidden);
            let started = std::time::Instant::now();
            self.browser.with_column_entries(depth, |entries| {
                column.model.set_entries(entries, show_hidden)
            });
            published(column.model.len() as usize, started);
            if column.model.selected().is_empty() {
                let positions = self.browser.selected_positions(depth);
                if !positions.is_empty() {
                    column.model.select_positions(&positions);
                }
            }
        }
        self.syncing.set(false);
    }

    fn view_page(&self) -> &'static str {
        match self.mode.get() {
            BrowserMode::Icons => "icons",
            BrowserMode::List => "list",
            BrowserMode::Columns => "columns",
        }
    }

    fn show_page(&self) {
        if self.filtering() {
            self.page.set_visible_child_name("results");
            return;
        }
        if self.page.visible_child_name().as_deref() != Some("status") {
            self.page.set_visible_child_name(self.view_page());
        }
    }

    pub(super) fn focus_view(&self) {
        match self.mode.get() {
            BrowserMode::Icons => self.icons.view.grab_focus(),
            BrowserMode::List => self.list.view.grab_focus(),
            BrowserMode::Columns => {
                if let Some(column) = self.depth().and_then(|d| self.columns.column(d)) {
                    column.view.grab_focus();
                }
                true
            }
        };
    }

    pub(super) fn selected(&self) -> Vec<FileEntry> {
        self.active_model().selected()
    }

    pub(super) fn shows_hidden(&self) -> bool {
        self.depth()
            .and_then(|d| self.browser.column_preferences(d))
            .is_some_and(|p| p.show_hidden)
    }

    fn activate(&self, index: u32) {
        if let Some(entry) = self.model.entry(index) {
            self.open(&entry);
        }
    }

    fn push_selection(&self) {
        if self.syncing.get() {
            return;
        }
        let Some(depth) = self.depth() else { return };
        let selected = self.model.selected_positions();
        let positions: Vec<usize> = selected.iter().map(|(_, p)| *p).collect();
        self.browser
            .set_selection(depth, &positions, positions.last().copied());
        self.preview_single_click(&self.model.selected());
    }

    /// With single-click previews on, selecting one file previews it.
    fn preview_single_click(&self, selected: &[FileEntry]) {
        if !PreferenceManager::shared().single_click_previews() {
            return;
        }
        if let [entry] = selected
            && !entry.is_directory()
            && let Some(entry) = crate::ui::preview::preview_target(Some(entry.clone()))
        {
            self.browser.request_automatic_preview(entry);
        }
    }

    /// The tab's folder or title changed (for the tab row and header).
    pub(super) fn connect_changed(&self, f: impl Fn() + 'static) {
        self.changed.borrow_mut().push(Box::new(f));
    }

    pub(super) fn location(&self) -> Option<Location> {
        self.depth()
            .and_then(|depth| self.browser.location_at(depth))
    }

    pub(super) fn title(&self) -> String {
        match self.location() {
            Some(location) if location.native_path() == Some(home().as_path()) => {
                crate::i18n::tr("Home")
            }
            Some(location) => location.display_name(),
            None => crate::i18n::tr("Files"),
        }
    }

    fn depth(&self) -> Option<usize> {
        self.browser.active_depth()
    }

    fn on_event(self: &Rc<Self>, event: &BrowserEvent) {
        if let Some(f) = self.events.borrow().as_ref() {
            f(event);
        }
        self.record_visit(event);
        self.peek.on_event(event);
        if !matches!(event, BrowserEvent::MetadataFilled { .. }) {
            self.requested.borrow_mut().clear();
        }
        if let BrowserEvent::LocationNavigationRejected { error }
        | BrowserEvent::NavigationRejected { error, .. } = event
        {
            if let Some(f) = self.rejected.borrow().as_ref() {
                f(error.clone());
            }
            return;
        }
        match event {
            BrowserEvent::EntryCreated { location } => {
                *self.select_next.borrow_mut() = Some(location.clone())
            }
            BrowserEvent::ItemRelocated { to, .. } => {
                *self.select_next.borrow_mut() = Some(to.clone())
            }
            _ => {}
        }
        if matches!(
            event,
            BrowserEvent::OpenRequested { .. }
                | BrowserEvent::SelectionSynced { .. }
                | BrowserEvent::SelectionSetChanged { .. }
                | BrowserEvent::FocusChanged { .. }
                | BrowserEvent::PreviewRequested { .. }
                | BrowserEvent::PeekStarted { .. }
                | BrowserEvent::PeekEntriesAdded { .. }
                | BrowserEvent::PeekFinished
                | BrowserEvent::PeekFailed { .. }
                | BrowserEvent::PeekClosed
        ) {
            return;
        }
        if self.refresh_queued.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(tab) = weak.upgrade() {
                tab.refresh_queued.set(false);
                tab.refresh();
            }
        });
    }

    fn refresh(self: &Rc<Self>) {
        let Some(depth) = self.depth() else { return };
        let Some(snapshot) = self.browser.column_snapshot(depth) else {
            return;
        };
        if let Some(error) = &snapshot.error {
            self.status.set_text(error);
            self.page.set_visible_child_name("status");
        } else {
            self.show_page();
        }
        let key = (snapshot.location.clone(), snapshot.count);
        let moved = self.shown.borrow().as_ref().map(|(l, _)| l) != Some(&key.0);
        let columns = self.mode.get() == BrowserMode::Columns;
        if columns {
            self.refresh_columns();
        } else {
            let show_hidden = self.shows_hidden();
            self.syncing.set(true);
            let started = std::time::Instant::now();
            self.browser.with_column_entries(depth, |entries| {
                self.model.set_entries(entries, show_hidden)
            });
            published(self.model.len() as usize, started);
            if self.model.selected().is_empty() {
                let positions = self.browser.selected_positions(depth);
                if !positions.is_empty() {
                    self.model.select_positions(&positions);
                }
            }
            self.syncing.set(false);
        }
        if let Some(location) = self.select_next.borrow().clone() {
            let model = self.active_model();
            self.syncing.set(!columns);
            let found = model.select_location(&location).is_some();
            self.syncing.set(false);
            if found {
                self.select_next.borrow_mut().take();
                if !columns {
                    self.push_selection();
                }
            }
        }
        *self.shown.borrow_mut() = Some(key);
        if moved {
            if self.filtering() {
                self.set_filter("");
            }
            self.requested.borrow_mut().clear();
            self.icons
                .set_size(PreferenceManager::shared().icons_size_for(self.location().as_ref()));
            if self.model.len() > 0 {
                self.list
                    .view
                    .scroll_to(0, None, gtk::ListScrollFlags::NONE, None);
                self.icons
                    .view
                    .scroll_to(0, gtk::ListScrollFlags::NONE, None);
            }
            for f in self.changed.borrow().iter() {
                f();
            }
        }
    }

    pub(super) fn open(&self, entry: &FileEntry) {
        if entry.is_directory() {
            self.browser.navigate(entry.location.clone());
            return;
        }
        if let Some(open) = self.open_file.borrow().as_ref() {
            open(entry);
            return;
        }
        crate::ui::browser::desktop::open_location_at(
            &entry.location,
            None,
            &self.page,
            &self.browser,
        );
    }

    /// The breadcrumbs for the folder showing: from Home when inside it, else from `/`.
    pub(super) fn crumbs(&self) -> (Vec<nyx_ui::pathbar::Crumb>, String) {
        let Some(location) = self.location() else {
            return (Vec::new(), String::new());
        };
        let home = home();
        let mut crumbs = Vec::new();
        let mut current = Some(location.clone());
        while let Some(here) = current {
            let path = here.native_path().map(PathBuf::from);
            if path.as_deref() == Some(home.as_path()) {
                crumbs.push(nyx_ui::pathbar::Crumb {
                    label: crate::i18n::tr("Home"),
                    icon: Some("user-home-symbolic".into()),
                    target: home.display().to_string(),
                });
                break;
            }
            let root = path.as_deref() == Some(std::path::Path::new("/"));
            crumbs.push(nyx_ui::pathbar::Crumb {
                label: if root {
                    String::new()
                } else {
                    here.display_name()
                },
                icon: root.then(|| "drive-harddisk-symbolic".into()),
                target: target_of(&here),
            });
            current = here.parent();
        }
        crumbs.reverse();
        (crumbs, location.display_path())
    }
}

fn target_of(location: &Location) -> String {
    match location.native_path() {
        Some(path) => path.display().to_string(),
        None => location.uri_value().unwrap_or_default().to_string(),
    }
}

/// A crumb's target back to a location.
pub(super) fn location_of(target: &str) -> Location {
    if target.starts_with('/') {
        Location::local(target)
    } else {
        Location::uri(target)
    }
}
