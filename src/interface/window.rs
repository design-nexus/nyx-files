// SPDX-License-Identifier: MIT

//! The main window: Nyx header, tab row, sidebar and the tabs' pages.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gtk::{gdk, glib, prelude::*};
use nyx_ui::{chrome, pathbar::PathBar, sidebar::Sidebar, tabs::TabRow, widgets};

use crate::{
    model::Location,
    ui::{
        browser_modes::BrowserMode,
        keys::{Binding, KeyPress, Keys, bindings},
        preferences::PreferenceManager,
    },
};

use super::{
    actions, settings,
    sidebar::{SidebarContent, Target},
    tab,
};

thread_local! {
    static WINDOWS: RefCell<Vec<Rc<Window>>> = const { RefCell::new(Vec::new()) };
}

pub(super) struct Window {
    pub(super) window: gtk::ApplicationWindow,
    pub(super) overlay: gtk::Overlay,
    tab_row: Rc<TabRow>,
    pages: gtk::Stack,
    sidebar: Rc<Sidebar>,
    sidebar_toggle: gtk::ToggleButton,
    content: Rc<SidebarContent>,
    path: Rc<PathBar>,
    back: gtk::Button,
    forward: gtk::Button,
    up: gtk::Button,
    tabs: RefCell<Vec<Rc<tab::Tab>>>,
    /// Closed tabs kept until their file operations finish, so progress and
    /// results still show.
    retired: RefCell<Vec<Rc<tab::Tab>>>,
    active: Cell<usize>,
    view_switch: RefCell<Vec<(BrowserMode, gtk::ToggleButton)>>,
    filter: gtk::SearchEntry,
    pub(super) progress: Rc<super::operations::Progress>,
    /// A folder being made for the selection, and what moves into it.
    pub(super) pending_group: RefCell<Option<(Location, Vec<Location>)>>,
    presence: Cell<(crate::app::Presence, bool)>,
    header_actions: RefCell<Option<Rc<super::header::Actions>>>,
    menu_point: RefCell<Option<(glib::WeakRef<gtk::Widget>, f64, f64)>>,
}

/// Drops every window; the app is shutting down.
pub(super) fn release_all() {
    let windows = WINDOWS.with(|w| std::mem::take(&mut *w.borrow_mut()));
    drop(windows);
}

/// The window with focus, for keys that reach every window.
pub(super) fn focused() -> Option<Rc<Window>> {
    WINDOWS.with(|windows| {
        let windows = windows.borrow();
        windows
            .iter()
            .find(|w| w.window.is_active())
            .or_else(|| windows.last())
            .cloned()
    })
}

fn start_location() -> Location {
    let prefs = PreferenceManager::shared();
    Location::local(prefs.default_directory().unwrap_or_else(tab::home))
}

/// True while the keyboard is typing into a text field.
fn typing(window: &gtk::ApplicationWindow) -> bool {
    gtk::prelude::GtkWindowExt::focus(window)
        .is_some_and(|f| f.is::<gtk::Text>() || f.is::<gtk::TextView>())
}

const QUIT: Binding = Binding::ctrl(gdk::Key::q);

fn with_keys(message: &str, binding: Binding) -> String {
    crate::ui::shortcut_reference::with_keys(message, Keys::One(binding))
}

impl Window {
    pub(super) fn open(application: &gtk::Application, location: Option<Location>) -> Rc<Window> {
        let window = gtk::ApplicationWindow::builder()
            .application(application)
            .title("Files")
            .default_width(1280)
            .default_height(800)
            .build();
        window.add_css_class("files-window");
        window.set_titlebar(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));

        let header = chrome::app_header(&window, "folder-symbolic", &crate::i18n::tr("Files"));
        let sidebar_toggle = gtk::ToggleButton::new();
        sidebar_toggle.set_icon_name("sidebar-show-symbolic");
        sidebar_toggle.add_css_class("bar-button");
        sidebar_toggle.set_valign(gtk::Align::Center);
        sidebar_toggle.set_tooltip_text(Some(&with_keys("Sidebar (%{keys})", bindings::SIDEBAR)));
        header.start.append(&sidebar_toggle);
        let back = widgets::bar_button(
            "go-previous-symbolic",
            &with_keys("Back (%{keys})", bindings::HISTORY_BACK),
        );
        let forward = widgets::bar_button(
            "go-next-symbolic",
            &with_keys("Forward (%{keys})", bindings::HISTORY_FORWARD),
        );
        let up = widgets::bar_button(
            "go-up-symbolic",
            &with_keys("Enclosing folder (%{keys})", bindings::PARENT_FOLDER),
        );
        let navigation = super::header::group();
        navigation.set_margin_start(6);
        for button in [&back, &forward, &up] {
            navigation.append(button);
        }
        header.start.append(&navigation);
        let path = PathBar::new();
        let path_holder = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let clamp = nyx_ui::clamp::Clamp::new(&path_holder, 380);
        clamp.set_start(true);
        clamp.set_margin_start(8);
        clamp.set_hexpand(false);
        header.start.append(&clamp);

