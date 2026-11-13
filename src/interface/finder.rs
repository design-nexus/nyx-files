// SPDX-License-Identifier: MIT

//! Search across Home and the drives from the header's field (Ctrl+K): the
//! results from every place in one list with their folders, narrowed by
//! place, kind, time and hidden files, and the selected one previewed beside them.

use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::{Rc, Weak},
    sync::mpsc::TryRecvError,
    time::{Duration, Instant},
};

use gtk::{gio, glib, prelude::*};
use nyx_ui::widgets;

use crate::{
    services::{SearchEvent, SearchHandle, SearchItem},
    ui::{
        keys::{Keys, bindings},
        preferences::PreferenceManager,
        window::devices::global_search_roots,
    },
};

use super::window::Window;

/// Rows shown at most; the index keeps finding more as it walks.
const SHOWN: usize = 200;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Folder,
    Home,
    Everywhere,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Any,
    Documents,
    Images,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Time {
    Any,
    Today,
    Week,
    Month,
    Year,
}

impl Time {
    const ALL: [(Time, &'static str); 5] = [
        (Time::Any, "Any time"),
        (Time::Today, "Today"),
        (Time::Week, "This week"),
        (Time::Month, "This month"),
        (Time::Year, "This year"),
    ];

    fn seconds(self) -> Option<i64> {
        Some(match self {
            Time::Any => return None,
            Time::Today => 24 * 3600,
            Time::Week => 7 * 24 * 3600,
            Time::Month => 31 * 24 * 3600,
            Time::Year => 366 * 24 * 3600,
        })
    }

    fn label(self) -> &'static str {
        Time::ALL
            .iter()
            .find(|(t, _)| *t == self)
            .map_or("Any time", |(_, l)| l)
    }
}

/// What the side panel shows for the selected result.
struct Inspector {
    root: gtk::Box,
    actions: gtk::Box,
    preview: gtk::Box,
    name: gtk::Label,
    kind: gtk::Label,
    modified: gtk::Label,
    place: gtk::Label,
    more: gtk::Button,
}

pub(super) struct Finder {
    /// The header's search field.
    pub(super) field: gtk::Box,
    text: gtk::Text,
    key: gtk::Label,
    /// The results page.
    pub(super) page: gtk::Box,
    title: gtk::Label,
    subtitle: gtk::Label,
    list: gtk::ListBox,
    scroll: gtk::ScrolledWindow,
    note: gtk::Box,
    note_text: gtk::Label,
    spinner: gtk::Spinner,
    folder_scope: gtk::ToggleButton,
    time_button: gtk::Button,
    inspector: Inspector,
    scope: Cell<Scope>,
    /// A narrow window has no room for the side panel.
    narrow: Cell<bool>,
    kind: Cell<Kind>,
    time: Cell<Time>,
    hidden: Cell<bool>,
    /// What the index found for the query, and the rows showing.
    found: RefCell<Vec<SearchItem>>,
    shown: RefCell<Vec<(SearchItem, Option<i64>)>>,
    handle: RefCell<Option<SearchHandle>>,
    roots: RefCell<Vec<(PathBuf, String)>>,
    generation: Cell<u64>,
    started: Cell<Option<Instant>>,
    /// Setting the text from code, which shouldn't run a search.
    quiet: Cell<bool>,
    window: RefCell<Weak<Window>>,
}

fn chip(label: &str, icon: Option<&str>) -> gtk::ToggleButton {
    let button = gtk::ToggleButton::new();
    button.add_css_class("chip");
    let row = widgets::hbox(6);
    if let Some(icon) = icon {
        row.append(&gtk::Image::from_icon_name(icon));
    }
    row.append(&gtk::Label::new(Some(&crate::i18n::tr(label))));
    button.set_child(Some(&row));
    button
}

fn segment(label: &str) -> gtk::ToggleButton {
    let button = gtk::ToggleButton::with_label(&crate::i18n::tr(label));
    button.add_css_class("segment");
    button
}

fn fact_row(card: &gtk::Box, key: &str) -> gtk::Label {
    let row = widgets::hbox(12);
    row.add_css_class("fact-row");
    row.append(&widgets::label(&crate::i18n::tr(key), "fact-key"));
    let value = widgets::label("", "fact-value");
    value.set_hexpand(true);
    value.set_xalign(1.0);
    value.set_ellipsize(gtk::pango::EllipsizeMode::Start);
    row.append(&value);
    card.append(&row);
    value
}

