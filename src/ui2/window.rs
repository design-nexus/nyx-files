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
    active: Cell<usize>,
    view_switch: RefCell<Vec<(BrowserMode, gtk::ToggleButton)>>,
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
        for button in [&back, &forward, &up] {
            header.start.append(button);
        }
        let path = PathBar::new();
        let clamp = nyx_ui::clamp::Clamp::new(&path.root, 720);
        clamp.set_start(true);
        clamp.set_margin_start(8);
        header.start.append(&clamp);

        let tab_row = TabRow::new(&with_keys("New tab (%{keys})", bindings::TAB_NEW));
        tab_row.set_hide_single(false);

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
            active: Cell::new(0),
            view_switch: RefCell::new(Vec::new()),
        });
        WINDOWS.with(|w| w.borrow_mut().push(this.clone()));
        this.window.connect_destroy(|window| {
            WINDOWS.with(|w| w.borrow_mut().retain(|kept| kept.window != *window));
        });

        let settings_button = chrome::settings_button(|| {
            if let Some(w) = focused() {
                settings::open(&w);
            }
        });
        header.end.append(&this.build_view_switch());
        header.end.append(&settings_button);

        this.wire();
        *holder.borrow_mut() = Rc::downgrade(&this);
        this.sidebar.follow_window(&this.window);
        let expanded = PreferenceManager::shared().sidebar_expanded();
        this.sidebar_toggle.set_active(expanded);
        this.sidebar.set_collapsed(!expanded);

        this.add_tab(location.unwrap_or_else(start_location));
        this.window.present();
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

    fn handle_key(self: &Rc<Self>, press: &KeyPress) -> bool {
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
        } else if press.is(bindings::LOCATION) {
            let (_, text) = tab.map(|t| t.crumbs()).unwrap_or_default();
            self.path.edit(&text);
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
        } else if press.is(bindings::REFRESH) {
            tab.iter().for_each(|t| t.browser.reload_active());
        } else if press.is(bindings::NEW_FOLDER) {
            tab.iter().for_each(actions::new_folder);
        } else if typing(&self.window) {
            return false;
        } else if self.handle_file_key(press) {
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
        PreferenceManager::shared().set_browser_mode(mode);
        for tab in self.tabs.borrow().iter() {
            tab.set_mode(mode);
        }
        for (m, button) in self.view_switch.borrow().iter() {
            if *m == mode && !button.is_active() {
                button.set_active(true);
            }
        }
        if let Some(tab) = self.current() {
            tab.focus_view();
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
        let items = match entry {
            Some(_) => actions::item_menu(self, tab, tab.selected()),
            None => actions::folder_menu(self, tab),
        };
        nyx_ui::menu::popup_at(widget, x, y, items);
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
        } else if press.is(bindings::DELETE) {
            actions::trash(self, &tab, selected);
        } else if press.is(bindings::RENAME) {
            if let [entry] = selected.as_slice() {
                actions::rename(&tab, entry.clone());
            }
        } else if press.is(bindings::UNDO) {
            actions::undo(&tab);
        } else if press.is_any(&[bindings::REDO, bindings::REDO_Y]) {
            actions::redo(&tab);
        } else if press.is(bindings::PROPERTIES) {
            if let [entry] = selected.as_slice() {
                super::properties::show(entry);
            }
        } else if press.is(bindings::SELECT_ALL) {
            tab.active_model().selection.select_all();
        } else if press.is(bindings::OPEN_IN_NEW_TAB) {
            for entry in selected.iter().filter(|e| e.is_directory()) {
                self.add_tab(entry.location.clone());
            }
        } else {
            return false;
        }
        true
    }

    pub(super) fn current_browser(&self) -> Option<Rc<crate::app::Browser>> {
        self.current().map(|t| t.browser.clone())
    }

    fn sidebar_menu(self: &Rc<Self>, target: Target, widget: &gtk::Widget, x: f64, y: f64) {
        nyx_ui::menu::popup_at(widget, x, y, actions::sidebar_menu(self, target));
    }

    fn current(&self) -> Option<Rc<tab::Tab>> {
        self.tabs.borrow().get(self.active.get()).cloned()
    }

    fn navigate(&self, location: Location) {
        if let Some(tab) = self.current() {
            tab.browser.navigate(location);
        }
    }

    fn add_tab(self: &Rc<Self>, location: Location) {
        let tab = tab::Tab::new(location);
        self.pages.add_child(&tab.page);
        let (weak, weak_tab) = (Rc::downgrade(self), Rc::downgrade(&tab));
        tab.connect_context(move |entry, widget, x, y| {
            if let (Some(w), Some(t)) = (weak.upgrade(), weak_tab.upgrade()) {
                w.show_menu(&t, entry, widget, x, y);
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
        let index = {
            let mut tabs = self.tabs.borrow_mut();
            tabs.push(tab);
            tabs.len() - 1
        };
        self.select(index);
    }

    fn select(&self, index: usize) {
        let Some(tab) = self.tabs.borrow().get(index).cloned() else {
            return;
        };
        self.active.set(index);
        self.pages.set_visible_child(&tab.page);
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
        self.pages.remove(&tab.page);
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

    /// Brings the header, tab row and sidebar up to date with the tabs.
    fn sync(&self) {
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
        let Some(tab) = tabs.get(self.active.get()) else {
            return;
        };
        let (crumbs, text) = tab.crumbs();
        self.path.set_crumbs(&crumbs, &text);
        self.back.set_sensitive(tab.browser.can_go_back());
        self.forward.set_sensitive(tab.browser.can_go_forward());
        self.up.set_sensitive(tab.browser.can_go_parent());
        let location = tab.location();
        self.content.mark(location.as_ref());
        chrome::set_title(&self.window, "Files", &tab.title());
    }
}
