// SPDX-License-Identifier: MIT

//! The sidebar's Places group.

use std::rc::Rc;

use gtk::{glib, prelude::*};
use nyx_ui::sidebar::{self, NavItem, Sidebar};

use crate::{model::Location, ui::preferences::PreferenceManager};

pub(super) struct Place {
    pub(super) item: NavItem,
    pub(super) location: Location,
}

fn standard(id: &str) -> Option<(&'static str, &'static str, glib::UserDirectory)> {
    Some(match id {
        "desktop" => ("user-desktop-symbolic", "Desktop", glib::UserDirectory::Desktop),
        "documents" => ("folder-documents-symbolic", "Documents", glib::UserDirectory::Documents),
        "downloads" => ("folder-download-symbolic", "Downloads", glib::UserDirectory::Downloads),
        "music" => ("folder-music-symbolic", "Music", glib::UserDirectory::Music),
        "pictures" => ("folder-pictures-symbolic", "Pictures", glib::UserDirectory::Pictures),
        "videos" => ("folder-videos-symbolic", "Videos", glib::UserDirectory::Videos),
        _ => return None,
    })
}

fn visible(prefs: &PreferenceManager, id: &str) -> bool {
    match id {
        "home" => prefs.sidebar_show_home(),
        "recent" => prefs.sidebar_show_recent(),
        "trash" => prefs.sidebar_show_trash(),
        "network" => prefs.sidebar_show_network(),
        "desktop" => prefs.sidebar_show_desktop(),
        "documents" => prefs.sidebar_show_documents(),
        "downloads" => prefs.sidebar_show_downloads(),
        "music" => prefs.sidebar_show_music(),
        "pictures" => prefs.sidebar_show_pictures(),
        "videos" => prefs.sidebar_show_videos(),
        _ => true,
    }
}

const ORDER: &[&str] = &[
    "recent", "home", "desktop", "documents", "downloads", "music", "pictures", "videos", "network", "trash",
];

/// Fills `sidebar` with Places; `open` runs with a place's location.
pub(super) fn fill(sidebar: &Rc<Sidebar>, open: impl Fn(Location) + Clone + 'static) -> Vec<Place> {
    let prefs = PreferenceManager::shared();
    let home = super::tab::home();
    sidebar.append(&sidebar::group_label(&crate::i18n::tr("Places"), true));
    let mut places = Vec::new();
    for &id in ORDER {
        if !visible(&prefs, id) {
            continue;
        }
        let (icon, name, location) = match id {
            "home" => ("user-home-symbolic", "Home", Location::local(&home)),
            "recent" => ("document-open-recent-symbolic", "Recent", Location::uri("recent:///")),
            "trash" => ("user-trash-symbolic", "Trash", Location::uri("trash:///")),
            "network" => ("network-workgroup-symbolic", "Network", Location::uri("network:///")),
            _ => {
                let Some((icon, name, dir)) = standard(id) else { continue };
                let Some(path) = glib::user_special_dir(dir) else { continue };
                if id == "desktop" && path == home {
                    continue;
                }
                (icon, name, Location::local(path))
            }
        };
        let title = crate::i18n::tr(name);
        let item = sidebar::nav_item(icon, &title, &title);
        let (open, target) = (open.clone(), location.clone());
        item.button.connect_clicked(move |_| open(target.clone()));
        sidebar.append(&item.button);
        places.push(Place { item, location });
    }
    sidebar.refresh();
    places
}

/// Marks the place for `location` as the one showing.
pub(super) fn mark(places: &[Place], location: Option<&Location>) {
    for place in places {
        place.item.set_active(Some(&place.location) == location);
    }
}