        let filter = gtk::SearchEntry::new();
        filter.add_css_class("header-search");
        filter.set_placeholder_text(Some(&with_keys("Filter (%{keys})", bindings::FILTER)));
        filter.set_valign(gtk::Align::Center);
        filter.set_width_chars(16);
        crate::ui::tenxer_mode::hide_while_enabled(&filter);
        let tab_row = TabRow::new(&with_keys("New tab (%{keys})", bindings::TAB_NEW));
        tab_row.set_hide_single(true);

        let holder: Rc<RefCell<std::rc::Weak<Window>>> = Rc::default();
        let (h1, h2) = (holder.clone(), holder.clone());
        let content = SidebarContent::new(
            move |location| {
                if let Some(w) = h1.borrow().upgrade() {
                    w.navigate(location);
                }
            },
            move |target, widget, x, y| {
                if let Some(w) = h2.borrow().upgrade() {
                    w.sidebar_menu(target, widget, x, y);
                }
            },
        );
        let sidebar = content.sidebar.clone();
        let pages = gtk::Stack::new();
        pages.set_hexpand(true);
        pages.add_css_class("files-pages");
        let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        body.append(&sidebar.root);
        body.append(&pages);
        body.set_vexpand(true);
        let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
        frame.add_css_class("window-frame");
        frame.append(&header.root);
        frame.append(&tab_row.root);
        frame.append(&body);
        let overlay = gtk::Overlay::new();
        let progress = super::operations::Progress::new(&overlay);
        overlay.set_child(Some(&frame));
        window.set_child(Some(&overlay));

        let this = Rc::new(Window {
            window,
            overlay,
            tab_row,
            pages,
            sidebar,
            sidebar_toggle,
            content,
            path,
            back,
            forward,
            up,
            tabs: RefCell::new(Vec::new()),
            retired: RefCell::default(),
            active: Cell::new(0),
            view_switch: RefCell::new(Vec::new()),
            progress,
            pending_group: RefCell::new(None),
            presence: Cell::new((crate::app::Presence::Focused, false)),
            header_actions: RefCell::new(None),
            menu_point: RefCell::new(None),
            filter,
        });
        WINDOWS.with(|w| w.borrow_mut().push(this.clone()));
        this.window.connect_destroy(|window| {
            let heir = WINDOWS.with(|w| {
                let mut windows = w.borrow_mut();
                let owned = windows.first().is_some_and(|first| first.window == *window);
                windows.retain(|kept| kept.window != *window);
                owned.then(|| windows.first().cloned()).flatten()
            });
            if let Some(heir) = heir {
                heir.persist_tabs();
            }
        });

        let settings_button = chrome::settings_button(|| {
            if let Some(w) = focused() {
                settings::open(&w);
            }
        });
        let path_group = super::header::path_group(&this.path, &this);
        path_group.set_hexpand(false);
        path_holder.append(&path_group);
        let header_actions = super::header::Actions::new(&this, &this.filter);
        header.end.append(&header_actions.root);
        this.header_actions.replace(Some(header_actions));
        let search_field = super::header::search_field(&this);
        search_field.set_margin_start(6);
        header.end.append(&search_field);
        let new_tab = super::header::group();
        new_tab.set_margin_start(6);
        let new_tab_button = widgets::bar_button(
            "tab-new-symbolic",
            &with_keys("New tab (%{keys})", bindings::TAB_NEW),
        );
        let weak = Rc::downgrade(&this);
        new_tab_button.connect_clicked(move |_| {
            if let Some(w) = weak.upgrade() {
                let location = w
                    .current()
                    .and_then(|t| t.location())
                    .unwrap_or_else(start_location);
                w.add_tab(location);
            }
        });
        new_tab.append(&new_tab_button);
        header.end.append(&new_tab);
        header.end.append(&this.build_view_switch());
        let appearance = super::appearance::button(&this);
        appearance.add_css_class("header-button");
        header.end.append(&appearance);
        super::appearance::install(&this.window);
        settings_button.add_css_class("header-button");
        header.end.append(&settings_button);

