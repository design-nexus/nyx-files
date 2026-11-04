// SPDX-License-Identifier: MIT

//! One tab: an `app::Browser` and the view showing its folder.

use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};

use gtk::{gio, glib, prelude::*};

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

pub(super) struct Tab {
    pub(super) browser: Rc<Browser>,
    pub(super) model: Rc<EntryModel>,
    columns: ColumnsView,
    list: ListView,
    icons: IconsView,
    pub(super) page: gtk::Stack,
    status: gtk::Label,
    mode: Cell<BrowserMode>,
    refresh_queued: Cell<bool>,
    syncing: Cell<bool>,
    shown: RefCell<Option<(Location, usize)>>,
    changed: RefCell<Vec<Changed>>,
    context: RefCell<Option<ContextFn>>,
    select_next: RefCell<Option<Location>>,
}

pub(super) fn home() -> PathBuf {
    glib::home_dir()
}

impl Tab {
    pub(super) fn new(location: Location) -> Rc<Tab> {
        let browser = Browser::with_preferences(Rc::new(LocalFileSource), PreferenceManager::shared().sort_preferences());
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
        let tab = Rc::new(Tab {
            browser,
            model,
            columns,
            list,
            icons,
            page,
            status,
            mode: Cell::new(PreferenceManager::shared().browser_mode()),
            refresh_queued: Cell::new(false),
            syncing: Cell::new(false),
            shown: RefCell::new(None),
            changed: Default::default(),
            context: RefCell::new(None),
            select_next: RefCell::new(None),
        });
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
        tab.model.selection.connect_selection_changed(move |_, _, _| {
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
            {
                tab.browser.request_visible_metadata_fill(depth, position, entry.location.clone(), false);
            }
        }));
        tab.browser.navigate(location);
        tab
    }

    /// Right-clicks: the item under the pointer (selected for the menu), or none.
    pub(super) fn connect_context(&self, f: impl Fn(Option<FileEntry>, &gtk::Widget, f64, f64) + 'static) {
        *self.context.borrow_mut() = Some(Box::new(f));
    }

    pub(super) fn set_mode(self: &Rc<Self>, mode: BrowserMode) {
        self.mode.set(mode);
        self.refresh();
        self.show_page();
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
        (0..).take_while(|&d| self.browser.location_at(d).is_some()).count()
    }

    fn wire_column(self: &Rc<Self>, depth: usize, column: &Rc<Column>) {
        let weak = Rc::downgrade(self);
        *column.model.hooks.bind.borrow_mut() = Some(Box::new(move |position, entry| {
            if let Some(tab) = weak.upgrade()
                && entry.modified_unix_seconds == MetadataValue::Unknown
            {
                tab.browser.request_visible_metadata_fill(depth, position, entry.location.clone(), false);
            }
        }));
        let (weak, model) = (Rc::downgrade(self), Rc::downgrade(&column.model));
        *column.model.hooks.context.borrow_mut() = Some(Box::new(move |index, widget, x, y| {
            let (Some(tab), Some(model)) = (weak.upgrade(), model.upgrade()) else { return };
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
        column.model.selection.connect_selection_changed(move |_, _, _| {
            if let Some(tab) = weak.upgrade() {
                tab.column_selected(depth);
            }
        });
        let (weak, model) = (Rc::downgrade(self), Rc::downgrade(&column.model));
        column.view.connect_activate(move |_, index| {
            let (Some(tab), Some(model)) = (weak.upgrade(), model.upgrade()) else { return };
            let Some(entry) = model.entry(index) else { return };
            if entry.is_directory() {
                tab.browser.descend(depth, entry.location.clone());
            } else {
                tab.open(&entry);
            }
        });
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(self);
        crate::ui::keys::on_press(&keys, move |_, press| {
            let Some(tab) = weak.upgrade() else { return glib::Propagation::Proceed };
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
        let Some(column) = self.columns.column(depth) else { return };
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
        let Some(column) = self.columns.column(depth) else { return };
        let selected = column.model.selected_positions();
        let positions: Vec<usize> = selected.iter().map(|(_, p)| *p).collect();
        self.browser.set_active_column(depth);
        self.browser.set_selection(depth, &positions, positions.last().copied());
        let entries = column.model.selected();
        match entries.as_slice() {
            [entry] if entry.is_directory() => self.browser.show_child(depth, entry.location.clone()),
            _ if self.columns.column(depth + 1).is_some() => self.browser.close_column(depth + 1),
            _ => {}
        }
    }

    fn refresh_columns(self: &Rc<Self>) {
        let count = self.column_count();
        self.columns.ensure(count, |depth, column| self.wire_column(depth, column));
        self.syncing.set(true);
        for depth in 0..count {
            let Some(column) = self.columns.column(depth) else { continue };
            let show_hidden = self.browser.column_preferences(depth).is_some_and(|p| p.show_hidden);
            self.browser.with_column_entries(depth, |entries| column.model.set_entries(entries, show_hidden));
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
        self.depth().and_then(|d| self.browser.column_preferences(d)).is_some_and(|p| p.show_hidden)
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
        self.browser.set_selection(depth, &positions, positions.last().copied());
    }

    /// The tab's folder or title changed (for the tab row and header).
    pub(super) fn connect_changed(&self, f: impl Fn() + 'static) {
        self.changed.borrow_mut().push(Box::new(f));
    }

    pub(super) fn location(&self) -> Option<Location> {
        self.depth().and_then(|depth| self.browser.location_at(depth))
    }

    pub(super) fn title(&self) -> String {
        match self.location() {
            Some(location) if location.native_path() == Some(home().as_path()) => crate::i18n::tr("Home"),
            Some(location) => location.display_name(),
            None => crate::i18n::tr("Files"),
        }
    }

    fn depth(&self) -> Option<usize> {
        self.browser.active_depth()
    }

    fn on_event(self: &Rc<Self>, event: &BrowserEvent) {
        match event {
            BrowserEvent::EntryCreated { location } => *self.select_next.borrow_mut() = Some(location.clone()),
            BrowserEvent::ItemRelocated { to, .. } => *self.select_next.borrow_mut() = Some(to.clone()),
            _ => {}
        }
        if let BrowserEvent::OpenRequested { .. } = event {
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
        let Some(snapshot) = self.browser.column_snapshot(depth) else { return };
        if let Some(error) = &snapshot.error {
            self.status.set_text(error);
            self.page.set_visible_child_name("status");
        } else {
            self.page.set_visible_child_name(self.view_page());
        }
        let key = (snapshot.location.clone(), snapshot.count);
        let moved = self.shown.borrow().as_ref().map(|(l, _)| l) != Some(&key.0);
        let columns = self.mode.get() == BrowserMode::Columns;
        if columns {
            self.refresh_columns();
        } else {
            let show_hidden = self.shows_hidden();
            self.syncing.set(true);
            self.browser.with_column_entries(depth, |entries| self.model.set_entries(entries, show_hidden));
            self.syncing.set(false);
        }
        if let Some(location) = self.select_next.borrow().clone() {
            let model = self.active_model();
            self.syncing.set(!columns);
            if model.select_location(&location).is_some() {
                self.select_next.borrow_mut().take();
            }
            self.syncing.set(false);
        }
        if !columns {
            self.push_selection();
        }
        *self.shown.borrow_mut() = Some(key);
        if moved {
            if self.model.len() > 0 {
                self.list.view.scroll_to(0, None, gtk::ListScrollFlags::NONE, None);
                self.icons.view.scroll_to(0, gtk::ListScrollFlags::NONE, None);
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
        let file = match entry.location.native_path() {
            Some(path) => gio::File::for_path(path),
            None => match entry.location.uri_value() {
                Some(uri) => gio::File::for_uri(uri),
                None => return,
            },
        };
        let uri = file.uri();
        if let Err(error) = gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>) {
            tracing::warn!(%error, "couldn't open {uri}");
        }
    }

    /// The breadcrumbs for the folder showing: from Home when inside it, else from `/`.
    pub(super) fn crumbs(&self) -> (Vec<nyx_ui::pathbar::Crumb>, String) {
        let Some(location) = self.location() else { return (Vec::new(), String::new()) };
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
                label: if root { String::new() } else { here.display_name() },
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
