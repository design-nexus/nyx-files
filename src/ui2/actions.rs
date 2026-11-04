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

/// The files as clipboard or drag content: a file list and `text/uri-list`,
/// plus GNOME's copied-files form when `cut` says which it is.
fn content(entries: &[FileEntry], cut: Option<bool>) -> Option<gdk::ContentProvider> {
    let files: Vec<gio::File> = entries
        .iter()
        .filter_map(|e| file_of(&e.location))
        .collect();
    if files.is_empty() {
        return None;
    }
    let list = gdk::ContentProvider::for_value(&gdk::FileList::from_array(&files).to_value());
    let uris: String = files.iter().map(|f| format!("{}\r\n", f.uri())).collect();
    let uri_list = gdk::ContentProvider::for_bytes(
        "text/uri-list",
        &glib::Bytes::from_owned(uris.clone().into_bytes()),
    );
    let mut parts = vec![list, uri_list];
    if let Some(cut) = cut {
        let gnome = format!(
            "{}\n{}",
            if cut { "cut" } else { "copy" },
            uris.replace("\r\n", "\n").trim_end()
        );
        parts.push(gdk::ContentProvider::for_bytes(
            "x-special/gnome-copied-files",
            &glib::Bytes::from_owned(gnome.into_bytes()),
        ));
    }
    Some(gdk::ContentProvider::new_union(&parts))
}

pub(super) fn file_content(entries: &[FileEntry]) -> Option<gdk::ContentProvider> {
    content(entries, None)
}

pub(super) fn dropped(value: &glib::Value) -> Option<Vec<Location>> {
    let files = value.get::<gdk::FileList>().ok()?;
    let locations: Vec<Location> = files.files().iter().map(location_of).collect();
    (!locations.is_empty()).then_some(locations)
}

/// Shift moves and Ctrl copies; otherwise empty, and the drop decides.
pub(super) fn forced_action(target: &gtk::DropTarget) -> gdk::DragAction {
    let state = target.current_event_state();
    if state.contains(gdk::ModifierType::SHIFT_MASK) {
        gdk::DragAction::MOVE
    } else if state.contains(gdk::ModifierType::CONTROL_MASK) {
        gdk::DragAction::COPY
    } else {
        gdk::DragAction::empty()
    }
}

/// What the pointer shows while over a target.
pub(super) fn drop_action(target: &gtk::DropTarget) -> gdk::DragAction {
    let forced = forced_action(target);
    if forced.is_empty() {
        gdk::DragAction::MOVE
    } else {
        forced
    }
}

fn same_device(a: &Location, b: &Location) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (a.native_path(), b.native_path()) {
        (Some(a), Some(b)) => match (std::fs::symlink_metadata(a), std::fs::metadata(b)) {
            (Ok(a), Ok(b)) => a.dev() == b.dev(),
            _ => false,
        },
        _ => false,
    }
}

