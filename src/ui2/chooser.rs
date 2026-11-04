// SPDX-License-Identifier: MIT

//! The system file chooser (the portal's Open, Save and Select Folder
//! dialogs) on the 2.0 views, sidebar and header.

mod convert;

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::TryRecvError,
    },
    time::Duration,
};

use ashpd::{
    PortalError,
    desktop::file_chooser::{Choice, FileFilter, SelectedFiles},
};
use gtk::{glib, prelude::*};
use nyx_ui::{chrome, pathbar::PathBar, widgets};

use crate::{
    app::BrowserEvent,
    model::{EntryKind, FileEntry, Location, MetadataValue},
    portal::{
        ChooserKind, ChooserRequest, check_destinations, local_uri, open_selection, safe_filename,
        writable_from_read_only,
    },
    services::{RemoteDownload, download_remote, remote_file_url},
    ui::{
        browser_modes::BrowserMode,
        chooser::{
            ChooserFileSource, PortalFilter, apply_external_parent, chooser_initial_dimensions,
            detect_monitor_geometry, download::DownloadProgress, eligible_open_entries,
            portal_filters,
        },
        keys::{self, Binding, KeyPress, Keys, bindings::*},
        preferences::PreferenceManager,
        tenxer_mode::Prompt,
    },
};

use super::{sidebar::SidebarContent, tab::Tab, tenxer};

type Completion = Box<dyn FnOnce(ashpd::backend::Result<SelectedFiles>)>;

thread_local! {
    static CHOOSERS: RefCell<HashMap<String, Rc<Chooser>>> = RefCell::new(HashMap::new());
}

pub(crate) fn prepare() {
    super::theme::install();
}

pub(crate) fn present(
    request: ChooserRequest,
    cancelled: Arc<AtomicBool>,
    completion: impl FnOnce(ashpd::backend::Result<SelectedFiles>) + 'static,
) {
    super::theme::install();
    if cancelled.load(Ordering::SeqCst) {
        completion(Err(PortalError::Cancelled(
            "file chooser request was cancelled".into(),
        )));
        return;
    }
    let chooser = Chooser::build(request, Box::new(completion));
    let previous = CHOOSERS.with(|c| {
        c.borrow_mut()
            .insert(chooser.request.token.clone(), chooser.clone())
    });
    if let Some(previous) = previous {
        previous.cancel();
    }
    if cancelled.load(Ordering::SeqCst) {
        chooser.cancel();
        return;
    }
    chooser.show();
}

pub(crate) fn cancel(token: &str) {
    if let Some(chooser) = CHOOSERS.with(|c| c.borrow().get(token).cloned()) {
        chooser.cancel();
    }
}

fn with_keys(message: &str, binding: Binding) -> String {
    crate::ui::shortcut_reference::with_keys(message, Keys::One(binding))
}

enum ChoiceControl {
    Boolean {
        id: String,
        check: gtk::CheckButton,
    },
    Select {
        id: String,
        values: Vec<String>,
        dropdown: gtk::DropDown,
    },
}

impl ChoiceControl {
    fn value(&self) -> (String, String) {
        match self {
            Self::Boolean { id, check } => (id.clone(), check.is_active().to_string()),
            Self::Select {
                id,
                values,
                dropdown,
            } => (
                id.clone(),
                values
                    .get(dropdown.selected() as usize)
                    .cloned()
                    .unwrap_or_default(),
            ),
        }
    }
}

fn option_row(label: &str, control: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = widgets::hbox(8);
    row.add_css_class("chooser-option");
    row.append(&widgets::label(label, "dim"));
    row.append(control);
    row
}

fn build_choices(choices: &[Choice], parent: &gtk::Box) -> Vec<ChoiceControl> {
    choices
        .iter()
        .map(|choice| {
            let pairs = choice.pairs();
            if pairs.is_empty() {
                let check = gtk::CheckButton::with_label(choice.label());
                check.set_active(choice.initial_selection() == "true");
                parent.append(&check);
                ChoiceControl::Boolean {
                    id: choice.id().to_owned(),
                    check,
                }
            } else {
                let options: Vec<(String, String)> = pairs
                    .iter()
                    .map(|(value, label)| ((*value).to_owned(), (*label).to_owned()))
                    .collect();
                let dropdown = widgets::dropdown(&options, choice.initial_selection());
                if dropdown.selected() == gtk::INVALID_LIST_POSITION {
                    dropdown.set_selected(0);
                }
                parent.append(&option_row(choice.label(), &dropdown));
                ChoiceControl::Select {
                    id: choice.id().to_owned(),
                    values: options.into_iter().map(|(value, _)| value).collect(),
                    dropdown,
                }
            }
        })
        .collect()
}

/// In 10xer mode, the keys a save dialog adds, shown beside the buttons.
fn save_hints(kind: &ChooserKind) -> Option<gtk::Box> {
    let hints: &[(Binding, &str)] = match kind {
        ChooserKind::SaveFile { .. } => &[
            (ENTER, "Save here"),
            (TENXER_RENAME, "Edit name"),
            (OPEN, "Replace file"),
        ],
        ChooserKind::SaveFiles { .. } => &[(ENTER, "Save here")],
        ChooserKind::Open { .. } => return None,
    };
    let row = widgets::hbox(6);
    row.add_css_class("chooser-hints");
    row.set_valign(gtk::Align::Center);
    for (binding, action) in hints {
        row.append(&widgets::label(
            &keys::label(&Keys::One(*binding)),
            "keycap",
        ));
        row.append(&widgets::label(&crate::i18n::tr(action), "dim"));
    }
    PreferenceManager::shared().bind_preference(
        &row,
        PreferenceManager::tenxer_mode,
        |row, enabled| row.set_visible(enabled),
    );
    Some(row)
}

