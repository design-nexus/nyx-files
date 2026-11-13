// SPDX-License-Identifier: MIT

//! The item and folder menus, with every action Files offers there, and the
//! actions only these menus (and their keys) reach.

use std::{path::PathBuf, rc::Rc};

use gtk::{gio, glib, prelude::*};
use nyx_ui::{menu, toast, widgets};

use crate::{
    model::{FileEntry, Location},
    ui::{browser::desktop, keys::bindings},
};

use super::{
    actions::{self, keys},
    tab::Tab,
    window::Window,
};

thread_local! {
    /// Removals from Recent still in flight, so a second request skips them.
    static RECENT_REMOVAL: crate::adapters::RecentRemovalState =
        crate::adapters::RecentRemovalState::new();
}

fn in_trash(location: &Location) -> bool {
    location
        .uri_value()
        .is_some_and(|uri| uri.starts_with("trash:"))
}

fn copy_text(text: &str) {
    if let Some(display) = gtk::gdk::Display::default() {
        display.clipboard().set_text(text);
    }
}

/// Names the mount where an earlier trash attempt failed, when the first item is on it.
fn trash_unavailable_reason(entries: &[FileEntry]) -> Option<String> {
    let parent = entries.first()?.location.parent()?;
    let (mount, _) = crate::adapters::trash_support::unsupported_reason_for(parent.native_path()?)?;
    Some(
        rust_i18n::t!(
            "Trash isn't available on %{mount_point} (%{fs_type}).",
            mount_point = mount.mount_point.display(),
            fs_type = mount.fs_type
        )
        .into_owned(),
    )
}

/// Asks before deleting for good; Trash can't take these back.
pub(super) fn delete_permanently(window: &Rc<Window>, tab: &Rc<Tab>, entries: Vec<FileEntry>) {
    if entries.is_empty() {
        return;
    }
    let (dialog, card) = widgets::dialog(&crate::i18n::tr("Delete permanently?"), 420);
    let names: Vec<String> = entries
        .iter()
        .take(4)
        .map(|e| e.display_name.clone())
        .collect();
    let mut text = names.join(", ");
    if entries.len() > names.len() {
        text.push_str(&format!(" +{}", entries.len() - names.len()));
    }
    let single = entries.len() == 1;
    let remote = entries.iter().any(|e| e.location.native_path().is_none());
    let reach = crate::i18n::tr(match (remote, single) {
        (false, true) => "This item will be permanently deleted. This action cannot be undone.",
        (false, false) => "These items will be permanently deleted. This action cannot be undone.",
        (true, true) => {
            "This item will be deleted without using Trash. Files can't undo this; whether it can be recovered depends on the storage provider."
        }
        (true, false) => {
            "These items will be deleted without using Trash. Files can't undo this; whether they can be recovered depends on the storage provider."
        }
    });
    let mut message = format!("{text}\n\n{reach}");
    if let Some(reason) = trash_unavailable_reason(&entries) {
        message = format!("{reason}\n{message}");
    }
    let note = widgets::label(&message, "dim");
    note.set_wrap(true);
    card.append(&note);
    let buttons = widgets::hbox(8);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_top(6);
    let cancel = gtk::Button::with_label(&crate::i18n::tr("Cancel"));
    let delete = gtk::Button::with_label(&crate::i18n::tr("Delete"));
    delete.add_css_class("destructive-action");
    buttons.append(&cancel);
    buttons.append(&delete);
    card.append(&buttons);
    let d = dialog.clone();
    cancel.connect_clicked(move |_| d.close());
    let (d, t) = (dialog.clone(), tab.clone());
    delete.connect_clicked(move |_| {
        d.close();
        t.browser.delete(entries.clone(), true);
    });
    let _ = window;
    dialog.present();
    cancel.grab_focus();
}

/// Trash, or deleting for good where Trash can't take them (inside Trash, or
/// a place without one).
pub(super) fn remove(window: &Rc<Window>, tab: &Rc<Tab>, entries: Vec<FileEntry>) {
    let here = tab.location();
    let no_trash = here.as_ref().is_some_and(in_trash)
        || tab
            .browser
            .active_depth()
            .and_then(|d| tab.browser.can_trash_at(d))
            == Some(false);
    if no_trash {
        delete_permanently(window, tab, entries);
    } else {
        actions::trash(window, tab, entries);
    }
}