/// Files dropped into `destination`: moved on the same drive, else as the
/// "Drag & drop to another device" setting says; Shift and Ctrl override.
pub(super) fn drop_into(
    window: &Rc<Window>,
    tab: &Rc<Tab>,
    destination: Location,
    sources: Vec<Location>,
    forced: gdk::DragAction,
) {
    let sources: Vec<Location> = sources
        .into_iter()
        .filter(|s| s != &destination && !destination.is_within(s))
        .collect();
    if sources.is_empty() {
        return;
    }
    if forced.contains(gdk::DragAction::MOVE) || forced.contains(gdk::DragAction::COPY) {
        transfer(
            window,
            tab,
            destination,
            sources,
            forced.contains(gdk::DragAction::MOVE),
        );
        return;
    }
    if sources.iter().all(|s| same_device(s, &destination)) {
        transfer(window, tab, destination, sources, true);
        return;
    }
    use crate::services::CrossVolumeDropStrategy;
    match crate::ui::preferences::PreferenceManager::shared().cross_volume_drop_strategy() {
        CrossVolumeDropStrategy::Copy => transfer(window, tab, destination, sources, false),
        CrossVolumeDropStrategy::Move => transfer(window, tab, destination, sources, true),
        CrossVolumeDropStrategy::Ask => {
            let (dialog, card) =
                widgets::dialog(&crate::i18n::tr("Copy or move to another device?"), 400);
            let buttons = widgets::hbox(8);
            buttons.set_halign(gtk::Align::End);
            let cancel = gtk::Button::with_label(&crate::i18n::tr("Cancel"));
            let copy_button = gtk::Button::with_label(&crate::i18n::tr("Copy"));
            let move_button = gtk::Button::with_label(&crate::i18n::tr("Move"));
            copy_button.add_css_class("suggested-action");
            for b in [&cancel, &move_button, &copy_button] {
                buttons.append(b);
            }
            card.append(&buttons);
            let d = dialog.clone();
            cancel.connect_clicked(move |_| d.close());
            for (button, moving) in [(&copy_button, false), (&move_button, true)] {
                let (d, w, t, dest, src) = (
                    dialog.clone(),
                    window.clone(),
                    tab.clone(),
                    destination.clone(),
                    sources.clone(),
                );
                button.connect_clicked(move |_| {
                    d.close();
                    transfer(&w, &t, dest.clone(), src.clone(), moving);
                });
            }
            dialog.present();
            copy_button.grab_focus();
        }
    }
}

pub(super) fn copy(entries: &[FileEntry], cut: bool) {
    let Some(content) = content(entries, Some(cut)) else {
        return;
    };
    if let Some(display) = gdk::Display::default() {
        display.clipboard().set_content(Some(&content)).ok();
    }
    CUT.with(|c| {
        *c.borrow_mut() = if cut {
            entries.iter().map(|e| e.location.clone()).collect()
        } else {
            Vec::new()
        }
    });
}

pub(super) fn copy_paths(entries: &[FileEntry]) {
    let text: Vec<String> = entries
        .iter()
        .map(|e| {
            e.location
                .native_path()
                .map_or_else(|| e.location.display_path(), |p| p.display().to_string())
        })
        .collect();
    if let Some(display) = gdk::Display::default() {
        display.clipboard().set_text(&text.join("\n"));
    }
}