pub(super) struct Chooser {
    request: ChooserRequest,
    window: gtk::Window,
    tab: Rc<Tab>,
    content: Rc<SidebarContent>,
    sidebar_toggle: gtk::ToggleButton,
    path: Rc<PathBar>,
    back: gtk::Button,
    forward: gtk::Button,
    up: gtk::Button,
    view_switch: RefCell<Vec<(BrowserMode, gtk::ToggleButton)>>,
    source: Rc<ChooserFileSource>,
    filename: Option<gtk::Entry>,
    filename_selection: RefCell<Vec<Location>>,
    filename_edited: Cell<bool>,
    /// The exact name last copied into `filename`; the entry holds only its
    /// lossy UTF-8 rendering.
    filename_source: RefCell<Option<OsString>>,
    filter_dropdown: Option<gtk::DropDown>,
    filters: Vec<PortalFilter>,
    choices: Vec<ChoiceControl>,
    read_only: Option<gtk::CheckButton>,
    error: gtk::Label,
    status: gtk::Stack,
    accept_button: gtk::Button,
    destination_check: Cell<bool>,
    accept_generation: Cell<u64>,
    completion: RefCell<Option<Completion>>,
    download_cancel: RefCell<Option<Arc<AtomicBool>>>,
    downloaded_file: RefCell<Option<(String, PathBuf)>>,
    download_holder: gtk::Box,
    download_progress: RefCell<Option<DownloadProgress>>,
    tenxer: tenxer::State,
    /// A dialog over the chooser (overwrite or convert), while one is open.
    dialog: RefCell<Option<gtk::Window>>,
}

impl Chooser {
    fn build(request: ChooserRequest, completion: Completion) -> Rc<Self> {
        let (filters, selected_filter) =
            portal_filters(&request.filters, request.current_filter.as_ref());
        let source = ChooserFileSource::new();
        source.set_filter(
            selected_filter
                .and_then(|i| filters.get(i))
                .map(|f| f.native.clone()),
        );
        source.directory_only.set(matches!(
            request.kind,
            ChooserKind::Open {
                directory: true,
                ..
            }
        ));
        let tab = Tab::with_source(Location::local(&request.initial_directory), source.clone());

        let (width, height) = chooser_initial_dimensions(detect_monitor_geometry(None, None));
        let window = gtk::Window::builder()
            .title(&request.title)
            .default_width(width)
            .default_height(height)
            .modal(request.modal)
            .build();
        if let Some(application) =
            gtk::gio::Application::default().and_downcast::<gtk::Application>()
        {
            window.set_application(Some(&application));
        }
        window.add_css_class("files-window");
        window.add_css_class("files-chooser");
        window.set_titlebar(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));

        let header = chrome::app_header(&window, "folder-symbolic", &request.title);
        let sidebar_toggle = gtk::ToggleButton::new();
        sidebar_toggle.set_icon_name("sidebar-show-symbolic");
        sidebar_toggle.add_css_class("bar-button");
        sidebar_toggle.set_valign(gtk::Align::Center);
        sidebar_toggle.set_tooltip_text(Some(&with_keys("Sidebar (%{keys})", SIDEBAR)));
        header.start.append(&sidebar_toggle);
        let back = widgets::bar_button(
            "go-previous-symbolic",
            &with_keys("Back (%{keys})", HISTORY_BACK),
        );
        let forward = widgets::bar_button(
            "go-next-symbolic",
            &with_keys("Forward (%{keys})", HISTORY_FORWARD),
        );
        let up = widgets::bar_button(
            "go-up-symbolic",
            &with_keys("Enclosing folder (%{keys})", PARENT_FOLDER),
        );
        for button in [&back, &forward, &up] {
            header.start.append(button);
        }
        let path = PathBar::new();
        let clamp = nyx_ui::clamp::Clamp::new(&path.root, 720);
        clamp.set_start(true);
        clamp.set_margin_start(8);
        header.start.append(&clamp);

        let holder: Rc<RefCell<std::rc::Weak<Tab>>> = Rc::default();
        let h = holder.clone();
        let content = SidebarContent::new(
            move |location| {
                if let Some(tab) = h.borrow().upgrade() {
                    tab.browser.navigate(location);
                }
            },
            |_, _, _, _| {},
        );
        *holder.borrow_mut() = Rc::downgrade(&tab);

        let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        body.append(&content.sidebar.root);
        body.append(&tab.container);
        tab.container.set_hexpand(true);
        body.set_vexpand(true);

        let panel = widgets::vbox(10);
        panel.add_css_class("chooser-panel");
        let filename = match &request.kind {
            ChooserKind::SaveFile { current_name } => {
                let entry = gtk::Entry::new();
                entry.set_hexpand(true);
                entry.set_placeholder_text(Some(&crate::i18n::tr("Enter a filename")));
                if let Some(name) = current_name {
                    entry.set_text(&name.to_string_lossy());
                }
                panel.append(&option_row(&crate::i18n::tr("Name"), &entry));
                Some(entry)
            }
            ChooserKind::SaveFiles { names } => {
                let names =
                    crate::i18n::list(names.iter().map(|name| name.to_string_lossy().into_owned()));
                let label = widgets::label(&names, "");
                label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
                label.set_hexpand(true);
                panel.append(&option_row(&crate::i18n::tr("Files"), &label));
                None
            }
            ChooserKind::Open {
                directory: false, ..
            } => {
                let entry = gtk::Entry::new();
                entry.set_hexpand(true);
                entry.set_placeholder_text(Some(&crate::i18n::tr(
                    "Enter a filename or https:// URL",
                )));
                panel.append(&option_row(&crate::i18n::tr("Name"), &entry));
                Some(entry)
            }
            ChooserKind::Open {
                directory: true, ..
            } => None,
        };
        let options = widgets::hbox(16);
        options.add_css_class("chooser-options");
        let filter_dropdown = (!filters.is_empty()).then(|| {
            let labels: Vec<(String, String)> = filters
                .iter()
                .enumerate()
                .map(|(i, f)| (i.to_string(), f.portal.label().to_owned()))
                .collect();
            let dropdown = widgets::dropdown(&labels, &selected_filter.unwrap_or(0).to_string());
            options.append(&option_row(&crate::i18n::tr("chooser.filter"), &dropdown));
            dropdown
        });
        let choices = build_choices(&request.choices, &options);
        let read_only = matches!(
            request.kind,
            ChooserKind::Open {
                directory: false,
                ..
            }
        )
        .then(|| {
            let check = gtk::CheckButton::with_label(&crate::i18n::tr("Open files read-only"));
            options.append(&check);
            check
        });
        if options.first_child().is_some() {
            panel.append(&options);
        }

