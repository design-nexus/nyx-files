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
pub(crate) fn name_for(entry: &FileEntry) -> String {
    let theme = theme();
    if entry.is_broken_symbolic_link() {
        return "emblem-unreadable".into();
    }
    if entry.is_directory() {
        return folder_icon_for(
            theme.as_ref(),
            entry.location.native_path(),
            &entry.display_name,
        );
    }
    file_name_icon(theme.as_ref(), &entry.native_name)
}

/// The symbolic icon name for a folder location (custom icon, XDG/special
/// folder, or `folder-<name>-symbolic` when available in the icon theme).
pub(crate) fn symbolic_folder_icon(location: &crate::model::Location) -> String {
    let theme = theme();
    let has = |name: &str| theme.as_ref().is_some_and(|t| t.has_icon(name));
    if let Some(path) = location.native_path() {
        if let Some(custom) = super::preferences::PreferenceManager::shared().custom_icon(path)
            && !custom.starts_with("emoji:")
        {
            let symbolic = if custom.ends_with("-symbolic") {
                custom.clone()
            } else {
                format!("{custom}-symbolic")
            };
            if has(&symbolic) {
                return symbolic;
            }
            if has(&custom) {
                return custom;
            }
        }
        let base = folder_icon_for(theme.as_ref(), Some(path), &location.display_name());
        let symbolic = if base.ends_with("-symbolic") {
            base.clone()
        } else {
            format!("{base}-symbolic")
        };
        if has(&symbolic) {
            return symbolic;
        }
        if let Some(glyph) = symbolic_glyph_fallback(&base, &has) {
            return glyph.into();
        }
    }
    if location.native_path().is_some() {
        "folder-symbolic".into()
    } else {
        "folder-remote-symbolic".into()
    }
}

fn symbolic_glyph_fallback(base: &str, has: &impl Fn(&str) -> bool) -> Option<&'static str> {
    let candidates: &[&str] = match base {
        "folder-screenshots" | "folder-screenshot" => &[
            "camera-photo-symbolic",
            "applets-screenshooter-symbolic",
            "folder-pictures-symbolic",
        ],
        "folder-code" | "folder-development" | "folder-git" | "folder-repositories" => {
            &["folder-projects-symbolic", "applications-development-symbolic"]
        }
        "folder-porn" | "folder-private" => {
            &["folder-private-symbolic", "dialog-password-symbolic"]
        }
        "folder-books" | "folder-ebooks" => {
            &["accessories-dictionary-symbolic", "folder-documents-symbolic"]
        }
        "folder-backup" | "folder-backups" => {
            &["document-save-symbolic", "drive-harddisk-symbolic"]
        }
        "folder-applications" | "folder-apps" => {
            &["view-app-grid-symbolic", "applications-other-symbolic"]
        }
        "folder-bookmarks" => &["bookmark-symbolic"],
        "folder-gdrive" | "folder-mega" | "folder-proton-drive" => {
            &["folder-cloud-symbolic", "weather-overcast-symbolic"]
        }
        "folder-audio" => &["folder-music-symbolic"],
        "folder-images" | "folder-photos" => &["folder-pictures-symbolic"],
        "folder-video" | "folder-movies" => &["folder-videos-symbolic"],
        "folder-wallpaper" => &["folder-wallpapers-symbolic"],
        "folder-public" => &["folder-publicshare-symbolic"],
        "folder-downloads" => &["folder-download-symbolic"],
        _ => return None,
    };
    candidates.iter().copied().find(|name| has(name))
}

fn folder_icon_for(
    theme: Option<&gtk::IconTheme>,
    path: Option<&std::path::Path>,
    display_name: &str,
) -> String {
    let has = |name: &str| theme.is_some_and(|t| t.has_icon(name));
    if let Some(path) = path
        && let Some(name) = special_folder(path)
        && has(name)
    {
        return name.into();
    }
    let named = display_name
        .trim_start_matches('.')
        .to_lowercase()
        .replace([' ', '_'], "-");
    if !named.is_empty() && !STATE_FOLDERS.contains(&named.as_str()) {
        let candidate = format!("folder-{named}");
        if has(&candidate) {
            return candidate;
        }
    }
    "folder".into()
}

/// The theme icon name for a file called `name`, by its guessed content type.
pub(crate) fn name_for_file_name(name: &str) -> String {
    file_name_icon(theme().as_ref(), name)
}

fn file_name_icon(theme: Option<&gtk::IconTheme>, name: impl AsRef<std::path::Path>) -> String {
    let (content_type, _) = gio::content_type_guess(Some(name.as_ref()), None::<&[u8]>);
    if let Some(themed) =
        gio::content_type_get_icon(&content_type).downcast_ref::<gio::ThemedIcon>()
        && let Some(name) = themed
            .names()
            .into_iter()
            .find(|n| theme.is_some_and(|t| t.has_icon(n)))
    {
        return name.to_string();
    }
    "text-x-generic".into()
}
