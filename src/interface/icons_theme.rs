// SPDX-License-Identifier: MIT

//! File and folder icons from the system icon theme (Umbra in the suite):
//! special and well-known folders by name, files by content type.

use gtk::{gio, glib, prelude::*};

use crate::model::FileEntry;

/// Folder icon names that describe a state, not a folder's purpose.
const STATE_FOLDERS: &[&str] = &[
    "open",
    "drag-accept",
    "remote",
    "saved-search",
    "visiting",
    "recent",
];

fn theme() -> Option<gtk::IconTheme> {
    gtk::gdk::Display::default().map(|d| gtk::IconTheme::for_display(&d))
}

fn special_folder(path: &std::path::Path) -> Option<&'static str> {
    if path == glib::home_dir() {
        return Some("user-home");
    }
    [
        (glib::UserDirectory::Desktop, "user-desktop"),
        (glib::UserDirectory::Documents, "folder-documents"),
        (glib::UserDirectory::Downloads, "folder-download"),
        (glib::UserDirectory::Music, "folder-music"),
        (glib::UserDirectory::Pictures, "folder-pictures"),
        (glib::UserDirectory::Videos, "folder-videos"),
        (glib::UserDirectory::Templates, "folder-templates"),
        (glib::UserDirectory::PublicShare, "folder-publicshare"),
    ]
    .into_iter()
    .find(|(dir, _)| glib::user_special_dir(*dir).as_deref() == Some(path))
    .map(|(_, name)| name)
}

/// The theme icon name for `entry`.
pub(super) fn name_for(entry: &FileEntry) -> String {
    let theme = theme();
    let has = |name: &str| theme.as_ref().is_some_and(|t| t.has_icon(name));
    if entry.is_broken_symbolic_link() {
        return "emblem-unreadable".into();
    }
    if entry.is_directory() {
        if let Some(path) = entry.location.native_path()
            && let Some(name) = special_folder(path)
            && has(name)
        {
            return name.into();
        }
        let named = entry
            .display_name
            .trim_start_matches('.')
            .to_lowercase()
            .replace([' ', '_'], "-");
        if !named.is_empty() && !STATE_FOLDERS.contains(&named.as_str()) {
            let candidate = format!("folder-{named}");
            if has(&candidate) {
                return candidate;
            }
        }
        return "folder".into();
    }
    let (content_type, _) = gio::content_type_guess(Some(&entry.native_name), None::<&[u8]>);
    if let Some(themed) =
        gio::content_type_get_icon(&content_type).downcast_ref::<gio::ThemedIcon>()
        && let Some(name) = themed.names().into_iter().find(|n| has(n))
    {
        return name.to_string();
    }
    "text-x-generic".into()
}