        let error = widgets::label("", "error");
        error.set_accessible_role(gtk::AccessibleRole::Alert);
        error.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let download_holder = widgets::hbox(0);
        download_holder.set_visible(false);
        let normal = widgets::hbox(8);
        normal.append(&download_holder);
        if let Some(hints) = save_hints(&request.kind) {
            normal.append(&hints);
        }
        let status = gtk::Stack::builder()
            .hexpand(true)
            .hhomogeneous(false)
            .build();
        status.add_named(&normal, Some("normal"));
        status.add_named(&error, Some("error"));
        let cancel = gtk::Button::with_label(&crate::i18n::tr("Cancel"));
        cancel.set_tooltip_text(Some(&with_keys("Cancel file selection (%{keys})", ESCAPE)));
        let accept = gtk::Button::with_mnemonic(&request.accept_label);
        accept.add_css_class("suggested-action");
        let buttons = widgets::hbox(8);
        buttons.add_css_class("chooser-actions");
        buttons.append(&status);
        buttons.append(&cancel);
        buttons.append(&accept);
        panel.append(&buttons);

        let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
        frame.add_css_class("window-frame");
        frame.append(&header.root);
        frame.append(&body);
        frame.append(&panel);
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&frame));
        window.set_child(Some(&overlay));
        window.set_default_widget(Some(&accept));

        let filename_source = match &request.kind {
            ChooserKind::SaveFile { current_name } => current_name.clone(),
            _ => None,
        };
        let chooser = Rc::new(Chooser {
            request,
            window,
            tab,
            content,
            sidebar_toggle,
            path,
            back,
            forward,
            up,
            view_switch: RefCell::default(),
            source,
            filename,
            filename_selection: RefCell::default(),
            filename_edited: Cell::new(false),
            filename_source: RefCell::new(filename_source),
            filter_dropdown,
            filters,
            choices,
            read_only,
            error,
            status,
            accept_button: accept.clone(),
            destination_check: Cell::new(false),
            accept_generation: Cell::new(0),
            completion: RefCell::new(Some(completion)),
            download_cancel: RefCell::default(),
            downloaded_file: RefCell::default(),
            download_holder,
            download_progress: RefCell::default(),
            tenxer: tenxer::State::default(),
            dialog: RefCell::default(),
        });
        header.end.append(&chooser.build_view_switch());
        let weak = Rc::downgrade(&chooser.tab);
        header
            .end
            .append(&super::appearance::button_for(Rc::new(move || {
                weak.upgrade()
            })));
        super::appearance::install(&chooser.window);
        chooser.wire(&cancel);
        chooser
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
                VIEW_COLUMNS,
            ),
            (
                BrowserMode::Icons,
                "view-grid-symbolic",
                "Icons (%{keys})",
                VIEW_ICONS,
            ),
            (
                BrowserMode::List,
                "view-list-symbolic",
                "List (%{keys})",
                VIEW_LIST,
            ),
        ] {
            let button = gtk::ToggleButton::new();
            button.set_icon_name(icon);
            button.add_css_class("segment");
            button.set_tooltip_text(Some(&with_keys(message, binding)));
            match &first {
                Some(f) => button.set_group(Some(f)),
                None => first = Some(button.clone()),
            }
            button.set_active(mode == current);
            let weak = Rc::downgrade(self);
            button.connect_toggled(move |b| {
                if b.is_active()
                    && let Some(c) = weak.upgrade()
                {
                    c.set_mode(mode);
                }
            });
            switch.append(&button);
            self.view_switch.borrow_mut().push((mode, button));
        }
        switch
    }

    fn set_mode(&self, mode: BrowserMode) {
        PreferenceManager::shared().set_browser_mode(mode);
        self.tab.set_mode(mode);
        for (m, button) in self.view_switch.borrow().iter() {
            if *m == mode && !button.is_active() {
                button.set_active(true);
            }
        }
        self.tab.focus_view();
    }

    fn wire(self: &Rc<Self>, cancel: &gtk::Button) {
        let weak = Rc::downgrade(self);
        self.accept_button.connect_clicked(move |_| {
            if let Some(c) = weak.upgrade() {
                c.accept();
            }
        });
        let weak = Rc::downgrade(self);
        cancel.connect_clicked(move |_| {
            if let Some(c) = weak.upgrade() {
                c.cancel();
            }
        });
        if let Some(filename) = &self.filename {
            let weak = Rc::downgrade(self);
            filename.connect_changed(move |_| {
                if let Some(c) = weak.upgrade() {
                    c.filename_selection.replace(
                        c.name_selection_entries()
                            .iter()
                            .map(|e| e.location.clone())
                            .collect(),
                    );
                    c.filename_edited.set(true);
                }
            });
            let weak = Rc::downgrade(self);
            filename.connect_activate(move |_| {
                if let Some(c) = weak.upgrade() {
                    c.accept();
                }
            });
        }
        if let Some(dropdown) = &self.filter_dropdown {
            let weak = Rc::downgrade(self);
            dropdown.connect_selected_notify(move |d| {
                let Some(c) = weak.upgrade() else { return };
                c.source.set_filter(
                    c.filters
                        .get(d.selected() as usize)
                        .map(|f| f.native.clone()),
                );
                c.tab.browser.reload_active();
            });
        }

        let weak = Rc::downgrade(self);
        self.tab.connect_open_file(move |entry| {
            if let Some(c) = weak.upgrade() {
                c.confirm_file(entry);
            }
        });
        let weak = Rc::downgrade(self);
        self.tab.connect_result_open(move |item| {
            let Some(c) = weak.upgrade() else { return };
            let location = Location::local(&item.path);
            if item.is_directory {
                c.tab.browser.navigate(location);
            } else if let Some(entry) = super::actions::entry_for(&location) {
                c.confirm_file(&entry);
            }
        });
        let weak = Rc::downgrade(self);
        self.tab.connect_events(move |event| {
            let Some(c) = weak.upgrade() else { return };
            match event {
                BrowserEvent::OpenRequested { location } => c.activate_file(location),
                BrowserEvent::FocusChanged { .. }
                | BrowserEvent::SelectionSetChanged { .. }
                | BrowserEvent::SelectionSynced { .. } => {
                    let weak = Rc::downgrade(&c);
                    glib::idle_add_local_once(move || {
                        if let Some(c) = weak.upgrade() {
                            c.update_selected_filename();
                        }
                    });
                }
                _ => {}
            }
        });
        let weak = Rc::downgrade(self);
        self.tab.connect_rejected(move |error| {
            if let Some(c) = weak.upgrade() {
                c.show_error(&error.to_string());
            }
        });
        let weak = Rc::downgrade(self);
        self.tab.connect_changed(move || {
            if let Some(c) = weak.upgrade() {
                c.sync();
            }
        });
        let weak = Rc::downgrade(self);
        self.tab.footer.connect_prompt_activated(move || {
            if let Some(c) = weak.upgrade() {
                tenxer::submit_prompt(&c.tenxer, None, &c.tab);
            }
        });
        let weak = Rc::downgrade(&self.tab);
        self.tab.footer.connect_prompt_changed(move |kind, text| {
            let Some(tab) = weak.upgrade() else { return };
            match kind {
                Prompt::Filter => tab.set_search(&text, false),
                Prompt::Search => tab.set_search(&text, true),
                _ => {}
            }
        });
        let content = Rc::downgrade(&self.content);
        self.tab.footer.connect_chord_changed(move |chord| {
            if let Some(content) = content.upgrade() {
                content.show_keycaps(chord == Some(crate::ui::tenxer_mode::Chord::Go));
            }
        });

        let weak = Rc::downgrade(self);
        self.sidebar_toggle.connect_toggled(move |toggle| {
            if let Some(c) = weak.upgrade() {
                c.content.sidebar.set_collapsed(!toggle.is_active());
            }
        });
        let weak = Rc::downgrade(&self.tab);
        self.back.connect_clicked(move |_| {
            if let Some(tab) = weak.upgrade() {
                tab.browser.back();
            }
        });
        let weak = Rc::downgrade(&self.tab);
        self.forward.connect_clicked(move |_| {
            if let Some(tab) = weak.upgrade() {
                tab.browser.forward();
            }
        });
        let weak = Rc::downgrade(&self.tab);
        self.up.connect_clicked(move |_| {
            if let Some(tab) = weak.upgrade() {
                tab.browser.parent();
            }
        });
        let weak = Rc::downgrade(&self.tab);
        self.path.connect_open(move |target| {
            if let Some(tab) = weak.upgrade() {
                tab.browser.navigate(super::tab::location_of(target));
            }
        });
        let weak = Rc::downgrade(self);
        self.path.connect_typed(move |text| {
            let Some(c) = weak.upgrade() else { return };
            let base = c
                .tab
                .location()
                .and_then(|l| l.native_path().map(PathBuf::from));
            if let Err(error) = c.tab.browser.navigate_input_located(text, base.as_deref()) {
                c.show_error(&error.to_string());
            }
        });

        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        crate::ui::keys::on_press(&keys, move |_, press| match weak.upgrade() {
            Some(c) if c.handle_key(press) => glib::Propagation::Stop,
            _ => glib::Propagation::Proceed,
        });
        self.window.add_controller(keys);

        let weak = Rc::downgrade(self);
        self.window.connect_close_request(move |_| {
            if let Some(c) = weak.upgrade() {
                c.cancel();
            }
            glib::Propagation::Proceed
        });
        self.content.sidebar.follow_window(&self.window);
        let expanded = PreferenceManager::shared().sidebar_expanded();
        self.sidebar_toggle.set_active(expanded);
        self.content.sidebar.set_collapsed(!expanded);
    }

    fn sync(&self) {
        let (crumbs, text) = self.tab.crumbs();
        self.path.set_crumbs(&crumbs, &text);
        self.back.set_sensitive(self.tab.browser.can_go_back());
        self.forward
            .set_sensitive(self.tab.browser.can_go_forward());
        self.up.set_sensitive(self.tab.browser.can_go_parent());
        self.content.mark(self.tab.location().as_ref());
    }

    fn show(self: &Rc<Self>) {
        WidgetExt::realize(&self.window);
        apply_external_parent(&self.window, self.request.parent.as_ref());
        let (width, height) =
            chooser_initial_dimensions(detect_monitor_geometry(None, Some(&self.window)));
        self.window.set_default_size(width, height);
        self.sync();
        self.window.present();
        if PreferenceManager::shared().tenxer_mode() {
            self.tab.focus_view();
        } else if let Some(filename) = &self.filename {
            filename.grab_focus();
            filename.select_region(0, stem_end(&filename.text()));
        } else {
            self.tab.focus_view();
        }
    }

    fn typing(&self) -> bool {
        gtk::prelude::GtkWindowExt::focus(&self.window)
            .is_some_and(|w| w.is::<gtk::Text>() || w.ancestor(gtk::Text::static_type()).is_some())
    }

    fn handle_key(self: &Rc<Self>, press: &KeyPress) -> bool {
        if press.is_modifier() || self.dialog.borrow().is_some() {
            return false;
        }
        let prefs = PreferenceManager::shared();
        if press.is(TENXER_MODE) {
            prefs.set_tenxer_mode(!prefs.tenxer_mode());
            return true;
        }
        let tab = &self.tab;
        if tab.footer.prompt_has_focus() {
            if press.is(ESCAPE_ANY_SHIFT) {
                tenxer::cancel_prompt(tab);
                return true;
            }
            return false;
        }
        if tab.footer_key(press) {
            return true;
        }
        if press.is(ESCAPE_ANY) {
            if prefs.tenxer_mode() && self.typing() {
                tab.focus_view();
            } else if tab.filtering() {
                tab.set_filter("");
                tab.focus_view();
            } else if tab.preview.is_open() {
                tab.preview.close();
            } else if !self.cancel_download() {
                self.cancel();
            }
            return true;
        }
        if press.is(LOCATION) {
            let (_, text) = tab.crumbs();
            self.path.edit(&text);
            return true;
        }
        if press.is(SIDEBAR) {
            self.sidebar_toggle
                .set_active(!self.sidebar_toggle.is_active());
            return true;
        }
        if self.typing() {
            return false;
        }
        if prefs.tenxer_mode() {
            return self.tenxer_key(press);
        }
        if press.is(HISTORY_BACK) {
            tab.browser.back();
        } else if press.is(HISTORY_FORWARD) {
            tab.browser.forward();
        } else if press.is(PARENT_FOLDER) {
            tab.browser.parent();
        } else if press.is(VIEW_COLUMNS) {
            self.set_mode(BrowserMode::Columns);
        } else if press.is(VIEW_ICONS) {
            self.set_mode(BrowserMode::Icons);
        } else if press.is(VIEW_LIST) {
            self.set_mode(BrowserMode::List);
        } else if press.is_any(&[HIDDEN_FILES, HIDDEN_FILES_PERIOD]) {
            tab.browser.toggle_hidden();
        } else if press.is(SPACE) {
            match tab.selected().as_slice() {
                [entry] if entry.is_directory() => tab.open(entry),
                [_] => tab.toggle_preview(),
                _ => return false,
            }
        } else if press.is(SELECT_ALL) && self.multiple() {
            tab.active_model().selection.select_all();
        } else if press.is(PROPERTIES) {
            super::menus::properties(tab.selected());
        } else if press.is(FILTER_SLASH) || press.is(FILTER) {
            tab.footer
                .open_prompt_with(Prompt::Filter, &tab.filter_text());
        } else {
            return false;
        }
        true
    }

    fn multiple(&self) -> bool {
        matches!(self.request.kind, ChooserKind::Open { multiple: true, .. })
    }

    fn save_request(&self) -> bool {
        matches!(
            self.request.kind,
            ChooserKind::SaveFile { .. } | ChooserKind::SaveFiles { .. }
        )
    }

    fn tenxer_key(self: &Rc<Self>, press: &KeyPress) -> bool {
        let tab = &self.tab;
        if tab.footer.armed_chord().is_none() {
            if let Some(reason) =
                crate::ui::window::keyboard::chooser::refusal(press, self.multiple(), true)
            {
                tab.footer.show_feedback(&crate::i18n::tr(reason));
                return true;
            }
            if press.is(CHOOSE_FOLDER) || (press.is(ENTER) && self.save_request()) {
                self.accept();
                return true;
            }
            if press.is(TENXER_RENAME) && self.save_request() {
                self.edit_name();
                return true;
            }
            if press.is(TENXER_SIDEBAR) {
                self.sidebar_toggle.set_active(true);
                self.content.focus();
                return true;
            }
        }
        tenxer::browse(&self.tenxer, tab, press, &self.content.visible_pins())
    }

    fn edit_name(&self) {
        if let Some(filename) = &self.filename {
            filename.grab_focus();
            filename.select_region(0, stem_end(&filename.text()));
        }
    }

    fn cancel(&self) {
        self.finish(Err(PortalError::Cancelled("file chooser dismissed".into())));
    }

    fn finish(&self, result: ashpd::backend::Result<SelectedFiles>) {
        self.cancel_download();
        let Some(completion) = self.completion.take() else {
            return;
        };
        CHOOSERS.with(|c| {
            let mut choosers = c.borrow_mut();
            if choosers
                .get(&self.request.token)
                .is_some_and(|kept| kept.window == self.window)
            {
                choosers.remove(&self.request.token);
            }
        });
        if let Some(dialog) = self.dialog.take() {
            dialog.close();
        }
        self.window.close();
        completion(result);
    }

    fn show_error(&self, message: &str) {
        self.error.set_label(message);
        self.status.set_visible_child_name("error");
    }

    fn clear_error(&self) {
        self.status.set_visible_child_name("normal");
    }

    /// A dialog over the chooser; only one at a time.
    fn dialog(self: &Rc<Self>, title: &str) -> (gtk::Window, gtk::Box) {
        let (dialog, card) = widgets::dialog(title, 420);
        dialog.set_transient_for(Some(&self.window));
        dialog.add_css_class("files-window");
        let weak = Rc::downgrade(self);
        dialog.connect_close_request(move |_| {
            if let Some(c) = weak.upgrade() {
                c.dialog.take();
            }
            glib::Propagation::Proceed
        });
        self.dialog.replace(Some(dialog.clone()));
        (dialog, card)
    }

    /// A yes/no dialog; `confirm` runs on the confirming button.
    fn confirm(
        self: &Rc<Self>,
        title: &str,
        text: &str,
        action: &str,
        destructive: bool,
        confirm: impl Fn(&Rc<Self>) + 'static,
    ) {
        let (dialog, card) = self.dialog(title);
        let note = widgets::label(text, "dim");
        note.set_wrap(true);
        card.append(&note);
        let buttons = widgets::hbox(8);
        buttons.set_halign(gtk::Align::End);
        buttons.set_margin_top(6);
        let cancel = gtk::Button::with_label(&crate::i18n::tr("Cancel"));
        let ok = gtk::Button::with_label(action);
        ok.add_css_class(if destructive {
            "destructive-action"
        } else {
            "suggested-action"
        });
        buttons.append(&cancel);
        buttons.append(&ok);
        card.append(&buttons);
        let d = dialog.clone();
        cancel.connect_clicked(move |_| d.close());
        let (d, weak) = (dialog.clone(), Rc::downgrade(self));
        ok.connect_clicked(move |_| {
            d.close();
            if let Some(c) = weak.upgrade() {
                confirm(&c);
            }
        });
        dialog.present();
        cancel.grab_focus();
    }

    fn download_in_progress(&self) -> bool {
        self.download_cancel.borrow().is_some()
    }

    fn cancel_download(&self) -> bool {
        let Some(cancelled) = self.download_cancel.borrow_mut().take() else {
            return false;
        };
        cancelled.store(true, Ordering::SeqCst);
        self.dismiss_download_progress();
        self.accept_button.set_sensitive(true);
        true
    }

    fn show_download_progress(&self, url: &str, on_cancel: Rc<dyn Fn()>) {
        self.dismiss_download_progress();
        let progress = DownloadProgress::new(url, on_cancel);
        self.download_holder.append(&progress.root);
        self.download_holder.set_visible(true);
        self.download_progress.replace(Some(progress));
    }

    fn dismiss_download_progress(&self) {
        if let Some(progress) = self.download_progress.take() {
            self.download_holder.remove(&progress.root);
        }
        self.download_holder.set_visible(false);
    }

    fn open_remote(self: &Rc<Self>, url: &str) {
        if !matches!(
            self.request.kind,
            ChooserKind::Open {
                directory: false,
                ..
            }
        ) {
            self.show_error(&crate::i18n::tr(
                "Remote links are only supported when opening files",
            ));
            return;
        }
        if self.completion.borrow().is_none() || self.dialog.borrow().is_some() {
            return;
        }
        self.accept_generation
            .set(self.accept_generation.get().wrapping_add(1));
        self.cancel_download();
        self.clear_error();
        let cached = self
            .downloaded_file
            .borrow()
            .as_ref()
            .filter(|(previous, _)| previous == url)
            .map(|(_, path)| path.clone());
        match cached {
            Some(path) => self.reuse_download(url.to_owned(), path),
            None => self.download(url.to_owned()),
        }
    }

    fn download(self: &Rc<Self>, url: String) {
        self.downloaded_file.take();
        let cancelled = Arc::new(AtomicBool::new(false));
        self.download_cancel.replace(Some(cancelled.clone()));
        let weak = Rc::downgrade(self);
        self.show_download_progress(
            &url,
            Rc::new(move || {
                if let Some(c) = weak.upgrade() {
                    c.cancel_download();
                }
            }),
        );
        self.accept_button.set_sensitive(false);
        let receiver = download_remote(url.clone(), cancelled.clone());
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(120), move || {
            let Some(c) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let current = || {
                c.download_cancel
                    .borrow()
                    .as_ref()
                    .is_some_and(|flag| Arc::ptr_eq(flag, &cancelled))
            };
            if !current() {
                return glib::ControlFlow::Break;
            }
            let mut latest = None;
            let flow = loop {
                match receiver.try_recv() {
                    Ok(RemoteDownload::Named(name)) => {
                        if let Some(progress) = c.download_progress.borrow().as_ref() {
                            progress.set_name(&name);
                        }
                    }
                    Ok(RemoteDownload::Progress { downloaded, total }) => {
                        latest = Some((downloaded, total));
                    }
                    Ok(RemoteDownload::Finished(path)) => {
                        c.dismiss_download_progress();
                        c.download_cancel.take();
                        c.downloaded_file.replace(Some((url.clone(), path.clone())));
                        c.complete_download(path);
                        break glib::ControlFlow::Break;
                    }
                    Ok(RemoteDownload::Failed(message)) => {
                        c.dismiss_download_progress();
                        c.accept_button.set_sensitive(true);
                        if c.download_cancel.take().is_some() {
                            c.show_error(&message);
                        }
                        break glib::ControlFlow::Break;
                    }
                    Err(TryRecvError::Empty) => break glib::ControlFlow::Continue,
                    Err(TryRecvError::Disconnected) => {
                        c.dismiss_download_progress();
                        c.accept_button.set_sensitive(true);
                        c.download_cancel.take();
                        c.show_error(&crate::i18n::tr(
                            "The download stopped unexpectedly. Try again.",
                        ));
                        break glib::ControlFlow::Break;
                    }
                }
            };
            if current()
                && let Some((downloaded, total)) = latest
                && let Some(progress) = c.download_progress.borrow().as_ref()
            {
                progress.update(downloaded, total);
            }
            flow
        });
    }

    fn finish_remote(&self, path: PathBuf) {
        let name = path
            .file_name()
            .map(OsStr::to_os_string)
            .unwrap_or_default();
        let entry = FileEntry {
            location: Location::local(&path),
            thumbnail_path: None,
            native_name: name.clone(),
            display_name: name.to_string_lossy().into_owned(),
            kind: EntryKind::File,
            size: MetadataValue::Unknown,
            modified_unix_seconds: MetadataValue::Unknown,
            recent_unix_seconds: MetadataValue::Unknown,
            mode: MetadataValue::Unknown,
            image_dimensions: MetadataValue::Unknown,
            child_count: MetadataValue::Unknown,
            duration_seconds: MetadataValue::Unknown,
            is_hidden: false,
            recent_uri: None,
        };
        if !self.tab.browser.allows_entry(&entry) {
            self.accept_button.set_sensitive(true);
            self.show_error(&crate::i18n::tr(
                "The file does not match the selected filter",
            ));
            return;
        }
        self.complete_paths(vec![path], self.writable());
    }

    fn writable(&self) -> Option<bool> {
        self.read_only
            .as_ref()
            .map(|check| writable_from_read_only(check.is_active()))
    }

    /// The selection; the automatic first-row selection counts only when
    /// `load_cursor` allows it.
    fn chosen_entries(&self, load_cursor: bool) -> Vec<FileEntry> {
        if self.tab.browser.selection_is_load_cursor() && !load_cursor {
            return Vec::new();
        }
        self.tab.selected()
    }

    fn has_fill(&self) -> bool {
        !self.tab.browser.selection_is_load_cursor() && !self.tab.selected().is_empty()
    }

    fn selected_folder(&self) -> Option<PathBuf> {
        if PreferenceManager::shared().tenxer_mode() {
            return None;
        }
        match self.chosen_entries(false).as_slice() {
            [entry] if entry.is_directory() => entry.location.native_path().map(Path::to_path_buf),
            _ => None,
        }
    }

    fn name_selection_entries(&self) -> Vec<FileEntry> {
        if self.tab.browser.selection_is_load_cursor() && self.tab.selected().len() <= 1 {
            return Vec::new();
        }
        self.chosen_entries(true)
    }

    fn fill_filename(&self, filename: &gtk::Entry, name: &OsStr) {
        filename.set_text(&name.to_string_lossy());
        self.filename_source.replace(Some(name.to_owned()));
    }

    fn clear_filename(&self, filename: &gtk::Entry) {
        filename.set_text("");
        self.filename_source.replace(None);
        self.filename_edited.set(false);
    }

    fn save_destination_name(&self, text: &str) -> OsString {
        match self.filename_source.borrow().as_deref() {
            Some(source) if source.to_string_lossy() == text => source.to_owned(),
            _ => OsString::from(text),
        }
    }

    fn update_selected_filename(&self) {
        let Some(filename) = &self.filename else {
            return;
        };
        if PreferenceManager::shared().tenxer_mode() {
            return;
        }
        let entries = self.name_selection_entries();
        let selected: Vec<Location> = entries.iter().map(|e| e.location.clone()).collect();
        if self.filename_selection.replace(selected.clone()) == selected {
            return;
        }
        if let [entry] = entries.as_slice()
            && !entry.is_directory()
            && let Some(name) = entry.location.native_path().and_then(Path::file_name)
            && safe_filename(name)
        {
            self.fill_filename(filename, name);
            self.filename_edited.set(false);
            filename.remove_css_class("error");
        } else if matches!(
            self.request.kind,
            ChooserKind::Open {
                directory: false,
                ..
            }
        ) {
            self.clear_filename(filename);
        }
    }

    fn active_folder(&self) -> Result<PathBuf, &'static str> {
        if let Some(folder) = self.selected_folder() {
            return Ok(folder);
        }
        let current = self.tab.browser.active_location();
        if current.as_ref().is_some_and(Location::is_recent_root)
            && let [entry] = self.chosen_entries(false).as_slice()
            && let Some(parent) = entry.location.native_path().and_then(Path::parent)
        {
            return Ok(parent.to_path_buf());
        }
        current
            .and_then(|l| l.native_path().map(Path::to_path_buf))
            .ok_or("Choose an accessible local folder")
    }

    fn selected_filter(&self) -> Option<FileFilter> {
        self.filter_dropdown
            .as_ref()
            .and_then(|d| self.filters.get(d.selected() as usize))
            .map(|f| f.portal.clone())
    }

    fn complete_paths(&self, paths: Vec<PathBuf>, writable: Option<bool>) {
        let mut result = SelectedFiles::default();
        for path in paths {
            match local_uri(&path) {
                Ok(uri) => result = result.uri(uri),
                Err(error) => {
                    self.finish(Err(error));
                    return;
                }
            }
        }
        for (id, value) in self.choices.iter().map(ChoiceControl::value) {
            result = result.choice(&id, &value);
        }
        result = result
            .current_filter(self.selected_filter())
            .writable(writable);
        self.finish(Ok(result));
    }

    fn accept(self: &Rc<Self>) {
        if self.completion.borrow().is_none()
            || self.destination_check.get()
            || self.download_in_progress()
            || self.dialog.borrow().is_some()
        {
            return;
        }
        self.clear_error();
        match &self.request.kind {
            ChooserKind::Open {
                directory,
                multiple,
            } => {
                self.update_selected_filename();
                if !directory
                    && self.filename_edited.get()
                    && let Some(filename) = &self.filename
                    && !filename.text().is_empty()
                {
                    self.accept_open_name(&filename.text());
                    return;
                }
                let Some(current) = self.tab.browser.active_location() else {
                    self.show_error(&crate::i18n::tr("Choose an accessible local folder"));
                    return;
                };
                let entries: Vec<FileEntry> =
                    eligible_open_entries(self.chosen_entries(!*directory), *directory)
                        .into_iter()
                        .filter(|e| self.tab.browser.allows_entry(e))
                        .collect();
                match open_selection(&entries, &current, *directory, *multiple) {
                    Ok(paths) => self.complete_paths(paths, self.writable()),
                    Err(message) => self.show_error(&crate::i18n::tr(message)),
                }
            }
            ChooserKind::SaveFile { .. } => self.accept_save_file(),
            ChooserKind::SaveFiles { names } => match self.active_folder() {
                Ok(folder) => self.accept_destinations(folder, names.clone()),
                Err(message) => self.show_error(&crate::i18n::tr(message)),
            },
        }
    }

    fn accept_open_name(self: &Rc<Self>, name: &str) {
        if let Some(url) = remote_file_url(name) {
            self.open_remote(&url);
            return;
        }
        if let Err(message) = crate::services::validate_basename(name) {
            self.show_error(&crate::i18n::tr(message));
            return;
        }
        let folder = self
            .tab
            .browser
            .active_location()
            .and_then(|l| l.native_path().map(Path::to_path_buf))
            .or_else(|| self.active_folder().ok());
        let Some(folder) = folder else {
            self.show_error(&crate::i18n::tr("Choose an accessible local folder"));
            return;
        };
        let location = Location::local(folder.join(name));
        self.destination_check.set(true);
        self.accept_button.set_sensitive(false);
        let weak = Rc::downgrade(self);
        let generation = self.accept_generation.get();
        glib::MainContext::default().spawn_local(async move {
            let result = crate::adapters::query_file_entry(location).await;
            let Some(c) = weak.upgrade() else { return };
            c.destination_check.set(false);
            if c.completion.borrow().is_none() || c.accept_generation.get() != generation {
                return;
            }
            c.accept_button.set_sensitive(true);
            match result {
                Ok(entry) if entry.is_directory() => {
                    c.tab.browser.navigate(entry.location);
                    if let Some(filename) = &c.filename {
                        c.clear_filename(filename);
                    }
                }
                Ok(entry) if c.tab.browser.allows_entry(&entry) => {
                    match entry.location.native_path() {
                        Some(path) => c.finish_remote(path.to_path_buf()),
                        None => {
                            c.show_error(&crate::i18n::tr("Choose an existing, accessible file"))
                        }
                    }
                }
                Ok(_) => c.show_error(&crate::i18n::tr(
                    "The file does not match the selected filter",
                )),
                Err(_) => c.show_error(&crate::i18n::tr("Choose an existing, accessible file")),
            }
        });
    }

    fn accept_save_file(self: &Rc<Self>) {
        let Some(filename) = &self.filename else {
            return;
        };
        let name = filename.text().to_string();
        if let Err(message) = crate::services::validate_basename(&name) {
            filename.add_css_class("error");
            self.show_error(&crate::i18n::tr(message));
            filename.grab_focus();
            return;
        }
        filename.remove_css_class("error");
        match self.active_folder() {
            Ok(folder) => {
                let name = self.save_destination_name(&name);
                self.accept_destinations(folder, vec![name]);
            }
            Err(message) => self.show_error(&crate::i18n::tr(message)),
        }
    }

    fn accept_destinations(self: &Rc<Self>, folder: PathBuf, names: Vec<OsString>) {
        if self.destination_check.replace(true) {
            return;
        }
        self.accept_button.set_sensitive(false);
        let generation = self.accept_generation.get();
        let weak = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let result = check_destinations(&folder, &names).await;
            let Some(c) = weak.upgrade() else { return };
            c.destination_check.set(false);
            if c.accept_generation.get() != generation {
                return;
            }
            c.accept_button.set_sensitive(true);
            if c.completion.borrow().is_none() {
                return;
            }
            match result {
                Ok(d) if !d.existing_files => c.complete_paths(d.paths, None),
                Ok(d) => c.confirm_overwrite(d.paths),
                Err(message) => c.show_error(&message),
            }
        });
    }

    fn confirm_overwrite(self: &Rc<Self>, paths: Vec<PathBuf>) {
        let many = paths.len() > 1;
        let names = crate::i18n::list(
            paths
                .iter()
                .take(3)
                .filter_map(|p| p.file_name())
                .map(|n| n.to_string_lossy().chars().take(80).collect::<String>()),
        );
        let text = format!(
            "{names}\n{}",
            crate::i18n::tr(if many {
                "One or more destination files already exist. Continuing may overwrite them."
            } else {
                "The destination file already exists. Continuing may overwrite it."
            })
        );
        self.confirm(
            &crate::i18n::tr(if many {
                "Replace existing files?"
            } else {
                "Replace existing file?"
            }),
            &text,
            &crate::i18n::tr("Replace"),
            true,
            move |c| c.complete_paths(paths.clone(), None),
        );
    }

    fn confirm_file(self: &Rc<Self>, entry: &FileEntry) {
        if self.completion.borrow().is_none() || !self.tab.browser.allows_entry(entry) {
            return;
        }
        match self.request.kind {
            ChooserKind::Open {
                directory: true, ..
            } => self.show_error(&crate::i18n::tr("Choose folders only")),
            ChooserKind::Open {
                directory: false,
                multiple: true,
            } if self.has_fill() && self.tab.selected().len() > 1 => self.accept(),
            _ => self.activate_file(&entry.location),
        }
    }

    fn activate_file(self: &Rc<Self>, location: &Location) {
        if self.download_in_progress() {
            return;
        }
        match &self.request.kind {
            ChooserKind::Open {
                directory: false, ..
            } => match location.native_path() {
                Some(path) => self.complete_paths(vec![path.to_path_buf()], self.writable()),
                None => self.show_error(&crate::i18n::tr("Choose a local file")),
            },
            ChooserKind::SaveFile { .. } => {
                let Some((folder, name)) = location
                    .native_path()
                    .and_then(|path| Some((path.parent()?, path.file_name()?)))
                    .filter(|(_, name)| safe_filename(name))
                    .map(|(folder, name)| (folder.to_owned(), name.to_owned()))
                else {
                    return;
                };
                if let Some(filename) = &self.filename {
                    self.fill_filename(filename, &name);
                }
                self.accept_destinations(folder, vec![name]);
            }
            _ => {}
        }
    }
}

