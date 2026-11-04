// SPDX-License-Identifier: MIT

//! Search: a card over the window for Home and the mounted drives (Ctrl+K)
//! or folders visited before (Ctrl+Shift+K), and the filter that narrows the
//! folder showing (the header field, Ctrl+F, / or typing).

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::mpsc::TryRecvError,
    time::Duration,
};

use gtk::{gio, glib, prelude::*};
use nyx_ui::widgets;

use crate::{
    services::{NavigationHistory, SearchEvent, SearchHandle, SearchItem},
    ui::{preferences::PreferenceManager, window::devices::global_search_roots},
};

use super::window::Window;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Everywhere,
    Folders,
}

struct Card {
    veil: gtk::Box,
    entry: gtk::SearchEntry,
    list: gtk::ListBox,
    status: gtk::Label,
    spinner: gtk::Spinner,
    items: RefCell<Vec<SearchItem>>,
    handle: RefCell<Option<SearchHandle>>,
    mode: Cell<Mode>,
    generation: Cell<u64>,
}

thread_local! {
    static CARD: RefCell<Option<Rc<Card>>> = const { RefCell::new(None) };
}

fn item_icon(item: &SearchItem) -> gio::Icon {
    if item.is_directory {
        return gio::ThemedIcon::new("folder-symbolic").upcast();
    }
    let (content_type, _) = gio::content_type_guess(Some(&item.name), None::<&[u8]>);
    gio::content_type_get_symbolic_icon(&content_type)
}

pub(super) fn row(item: &SearchItem) -> gtk::ListBoxRow {
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    line.add_css_class("search-result");
    let icon = gtk::Image::from_gicon(&item_icon(item));
    line.append(&icon);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
    text.set_hexpand(true);
    let name = widgets::label(&item.name, "search-name");
    name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    text.append(&name);
    let folder = item
        .path
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let home = super::tab::home().display().to_string();
    let folder = folder
        .strip_prefix(&home)
        .map_or(folder.clone(), |rest| format!("~{rest}"));
    let path = widgets::label(&folder, "dim");
    path.set_ellipsize(gtk::pango::EllipsizeMode::Start);
    text.append(&path);
    line.append(&text);
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&line));
    row
}

impl Card {
    fn render(&self, items: Vec<SearchItem>) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        for item in items.iter().take(200) {
            self.list.append(&row(item));
        }
        if let Some(first) = self.list.row_at_index(0) {
            self.list.select_row(Some(&first));
        }
        let query = self.entry.text();
        self.status.set_visible(items.is_empty());
        self.status
            .set_text(&crate::i18n::tr(if query.trim().is_empty() {
                match self.mode.get() {
                    Mode::Everywhere => "Type to search Home and mounted local drives",
                    Mode::Folders => "Type to find a folder you visited",
                }
            } else {
                "No matches"
            }));
        *self.items.borrow_mut() = items;
    }

    fn query(&self) {
        let text = self.entry.text().trim().to_string();
        match self.mode.get() {
            Mode::Folders => self.render(NavigationHistory::shared().search(&text)),
            Mode::Everywhere => {
                if text.is_empty() {
                    self.render(Vec::new());
                } else if let Some(handle) = self.handle.borrow().as_ref() {
                    handle.query(&text);
                }
            }
        }
    }

    fn step(&self, delta: i32) {
        let current = self.list.selected_row().map_or(-1, |r| r.index());
        let count = self.items.borrow().len().min(200) as i32;
        if count == 0 {
            return;
        }
        let next = (current + delta).clamp(0, count - 1);
        if let Some(row) = self.list.row_at_index(next) {
            self.list.select_row(Some(&row));
            self.entry.grab_focus();
            self.entry.set_position(-1);
        }
    }

    fn close(&self) {
        self.generation.set(self.generation.get() + 1);
        self.handle.borrow_mut().take();
        self.veil.set_visible(false);
    }
}

