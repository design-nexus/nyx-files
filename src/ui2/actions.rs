// SPDX-License-Identifier: MIT

//! File actions on a tab's selection: the clipboard, trash, rename, new
//! folder, copy path, and the context menus that offer them.

use std::{cell::RefCell, rc::Rc};

use gtk::{gdk, gio, glib, prelude::*};
use nyx_ui::{menu, toast, widgets};

use crate::{
    model::{FileEntry, Location},
    services::{PasteItem, TransferConflict},
    ui::keys::{Keys, bindings, label as key_label},
};

use super::{tab::Tab, window::Window};

thread_local! {
    /// What the last Cut put on the clipboard; pasting exactly these moves them.
    static CUT: RefCell<Vec<Location>> = const { RefCell::new(Vec::new()) };
}

fn file_of(location: &Location) -> Option<gio::File> {
    match location.native_path() {
        Some(path) => Some(gio::File::for_path(path)),
        None => location.uri_value().map(gio::File::for_uri),
    }
}

fn location_of(file: &gio::File) -> Location {
    match file.path() {
        Some(path) => Location::local(path),
        None => Location::uri(file.uri().to_string()),
    }
}

fn keys(binding: crate::ui::keys::Binding) -> String {
    key_label(&Keys::One(binding))
}

pub(super) fn copy(entries: &[FileEntry], cut: bool) {
    let files: Vec<gio::File> = entries.iter().filter_map(|e| file_of(&e.location)).collect();
    if files.is_empty() {
        return;
    }
    let list = gdk::ContentProvider::for_value(&gdk::FileList::from_array(&files).to_value());
    let uris: String = files.iter().map(|f| format!("{}\r\n", f.uri())).collect();
    let uri_list = gdk::ContentProvider::for_bytes("text/uri-list", &glib::Bytes::from_owned(uris.clone().into_bytes()));
    let gnome = format!("{}\n{}", if cut { "cut" } else { "copy" }, uris.replace("\r\n", "\n").trim_end());
    let gnome = gdk::ContentProvider::for_bytes("x-special/gnome-copied-files", &glib::Bytes::from_owned(gnome.into_bytes()));
    if let Some(display) = gdk::Display::default() {
        display.clipboard().set_content(Some(&gdk::ContentProvider::new_union(&[list, uri_list, gnome]))).ok();
    }
    CUT.with(|c| *c.borrow_mut() = if cut { entries.iter().map(|e| e.location.clone()).collect() } else { Vec::new() });
}

pub(super) fn copy_paths(entries: &[FileEntry]) {
    let text: Vec<String> = entries
        .iter()
        .map(|e| e.location.native_path().map_or_else(|| e.location.display_path(), |p| p.display().to_string()))
        .collect();
    if let Some(display) = gdk::Display::default() {
        display.clipboard().set_text(&text.join("\n"));
    }
}

/// Pastes the clipboard's files into `destination`, asking once if names clash.
pub(super) fn paste(window: &Rc<Window>, tab: &Rc<Tab>, destination: Location) {
    let Some(display) = gdk::Display::default() else { return };
    let clipboard = display.clipboard();
    let (window, tab) = (window.clone(), tab.clone());
    glib::MainContext::default().spawn_local(async move {
        let Ok(value) = clipboard.read_value_future(gdk::FileList::static_type(), glib::Priority::DEFAULT).await else {
            return;
        };
        let Ok(files) = value.get::<gdk::FileList>() else { return };
        let sources: Vec<Location> = files.files().iter().map(location_of).collect();
        if sources.is_empty() {
            return;
        }
        let moving = CUT.with(|c| *c.borrow() == sources);
        transfer(&window, &tab, destination, sources, moving);
        if moving {
            CUT.with(|c| c.borrow_mut().clear());
        }
    });
}