/// Puts trashed items back where they came from, after showing where that is.
pub(super) fn restore(window: &Rc<Window>, tab: &Rc<Tab>, entries: Vec<FileEntry>) {
    let (window, tab) = (window.clone(), tab.clone());
    glib::MainContext::default().spawn_local(async move {
        let lookups = entries
            .iter()
            .map(|e| (e.location.clone(), e.thumbnail_path.clone()))
            .collect();
        let found =
            crate::adapters::trash_restore::restore_destinations_for_locations(lookups).await;
        let mut items = Vec::new();
        let mut problems = Vec::new();
        for (entry, destination) in entries.into_iter().zip(found) {
            match destination {
                Ok(destination) => {
                    items.push(crate::services::RestoreTrashItem { entry, destination })
                }
                Err(error) => {
                    problems.push(format!("{}: {}", entry.display_name, error.user_message()))
                }
            }
        }
        if items.is_empty() {
            toast::show(&window.overlay, &problems.join("\n"));
            return;
        }
        let (dialog, card) = widgets::dialog(&crate::i18n::tr("Restore"), 460);
        for item in &items {
            let row = widgets::hbox(8);
            let name = widgets::label(&item.entry.display_name, "search-name");
            name.set_hexpand(true);
            row.append(&name);
            row.append(&widgets::label(
                &item.destination.display().to_string(),
                "dim",
            ));
            card.append(&row);
        }
        if !problems.is_empty() {
            let note = widgets::label(&problems.join("\n"), "dim");
            note.set_wrap(true);
            card.append(&note);
        }
        let buttons = widgets::hbox(8);
        buttons.set_halign(gtk::Align::End);
        buttons.set_margin_top(6);
        let cancel = gtk::Button::with_label(&crate::i18n::tr("Cancel"));
        let ok = gtk::Button::with_label(&crate::i18n::tr("Restore"));
        ok.add_css_class("suggested-action");
        buttons.append(&cancel);
        buttons.append(&ok);
        card.append(&buttons);
        let d = dialog.clone();
        cancel.connect_clicked(move |_| d.close());
        let d = dialog.clone();
        ok.connect_clicked(move |_| {
            d.close();
            tab.browser.restore(items.clone());
        });
        dialog.present();
        ok.grab_focus();
    });
}

/// A copy of each item beside it.
pub(super) fn duplicate(window: &Rc<Window>, tab: &Rc<Tab>, entries: &[FileEntry]) {
    let Some(parent) = entries.first().and_then(|e| e.location.parent()) else {
        return;
    };
    if entries
        .iter()
        .any(|e| e.location.parent().as_ref() != Some(&parent))
    {
        return;
    }
    actions::transfer(
        window,
        tab,
        parent,
        entries.iter().map(|e| e.location.clone()).collect(),
        false,
    );
}

/// Names a new folder, then moves the selection into it.
pub(super) fn new_folder_with_selection(
    window: &Rc<Window>,
    tab: &Rc<Tab>,
    entries: Vec<FileEntry>,
) {
    let Some(parent) = tab.location() else { return };
    let (w, t) = (window.clone(), tab.clone());
    widgets::ask_text(
        &crate::i18n::tr("New folder with selection"),
        "",
        &crate::i18n::tr("New Folder"),
        &crate::i18n::tr("Create"),
        move |name| {
            let Some(folder) = parent.child(std::ffi::OsStr::new(&name)) else {
                return;
            };
            w.pending_group.replace(Some((
                folder,
                entries.iter().map(|e| e.location.clone()).collect(),
            )));
            t.browser.create_exact_entry(parent.clone(), name, true);
        },
    );
}

pub(super) fn new_file(tab: &Rc<Tab>) {
    let Some(parent) = tab.location() else { return };
    let tab = tab.clone();
    widgets::ask_text(
        &crate::i18n::tr("New file"),
        "",
        &crate::i18n::tr("New File"),
        &crate::i18n::tr("Create"),
        move |name| tab.browser.create_exact_entry(parent.clone(), name, false),
    );
}