fn kind_matches(kind: Kind, item: &SearchItem) -> bool {
    if kind == Kind::Any {
        return true;
    }
    if item.is_directory {
        return false;
    }
    let (content_type, _) = gio::content_type_guess(Some(&item.name), None::<&[u8]>);
    match kind {
        Kind::Images => content_type.starts_with("image/"),
        _ => {
            content_type.starts_with("text/")
                || gio::content_type_is_a(&content_type, "application/pdf")
                || [
                    "document",
                    "spreadsheet",
                    "presentation",
                    "msword",
                    "excel",
                    "powerpoint",
                    "rtf",
                ]
                .iter()
                .any(|k| content_type.contains(k))
        }
    }
}

fn modified(path: &Path) -> Option<i64> {
    let since = std::fs::symlink_metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    Some(since.as_secs() as i64)
}

/// The name with the first match of `query` marked.
fn marked(name: &str, query: &str) -> String {
    let lower = name.to_lowercase();
    let needle = query.trim().to_lowercase();
    if let Some(start) = (!needle.is_empty() && lower.len() == name.len())
        .then(|| lower.find(&needle))
        .flatten()
        && name.is_char_boundary(start)
        && name.is_char_boundary(start + needle.len())
    {
        let end = start + needle.len();
        return format!(
            "{}<span background=\"#e5a50a\" bgalpha=\"38%\">{}</span>{}",
            glib::markup_escape_text(&name[..start]),
            glib::markup_escape_text(&name[start..end]),
            glib::markup_escape_text(&name[end..])
        );
    }
    glib::markup_escape_text(name).to_string()
}

impl Finder {
    pub(super) fn new() -> Rc<Finder> {
        let field = widgets::hbox(8);
        field.add_css_class("finder-field");
        field.set_valign(gtk::Align::Center);
        field.append(&gtk::Image::from_icon_name("system-search-symbolic"));
        let text = gtk::Text::new();
        text.set_hexpand(true);
        text.set_width_chars(1);
        text.set_placeholder_text(Some(&crate::i18n::tr("Search Home and drives")));
        text.update_property(&[gtk::accessible::Property::Label(&crate::i18n::tr(
            "Search Home and drives",
        ))]);
        field.append(&text);
        let key = widgets::label(
            &crate::ui::shortcut_reference::with_keys("%{keys}", Keys::One(bindings::SEARCH)),
            "finder-key",
        );
        field.append(&key);
        field.set_hexpand(false);

        let page = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        page.add_css_class("finder-page");
        let main = gtk::Box::new(gtk::Orientation::Vertical, 0);
        main.set_hexpand(true);
        let bar = widgets::hbox(12);
        bar.add_css_class("finder-bar");
        let heading = gtk::Box::new(gtk::Orientation::Vertical, 2);
        heading.set_hexpand(true);
        let title = widgets::label("", "finder-title");
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let subtitle = widgets::label(
            &crate::i18n::tr("Names of files and folders"),
            "finder-subtitle",
        );
        heading.append(&title);
        heading.append(&subtitle);
        bar.append(&heading);
        let spinner = gtk::Spinner::new();
        bar.append(&spinner);
        let scopes = widgets::hbox(0);
        scopes.add_css_class("segmented");
        scopes.set_valign(gtk::Align::Center);
        let folder_scope = segment("This folder");
        let home_scope = segment("Home");
        let everywhere = segment("Everywhere");
        home_scope.set_group(Some(&folder_scope));
        everywhere.set_group(Some(&folder_scope));
        everywhere.set_active(true);
        for b in [&folder_scope, &home_scope, &everywhere] {
            scopes.append(b);
        }
        bar.append(&scopes);
        main.append(&bar);
        let chips = widgets::hbox(6);
        chips.add_css_class("finder-chips");
        let any = chip("Any kind", Some("view-filter-symbolic"));
        let documents = chip("Documents", Some("x-office-document-symbolic"));
        let images = chip("Images", Some("image-x-generic-symbolic"));
        documents.set_group(Some(&any));
        images.set_group(Some(&any));
        any.set_active(true);
        let time_button = gtk::Button::new();
        time_button.add_css_class("chip");
        let hidden = chip("Include hidden files", None);
        for w in [
            any.upcast_ref::<gtk::Widget>(),
            documents.upcast_ref(),
            images.upcast_ref(),
            time_button.upcast_ref(),
            hidden.upcast_ref(),
        ] {
            chips.append(w);
        }
        main.append(&chips);
        let list = gtk::ListBox::new();
        list.add_css_class("finder-results");
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.update_property(&[gtk::accessible::Property::Label(&crate::i18n::tr(
            crate::ui::SEARCH_RESULTS_LABEL,
        ))]);
        let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
        column.append(&list);
        let note = widgets::hbox(8);
        note.add_css_class("finder-note");
        note.append(&gtk::Image::from_icon_name("dialog-information-symbolic"));
        let note_text = widgets::label("", "");
        note_text.set_wrap(true);
        note.append(&note_text);
        note.set_visible(false);
        column.append(&note);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&column)
            .build();
        main.append(&scroll);
        page.append(&main);
        let inspector = Self::inspector();
        page.append(&inspector.root);