/// Copies or moves `sources` into `destination`; clashing names get one question.
pub(super) fn transfer(window: &Rc<Window>, tab: &Rc<Tab>, destination: Location, sources: Vec<Location>, moving: bool) {
    let dest_dir = destination.native_path().map(std::path::PathBuf::from);
    let clashes: Vec<&Location> = sources
        .iter()
        .filter(|s| s.parent().as_ref() != Some(&destination))
        .filter(|s| match (&dest_dir, s.file_name()) {
            (Some(dir), Some(name)) => dir.join(name).symlink_metadata().is_ok(),
            _ => false,
        })
        .collect();
    let start = {
        let (tab, sources, destination) = (tab.clone(), sources.clone(), destination.clone());
        move |conflict: TransferConflict| {
            let items = sources
                .iter()
                .map(|source| {
                    let same_folder = source.parent().as_ref() == Some(&destination);
                    let folder = source.native_path().is_some_and(std::path::Path::is_dir);
                    let conflict = match conflict {
                        _ if same_folder => TransferConflict::KeepBoth,
                        TransferConflict::ReplaceExisting if folder => TransferConflict::Merge,
                        other => other,
                    };
                    PasteItem { source: source.clone(), conflict, target_name: None }
                })
                .collect();
            tab.browser.transfer(destination.clone(), items, moving, true);
        }
    };
    if clashes.is_empty() {
        start(TransferConflict::FailIfExists);
        return;
    }
    let title = if clashes.len() == 1 {
        crate::i18n::tr("An item with this name already exists")
    } else {
        crate::i18n::tr("Some items already exist here")
    };
    let (dialog, card) = widgets::dialog(&title, 420);
    let names: Vec<String> = clashes.iter().filter_map(|l| l.file_name()).map(|n| n.to_string_lossy().into_owned()).take(4).collect();
    let mut text = names.join(", ");
    if clashes.len() > names.len() {
        text.push_str(&format!(" +{}", clashes.len() - names.len()));
    }
    let note = widgets::label(&text, "dim");
    note.set_wrap(true);
    card.append(&note);
    let buttons = widgets::hbox(8);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_top(6);
    let cancel = gtk::Button::with_label(&crate::i18n::tr("Cancel"));
    let keep = gtk::Button::with_label(&crate::i18n::tr("Keep both"));
    let replace = gtk::Button::with_label(&crate::i18n::tr("Replace"));
    replace.add_css_class("suggested-action");
    for b in [&cancel, &keep, &replace] {
        buttons.append(b);
    }
    card.append(&buttons);
    let start = Rc::new(start);
    let d = dialog.clone();
    cancel.connect_clicked(move |_| d.close());
    let (d, s) = (dialog.clone(), start.clone());
    keep.connect_clicked(move |_| {
        d.close();
        s(TransferConflict::KeepBoth);
    });
    let (d, s) = (dialog.clone(), start);
    replace.connect_clicked(move |_| {
        d.close();
        s(TransferConflict::ReplaceExisting);
    });
    let _ = window;
    dialog.present();
    cancel.grab_focus();
}

pub(super) fn trash(window: &Rc<Window>, tab: &Rc<Tab>, entries: Vec<FileEntry>) {
    if entries.is_empty() {
        return;
    }
    let count = entries.len();
    tab.browser.delete(entries, false);
    let message = if count == 1 {
        crate::i18n::tr("Moved to Trash")
    } else {
        format!("{} · {}", crate::i18n::tr("Moved to Trash"), crate::i18n::count("items", count))
    };
    let browser = tab.browser.clone();
    toast::show_undo(&window.overlay, &message, move || {
        browser.undo_last_trash();
    });
}

pub(super) fn rename(tab: &Rc<Tab>, entry: FileEntry) {
    let tab = tab.clone();
    let name = entry.display_name.clone();
    widgets::ask_text(&crate::i18n::tr("Rename"), "", &name, &crate::i18n::tr("Rename"), move |new_name| {
        if new_name != entry.display_name {
            tab.browser.rename(entry.clone(), new_name);
        }
    });
}