/// Where a file's name stops before its extension, for selecting it.
fn stem_end(name: &str) -> i32 {
    match name.rfind('.') {
        Some(dot) if dot > 0 => name[..dot].chars().count() as i32,
        _ => -1,
    }
}

/// A chooser over the real portal path, for looking at the dialog without a
/// portal request: `NYX_FILES_CHOOSER=open|files|folder|save`.
pub(super) fn present_for_development(kind: &str) {
    let kind = match kind {
        "files" => ChooserKind::Open {
            directory: false,
            multiple: true,
        },
        "folder" => ChooserKind::Open {
            directory: true,
            multiple: false,
        },
        "save" => ChooserKind::SaveFile {
            current_name: Some("Untitled.txt".into()),
        },
        _ => ChooserKind::Open {
            directory: false,
            multiple: false,
        },
    };
    let save = matches!(kind, ChooserKind::SaveFile { .. });
    let request = ChooserRequest {
        token: "development".into(),
        title: crate::i18n::tr(if save { "Save File" } else { "Open File" }),
        accept_label: crate::i18n::tr(if save { "_Save" } else { "_Open" }),
        modal: false,
        parent: None,
        parent_size_hint: None,
        initial_directory: glib::home_dir(),
        kind,
        filters: Vec::new(),
        current_filter: None,
        choices: Vec::new(),
    };
    present(request, Arc::new(AtomicBool::new(false)), |result| {
        tracing::info!(?result, "development chooser finished");
    });
}