        this.wire();
        *holder.borrow_mut() = Rc::downgrade(&this);
        let weak = Rc::downgrade(&this);
        crate::ui::presence::follow(&this.window, move |presence, power_saver| {
            if let Some(w) = weak.upgrade() {
                w.presence.set((presence, power_saver));
                w.share_presence();
            }
        });
        let weak = Rc::downgrade(&this);
        this.content
            .connect_drop(move |destination, sources, action| {
                let Some(w) = weak.upgrade() else { return };
                let Some(tab) = w.current() else { return };
                if destination.uri_value() == Some("trash:///") {
                    let entries: Vec<_> = sources
                        .iter()
                        .filter_map(super::actions::entry_for)
                        .collect();
                    actions::trash(&w, &tab, entries);
                } else {
                    actions::drop_into(&w, &tab, destination, sources, action);
                }
            });
        this.sidebar.follow_window(&this.window);
        this.sidebar.append_foot(&this.shortcuts_button());
        let expanded = PreferenceManager::shared().sidebar_expanded();
        this.sidebar_toggle.set_active(expanded);
        this.sidebar.set_collapsed(!expanded);

        this.add_tab(location.unwrap_or_else(start_location));
        crate::ui::jobs::guard_close(&this.window);
        this.window.present();
        crate::metrics::mark_window_presented();
        this.window.add_tick_callback(|_, _| {
            crate::metrics::mark_first_themed_frame();
            glib::ControlFlow::Break
        });
        // Developer aid, like nyx-ui's NYX_PALETTE: open settings once the window is up.
        if std::env::var_os("NYX_FILES_SETTINGS").is_some() {
            let weak = Rc::downgrade(&this);
            glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
                if let Some(w) = weak.upgrade() {
                    settings::open(&w);
                }
            });
        }
        this
    }

    fn wire(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.filter.connect_search_changed(move |entry| {
            if let Some(tab) = weak.upgrade().and_then(|w| w.current())
                && tab.filter_text() != entry.text().trim()
            {
                tab.set_filter(&entry.text());
            }
        });
        let weak = Rc::downgrade(self);
        self.filter.connect_activate(move |_| {
            if let Some(tab) = weak.upgrade().and_then(|w| w.current()) {
                tab.open_selected_result();
            }
        });
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            if key == gdk::Key::Down
                && modifiers.is_empty()
                && let Some(tab) = weak.upgrade().and_then(|w| w.current())
            {
                tab.focus_results();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        self.filter.add_controller(keys);
        let weak = Rc::downgrade(self);
        self.filter.connect_stop_search(move |_| {
            if let Some(w) = weak.upgrade() {
                w.close_filter();
                if let Some(tab) = w.current() {
                    tab.focus_view();
                }
            }
        });
        let weak = Rc::downgrade(self);
        self.sidebar_toggle.connect_toggled(move |toggle| {
            if let Some(w) = weak.upgrade() {
                PreferenceManager::shared().set_sidebar_expanded(toggle.is_active());
                w.sidebar.set_collapsed(!toggle.is_active());
            }
        });
        let weak = Rc::downgrade(self);
        self.back.connect_clicked(move |_| {
            if let Some(tab) = weak.upgrade().and_then(|w| w.current()) {
                tab.browser.back();
            }
        });
        let weak = Rc::downgrade(self);
        self.forward.connect_clicked(move |_| {
            if let Some(tab) = weak.upgrade().and_then(|w| w.current()) {
                tab.browser.forward();
            }
        });
        let weak = Rc::downgrade(self);
        self.up.connect_clicked(move |_| {
            if let Some(tab) = weak.upgrade().and_then(|w| w.current()) {
                tab.browser.parent();
            }
        });
        let weak = Rc::downgrade(self);
        self.path.connect_open(move |target| {
            if let Some(w) = weak.upgrade() {
                w.navigate(tab::location_of(target));
            }
        });
        let weak = Rc::downgrade(self);
        self.path.connect_typed(move |text| {
            let Some(w) = weak.upgrade() else { return };
            let Some(tab) = w.current() else { return };
            let base = tab
                .location()
                .and_then(|l| l.native_path().map(std::path::PathBuf::from));
            if let Err(error) = tab.browser.navigate_input_located(text, base.as_deref()) {
                nyx_ui::toast::show(&w.overlay, &error.to_string());
            }
        });
        let weak = Rc::downgrade(self);
        self.tab_row.connect_select(move |i| {
            if let Some(w) = weak.upgrade() {
                w.select(i);
            }
        });
        let weak = Rc::downgrade(self);
        self.tab_row.connect_close(move |i| {
            if let Some(w) = weak.upgrade() {
                w.close_tab(i);
            }
        });
        let weak = Rc::downgrade(self);
        self.tab_row.connect_new(move || {
            if let Some(w) = weak.upgrade() {
                let location = w
                    .current()
                    .and_then(|t| t.location())
                    .unwrap_or_else(start_location);
                w.add_tab(location);
            }
        });
        let weak = Rc::downgrade(self);
        self.tab_row.connect_reorder(move |from, to| {
            if let Some(w) = weak.upgrade() {
                w.move_tab(from, to);
            }
        });

        nyx_ui::keys::install(
            &self.window,
            nyx_ui::keys::Common {
                settings: Some(|| {
                    if let Some(w) = focused() {
                        settings::open(&w);
                    }
                }),
                shortcuts: None,
                close: false,
                palette: None,
            },
        );
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        crate::ui::keys::on_press(&keys, move |_, press| {
            let Some(w) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if w.handle_tenxer_key(press) {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        self.window.add_controller(keys);
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(self);
        crate::ui::keys::on_press(&keys, move |_, press| {
            let Some(w) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if w.handle_key(press) {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        self.window.add_controller(keys);
    }

    /// 10xer keys run first, ahead of the views, when the mode is on.
    fn handle_tenxer_key(self: &Rc<Self>, press: &KeyPress) -> bool {
        if press.is(bindings::TENXER_MODE) {
            let prefs = PreferenceManager::shared();
            prefs.set_tenxer_mode(!prefs.tenxer_mode());
            return true;
        }
        if press.is_modifier() {
            return false;
        }
        let Some(tab) = self.current() else {
            return false;
        };
        if super::preview_keys::has_focus(&tab) && !tab.footer.prompt_has_focus() {
            use super::preview_keys::Outcome;
            match super::preview_keys::handle(&tab, press, super::tenxer::enabled()) {
                Outcome::Stop => return true,
                Outcome::Widget => return false,
                Outcome::Listing => {}
            }
        }
        if !super::tenxer::enabled() {
            // Space previews before the list views take it to select the row.
            if press.is(bindings::SPACE)
                && !typing(&self.window)
                && !super::preview_keys::has_focus(&tab)
                && self.focus_in_files(&tab)
            {
                return self.space(&tab);
            }
            return false;
        }
        if tab.footer.prompt_has_focus() {
            if press.is(bindings::ESCAPE_ANY_SHIFT) {
                super::tenxer::cancel_prompt(&tab);
                return true;
            }
            return false;
        }
        if typing(&self.window) || !self.focus_in_files(&tab) || tab.footer_key(press) {
            return false;
        }
        super::tenxer::handle(self, &tab, press)
    }

    /// Keys for the files act only while focus is in the tab, not in the
    /// sidebar, the header or Settings.
    fn focus_in_files(&self, tab: &tab::Tab) -> bool {
        gtk::prelude::GtkWindowExt::focus(&self.window)
            .is_none_or(|focus| focus.is_ancestor(&tab.container))
    }

    /// Space: a folder opens; a file shows in the preview, or hides it again.
    fn space(&self, tab: &tab::Tab) -> bool {
        match tab.selected().as_slice() {
            [entry] if entry.is_directory() => tab.open(entry),
            [entry] => tab.preview.toggle(
                crate::ui::preview::preview_target(Some(entry.clone())),
                tab.browser.active_depth(),
            ),
            _ => return false,
        }
        true
    }

    /// "F1 Shortcuts" at the foot of the sidebar: the current tab's shortcut
    /// reference, shown when the setting shows the button.
    fn shortcuts_button(self: &Rc<Self>) -> gtk::Button {
        let label = crate::ui::shortcut_reference::with_keys(
            "%{keys} Shortcuts",
            Keys::One(bindings::REFERENCE),
        );
        let button = gtk::Button::with_label(&label);
        button.add_css_class("sidebar-shortcuts");
        button.add_css_class("compact-hide");
        button.set_halign(gtk::Align::Start);
        button.set_tooltip_text(Some(&crate::ui::shortcut_reference::with_keys(
            "Show all file-view shortcuts (%{keys})",
            Keys::One(bindings::REFERENCE),
        )));
        let weak = Rc::downgrade(self);
        button.connect_clicked(move |_| {
            if let Some(tab) = weak.upgrade().and_then(|w| w.current()) {
                tab.footer.toggle_reference();
            }
        });
        PreferenceManager::shared().bind_preference(
            &button,
            PreferenceManager::show_keybinding_hints,
            |button, shown| button.set_visible(shown),
        );
        button
    }

    pub(super) fn focus_filter(&self) {
        match self.header_actions.borrow().as_ref() {
            Some(actions) => actions.open_filter(&self.filter),
            None => {
                self.filter.grab_focus();
            }
        }
    }

    fn close_filter(&self) {
        match self.header_actions.borrow().as_ref() {
            Some(actions) if actions.filter_toggle.is_active() => {
                actions.filter_toggle.set_active(false)
            }
            _ => self.filter.set_text(""),
        }
    }

    pub(super) fn sync_header(&self) {
        self.sync();
    }

    pub(super) fn focus_sidebar(&self) {
        if !self.sidebar_toggle.is_active() {
            self.sidebar_toggle.set_active(true);
        }
        self.content.focus();
    }

    pub(super) fn edit_path(&self) {
        let (_, text) = self.current().map(|t| t.crumbs()).unwrap_or_default();
        self.path.edit(&text);
    }

    pub(super) fn visible_pins(&self) -> Vec<Location> {
        self.content.visible_pins()
    }

    /// Selects the tab `delta` places away, wrapping.
    pub(super) fn step_tab(&self, delta: isize) {
        let count = self.tabs.borrow().len().max(1) as isize;
        let next = (self.active.get() as isize + delta).rem_euclid(count);
        self.select(next as usize);
    }

    pub(super) fn select_tab(&self, index: usize) {
        self.select(index);
    }

    pub(super) fn close_current_tab(&self) {
        self.close_tab(self.active.get());
    }

    fn handle_key(self: &Rc<Self>, press: &KeyPress) -> bool {
        if self.current().is_some_and(|t| t.footer_key(press)) {
            return true;
        }
        let tab = self.current();
        let count = self.tabs.borrow().len().max(1);
        if press.is(bindings::TAB_NEW) {
            let location = tab
                .and_then(|t| t.location())
                .unwrap_or_else(start_location);
            self.add_tab(location);
        } else if press.is(bindings::TAB_CLOSE) {
            self.close_tab(self.active.get());
        } else if press.is(QUIT) {
            self.window.close();
        } else if press.is(bindings::FILTER) {
            self.focus_filter();
        } else if press.is(bindings::SEARCH) {
            super::search::open(self, super::search::Mode::Everywhere);
        } else if press.is(bindings::JUMP_TO_FOLDER) {
            super::search::open(self, super::search::Mode::Folders);
        } else if press.is(bindings::LOCATION) {
            self.edit_path();
        } else if press.is(bindings::SIDEBAR) {
            self.sidebar_toggle
                .set_active(!self.sidebar_toggle.is_active());
        } else if press.is(bindings::HISTORY_BACK) {
            tab.iter().for_each(|t| t.browser.back());
        } else if press.is(bindings::HISTORY_FORWARD) {
            tab.iter().for_each(|t| t.browser.forward());
        } else if press.is(bindings::PARENT_FOLDER) {
            tab.iter().for_each(|t| t.browser.parent());
        } else if press.is(bindings::VIEW_COLUMNS) {
            self.set_mode(BrowserMode::Columns);
        } else if press.is(bindings::VIEW_ICONS) {
            self.set_mode(BrowserMode::Icons);
        } else if press.is(bindings::VIEW_LIST) {
            self.set_mode(BrowserMode::List);
        } else if press.is_any(&[bindings::HIDDEN_FILES, bindings::HIDDEN_FILES_PERIOD]) {
            tab.iter().for_each(|t| t.browser.toggle_hidden());
        } else if press.is_any(&[bindings::TEXT_LARGER, bindings::TEXT_LARGER_EQUAL]) {
            super::appearance::step_text(1);
        } else if press.is(bindings::TEXT_SMALLER) {
            super::appearance::step_text(-1);
        } else if press.is(bindings::TEXT_RESET) {
            super::appearance::step_text(0);
        } else if let Some(resize) = crate::ui::browser_modes::ThumbnailResize::for_shortcut(press)
        {
            if let Some(tab) = &tab {
                tab.resize_icons(match resize {
                    crate::ui::browser_modes::ThumbnailResize::Step(steps) => steps,
                    crate::ui::browser_modes::ThumbnailResize::Reset => 0,
                });
            }
        } else if press.is(bindings::REFRESH) {
            // A user Reload also retries Trash on mounts that refused it earlier.
            crate::adapters::trash_support::forget_all();
            tab.iter().for_each(|t| t.browser.reload_active());
        } else if press.is(bindings::NEW_FOLDER) {
            tab.iter().for_each(actions::new_folder);
        } else if typing(&self.window)
            || self
                .current()
                .is_some_and(|t| super::preview_keys::has_focus(&t))
        {
            return false;
        } else if self.current().is_some_and(|t| self.focus_in_files(&t))
            && self.handle_file_key(press)
        {
        } else if press.is_any(&[bindings::TAB_NEXT, bindings::TAB_NEXT_PAGE_DOWN]) {
            self.select((self.active.get() + 1) % count);
        } else if press.is_any(&[bindings::TAB_PREVIOUS, bindings::TAB_PREVIOUS_PAGE_UP]) {
            self.select((self.active.get() + count - 1) % count);
        } else {
            return false;
        }
        true
    }

    fn build_view_switch(self: &Rc<Self>) -> gtk::Box {
        let switch = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        switch.add_css_class("segmented");
        switch.add_css_class("view-switch");
        switch.set_valign(gtk::Align::Center);
        switch.set_margin_end(6);
        let current = PreferenceManager::shared().browser_mode();
        let mut first: Option<gtk::ToggleButton> = None;
        for (mode, icon, message, binding) in [
            (
                BrowserMode::Columns,
                "view-dual-symbolic",
                "Columns (%{keys})",
                bindings::VIEW_COLUMNS,
            ),
            (
                BrowserMode::Icons,
                "view-grid-symbolic",
                "Icons (%{keys})",
                bindings::VIEW_ICONS,
            ),
            (
                BrowserMode::List,
                "view-list-symbolic",
                "List (%{keys})",
                bindings::VIEW_LIST,
            ),
        ] {
            let button = gtk::ToggleButton::new();
            button.set_icon_name(icon);
            button.add_css_class("segment");
            button.set_tooltip_text(Some(&with_keys(message, binding)));
            if let Some(f) = &first {
                button.set_group(Some(f));
            } else {
                first = Some(button.clone());
            }
            button.set_active(mode == current);
            let weak = Rc::downgrade(self);
            button.connect_toggled(move |b| {
                if b.is_active()
                    && let Some(w) = weak.upgrade()
                {
                    w.set_mode(mode);
                }
            });
            switch.append(&button);
            self.view_switch.borrow_mut().push((mode, button));
        }
        switch
    }

    fn set_mode(&self, mode: BrowserMode) {
        let Some(tab) = self.current() else {
            return;
        };
        tab.choose_mode(mode);
        self.sync();
        tab.focus_view();
    }

    fn show_mode(&self, mode: BrowserMode) {
        for (m, button) in self.view_switch.borrow().iter() {
            if *m == mode && !button.is_active() {
                button.set_active(true);
            }
        }
    }

    pub(super) fn open_window(&self, location: Location) {
        if let Some(app) = self.window.application() {
            Window::open(&app, Some(location));
        }
    }

    pub(super) fn open_tab(self: &Rc<Self>, location: Location) {
        self.add_tab(location);
    }

    fn show_menu(
        self: &Rc<Self>,
        tab: &Rc<tab::Tab>,
        entry: Option<crate::model::FileEntry>,
        widget: &gtk::Widget,
        x: f64,
        y: f64,
    ) {
        self.menu_point
            .replace(Some((widget.downgrade(), x, y)));
        let items = match entry {
            Some(_) => super::menus::item_menu(self, tab, tab.selected()),
            None => super::menus::folder_menu(self, tab),
        };
        nyx_ui::menu::popup_at(widget, x, y, items);
    }

    /// Where the last context menu opened, for a menu that follows from it.
    pub(super) fn menu_point(&self) -> Option<(gtk::Widget, f64, f64)> {
        let (widget, x, y) = self.menu_point.borrow().clone()?;
        Some((widget.upgrade()?, x, y))
    }

    /// Keys that act on the selected items; not while typing in a field.
    fn handle_file_key(self: &Rc<Self>, press: &KeyPress) -> bool {
        let Some(tab) = self.current() else {
            return false;
        };
        let selected = tab.selected();
        if press.is(bindings::COPY) {
            actions::copy(&selected, false);
        } else if press.is(bindings::CUT) {
            actions::copy(&selected, true);
        } else if press.is(bindings::PASTE) {
            if let Some(here) = tab.location() {
                actions::paste(self, &tab, here);
            }
        } else if press.is(bindings::DELETE_PERMANENTLY) {
            super::menus::delete_permanently(self, &tab, selected);
        } else if press.is(bindings::DELETE) {
            super::menus::remove(self, &tab, selected);
        } else if press.is(bindings::DUPLICATE) {
            super::menus::duplicate(self, &tab, &selected);
        } else if press.is(bindings::NEW_FOLDER_WITH_SELECTION) {
            super::menus::new_folder_with_selection(self, &tab, selected);
        } else if press.is(bindings::RENAME) {
            if let [entry] = selected.as_slice() {
                actions::rename(&tab, entry.clone());
            }
        } else if press.is(bindings::SPACE) {
            return self.space(&tab);
        } else if press.is(bindings::ESCAPE) && tab.filtering() {
            self.close_filter();
            tab.focus_view();
        } else if press.is(bindings::FILTER_SLASH) {
            self.focus_filter();
        } else if press.is(bindings::ESCAPE) && tab.preview.is_open() {
            tab.preview.close();
        } else if tab.preview.handle_video_key(press) {
        } else if press.is(bindings::UNDO) {
            actions::undo(&tab);
        } else if press.is_any(&[bindings::REDO, bindings::REDO_Y]) {
            actions::redo(&tab);
        } else if press.is(bindings::PROPERTIES) {
            super::menus::properties(selected);
        } else if press.is(bindings::SELECT_ALL) {
            tab.active_model().selection.select_all();
        } else if press.is(bindings::OPEN_IN_NEW_TAB) {
            for entry in selected.iter().filter(|e| e.is_directory()) {
                self.add_tab(entry.location.clone());
            }
        } else if let Some(c) = press
            .text()
            .filter(|c| !c.is_control() && !c.is_whitespace())
            && press.modifiers().is_empty()
            && PreferenceManager::shared().type_to_search()
        {
            self.focus_filter();
            self.filter.set_text(&format!("{}{c}", self.filter.text()));
            self.filter.set_position(-1);
        } else {
            return false;
        }
        true
    }

    pub(super) fn navigate_to(&self, location: Location) {
        self.navigate(location);
    }

    /// A location that needs mounting first is mounted, then opened; other
    /// refusals are shown as a toast.
    fn rejected(
        self: &Rc<Self>,
        tab: &Rc<tab::Tab>,
        error: crate::services::LocationValidationError,
    ) {
        use crate::services::LocationValidationError as E;
        match error {
            E::NotMounted(location) | E::Mountable(location) => {
                let (overlay, tab) = (self.overlay.clone(), Rc::downgrade(tab));
                let target = location.clone();
                super::mount::mount_location(&location, move |result| match result {
                    Ok(()) => {
                        if let Some(tab) = tab.upgrade() {
                            tab.browser.navigate(target);
                        }
                    }
                    Err(Some(message)) => nyx_ui::toast::show(&overlay, &message),
                    Err(None) => {}
                });
            }
            other => nyx_ui::toast::show(&self.overlay, &other.to_string()),
        }
    }

    pub(super) fn current_browser(&self) -> Option<Rc<crate::app::Browser>> {
        self.current().map(|t| t.browser.clone())
    }

    fn sidebar_menu(self: &Rc<Self>, target: Target, widget: &gtk::Widget, x: f64, y: f64) {
        nyx_ui::menu::popup_at(widget, x, y, actions::sidebar_menu(self, target));
    }

    /// A search result: folders open; files open directly or are previewed
    /// and shown in their folder, as "Open search results directly" says.
    pub(super) fn open_search_result(&self, item: crate::services::SearchItem) {
        let Some(tab) = self.current() else { return };
        let location = Location::local(item.path.clone());
        if item.is_directory {
            tab.browser.navigate(location);
            return;
        }
        let Some(entry) = actions::entry_for(&location) else {
            return;
        };
        if PreferenceManager::shared().search_open_files_directly() {
            tab.open(&entry);
            return;
        }
        if let Some(parent) = location.parent() {
            tab.browser.reveal_locations(parent, vec![location]);
        }
        tab.preview.show(entry, tab.browser.active_depth());
    }

    pub(super) fn current(&self) -> Option<Rc<tab::Tab>> {
        self.tabs.borrow().get(self.active.get()).cloned()
    }

    fn navigate(&self, location: Location) {
        if let Some(tab) = self.current() {
            tab.browser.navigate(location);
        }
    }

    fn add_tab(self: &Rc<Self>, location: Location) {
        let tab = tab::Tab::new(location);
        // The window shows the Shortcuts button once, under the sidebar.
        tab.footer.remove_shortcuts_button();
        self.pages.add_child(&tab.container);
        let (weak, weak_tab) = (Rc::downgrade(self), Rc::downgrade(&tab));
        tab.connect_context(move |entry, widget, x, y| {
            if let (Some(w), Some(t)) = (weak.upgrade(), weak_tab.upgrade()) {
                w.show_menu(&t, entry, widget, x, y);
            }
        });
        let (weak, weak_tab) = (Rc::downgrade(self), Rc::downgrade(&tab));
        tab.connect_events(move |event| {
            let (weak, weak_tab, event) = (weak.clone(), weak_tab.clone(), event.clone());
            glib::idle_add_local_once(move || {
                if let (Some(w), Some(t)) = (weak.upgrade(), weak_tab.upgrade()) {
                    super::operations::handle(&w, &t, &event);
                    if !t.browser.has_operations() {
                        w.retired.borrow_mut().retain(|r| !Rc::ptr_eq(r, &t));
                    }
                }
            });
        });
        let weak = Rc::downgrade(self);
        tab.connect_result_open(move |item| {
            if let Some(w) = weak.upgrade() {
                w.open_search_result(item);
            }
        });
        let (weak, weak_tab) = (Rc::downgrade(self), Rc::downgrade(&tab));
        tab.connect_rejected(move |error| {
            if let (Some(w), Some(t)) = (weak.upgrade(), weak_tab.upgrade()) {
                w.rejected(&t, error);
            }
        });
        let (weak, weak_tab) = (Rc::downgrade(self), Rc::downgrade(&tab));
        tab.connect_drop(move |destination, sources, action| {
            if let (Some(w), Some(t)) = (weak.upgrade(), weak_tab.upgrade()) {
                actions::drop_into(&w, &t, destination, sources, action);
            }
        });
        let weak = Rc::downgrade(self);
        tab.connect_changed(move || {
            if let Some(w) = weak.upgrade() {
                w.sync();
            }
        });
        super::tenxer::wire(self, &tab);
        let index = {
            let mut tabs = self.tabs.borrow_mut();
            tabs.push(tab);
            tabs.len() - 1
        };
        self.select(index);
    }

    fn share_presence(&self) {
        let (presence, power_saver) = self.presence.get();
        let active = self.active.get();
        for (i, tab) in self.tabs.borrow().iter().enumerate() {
            let shown = if i == active {
                presence
            } else {
                crate::app::Presence::Hidden
            };
            tab.set_presence(shown, power_saver);
        }
    }

    fn select(&self, index: usize) {
        let Some(tab) = self.tabs.borrow().get(index).cloned() else {
            return;
        };
        self.active.set(index);
        self.share_presence();
        self.pages.set_visible_child(&tab.container);
        self.sync();
        tab.focus_view();
    }

    fn close_tab(&self, index: usize) {
        let removed = {
            let mut tabs = self.tabs.borrow_mut();
            if index >= tabs.len() {
                return;
            }
            if tabs.len() == 1 {
                None
            } else {
                Some(tabs.remove(index))
            }
        };
        let Some(tab) = removed else {
            self.window.close();
            return;
        };
        self.pages.remove(&tab.container);
        if tab.browser.has_operations() {
            self.retired.borrow_mut().push(tab);
        }
        let len = self.tabs.borrow().len();
        let active = self.active.get();
        let next = if active > index || active >= len {
            active.saturating_sub(1)
        } else {
            active
        };
        self.select(next.min(len - 1));
    }

    fn move_tab(&self, from: usize, to: usize) {
        {
            let mut tabs = self.tabs.borrow_mut();
            if from >= tabs.len() || to >= tabs.len() {
                return;
            }
            let tab = tabs.remove(from);
            tabs.insert(to, tab);
        }
        let active = self.active.get();
        self.active.set(if active == from {
            to
        } else if from < active && to >= active {
            active - 1
        } else if from > active && to <= active {
            active + 1
        } else {
            active
        });
        self.sync();
    }

    /// The oldest window keeps its tabs for next time; when it closes, the
    /// next oldest takes over.
    fn persist_tabs(&self) {
        let owner = WINDOWS.with(|w| {
            w.borrow()
                .first()
                .is_none_or(|first| first.window == self.window)
        });
        if !owner {
            return;
        }
        let locations: Vec<Location> = self
            .tabs
            .borrow()
            .iter()
            .filter_map(|t| t.location())
            .collect();
        super::launch::persist_session(&locations, self.active.get());
    }

    /// Brings the header, tab row and sidebar up to date with the tabs.
    fn sync(&self) {
        self.persist_tabs();
        let tabs = self.tabs.borrow().clone();
        let shown: Vec<nyx_ui::tabs::Tab> = tabs
            .iter()
            .map(|t| nyx_ui::tabs::Tab {
                title: t.title(),
                icon: None,
                tooltip: t.location().map(|l| l.display_path()),
                dirty: false,
            })
            .collect();
        self.tab_row.set_tabs(&shown, self.active.get());
        // One tab needs no strip; the header's New tab button opens a second.
        self.tab_row.root.set_visible(shown.len() > 1);
        let Some(tab) = tabs.get(self.active.get()) else {
            return;
        };
        if self.filter.text().trim() != tab.filter_text() {
            self.filter.set_text(&tab.filter_text());
        }
        let (crumbs, text) = tab.crumbs();
        self.path.set_crumbs(&crumbs, &text);
        super::header::wire_separators(&self.path, &text);
        if let Some(actions) = self.header_actions.borrow().as_ref() {
            actions.sync(tab);
        }
        self.show_mode(tab.mode());
        self.back.set_sensitive(tab.browser.can_go_back());
        self.forward.set_sensitive(tab.browser.can_go_forward());
        self.up.set_sensitive(tab.browser.can_go_parent());
        let location = tab.location();
        self.content.mark(location.as_ref());
        chrome::set_title(&self.window, "Files", &tab.title());
    }
}