/// Pastes the clipboard's files into `destination`, asking once if names clash.
pub(super) fn paste(window: &Rc<Window>, tab: &Rc<Tab>, destination: Location) {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let clipboard = display.clipboard();
    let (window, tab) = (window.clone(), tab.clone());
    glib::MainContext::default().spawn_local(async move {
        let Ok(value) = clipboard
            .read_value_future(gdk::FileList::static_type(), glib::Priority::DEFAULT)
            .await
        else {
            return;
        };
        let Ok(files) = value.get::<gdk::FileList>() else {
            return;
        };
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
pub(super) fn transfer(
    window: &Rc<Window>,
    tab: &Rc<Tab>,
    destination: Location,
    sources: Vec<Location>,
    moving: bool,
) {
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
                    PasteItem {
                        source: source.clone(),
                        conflict,
                        target_name: None,
                    }
                })
                .collect();
            tab.browser
                .transfer(destination.clone(), items, moving, true);
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
    let names: Vec<String> = clashes
        .iter()
        .filter_map(|l| l.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .take(4)
        .collect();
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
        format!(
            "{} · {}",
            crate::i18n::tr("Moved to Trash"),
            crate::i18n::count("items", count)
        )
    };
    let browser = tab.browser.clone();
    toast::show_undo(&window.overlay, &message, move || {
        browser.undo_last_trash();
    });
}

/// Asks for a new name with the part before the extension selected.
pub(super) fn rename(tab: &Rc<Tab>, entry: FileEntry) {
    let (dialog, card) = widgets::dialog(&crate::i18n::tr("Rename"), 420);
    let field = gtk::Entry::new();
    field.set_text(&entry.display_name);
    card.append(&field);
    let buttons = widgets::hbox(8);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_top(6);
    let cancel = gtk::Button::with_label(&crate::i18n::tr("Cancel"));
    let ok = gtk::Button::with_label(&crate::i18n::tr("Rename"));
    ok.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&ok);
    card.append(&buttons);
    let d = dialog.clone();
    cancel.connect_clicked(move |_| d.close());
    let submit = {
        let (d, f, tab) = (dialog.clone(), field.clone(), tab.clone());
        move || {
            let name = f.text().trim().to_string();
            if name.is_empty() {
                f.add_css_class("error");
                return;
            }
            d.close();
            if name != entry.display_name {
                tab.browser.rename(entry.clone(), name);
            }
        }
    };
    let s2 = submit.clone();
    ok.connect_clicked(move |_| s2());
    field.connect_activate(move |_| submit());
    dialog.present();
    field.grab_focus();
    let name = field.text();
    let stem = match name.rfind('.') {
        Some(dot) if dot > 0 && !name[..dot].is_empty() && name[dot..].len() <= 6 => {
            name[..dot].chars().count() as i32
        }
        _ => -1,
    };
    field.select_region(0, stem);
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
pub(super) fn item_menu(
    window: &Rc<Window>,
    tab: &Rc<Tab>,
    entries: Vec<FileEntry>,
) -> Vec<menu::Entry> {
    let one = (entries.len() == 1).then(|| entries[0].clone());
    let folder = one.as_ref().filter(|e| e.is_directory()).cloned();
    let mut items = Vec::new();
    {
        let (tab, entries) = (tab.clone(), entries.clone());
        items.push(
            menu::item(&crate::i18n::tr("Open"), move || {
                entries.iter().for_each(|e| tab.open(e))
            })
            .icon("document-open-symbolic")
            .key("Enter"),
        );
    }
    if let Some(folder) = folder.clone() {
        let w = window.clone();
        items.push(
            menu::item(&crate::i18n::tr("Open in new tab"), move || {
                w.open_tab(folder.location.clone())
            })
            .icon("tab-new-symbolic")
            .key(&keys(bindings::OPEN_IN_NEW_TAB)),
        );
    }
    items.push(menu::separator());
    {
        let e = entries.clone();
        items.push(
            menu::item(&crate::i18n::tr("Cut"), move || copy(&e, true))
                .icon("edit-cut-symbolic")
                .key(&keys(bindings::CUT)),
        );
    }
    {
        let e = entries.clone();
        items.push(
            menu::item(&crate::i18n::tr("Copy"), move || copy(&e, false))
                .icon("edit-copy-symbolic")
                .key(&keys(bindings::COPY)),
        );
    }
    if let Some(folder) = folder.clone() {
        let location = folder.location.clone();
        if super::sidebar::is_pinned(&location) {
            if !super::sidebar::is_place_location(&location) {
                items.push(
                    menu::item(&crate::i18n::tr("Unpin from sidebar"), move || {
                        super::sidebar::unpin(location.clone())
                    })
                    .icon("view-pin-symbolic"),
                );
            }
        } else {
            let name = folder.display_name.clone();
            items.push(
                menu::item(&crate::i18n::tr("Pin to sidebar"), move || {
                    super::sidebar::pin(location.clone(), name.clone())
                })
                .icon("view-pin-symbolic"),
            );
        }
    }
    if let Some(folder) = folder {
        let (w, t) = (window.clone(), tab.clone());
        items.push(
            menu::item(&crate::i18n::tr("Paste into folder"), move || {
                paste(&w, &t, folder.location.clone())
            })
            .icon("edit-paste-symbolic"),
        );
    }
    items.push(menu::separator());
    if let Some(entry) = one {
        let t = tab.clone();
        items.push(
            menu::item(&crate::i18n::tr("Rename…"), move || {
                rename(&t, entry.clone())
            })
            .icon("document-edit-symbolic")
            .key(&keys(bindings::RENAME)),
        );
    }
    {
        let e = entries.clone();
        items.push(
            menu::item(&crate::i18n::tr("Copy path"), move || copy_paths(&e))
                .icon("edit-copy-symbolic"),
        );
    }
    if let [entry] = entries.as_slice() {
        let entry = entry.clone();
        items.push(
            menu::item(&crate::i18n::tr("Properties"), move || {
                super::properties::show(&entry)
            })
            .icon("document-properties-symbolic")
            .key(&keys(bindings::PROPERTIES)),
        );
    }
    items.push(menu::separator());
    {
        let (w, t) = (window.clone(), tab.clone());
        items.push(
            menu::item(&crate::i18n::tr("Move to Trash"), move || {
                trash(&w, &t, entries.clone())
            })
            .icon("user-trash-symbolic")
            .key(&keys(bindings::DELETE))
            .danger(),
        );
    }
    items
}

/// The menu for the folder showing (a right-click on empty space).
pub(super) fn folder_menu(window: &Rc<Window>, tab: &Rc<Tab>) -> Vec<menu::Entry> {
    let Some(here) = tab.location() else {
        return Vec::new();
    };
    let mut items = Vec::new();
    {
        let t = tab.clone();
        items.push(
            menu::item(&crate::i18n::tr("New folder…"), move || new_folder(&t))
                .icon("folder-new-symbolic")
                .key(&keys(bindings::NEW_FOLDER)),
        );
    }
    {
        let (w, t) = (window.clone(), tab.clone());
        items.push(
            menu::item(&crate::i18n::tr("Paste"), move || {
                paste(&w, &t, here.clone())
            })
            .icon("edit-paste-symbolic")
            .key(&keys(bindings::PASTE)),
        );
    }
    items.push(menu::separator());
    let shown = tab.shows_hidden();
    {
        let t = tab.clone();
        items.push(
            menu::check(&crate::i18n::tr("Show hidden files"), shown, move |_| {
                t.browser.toggle_hidden()
            })
            .key(&keys(bindings::HIDDEN_FILES)),
        );
    }
    {
        let t = tab.clone();
        items.push(
            menu::item(&crate::i18n::tr("Refresh"), move || {
                t.browser.reload_active()
            })
            .icon("view-refresh-symbolic")
            .key(&keys(bindings::REFRESH)),
        );
    }
    items
}

fn exists(location: &Location) -> bool {
    location
        .native_path()
        .is_none_or(|path| path.symlink_metadata().is_ok())
}

fn still_there(locations: Vec<Location>) -> Vec<Location> {
    locations.into_iter().filter(exists).collect()
}

fn move_items(records: Vec<crate::services::MoveRecord>) -> Vec<crate::services::UndoMoveItem> {
    records
        .into_iter()
        .filter(|r| exists(&r.current))
        .map(|record| crate::services::UndoMoveItem {
            record,
            conflict: TransferConflict::FailIfExists,
        })
        .collect()
}

/// Undoes the tab's latest rename, move, copy, merge or trash.
pub(super) fn undo(tab: &Rc<Tab>) -> bool {
    let b = &tab.browser;
    if let Some((generation, _, _)) = b.pending_undo_rename() {
        return b.undo_rename(generation);
    }
    if let Some((generation, records)) = b.pending_undo_move() {
        let items = move_items(records);
        return if items.is_empty() {
            b.discard_pending_replay(false, generation);
            false
        } else {
            b.undo_move(generation, items)
        };
    }
    if let Some((generation, records)) = b.pending_undo_group() {
        let items = move_items(records);
        return if items.is_empty() {
            b.discard_pending_replay(false, generation);
            false
        } else {
            b.undo_group(generation, items)
        };
    }
    if let Some((generation, locations)) = b.pending_undo_copy() {
        let locations = still_there(locations);
        return if locations.is_empty() {
            b.discard_pending_replay(false, generation);
            false
        } else {
            b.undo_copy(generation, locations)
        };
    }
    if let Some((generation, created, overwritten)) = b.pending_undo_merge() {
        let created = still_there(created);
        if created.is_empty() && overwritten.is_empty() {
            b.discard_pending_replay(false, generation);
            return false;
        }
        return b.undo_merge(generation, created, overwritten);
    }
    b.undo_last_trash()
}

/// Does again what the last undo reverted.
pub(super) fn redo(tab: &Rc<Tab>) -> bool {
    let b = &tab.browser;
    if let Some((generation, _, _)) = b.pending_redo_rename() {
        return b.redo_rename(generation);
    }
    if let Some((generation, records)) = b.pending_redo_move() {
        let items = move_items(records);
        return if items.is_empty() {
            b.discard_pending_replay(true, generation);
            false
        } else {
            b.redo_move(generation, items)
        };
    }
    if let Some((generation, locations)) = b.pending_redo_copy() {
        return b.redo_copy(generation, locations);
    }
    if let Some((generation, locations)) = b.pending_redo_trash() {
        let locations = still_there(locations);
        return if locations.is_empty() {
            b.discard_pending_replay(true, generation);
            false
        } else {
            b.redo_trash(generation, locations)
        };
    }
    false
}

/// The menu for a sidebar row: open it, pin or unpin, eject or unmount.
pub(super) fn sidebar_menu(
    window: &Rc<Window>,
    target: super::sidebar::Target,
) -> Vec<menu::Entry> {
    use super::sidebar::Target;
    let location = match &target {
        Target::Place(_, l) | Target::Pin(l) => Some(l.clone()),
        Target::Volume(v) => v.get_mount().map(|m| location_of(&m.root())),
        Target::Mount(m) => Some(location_of(&m.root())),
    };
    let mut items = Vec::new();
    if let Some(location) = location.clone() {
        let w = window.clone();
        items.push(
            menu::item(&crate::i18n::tr("Open in new tab"), move || {
                w.open_tab(location.clone())
            })
            .icon("tab-new-symbolic"),
        );
    }
    match &target {
        Target::Pin(location) => {
            let l = location.clone();
            items.push(
                menu::item(&crate::i18n::tr("Unpin"), move || {
                    super::sidebar::unpin(l.clone())
                })
                .icon("view-pin-symbolic"),
            );
        }
        Target::Place(id, _) => {
            let id = *id;
            items.push(
                menu::item(&crate::i18n::tr("Hide from sidebar"), move || {
                    hide_place(id)
                })
                .icon("view-conceal-symbolic"),
            );
            if id == "network" {
                let w = window.clone();
                items.push(
                    menu::item(&crate::i18n::tr("Connect to server…"), move || {
                        connect_to_server(&w)
                    })
                    .icon("network-server-symbolic"),
                );
            }
            if id == "trash" {
                let w = window.clone();
                items.push(menu::separator());
                items.push(
                    menu::item(&crate::i18n::tr("Empty Trash…"), move || empty_trash(&w))
                        .icon("user-trash-full-symbolic")
                        .danger(),
                );
            }
        }
        Target::Volume(_) | Target::Mount(_) => {
            if let Target::Volume(volume) = &target
                && volume.get_mount().is_none()
                && volume.can_mount()
            {
                let w = window.clone();
                let v = volume.clone();
                items.push(
                    menu::item(&crate::i18n::tr("Mount"), move || {
                        let w = w.clone();
                        let v2 = v.clone();
                        v.mount(
                            gio::MountMountFlags::NONE,
                            Some(&super::mount::operation()),
                            gio::Cancellable::NONE,
                            move |r| match r {
                                Ok(()) => {
                                    if let Some(m) = v2.get_mount() {
                                        w.navigate_to(location_of(&m.root()));
                                    }
                                }
                                Err(e) if e.matches(gio::IOErrorEnum::FailedHandled) => {}
                                Err(e) => toast::show(&w.overlay, e.message()),
                            },
                        );
                    })
                    .icon("drive-harddisk-symbolic"),
                );
            }
            {
                let t = target.clone();
                items.push(
                    menu::item(&crate::i18n::tr("Properties"), move || {
                        super::drives::show_properties(&t)
                    })
                    .icon("document-properties-symbolic"),
                );
            }
            {
                let t = target.clone();
                items.push(
                    menu::item(&crate::i18n::tr("Set label…"), move || {
                        super::drives::set_label(&t)
                    })
                    .icon("document-edit-symbolic"),
                );
            }
            if let Some(label) = super::devices::release_label(&target) {
                let t = target.clone();
                let w = window.clone();
                items.push(menu::separator());
                items.push(
                    menu::item(&crate::i18n::tr(label), move || {
                        super::devices::release(&t, w.window.upcast_ref())
                    })
                    .icon("media-eject-symbolic"),
                );
            }
            if super::drives::can_format(&target) {
                let (t, w) = (target.clone(), window.clone());
                items.push(
                    menu::item(&crate::i18n::tr("Format…"), move || {
                        super::drives::format(&t, &w.overlay)
                    })
                    .icon("edit-clear-all-symbolic")
                    .danger(),
                );
            }
        }
    }
    items
}

fn hide_place(id: &str) {
    let prefs = crate::ui::preferences::PreferenceManager::shared();
    match id {
        "recent" => prefs.set_sidebar_show_recent(false),
        "home" => prefs.set_sidebar_show_home(false),
        "desktop" => prefs.set_sidebar_show_desktop(false),
        "documents" => prefs.set_sidebar_show_documents(false),
        "downloads" => prefs.set_sidebar_show_downloads(false),
        "music" => prefs.set_sidebar_show_music(false),
        "pictures" => prefs.set_sidebar_show_pictures(false),
        "videos" => prefs.set_sidebar_show_videos(false),
        "network" => prefs.set_sidebar_show_network(false),
        "trash" => prefs.set_sidebar_show_trash(false),
        _ => {}
    }
}

/// Measures Trash, asks, then deletes everything in it for good.
pub(super) fn empty_trash(window: &Rc<Window>) {
    let window = window.clone();
    glib::MainContext::default().spawn_local(async move {
        let trash = gio::File::for_uri("trash:///");
        let summary = crate::adapters::directory_summary::summarize_directory(&trash).await;
        let Ok(summary) = summary else { return };
        if summary.item_count == 0 {
            toast::show(&window.overlay, &crate::i18n::tr("Trash is empty"));
            return;
        }
        let (dialog, card) = widgets::dialog(&crate::i18n::tr("Empty Trash?"), 420);
        let note = widgets::label(
            &format!(
                "{} · {}. {}",
                crate::i18n::count("items", summary.item_count),
                glib::format_size(summary.total_size),
                crate::i18n::tr("They will be deleted permanently.")
            ),
            "dim",
        );
        note.set_wrap(true);
        card.append(&note);
        let buttons = widgets::hbox(8);
        buttons.set_halign(gtk::Align::End);
        buttons.set_margin_top(6);
        let cancel = gtk::Button::with_label(&crate::i18n::tr("Cancel"));
        let empty = gtk::Button::with_label(&crate::i18n::tr("Empty Trash"));
        empty.add_css_class("destructive-action");
        buttons.append(&cancel);
        buttons.append(&empty);
        card.append(&buttons);
        let d = dialog.clone();
        cancel.connect_clicked(move |_| d.close());
        let d = dialog.clone();
        empty.connect_clicked(move |_| {
            d.close();
            let window = window.clone();
            glib::MainContext::default().spawn_local(async move {
                let root = gio::File::for_uri("trash:///");
                let message = match crate::adapters::trash::empty_trash(&root, |_| {}).await {
                    Ok(outcome) if outcome.failed == 0 => crate::i18n::tr("Trash emptied"),
                    Ok(outcome) => rust_i18n::t!(
                        "%{items} could not be deleted. The remaining items were processed.",
                        items = crate::i18n::count("items", outcome.failed)
                    )
                    .into_owned(),
                    Err(error) => error.message().to_string(),
                };
                toast::show(&window.overlay, &message);
            });
        });
        dialog.present();
        cancel.grab_focus();
    });
}

pub(super) fn connect_to_server(window: &Rc<Window>) {
    let w = window.clone();
    super::mount::connect_dialog(move |location, pin| {
        if pin {
            let name = location.display_name();
            super::sidebar::pin(location.clone(), name);
        }
        w.navigate_to(location);
    });
}