/// Moves or copies the selection into a folder chosen in a dialog.
pub(super) fn transfer_to(
    window: &Rc<Window>,
    tab: &Rc<Tab>,
    entries: Vec<FileEntry>,
    moving: bool,
) {
    let title = crate::i18n::tr(if moving { "Move to" } else { "Copy to" });
    let dialog = gtk::FileDialog::builder().title(title).modal(true).build();
    if let Some(folder) = tab
        .location()
        .and_then(|l| l.native_path().map(gio::File::for_path))
    {
        dialog.set_initial_folder(Some(&folder));
    }
    let (w, t) = (window.clone(), tab.clone());
    dialog.select_folder(
        Some(&window.window),
        gio::Cancellable::NONE,
        move |result| {
            if let Some(path) = result.ok().and_then(|f| f.path()) {
                let sources = entries.iter().map(|e| e.location.clone()).collect();
                actions::transfer(&w, &t, Location::local(path), sources, moving);
            }
        },
    );
}

/// Send to: removable drives (their root, a recent folder there, or one chosen).
fn send_to(window: &Rc<Window>, tab: &Rc<Tab>, entries: &[FileEntry]) -> Option<menu::Entry> {
    let items = send_to_items(window, tab, entries);
    if items.is_empty() {
        return None;
    }
    Some(menu::sub(&crate::i18n::tr("Send to…"), items).icon("document-send-symbolic"))
}

/// The drives and their recent folders Send to offers.
pub(super) fn send_to_items(window: &Rc<Window>, tab: &Rc<Tab>, entries: &[FileEntry]) -> Vec<menu::Entry> {
    let destinations = crate::ui::window::devices::removable_destinations();
    let sources: Vec<Location> = entries.iter().map(|e| e.location.clone()).collect();
    let mut items = Vec::new();
    for destination in destinations {
        let (w, t, s, root) = (
            window.clone(),
            tab.clone(),
            sources.clone(),
            destination.root.clone(),
        );
        items.push(
            menu::item(&destination.name, move || {
                actions::transfer(&w, &t, Location::local(root.clone()), s.clone(), false)
            })
            .icon("drive-removable-media-symbolic"),
        );
        for folder in crate::ui::window::devices::send_to_recent_folders(&destination)
            .into_iter()
            .take(3)
        {
            let (w, t, s) = (window.clone(), tab.clone(), sources.clone());
            let target: PathBuf = destination.root.join(&folder);
            let label = format!("{} › {}", destination.name, folder.display());
            items.push(
                menu::item(&label, move || {
                    actions::transfer(&w, &t, Location::local(target.clone()), s.clone(), false)
                })
                .icon("folder-symbolic"),
            );
        }
    }
    items
}

/// Extracts `entry` into a folder chosen in a dialog.
pub(super) fn extract_to(window: &Rc<Window>, tab: &Rc<Tab>, entry: FileEntry) {
    let dialog = gtk::FileDialog::builder()
        .title(crate::i18n::tr("Extract to"))
        .modal(true)
        .build();
    let (w, t) = (window.clone(), tab.clone());
    dialog.select_folder(
        Some(&window.window),
        gio::Cancellable::NONE,
        move |result| {
            if let Some(path) = result.ok().and_then(|f| f.path()) {
                super::operations::extract(&w, &t, entry.clone(), Some(Location::local(path)));
            }
        },
    );
}

/// Your own actions for the selection (or this folder when nothing is
/// selected), in menu order, with what to run them on.
pub(super) fn numbered_actions(
    tab: &Rc<Tab>,
) -> Option<(
    Vec<crate::services::MatchedAction>,
    Vec<PathBuf>,
    PathBuf,
    bool,
)> {
    let parent = tab.location()?.native_path().map(PathBuf::from)?;
    let selected = tab.selected();
    let background = selected.is_empty();
    let (inputs, paths) = if background {
        let here = tab.location()?;
        (
            vec![crate::ui::actions::folder_input(&here)?],
            vec![parent.clone()],
        )
    } else {
        (
            crate::ui::actions::inputs_for_entries(&selected)?,
            crate::ui::actions::native_paths(&selected)?,
        )
    };
    let matched = crate::ui::actions::shared().catalog().matches(&inputs);
    Some((matched, paths, parent, background))
}