fn build(window: &Rc<Window>) -> Rc<Card> {
    let veil = gtk::Box::new(gtk::Orientation::Vertical, 0);
    veil.add_css_class("settings-veil");
    veil.set_hexpand(true);
    veil.set_vexpand(true);
    let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    card.add_css_class("dialog-card");
    card.add_css_class("search-card");
    card.set_halign(gtk::Align::Center);
    card.set_valign(gtk::Align::Start);
    card.set_margin_top(70);
    card.set_size_request(620, -1);
    let head = widgets::hbox(8);
    let entry = gtk::SearchEntry::new();
    entry.set_hexpand(true);
    head.append(&entry);
    let spinner = gtk::Spinner::new();
    head.append(&spinner);
    card.append(&head);
    let status = widgets::label("", "dim");
    status.set_margin_start(6);
    card.append(&status);
    let list = gtk::ListBox::new();
    list.add_css_class("search-results");
    list.set_selection_mode(gtk::SelectionMode::Single);
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_height(80)
        .max_content_height(440)
        .propagate_natural_height(true)
        .child(&list)
        .build();
    card.append(&scroller);
    veil.append(&card);
    veil.set_visible(false);
    window.overlay.add_overlay(&veil);
    let state = Rc::new(Card {
        veil,
        entry,
        list,
        status,
        spinner,
        items: RefCell::default(),
        handle: RefCell::default(),
        mode: Cell::new(Mode::Everywhere),
        generation: Cell::new(0),
    });
    let weak = Rc::downgrade(&state);
    state.entry.connect_search_changed(move |_| {
        if let Some(card) = weak.upgrade() {
            card.query();
        }
    });
    let (weak, w) = (Rc::downgrade(&state), Rc::downgrade(window));
    let activate = move |index: i32| {
        let (Some(card), Some(window)) = (weak.upgrade(), w.upgrade()) else {
            return;
        };
        let Some(item) = usize::try_from(index)
            .ok()
            .and_then(|i| card.items.borrow().get(i).cloned())
        else {
            return;
        };
        card.close();
        window.open_search_result(item);
    };
    let a = activate.clone();
    state
        .list
        .connect_row_activated(move |_, row| a(row.index()));
    let weak = Rc::downgrade(&state);
    state.entry.connect_activate(move |_| {
        if let Some(card) = weak.upgrade()
            && let Some(row) = card.list.selected_row()
        {
            activate(row.index());
        }
    });
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = Rc::downgrade(&state);
    crate::ui::keys::on_press(&keys, move |_, press| {
        use crate::ui::keys::bindings::{DOWN, ESCAPE, UP};
        let Some(card) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        if press.is(ESCAPE) {
            card.close();
        } else if press.is(DOWN) {
            card.step(1);
        } else if press.is(UP) {
            card.step(-1);
        } else {
            return glib::Propagation::Proceed;
        }
        glib::Propagation::Stop
    });
    state.veil.add_controller(keys);
    let click = gtk::GestureClick::new();
    let (weak, inner) = (Rc::downgrade(&state), card.downgrade());
    click.connect_pressed(move |g, _, x, y| {
        let (Some(card), Some(inner), Some(veil)) = (weak.upgrade(), inner.upgrade(), g.widget())
        else {
            return;
        };
        let inside = inner
            .compute_bounds(&veil)
            .is_some_and(|b| b.contains_point(&gtk::graphene::Point::new(x as f32, y as f32)));
        if !inside {
            card.close();
        }
    });
    state.veil.add_controller(click);
    state
}

/// Opens the card in `mode`, building it the first time.
pub(super) fn open(window: &Rc<Window>, mode: Mode) {
    let card = CARD.with(|c| c.borrow().clone()).unwrap_or_else(|| {
        let card = build(window);
        CARD.with(|c| *c.borrow_mut() = Some(card.clone()));
        card
    });
    if card.veil.parent().as_ref() != Some(window.overlay.upcast_ref()) {
        if let Some(parent) = card.veil.parent().and_downcast::<gtk::Overlay>() {
            parent.remove_overlay(&card.veil);
        }
        window.overlay.add_overlay(&card.veil);
    }
    card.generation.set(card.generation.get() + 1);
    card.mode.set(mode);
    card.handle.borrow_mut().take();
    card.entry.set_text("");
    card.entry
        .set_placeholder_text(Some(&crate::i18n::tr(match mode {
            Mode::Everywhere => "Search files and folders…",
            Mode::Folders => "Jump to a folder…",
        })));
    card.veil.set_visible(true);
    card.entry.grab_focus();
    card.spinner.set_visible(false);
    if mode == Mode::Everywhere {
        let prefs = PreferenceManager::shared();
        let roots = global_search_roots();
        let (handle, receiver) = crate::services::index_trees_with_exclusions(
            roots,
            prefs.sort_preferences().show_hidden,
            prefs.search_exclusions(),
            prefs.search_limits(),
        );
        card.handle.replace(Some(handle));
        card.spinner.set_visible(true);
        card.spinner.start();
        let (weak, generation) = (Rc::downgrade(&card), card.generation.get());
        glib::timeout_add_local(Duration::from_millis(16), move || {
            let Some(card) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if card.generation.get() != generation || !card.veil.is_visible() {
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
            {
                card.spinner.set_visible(indexing);
                if !query.is_empty() && query == card.entry.text().trim() {
                    card.render(items);
                }
            }
            glib::ControlFlow::Continue
        });
    }
    card.render(Vec::new());
    if mode == Mode::Folders {
        card.query();
    }
}