        let finder = Rc::new(Finder {
            field,
            text,
            key,
            page,
            title,
            subtitle,
            list,
            scroll: scroll.clone(),
            note,
            note_text,
            spinner,
            folder_scope: folder_scope.clone(),
            time_button: time_button.clone(),
            inspector,
            scope: Cell::new(Scope::Everywhere),
            narrow: Cell::new(false),
            kind: Cell::new(Kind::Any),
            time: Cell::new(Time::Any),
            hidden: Cell::new(false),
            found: RefCell::default(),
            shown: RefCell::default(),
            handle: RefCell::default(),
            roots: RefCell::default(),
            generation: Cell::new(0),
            started: Cell::new(None),
            quiet: Cell::new(false),
            window: RefCell::default(),
        });
        finder.show_time();
        for (button, scope) in [
            (&folder_scope, Scope::Folder),
            (&home_scope, Scope::Home),
            (&everywhere, Scope::Everywhere),
        ] {
            let weak = Rc::downgrade(&finder);
            button.connect_toggled(move |b| {
                if b.is_active()
                    && let Some(f) = weak.upgrade()
                    && f.scope.replace(scope) != scope
                {
                    f.restart();
                }
            });
        }
        for (button, kind) in [
            (&any, Kind::Any),
            (&documents, Kind::Documents),
            (&images, Kind::Images),
        ] {
            let weak = Rc::downgrade(&finder);
            button.connect_toggled(move |b| {
                if b.is_active()
                    && let Some(f) = weak.upgrade()
                {
                    f.kind.set(kind);
                    f.render();
                }
            });
        }
        let weak = Rc::downgrade(&finder);
        hidden.connect_toggled(move |b| {
            if let Some(f) = weak.upgrade() {
                f.hidden.set(b.is_active());
                f.restart();
            }
        });
        let weak = Rc::downgrade(&finder);
        time_button.connect_clicked(move |b| {
            let Some(f) = weak.upgrade() else { return };
            let items = Time::ALL
                .iter()
                .map(|(time, label)| {
                    let (weak, time) = (Rc::downgrade(&f), *time);
                    nyx_ui::menu::check(&crate::i18n::tr(label), f.time.get() == time, move |_| {
                        if let Some(f) = weak.upgrade() {
                            f.time.set(time);
                            f.show_time();
                            f.render();
                        }
                    })
                })
                .collect();
            nyx_ui::menu::popup_below(b, items);
        });
        finder.wire();
        finder
    }

    fn inspector() -> Inspector {
        let side = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        side.append(&nyx_ui::frame::pane_line(gtk::Orientation::Vertical));
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("finder-inspector");
        root.set_size_request(300, -1);
        side.append(&root);
        let preview = gtk::Box::new(gtk::Orientation::Vertical, 0);
        preview.add_css_class("finder-preview");
        preview.set_overflow(gtk::Overflow::Hidden);
        root.append(&preview);
        let name = widgets::label("", "inspector-name");
        name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        let kind = widgets::label("", "inspector-kind");
        let names = gtk::Box::new(gtk::Orientation::Vertical, 2);
        names.add_css_class("inspector-names");
        names.append(&name);
        names.append(&kind);
        root.append(&names);
        let actions = widgets::hbox(6);
        actions.add_css_class("inspector-actions");
        root.append(&actions);
        let more = widgets::icon_button("view-more-symbolic", &crate::i18n::tr("More"));
        let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card.add_css_class("inspector-facts");
        let modified = fact_row(&card, "Modified");
        let place = fact_row(&card, "Where");
        root.append(&card);
        side.set_visible(false);
        Inspector {
            root: side,
            actions,
            preview,
            name,
            kind,
            modified,
            place,
            more,
        }
    }

    pub(super) fn attach(self: &Rc<Self>, window: &Rc<Window>) {
        *self.window.borrow_mut() = Rc::downgrade(window);
        let actions = &self.inspector.actions;
        let open = widgets::labeled_button("document-open-symbolic", &crate::i18n::tr("Open"));
        open.add_css_class("suggested-action");
        let reveal = widgets::labeled_button("folder-symbolic", &crate::i18n::tr("Show in folder"));
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        for w in [
            open.upcast_ref::<gtk::Widget>(),
            reveal.upcast_ref(),
            spacer.upcast_ref(),
            self.inspector.more.upcast_ref(),
        ] {
            actions.append(w);
        }
        let weak = Rc::downgrade(self);
        open.connect_clicked(move |_| {
            if let Some((f, item)) = weak.upgrade().and_then(|f| f.selected().map(|i| (f, i))) {
                f.open(item, true);
            }
        });
        let weak = Rc::downgrade(self);
        reveal.connect_clicked(move |_| {
            let Some(f) = weak.upgrade() else { return };
            let (Some(item), Some(w)) = (f.selected(), f.window.borrow().upgrade()) else {
                return;
            };
            f.reset();
            w.reveal(&crate::model::Location::local(&item.path));
        });
        let weak = Rc::downgrade(self);
        self.inspector.more.connect_clicked(move |b| {
            let Some(f) = weak.upgrade() else { return };
            let (Some(item), Some(w)) = (f.selected(), f.window.borrow().upgrade()) else {
                return;
            };
            let Some(entry) = super::overview::entry_at(&item.path) else {
                return;
            };
            let mut items = Vec::new();
            if !entry.is_directory() {
                items.push(super::open_with::submenu(&w, std::slice::from_ref(&entry)));
            }
            let e = entry.clone();
            items.push(
                nyx_ui::menu::item(&crate::i18n::tr("Copy path"), move || {
                    super::actions::copy_paths(std::slice::from_ref(&e))
                })
                .icon("edit-copy-symbolic"),
            );
            items.push(
                nyx_ui::menu::item(&crate::i18n::tr("Properties"), move || {
                    super::menus::properties(vec![entry.clone()])
                })
                .icon("document-properties-symbolic"),
            );
            nyx_ui::menu::popup_below(b, items);
        });
    }

    fn wire(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.text.connect_changed(move |_| {
            if let Some(f) = weak.upgrade() {
                f.changed();
            }
        });
        let weak = Rc::downgrade(self);
        self.text.connect_activate(move |_| {
            if let Some(f) = weak.upgrade()
                && let Some(item) = f.selected()
            {
                f.open(item, false);
            }
        });
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(self);
        crate::ui::keys::on_press(&keys, move |_, press| {
            let Some(f) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if press.is(bindings::ESCAPE) {
                f.cancel();
            } else if press.is(bindings::DOWN) {
                f.step(1);
            } else if press.is(bindings::UP) {
                f.step(-1);
            } else {
                return glib::Propagation::Proceed;
            }
            glib::Propagation::Stop
        });
        self.text.add_controller(keys);
        let focus = gtk::EventControllerFocus::new();
        let weak = Rc::downgrade(self);
        focus.connect_enter(move |_| {
            if let Some(f) = weak.upgrade() {
                f.show_key();
            }
        });
        let weak = Rc::downgrade(self);
        focus.connect_leave(move |_| {
            if let Some(f) = weak.upgrade() {
                f.show_key();
            }
        });
        self.text.add_controller(focus);
        let click = gtk::GestureClick::new();
        let text = self.text.downgrade();
        click.connect_pressed(move |_, _, _, _| {
            if let Some(text) = text.upgrade() {
                text.grab_focus();
            }
        });
        self.field.add_controller(click);
        let weak = Rc::downgrade(self);
        self.list.connect_row_selected(move |_, _| {
            if let Some(f) = weak.upgrade() {
                f.inspect();
            }
        });
        let weak = Rc::downgrade(self);
        self.list.connect_row_activated(move |_, row| {
            if let Some(f) = weak.upgrade() {
                let item = usize::try_from(row.index())
                    .ok()
                    .and_then(|i| f.shown.borrow().get(i).map(|(item, _)| item.clone()));
                if let Some(item) = item {
                    f.open(item, false);
                }
            }
        });
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(self);
        crate::ui::keys::on_press(&keys, move |_, press| {
            let Some(f) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if press.is(bindings::ESCAPE) {
                f.focus();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        self.list.add_controller(keys);
    }

    /// The key cap: Ctrl K to come here, Esc to leave once typing.
    fn show_key(&self) {
        let typing = self.text.has_focus() && !self.text.text().is_empty();
        self.key.set_text(&if typing {
            crate::i18n::tr("Esc")
        } else {
            crate::ui::shortcut_reference::with_keys("%{keys}", Keys::One(bindings::SEARCH))
        });
    }

    fn show_time(&self) {
        let row = widgets::hbox(6);
        row.append(&gtk::Image::from_icon_name("document-open-recent-symbolic"));
        row.append(&gtk::Label::new(Some(&crate::i18n::tr(
            self.time.get().label(),
        ))));
        self.time_button.set_child(Some(&row));
        if self.time.get() == Time::Any {
            self.time_button.remove_css_class("selected");
        } else {
            self.time_button.add_css_class("selected");
        }
    }

    /// Ctrl+K: into the field, its text selected.
    pub(super) fn focus(&self) {
        self.text.grab_focus();
        self.text.select_region(0, -1);
    }

    pub(super) fn type_query(&self, query: &str) {
        self.text.grab_focus();
        self.text.set_text(query);
        self.text.set_position(-1);
    }

    pub(super) fn query(&self) -> String {
        self.text.text().trim().to_string()
    }

    pub(super) fn set_narrow(&self, narrow: bool) {
        self.narrow.set(narrow);
        self.key.set_visible(!narrow);
        if narrow {
            self.field.add_css_class("narrow");
        } else {
            self.field.remove_css_class("narrow");
        }
        self.inspect();
    }

    pub(super) fn set_wide(&self, wide: bool) {
        if wide {
            self.field.add_css_class("wide");
        } else {
            self.field.remove_css_class("wide");
        }
    }

    /// Empties the field without leaving the page it's on.
    pub(super) fn reset(&self) {
        self.stop();
        self.quiet.set(true);
        self.text.set_text("");
        self.quiet.set(false);
        self.show_key();
    }

    /// Esc: the search ends and the page before it shows again.
    fn cancel(&self) {
        self.reset();
        if let Some(w) = self.window.borrow().upgrade() {
            w.leave_search(true);
        }
    }

    fn stop(&self) {
        self.generation.set(self.generation.get() + 1);
        self.handle.borrow_mut().take();
        self.spinner.stop();
        self.spinner.set_visible(false);
    }

    fn changed(self: &Rc<Self>) {
        self.show_key();
        if self.quiet.get() {
            return;
        }
        let Some(w) = self.window.borrow().upgrade() else {
            return;
        };
        let query = self.query();
        if query.is_empty() {
            self.stop();
            w.leave_search(false);
            return;
        }
        if !w.searching() {
            let here = w
                .current()
                .and_then(|t| t.location())
                .and_then(|l| l.native_path().map(Path::to_path_buf));
            self.folder_scope.set_sensitive(here.is_some());
            if here.is_none() && self.scope.get() == Scope::Folder {
                self.scope.set(Scope::Everywhere);
            }
            w.enter_search();
        }
        if self.handle.borrow().is_none() {
            self.restart();
        } else if let Some(handle) = self.handle.borrow().as_ref() {
            self.started.set(Some(Instant::now()));
            handle.query(&query);
        }
        self.title
            .set_text(&rust_i18n::t!("Searching for “%{query}”", query = query));
    }

    /// Indexes the places the scope covers again, then asks for the query.
    fn restart(self: &Rc<Self>) {
        let query = self.query();
        if query.is_empty() {
            return;
        }
        let Some(w) = self.window.borrow().upgrade() else {
            return;
        };
        self.stop();
        let home = super::tab::home();
        let roots: Vec<PathBuf> = match self.scope.get() {
            Scope::Folder => w
                .current()
                .and_then(|t| t.location())
                .and_then(|l| l.native_path().map(Path::to_path_buf))
                .into_iter()
                .collect(),
            Scope::Home => vec![home.clone()],
            Scope::Everywhere => global_search_roots(),
        };
        let named: Vec<(PathBuf, String)> = roots
            .iter()
            .map(|root| {
                let name = if *root == home {
                    crate::i18n::tr("Home")
                } else {
                    gio::File::for_path(root)
                        .find_enclosing_mount(gio::Cancellable::NONE)
                        .ok()
                        .filter(|m| m.root().path().as_deref() == Some(root.as_path()))
                        .map(|m| m.name().to_string())
                        .unwrap_or_else(|| super::overview::place_name(root))
                };
                (root.clone(), name)
            })
            .collect();
        self.roots.replace(named);
        let prefs = PreferenceManager::shared();
        let (handle, receiver) = crate::services::index_trees_with_exclusions(
            roots,
            self.hidden.get() || prefs.sort_preferences().show_hidden,
            prefs.search_exclusions(),
            prefs.search_limits(),
        );
        handle.query(&query);
        self.handle.replace(Some(handle));
        self.started.set(Some(Instant::now()));
        self.spinner.set_visible(true);
        self.spinner.start();
        let (weak, generation) = (Rc::downgrade(self), self.generation.get());
        glib::timeout_add_local(Duration::from_millis(16), move || {
            let Some(f) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if f.generation.get() != generation {
                return glib::ControlFlow::Break;
            }
            let mut latest = None;
            loop {
                match receiver.try_recv() {
                    Ok(event) => latest = Some(event),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return glib::ControlFlow::Break,
                }
            }
            if let Some(SearchEvent::Results {
                query,
                items,
                indexing,
                ..
            }) = latest
                && query == f.query()
            {
                f.spinner.set_visible(indexing);
                *f.found.borrow_mut() = items;
                f.render();
                if !indexing {
                    f.finished();
                }
            }
            glib::ControlFlow::Continue
        });
    }

    /// "Searched Home and the Backup drive in 0.4 s."
    fn finished(&self) {
        let names: Vec<String> = self.roots.borrow().iter().map(|(_, n)| n.clone()).collect();
        let seconds = self
            .started
            .get()
            .map_or(0.0, |s| s.elapsed().as_secs_f64());
        let searched = rust_i18n::t!(
            "Searched %{places} in %{seconds} s.",
            places = crate::i18n::list(names),
            seconds = format!("{seconds:.1}")
        );
        let excluded = if PreferenceManager::shared().search_exclusions().is_empty() {
            String::new()
        } else {
            crate::i18n::tr("Folders you excluded in Settings › Search are skipped.")
        };
        self.note_text
            .set_text(crate::i18n::join_sentences(&searched, &excluded).trim());
        self.note.set_visible(true);
    }

    fn render(&self) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        let query = self.query();
        let now = glib::real_time() / 1_000_000;
        let (kind, time) = (self.kind.get(), self.time.get().seconds());
        let mut shown = Vec::new();
        for item in self.found.borrow().iter() {
            if shown.len() >= SHOWN {
                break;
            }
            if !kind_matches(kind, item) {
                continue;
            }
            let changed = modified(&item.path);
            if let Some(limit) = time
                && changed.is_none_or(|t| now - t > limit)
            {
                continue;
            }
            shown.push((item.clone(), changed));
        }
        for (item, changed) in &shown {
            self.list.append(&self.row(item, *changed, &query));
        }
        let count = shown.len();
        *self.shown.borrow_mut() = shown;
        self.title.set_text(&if count == 0 {
            rust_i18n::t!("No results for “%{query}”", query = query).into_owned()
        } else {
            rust_i18n::t!(
                "%{results} for “%{query}”",
                results = crate::i18n::count("results", count),
                query = query
            )
            .into_owned()
        });
        self.subtitle
            .set_text(&crate::i18n::tr("Names of files and folders"));
        if let Some(first) = self.list.row_at_index(0) {
            self.list.select_row(Some(&first));
        } else {
            self.inspect();
        }
    }

    /// The folder an item is in, from Home or the drive it's on.
    fn folder_text(&self, path: &Path) -> String {
        let parent = path.parent().unwrap_or(path);
        let roots = self.roots.borrow();
        let best = roots
            .iter()
            .filter(|(root, _)| parent.starts_with(root))
            .max_by_key(|(root, _)| root.as_os_str().len());
        let (start, parts): (String, Vec<String>) = match best {
            Some((root, name)) => (
                name.clone(),
                parent
                    .strip_prefix(root)
                    .map(|rest| {
                        rest.iter()
                            .map(|c| c.to_string_lossy().into_owned())
                            .collect()
                    })
                    .unwrap_or_default(),
            ),
            None => (
                String::new(),
                parent
                    .iter()
                    .map(|c| c.to_string_lossy().into_owned())
                    .collect(),
            ),
        };
        let home = best.is_some_and(|(root, _)| *root == super::tab::home());
        let mut all: Vec<String> = Vec::new();
        if !(home && !parts.is_empty()) && !start.is_empty() {
            all.push(start);
        }
        all.extend(parts.into_iter().filter(|p| p != "/"));
        all.join(" › ")
    }

    fn row(&self, item: &SearchItem, changed: Option<i64>, query: &str) -> gtk::ListBoxRow {
        let line = widgets::hbox(12);
        line.add_css_class("finder-row");
        let icon = super::overview::kind_icon(&item.name, item.is_directory);
        icon.set_pixel_size(20);
        line.append(&icon);
        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.set_hexpand(true);
        let name = gtk::Label::new(None);
        name.set_xalign(0.0);
        name.add_css_class("finder-name");
        name.set_markup(&marked(&item.name, query));
        name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        text.append(&name);
        let folder = widgets::label(&self.folder_text(&item.path), "finder-folder");
        folder.set_ellipsize(gtk::pango::EllipsizeMode::Start);
        text.append(&folder);
        line.append(&text);
        if let Some(seconds) = changed {
            line.append(&widgets::label(
                &crate::util::modified_text(seconds),
                "finder-date",
            ));
        }
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&line));
        row
    }

    fn selected(&self) -> Option<SearchItem> {
        let index = usize::try_from(self.list.selected_row()?.index()).ok()?;
        self.shown.borrow().get(index).map(|(item, _)| item.clone())
    }

    fn step(&self, delta: i32) {
        let count = self.shown.borrow().len() as i32;
        if count == 0 {
            return;
        }
        let current = self.list.selected_row().map_or(-1, |r| r.index());
        if let Some(row) = self
            .list
            .row_at_index((current + delta).clamp(0, count - 1))
        {
            self.list.select_row(Some(&row));
            let adj = self.scroll.vadjustment();
            let y = row
                .compute_point(&self.list, &gtk::graphene::Point::zero())
                .map_or(0.0, |p| f64::from(p.y()));
            let h = f64::from(row.height());
            if y < adj.value() {
                adj.set_value(y);
            } else if y + h > adj.value() + adj.page_size() {
                adj.set_value(y + h - adj.page_size());
            }
        }
    }

    /// The selected result in the side panel.
    fn inspect(&self) {
        let ins = &self.inspector;
        let Some(item) = self.selected().filter(|_| !self.narrow.get()) else {
            ins.root.set_visible(false);
            return;
        };
        ins.root.set_visible(true);
        while let Some(child) = ins.preview.first_child() {
            ins.preview.remove(&child);
        }
        let entry = super::overview::entry_at(&item.path);
        if let Some(entry) = &entry {
            let thumb = super::overview::framed_thumbnail(entry, 276, 214, 56, 8.0);
            ins.preview.append(&thumb);
        }
        ins.name.set_text(&item.name);
        let kind = if item.is_directory {
            crate::i18n::tr("Folder")
        } else {
            let (content_type, _) = gio::content_type_guess(Some(&item.name), None::<&[u8]>);
            gio::content_type_get_description(&content_type).to_string()
        };
        let size = entry.as_ref().and_then(|e| match e.size {
            crate::model::MetadataValue::Known(s) if !item.is_directory => {
                Some(glib::format_size(s).to_string())
            }
            _ => None,
        });
        ins.kind.set_text(
            &[Some(kind), size]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · "),
        );
        ins.modified.set_text(
            &modified(&item.path)
                .map(crate::util::modified_text)
                .unwrap_or_default(),
        );
        ins.place.set_text(&self.folder_text(&item.path));
    }

    /// Opens a result as the search settings say, or straight away from Open.
    fn open(&self, item: SearchItem, direct: bool) {
        let Some(w) = self.window.borrow().upgrade() else {
            return;
        };
        self.reset();
        if direct && !item.is_directory {
            w.leave_search(true);
            if let Some(entry) = super::overview::entry_at(&item.path) {
                w.open_file(&entry);
            }
            return;
        }
        w.open_search_result(item);
    }
}