/// Runs the `slot`th (1-based) of [`numbered_actions`].
pub(super) fn run_numbered_action(window: &Rc<Window>, tab: &Rc<Tab>, slot: usize) -> bool {
    let Some((matched, paths, parent, background)) = numbered_actions(tab) else {
        return false;
    };
    let Some(found) = matched.get(slot.wrapping_sub(1)) else {
        return false;
    };
    let source = if background {
        crate::services::InvocationSource::Background
    } else {
        crate::services::InvocationSource::Selection
    };
    crate::ui::actions::run_action(
        &window.window,
        found.action.clone(),
        paths,
        parent,
        source,
        None,
    );
    true
}

/// Your own actions that apply to these items (or this folder).
fn custom_actions(
    window: &Rc<Window>,
    inputs: Vec<crate::model::ActionInput>,
    paths: Vec<PathBuf>,
    parent: PathBuf,
    background: bool,
) -> Option<menu::Entry> {
    let catalog = crate::ui::actions::shared().catalog();
    let matched = catalog.matches(&inputs);
    if matched.is_empty() {
        return None;
    }
    let source = if background {
        crate::services::InvocationSource::Background
    } else {
        crate::services::InvocationSource::Selection
    };
    let mut items = Vec::new();
    for found in matched {
        let action = found.action.clone();
        let (w, paths, parent) = (window.clone(), paths.clone(), parent.clone());
        let name = action.name().to_string();
        let enabled = action.is_available();
        items.push(
            menu::item(&name, move || {
                crate::ui::actions::run_action(
                    &w.window,
                    action.clone(),
                    paths.clone(),
                    parent.clone(),
                    source,
                    None,
                )
            })
            .icon(crate::ui::actions::action_icon(
                found.action.definition.icon.as_deref(),
            ))
            .enabled(enabled),
        );
    }
    Some(menu::sub(&crate::i18n::tr("Actions"), items).icon("system-run-symbolic"))
}

/// What sync and storage providers offer for these paths, in their own menu.
fn providers(window: &Rc<Window>, paths: Vec<PathBuf>, background: bool) -> Option<menu::Entry> {
    let root = crate::services::file_providers::config_root()?;
    if crate::services::file_providers::discover(&root).is_empty() {
        return None;
    }
    let w = window.clone();
    Some(
        menu::item(&crate::i18n::tr("Sync and storage…"), move || {
            let Some((anchor, x, y)) = w.menu_point() else {
                return;
            };
            let (owner, overlay) = (w.window.clone().upcast::<gtk::Widget>(), w.overlay.clone());
            crate::ui::file_providers::collect_menu(&owner, paths.clone(), background, move |entries| {
                if entries.is_empty() {
                    nyx_ui::toast::show(
                        &overlay,
                        &crate::i18n::tr("No sync or storage actions for these items"),
                    );
                } else {
                    menu::popup_at(&anchor, x, y, entries);
                }
            });
        })
        .icon("emblem-shared-symbolic"),
    )
}

/// Properties for several items: how many, of what kinds, and how big.
fn selection_properties(entries: Vec<FileEntry>) {
    let (dialog, card) = widgets::dialog(&crate::i18n::count("items", entries.len()), 420);
    let folders = entries.iter().filter(|e| e.is_directory()).count();
    let files = entries.len() - folders;
    let size: u64 = entries
        .iter()
        .filter_map(|e| match e.size {
            crate::model::MetadataValue::Known(s) if !e.is_directory() => Some(s),
            _ => None,
        })
        .sum();
    let details = gtk::Box::new(gtk::Orientation::Vertical, 6);
    details.add_css_class("properties-details");
    for (title, value) in [
        (
            "Contains",
            format!(
                "{}, {}",
                crate::i18n::count("files", files),
                crate::i18n::count("folders", folders)
            ),
        ),
        (
            "Size",
            if folders > 0 {
                format!("≥ {}", glib::format_size(size))
            } else {
                glib::format_size(size).to_string()
            },
        ),
        (
            "Location",
            entries
                .first()
                .and_then(|e| e.location.parent())
                .map(|l| l.display_path())
                .unwrap_or_default(),
        ),
    ] {
        let line = widgets::hbox(12);
        let key = widgets::label(&crate::i18n::tr(title), "properties-row-label");
        key.set_width_chars(12);
        line.append(&key);
        let value = widgets::label(&value, "properties-row-value");
        value.set_selectable(true);
        value.set_wrap(true);
        line.append(&value);
        details.append(&line);
    }
    card.append(&details);
    let close = gtk::Button::with_label(&crate::i18n::tr("Close"));
    close.set_halign(gtk::Align::End);
    let d = dialog.clone();
    close.connect_clicked(move |_| d.close());
    card.append(&close);
    dialog.present();
}