pub(super) fn new_folder(tab: &Rc<Tab>) {
    let Some(parent) = tab.location() else { return };
    let tab = tab.clone();
    widgets::ask_text(
        &crate::i18n::tr("New folder"),
        "",
        &crate::i18n::tr("New Folder"),
        &crate::i18n::tr("Create"),
        move |name| tab.browser.create_exact_entry(parent.clone(), name, true),
    );
}

/// The menu for the selected items (a right-click on one of them).
pub(super) fn item_menu(window: &Rc<Window>, tab: &Rc<Tab>, entries: Vec<FileEntry>) -> Vec<menu::Entry> {
    let one = (entries.len() == 1).then(|| entries[0].clone());
    let folder = one.as_ref().filter(|e| e.is_directory()).cloned();
    let mut items = Vec::new();
    {
        let (tab, entries) = (tab.clone(), entries.clone());
        items.push(menu::item(&crate::i18n::tr("Open"), move || entries.iter().for_each(|e| tab.open(e))).icon("document-open-symbolic").key("Enter"));
    }
    if let Some(folder) = folder.clone() {
        let w = window.clone();
        items.push(
            menu::item(&crate::i18n::tr("Open in new tab"), move || w.open_tab(folder.location.clone()))
                .icon("tab-new-symbolic")
                .key(&keys(bindings::OPEN_IN_NEW_TAB)),
        );
    }
    items.push(menu::separator());
    {
        let e = entries.clone();
        items.push(menu::item(&crate::i18n::tr("Cut"), move || copy(&e, true)).icon("edit-cut-symbolic").key(&keys(bindings::CUT)));
    }
    {
        let e = entries.clone();
        items.push(menu::item(&crate::i18n::tr("Copy"), move || copy(&e, false)).icon("edit-copy-symbolic").key(&keys(bindings::COPY)));
    }
    if let Some(folder) = folder {
        let (w, t) = (window.clone(), tab.clone());
        items.push(menu::item(&crate::i18n::tr("Paste into folder"), move || paste(&w, &t, folder.location.clone())).icon("edit-paste-symbolic"));
    }
    items.push(menu::separator());
    if let Some(entry) = one {
        let t = tab.clone();
        items.push(menu::item(&crate::i18n::tr("Rename…"), move || rename(&t, entry.clone())).icon("document-edit-symbolic").key(&keys(bindings::RENAME)));
    }
    {
        let e = entries.clone();
        items.push(menu::item(&crate::i18n::tr("Copy path"), move || copy_paths(&e)).icon("edit-copy-symbolic"));
    }
    items.push(menu::separator());
    {
        let (w, t) = (window.clone(), tab.clone());
        items.push(
            menu::item(&crate::i18n::tr("Move to Trash"), move || trash(&w, &t, entries.clone()))
                .icon("user-trash-symbolic")
                .key(&keys(bindings::DELETE))
                .danger(),
        );
    }
    items
}

/// The menu for the folder showing (a right-click on empty space).
pub(super) fn folder_menu(window: &Rc<Window>, tab: &Rc<Tab>) -> Vec<menu::Entry> {
    let Some(here) = tab.location() else { return Vec::new() };
    let mut items = Vec::new();
    {
        let t = tab.clone();
        items.push(menu::item(&crate::i18n::tr("New folder…"), move || new_folder(&t)).icon("folder-new-symbolic").key(&keys(bindings::NEW_FOLDER)));
    }
    {
        let (w, t) = (window.clone(), tab.clone());
        items.push(menu::item(&crate::i18n::tr("Paste"), move || paste(&w, &t, here.clone())).icon("edit-paste-symbolic").key(&keys(bindings::PASTE)));
    }
    items.push(menu::separator());
    let shown = tab.shows_hidden();
    {
        let t = tab.clone();
        items.push(menu::check(&crate::i18n::tr("Show hidden files"), shown, move |_| t.browser.toggle_hidden()).key(&keys(bindings::HIDDEN_FILES)));
    }
    {
        let t = tab.clone();
        items.push(menu::item(&crate::i18n::tr("Refresh"), move || t.browser.reload_active()).icon("view-refresh-symbolic").key(&keys(bindings::REFRESH)));
    }
    items
}