pub(super) fn properties(entries: Vec<FileEntry>) {
    match entries.as_slice() {
        [entry] => super::properties::show(entry),
        [] => {}
        _ => selection_properties(entries),
    }
}

/// The menu for the selected items (a right-click on one of them).
pub(super) fn item_menu(
    window: &Rc<Window>,
    tab: &Rc<Tab>,
    entries: Vec<FileEntry>,
) -> Vec<menu::Entry> {
    let one = (entries.len() == 1).then(|| entries[0].clone());
    let folder = one.as_ref().filter(|e| e.is_directory()).cloned();
    let trashed = entries.iter().all(|e| in_trash(&e.location));
    let native = entries.iter().all(|e| e.location.native_path().is_some());
    let mut items = Vec::new();
    if !trashed {
        items.push(edit_icons(window, tab, &entries));
    }
    {
        let (t, e) = (tab.clone(), entries.clone());
        items.push(
            menu::item(&crate::i18n::tr("Open"), move || {
                e.iter().for_each(|x| t.open(x))
            })
            .icon("document-open-symbolic")
            .key(&crate::i18n::tr("keys.name.enter")),
        );
    }
    if !entries.is_empty() && entries.iter().all(|e| e.recent_uri.is_some()) {
        let uris: Vec<String> = entries
            .iter()
            .filter_map(|e| e.recent_uri.clone())
            .collect();
        items.push(
            menu::item(&crate::i18n::tr("Remove from Recent"), move || {
                RECENT_REMOVAL
                    .with(|state| crate::adapters::recent_remove_entries(state, uris.clone()));
            })
            .icon("list-remove-symbolic"),
        );
    }
    if let Some(folder) = folder.clone() {
        let w = window.clone();
        let l = folder.location.clone();
        items.push(
            menu::item(&crate::i18n::tr("New Tab"), move || w.open_tab(l.clone()))
                .icon("tab-new-symbolic")
                .key(&keys(bindings::OPEN_IN_NEW_TAB)),
        );
        let w = window.clone();
        let l = folder.location.clone();
        items.push(
            menu::item(&crate::i18n::tr("New Window"), move || {
                w.open_window(l.clone())
            })
            .icon("window-new-symbolic"),
        );
    }
    if entries.iter().all(|e| !e.is_directory()) && !trashed {
        items.push(super::open_with::submenu(window, &entries));
    }
    if let Some(entry) = one.clone() {
        if !entry.is_directory() && crate::ui::preview::entry_supports_quick_preview(&entry) {
            let t = tab.clone();
            let e = entry.clone();
            items.push(
                menu::item(&crate::i18n::tr("Quick preview"), move || {
                    t.preview.toggle(
                        crate::ui::preview::preview_target(Some(e.clone())),
                        t.browser.active_depth(),
                    )
                })
                .icon("view-reveal-symbolic")
                .key(&crate::i18n::tr("keys.name.space")),
            );
        }
        if desktop::entry_is_regular_executable(&entry) {
            let (l, w) = (entry.location.clone(), window.clone());
            items.push(
                menu::item(&crate::i18n::tr("Run"), move || {
                    desktop::confirm_run_program(&l, &w.window)
                })
                .icon("media-playback-start-symbolic"),
            );
        }
        let shown_elsewhere = tab.location().as_ref() != entry.location.parent().as_ref();
        if shown_elsewhere && let Some(parent) = entry.location.parent() {
            let (t, l) = (tab.clone(), entry.location.clone());
            items.push(
                menu::item(&crate::i18n::tr("Open file location"), move || {
                    t.browser.reveal_locations(parent.clone(), vec![l.clone()]);
                })
                .icon("folder-open-symbolic"),
            );
        }
        if entry.is_directory() && entry.location.native_path().is_some() {
            let (l, w) = (entry.location.clone(), window.clone());
            items.push(
                menu::item(&crate::i18n::tr("Open in Terminal"), move || {
                    desktop::launch_terminal(&l, &w.window)
                })
                .icon("utilities-terminal-symbolic"),
            );
        }
        if !entry.is_directory() && crate::ui::preview::entry_supports_quick_preview(&entry) {
            let (t, e) = (tab.clone(), entry.clone());
            items.push(
                menu::item(&crate::i18n::tr("Print"), move || {
                    t.preview.print_entry(e.clone())
                })
                .icon("document-print-symbolic"),
            );
        }
    }
    if trashed {
        items.push(menu::separator());
        let (w, t, e) = (window.clone(), tab.clone(), entries.clone());
        items.push(
            menu::item(&crate::i18n::tr("Restore"), move || {
                restore(&w, &t, e.clone())
            })
            .icon("edit-undo-symbolic"),
        );
        let (w, t, e) = (window.clone(), tab.clone(), entries.clone());
        items.push(
            menu::item(&crate::i18n::tr("Delete permanently"), move || {
                delete_permanently(&w, &t, e.clone())
            })
            .icon("edit-delete-symbolic")
            .danger(),
        );
        return items;
    }
    items.push(menu::separator());
    {
        let (w, t, e) = (window.clone(), tab.clone(), entries.clone());
        items.push(
            menu::item(&crate::i18n::tr("Duplicate"), move || duplicate(&w, &t, &e))
                .icon("edit-copy-symbolic")
                .key(&keys(bindings::DUPLICATE)),
        );
    }
    items.push(copy_and_move(window, tab, &entries, native));
    items.push(copy_as_text(&entries));
    if let Some(archive) = archive(window, tab, &entries, one.as_ref(), native) {
        items.push(archive);
    }
    if let (Some(inputs), Some(paths), Some(parent)) = (
        crate::ui::actions::inputs_for_entries(&entries),
        crate::ui::actions::native_paths(&entries),
        tab.location()
            .and_then(|l| l.native_path().map(PathBuf::from)),
    ) {
        if let Some(custom) = custom_actions(window, inputs, paths.clone(), parent, false) {
            items.push(custom);
        }
        if let Some(provider) = providers(window, paths, false) {
            items.push(provider);
        }
    }
    items.push(menu::separator());
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
    if let Some(entry) = one.as_ref()
        && let Some(path) = entry.location.native_path()
    {
        let (path, directory, name) = (
            path.to_path_buf(),
            entry.is_directory(),
            entry.display_name.clone(),
        );
        items.push(
            menu::item(&crate::i18n::tr("Customize…"), move || {
                super::customize::show(path.clone(), directory, &name)
            })
            .icon("applications-graphics-symbolic"),
        );
    }
    {
        let e = entries.clone();
        items.push(
            menu::item(&crate::i18n::tr("Properties"), move || {
                properties(e.clone())
            })
            .icon("document-properties-symbolic")
            .key(&keys(bindings::PROPERTIES)),
        );
    }
    items.push(menu::separator());
    {
        let (w, t, e) = (window.clone(), tab.clone(), entries.clone());
        items.push(
            menu::item(&crate::i18n::tr("Delete without Trash"), move || {
                delete_permanently(&w, &t, e.clone())
            })
            .icon("edit-delete-symbolic")
            .key(&keys(bindings::DELETE_PERMANENTLY))
            .danger(),
        );
    }
    items
}

/// Cut, Copy, Paste (into a folder), Rename and Move to Trash, as icons at the top.
fn edit_icons(window: &Rc<Window>, tab: &Rc<Tab>, entries: &[FileEntry]) -> menu::Entry {
    let mut buttons = Vec::new();
    let e = entries.to_vec();
    buttons.push(
        menu::icon_button("edit-cut-symbolic", &crate::i18n::tr("Cut"), move || {
            actions::copy(&e, true)
        })
        .key(&keys(bindings::CUT)),
    );
    let e = entries.to_vec();
    buttons.push(
        menu::icon_button("edit-copy-symbolic", &crate::i18n::tr("Copy"), move || {
            actions::copy(&e, false)
        })
        .key(&keys(bindings::COPY)),
    );
    if let [folder] = entries
        && folder.is_directory()
    {
        let (w, t, l) = (window.clone(), tab.clone(), folder.location.clone());
        buttons.push(menu::icon_button(
            "edit-paste-symbolic",
            &crate::i18n::tr("Paste into folder"),
            move || actions::paste(&w, &t, l.clone()),
        ));
    }
    if let [entry] = entries {
        let (t, e) = (tab.clone(), entry.clone());
        buttons.push(
            menu::icon_button(
                "document-edit-symbolic",
                &crate::i18n::tr("Rename…"),
                move || actions::rename(&t, e.clone()),
            )
            .key(&keys(bindings::RENAME)),
        );
    }
    let (w, t, e) = (window.clone(), tab.clone(), entries.to_vec());
    buttons.push(
        menu::icon_button(
            "user-trash-symbolic",
            &crate::i18n::tr("Move to Trash"),
            move || remove(&w, &t, e.clone()),
        )
        .key(&keys(bindings::DELETE))
        .danger(),
    );
    menu::icons(buttons)
}

/// Move to…, Copy to…, Send to… and New Folder with Selection.
fn copy_and_move(
    window: &Rc<Window>,
    tab: &Rc<Tab>,
    entries: &[FileEntry],
    native: bool,
) -> menu::Entry {
    let (w, t, e) = (window.clone(), tab.clone(), entries.to_vec());
    let (w2, t2, e2) = (window.clone(), tab.clone(), entries.to_vec());
    let mut items = vec![
        menu::item(&crate::i18n::tr("Move to…"), move || {
            transfer_to(&w, &t, e.clone(), true)
        })
        .icon("go-jump-symbolic"),
        menu::item(&crate::i18n::tr("Copy to…"), move || {
            transfer_to(&w2, &t2, e2.clone(), false)
        })
        .icon("folder-symbolic"),
    ];
    if let Some(send) = send_to(window, tab, entries) {
        items.push(send);
    }
    if native && tab.location().is_some_and(|l| !l.is_recent_location()) {
        let (w, t, e) = (window.clone(), tab.clone(), entries.to_vec());
        items.push(
            menu::item(&crate::i18n::tr("New Folder with Selection"), move || {
                new_folder_with_selection(&w, &t, e.clone())
            })
            .icon("folder-new-symbolic")
            .key(&keys(bindings::NEW_FOLDER_WITH_SELECTION)),
        );
    }
    menu::sub(&crate::i18n::tr("Copy and move"), items).icon("go-jump-symbolic")
}

/// Copy path(s) and Copy name(s).
fn copy_as_text(entries: &[FileEntry]) -> menu::Entry {
    let single = entries.len() == 1;
    let e = entries.to_vec();
    let names: Vec<String> = entries.iter().map(|e| e.display_name.clone()).collect();
    menu::sub(
        &crate::i18n::tr("Copy as text"),
        vec![
            menu::item(
                &crate::i18n::tr(if single { "Copy path" } else { "Copy paths" }),
                move || actions::copy_paths(&e),
            )
            .icon("edit-copy-symbolic"),
            menu::item(
                &crate::i18n::tr(if single { "Copy name" } else { "Copy names" }),
                move || copy_text(&names.join("\n")),
            )
            .icon("edit-copy-symbolic"),
        ],
    )
    .icon("edit-copy-symbolic")
}

/// Compress… on its own, or with Extract here and Extract to… for an archive.
fn archive(
    window: &Rc<Window>,
    tab: &Rc<Tab>,
    entries: &[FileEntry],
    one: Option<&FileEntry>,
    native: bool,
) -> Option<menu::Entry> {
    let mut items = Vec::new();
    if native {
        let (w, t, e) = (window.clone(), tab.clone(), entries.to_vec());
        items.push(
            menu::item(&crate::i18n::tr("Compress…"), move || {
                super::operations::compress(&w, &t, e.clone())
            })
            .icon("package-x-generic-symbolic"),
        );
    }
    if let Some(entry) = one
        && !entry.is_directory()
        && crate::services::ArchiveFormat::from_extension(&entry.display_name).is_some()
    {
        let (w, t, e) = (window.clone(), tab.clone(), entry.clone());
        items.push(
            menu::item(&crate::i18n::tr("Extract here"), move || {
                super::operations::extract(&w, &t, e.clone(), None)
            })
            .icon("archive-extract-symbolic"),
        );
        let (w, t, e) = (window.clone(), tab.clone(), entry.clone());
        items.push(
            menu::item(&crate::i18n::tr("Extract to…"), move || {
                extract_to(&w, &t, e.clone())
            })
            .icon("folder-open-symbolic"),
        );
    }
    if items.len() > 1 {
        Some(menu::sub(&crate::i18n::tr("Archive"), items).icon("package-x-generic-symbolic"))
    } else {
        items.pop()
    }
}

/// New Folder, New File, Paste, Select All and Refresh, as icons at the top.
fn folder_icons(window: &Rc<Window>, tab: &Rc<Tab>, here: &Location) -> menu::Entry {
    let t = tab.clone();
    let new_folder = menu::icon_button(
        "folder-new-symbolic",
        &crate::i18n::tr("New Folder…"),
        move || actions::new_folder(&t),
    )
    .key(&keys(bindings::NEW_FOLDER));
    let t = tab.clone();
    let new_file = menu::icon_button(
        "document-new-symbolic",
        &crate::i18n::tr("New File…"),
        move || new_file(&t),
    );
    let (w, t, h) = (window.clone(), tab.clone(), here.clone());
    let paste = menu::icon_button(
        "edit-paste-symbolic",
        &crate::i18n::tr("Paste"),
        move || actions::paste(&w, &t, h.clone()),
    )
    .key(&keys(bindings::PASTE));
    let t = tab.clone();
    let select_all = menu::icon_button(
        "edit-select-all-symbolic",
        &crate::i18n::tr("Select All"),
        move || {
            t.active_model().selection.select_all();
        },
    )
    .key(&keys(bindings::SELECT_ALL));
    let t = tab.clone();
    let refresh = menu::icon_button(
        "view-refresh-symbolic",
        &crate::i18n::tr("Refresh"),
        move || t.browser.reload_active(),
    )
    .key(&keys(bindings::REFRESH));
    menu::icons(vec![new_folder, new_file, paste, select_all, refresh])
}

/// The menu for the folder showing (a right-click on empty space).
pub(super) fn folder_menu(window: &Rc<Window>, tab: &Rc<Tab>) -> Vec<menu::Entry> {
    let Some(here) = tab.location() else {
        return Vec::new();
    };
    let mut items = Vec::new();
    if in_trash(&here) {
        items.push(
            menu::item(&crate::i18n::tr("Empty Trash…"), {
                let w = window.clone();
                move || actions::empty_trash(&w)
            })
            .icon("user-trash-full-symbolic")
            .danger(),
        );
        return items;
    }
    items.push(folder_icons(window, tab, &here));
    if here.native_path().is_some() {
        let (l, w) = (here.clone(), window.clone());
        items.push(
            menu::item(&crate::i18n::tr("Open in Terminal"), move || {
                desktop::launch_terminal(&l, &w.window)
            })
            .icon("utilities-terminal-symbolic"),
        );
    }
    {
        let (w, l) = (window.clone(), here.clone());
        items.push(
            menu::item(&crate::i18n::tr("New Window"), move || {
                w.open_window(l.clone())
            })
            .icon("window-new-symbolic"),
        );
    }
    {
        let text = here
            .native_path()
            .map_or_else(|| here.display_path(), |p| p.display().to_string());
        items.push(
            menu::item(&crate::i18n::tr("Copy path"), move || copy_text(&text))
                .icon("edit-copy-symbolic"),
        );
    }
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
    if let (Some(input), Some(path)) = (crate::ui::actions::folder_input(&here), here.native_path())
    {
        if let Some(custom) = custom_actions(
            window,
            vec![input],
            vec![path.to_path_buf()],
            path.to_path_buf(),
            true,
        ) {
            items.push(menu::separator());
            items.push(custom);
        }
        if let Some(provider) = providers(window, vec![path.to_path_buf()], true) {
            items.push(provider);
        }
    }
    if let Some(entry) = here.native_path().and_then(|_| actions::entry_for(&here)) {
        items.push(menu::separator());
        items.push(
            menu::item(&crate::i18n::tr("Properties"), move || {
                super::properties::show(&entry)
            })
            .icon("document-properties-symbolic"),
        );
    }
    items
}
