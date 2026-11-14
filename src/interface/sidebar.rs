// SPDX-License-Identifier: MIT

//! The sidebar's content: Overview, Recent and Favorites, then Folders, then
//! Pinned, then Storage (drives, cloud accounts, Network and Trash), rebuilt
//! when pins, favorites, drives, cloud accounts or the shown places change.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gtk::{gio, glib, prelude::*};
use nyx_ui::sidebar::{self, NavItem, Sidebar};

use crate::{
    adapters::{bookmarks, favorites},
    model::Location,
    ui::{preferences::PreferenceManager, window::device_release},
};

/// What a sidebar row stands for, for its menu.
#[derive(Clone)]
pub(super) enum Target {
    Place(&'static str, Location),
    Pin(Location),
    Volume(gio::Volume),
    Mount(gio::Mount),
    Drive(gio::Drive),
}

type OpenFn = Rc<dyn Fn(Location)>;
type MenuFn = Rc<dyn Fn(Target, &gtk::Widget, f64, f64)>;
type DropFn = Rc<dyn Fn(Location, Vec<Location>, gtk::gdk::DragAction)>;
/// Shows the Overview; `true` scrolls to its pinned folders. The file chooser
/// has none and lists its pins in the sidebar instead.
type OverviewFn = Option<Rc<dyn Fn(bool)>>;

pub(super) struct SidebarContent {
    pub(super) sidebar: Rc<Sidebar>,
    rows: RefCell<Vec<(Location, NavItem)>>,
    open: OpenFn,
    menu: MenuFn,
    overview: OverviewFn,
    overview_item: RefCell<Option<NavItem>>,
    overview_shown: Cell<bool>,
    dropped: RefCell<Option<DropFn>>,
    current: RefCell<Option<Location>>,
    pins: RefCell<bookmarks::Pins>,
    queued: Cell<bool>,
    cloud_watch: Cell<bool>,
    monitor: gio::VolumeMonitor,
    keep: RefCell<Vec<Box<dyn std::any::Any>>>,
    /// Whether this window's Trash row shows full; every window checks the one Trash.
    trash_shown_full: Cell<bool>,
    /// The section rows being added belong to, for their icon color.
    section: Cell<&'static str>,
    /// The accounts nyx-cloud last listed, drawn at once on each rebuild so
    /// Cloud Storage keeps its place above Devices while a new listing loads.
    cloud_listing: RefCell<Option<CloudListing>>,
    /// Told when a new listing of cloud accounts arrives (the Overview lists them too).
    cloud_changed: RefCell<Option<Rc<dyn Fn()>>>,
}

#[derive(Clone)]
enum CloudListing {
    Unavailable,
    Accounts(Vec<nyx_cloud::Remote>),
}

impl CloudListing {
    fn from_result<E>(result: Result<anyhow::Result<Vec<nyx_cloud::Remote>>, E>) -> Self {
        match result {
            Ok(Ok(remotes)) => {
                Self::Accounts(remotes.into_iter().filter(|r| r.storage_enabled).collect())
            }
            _ => Self::Unavailable,
        }
    }

    /// Whether the sidebar would show the same rows for both.
    fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Unavailable, Self::Unavailable) => true,
            (Self::Accounts(left), Self::Accounts(right)) => {
                left.len() == right.len()
                    && left.iter().zip(right).all(|(a, b)| {
                        a.id == b.id && a.name == b.name && a.mount_path == b.mount_path
                    })
            }
            _ => false,
        }
    }
}

fn standard(id: &str) -> Option<(&'static str, &'static str, glib::UserDirectory)> {
    Some(match id {
        "desktop" => (
            "user-desktop-symbolic",
            "Desktop",
            glib::UserDirectory::Desktop,
        ),
        "documents" => (
            "folder-documents-symbolic",
            "Documents",
            glib::UserDirectory::Documents,
        ),
        "downloads" => (
            "folder-download-symbolic",
            "Downloads",
            glib::UserDirectory::Downloads,
        ),
        "music" => ("folder-music-symbolic", "Music", glib::UserDirectory::Music),
        "pictures" => (
            "folder-pictures-symbolic",
            "Pictures",
            glib::UserDirectory::Pictures,
        ),
        "videos" => (
            "folder-videos-symbolic",
            "Videos",
            glib::UserDirectory::Videos,
        ),
        _ => return None,
    })
}

thread_local! {
    static TRASH_FULL: Cell<bool> = const { Cell::new(false) };
    static TRASH_COUNT: Cell<u32> = const { Cell::new(0) };
}

type Shown = fn(&PreferenceManager) -> bool;

const PLACES: [(&str, Shown); 10] = [
    ("home", PreferenceManager::sidebar_show_home),
    ("desktop", PreferenceManager::sidebar_show_desktop),
    ("documents", PreferenceManager::sidebar_show_documents),
    ("downloads", PreferenceManager::sidebar_show_downloads),
    ("music", PreferenceManager::sidebar_show_music),
    ("pictures", PreferenceManager::sidebar_show_pictures),
    ("videos", PreferenceManager::sidebar_show_videos),
    ("recent", PreferenceManager::sidebar_show_recent),
    ("network", PreferenceManager::sidebar_show_network),
    ("trash", PreferenceManager::sidebar_show_trash),
];

fn place(
    id: &'static str,
    home: &std::path::Path,
) -> Option<(&'static str, &'static str, Location)> {
    Some(match id {
        "home" => ("user-home-symbolic", "Home", Location::local(home)),
        "recent" => (
            "document-open-recent-symbolic",
            "Recent",
            Location::uri("recent:///"),
        ),
        "trash" => (
            if TRASH_FULL.with(Cell::get) {
                "user-trash-full-symbolic"
            } else {
                "user-trash-symbolic"
            },
            "Trash",
            Location::uri("trash:///"),
        ),
        "network" => (
            "network-workgroup-symbolic",
            "Network",
            Location::uri("network:///"),
        ),
        _ => {
            let (icon, name, dir) = standard(id)?;
            let path = glib::user_special_dir(dir)?;
            if id == "desktop" && path == home {
                return None;
            }
            (icon, name, Location::local(path))
        }
    })
}

/// A place's icon and name, whether or not it exists on this computer.
pub(super) fn place_identity(id: &str) -> Option<(&'static str, &'static str)> {
    Some(match id {
        "home" => ("user-home-symbolic", "Home"),
        "recent" => ("document-open-recent-symbolic", "Recent"),
        "trash" => ("user-trash-symbolic", "Trash"),
        "network" => ("network-workgroup-symbolic", "Network"),
        _ => standard(id).map(|(icon, name, _)| (icon, name))?,
    })
}

/// Home and the standard folders are Places, so they are never listed as pins.
fn is_place(location: &Location) -> bool {
    let home = super::tab::home();
    location.native_path().is_some_and(|path| {
        path == home
            || [
                "desktop",
                "documents",
                "downloads",
                "music",
                "pictures",
                "videos",
            ]
            .iter()
            .filter_map(|id| standard(id).and_then(|(_, _, dir)| glib::user_special_dir(dir)))
            .any(|dir| dir == path)
    })
}

fn location_of_file(file: &gio::File) -> Location {
    match file.path() {
        Some(path) => Location::local(path),
        None => Location::uri(file.uri().to_string()),
    }
}

fn icon_name(icon: gio::Icon, fallback: &str) -> String {
    icon.downcast::<gio::ThemedIcon>()
        .ok()
        .and_then(|t| t.names().first().map(|n| n.to_string()))
        .unwrap_or_else(|| fallback.to_string())
}

impl SidebarContent {
    pub(super) fn new(
        open: impl Fn(Location) + 'static,
        menu: impl Fn(Target, &gtk::Widget, f64, f64) + 'static,
        overview: Option<impl Fn(bool) + 'static>,
    ) -> Rc<Self> {
        let content = Rc::new(SidebarContent {
            sidebar: Sidebar::new(),
            rows: RefCell::default(),
            open: Rc::new(open),
            menu: Rc::new(menu),
            overview: overview.map(|f| Rc::new(f) as Rc<dyn Fn(bool)>),
            overview_item: RefCell::default(),
            overview_shown: Cell::new(false),
            dropped: RefCell::new(None),
            current: RefCell::default(),
            pins: RefCell::new(bookmarks::current()),
            queued: Cell::new(false),
            cloud_watch: Cell::new(false),
            monitor: gio::VolumeMonitor::get(),
            keep: RefCell::default(),
            trash_shown_full: Cell::new(false),
            section: Cell::new("places"),
            cloud_listing: RefCell::default(),
            cloud_changed: RefCell::default(),
        });
        let weak = Rc::downgrade(&content);
        let pins = bookmarks::watch_changes(move |result| {
            let Some(c) = weak.upgrade() else {
                return false;
            };
            if let Ok(pins) = result
                && *c.pins.borrow() != *pins
            {
                *c.pins.borrow_mut() = pins.clone();
                c.queue_rebuild();
            }
            true
        });
        content.keep.borrow_mut().push(Box::new(pins));
        let weak = Rc::downgrade(&content);
        let starred = favorites::watch_changes(move |_| {
            if let Some(c) = weak.upgrade() {
                c.queue_rebuild();
            }
        });
        content.keep.borrow_mut().push(Box::new(starred));
        let weak = Rc::downgrade(&content);
        let counted = favorites::watch_count(move || {
            if let Some(c) = weak.upgrade() {
                c.queue_rebuild();
            }
        });
        content.keep.borrow_mut().push(Box::new(counted));
        let weak = Rc::downgrade(&content);
        let releases = device_release::watch_sidebars(move || {
            if let Some(c) = weak.upgrade() {
                c.queue_rebuild();
            }
        });
        content.keep.borrow_mut().push(Box::new(releases));
        macro_rules! on_monitor {
            ($($signal:ident),*) => {$(
                let weak = Rc::downgrade(&content);
                content.monitor.$signal(move |_, _| {
                    if let Some(c) = weak.upgrade() {
                        c.queue_rebuild();
                    }
                });
            )*};
        }
        on_monitor!(
            connect_volume_added,
            connect_volume_removed,
            connect_volume_changed,
            connect_mount_added,
            connect_mount_removed,
            connect_mount_changed,
            connect_drive_connected,
            connect_drive_disconnected
        );
        let prefs = PreferenceManager::shared();
        for (_, read) in PLACES {
            let weak = Rc::downgrade(&content);
            prefs.bind_preference(&content.sidebar.root, read, move |_, _| {
                if let Some(c) = weak.upgrade() {
                    c.queue_rebuild();
                }
            });
        }
        prefs.bind_preference(
            &content.sidebar.root,
            PreferenceManager::colored_sidebar_icons,
            |root, colored| {
                if colored {
                    root.add_css_class("colored-icons");
                } else {
                    root.remove_css_class("colored-icons");
                }
            },
        );
        let weak = Rc::downgrade(&content);
        prefs.bind_preference(
            &content.sidebar.root,
            PreferenceManager::places_order,
            move |_, _| {
                if let Some(c) = weak.upgrade() {
                    c.queue_rebuild();
                }
            },
        );
        let weak = Rc::downgrade(&content);
        prefs.bind_preference(
            &content.sidebar.root,
            PreferenceManager::cloud_sidebar,
            move |_, _| {
                if let Some(c) = weak.upgrade() {
                    c.queue_rebuild();
                }
            },
        );
        content.pin_on_drop();
        content.watch_trash();
        let weak = Rc::downgrade(&content);
        content.sidebar.connect_compact(move |compact| {
            if let Some(c) = weak.upgrade() {
                c.show_rail_marks(compact);
            }
        });
        content.rebuild();
        content
    }

    /// The icon rail marks where each group starts with a short bar in its colour.
    fn show_rail_marks(&self, compact: bool) {
        let mut child = self.sidebar.list.first_child();
        while let Some(widget) = child {
            if widget.has_css_class("compact-show") {
                widget.set_visible(compact);
            }
            child = widget.next_sibling();
        }
    }

    fn queue_rebuild(self: &Rc<Self>) {
        if self.queued.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(c) = weak.upgrade() {
                c.queued.set(false);
                c.rebuild();
            }
        });
    }

    /// Files dropped on a row: its folder, the files, and Shift/Ctrl's choice.
    pub(super) fn connect_drop(
        self: &Rc<Self>,
        f: impl Fn(Location, Vec<Location>, gtk::gdk::DragAction) + 'static,
    ) {
        *self.dropped.borrow_mut() = Some(Rc::new(f));
        self.rebuild();
    }

    fn drop_target(
        &self,
        widget: &impl IsA<gtk::Widget>,
        accepts: impl Fn() -> Option<Location> + 'static,
    ) {
        let target = gtk::DropTarget::new(
            gtk::gdk::FileList::static_type(),
            gtk::gdk::DragAction::COPY | gtk::gdk::DragAction::MOVE,
        );
        target.connect_enter(|t, _, _| {
            if let Some(w) = t.widget() {
                w.add_css_class("drop-target");
            }
            super::actions::drop_action(t)
        });
        target.connect_motion(|t, _, _| super::actions::drop_action(t));
        target.connect_leave(|t| {
            if let Some(w) = t.widget() {
                w.remove_css_class("drop-target");
            }
        });
        let dropped = self.dropped.borrow().clone();
        target.connect_drop(move |t, value, _, _| {
            if let Some(w) = t.widget() {
                w.remove_css_class("drop-target");
            }
            let (Some(destination), Some(sources), Some(f)) =
                (accepts(), super::actions::dropped(value), dropped.as_ref())
            else {
                return false;
            };
            f(destination, sources, super::actions::forced_action(t));
            true
        });
        widget.add_controller(target);
    }

    /// Marks the row for `location` as the one showing.
    pub(super) fn mark(&self, location: Option<&Location>) {
        *self.current.borrow_mut() = location.cloned();
        let overview = self.overview_shown.get();
        for (row_location, item) in self.rows.borrow().iter() {
            item.set_active(!overview && Some(row_location) == location);
        }
        if let Some(item) = self.overview_item.borrow().as_ref() {
            item.set_active(overview);
        }
    }

    /// Marks Overview as the page showing, or the folder again.
    pub(super) fn mark_overview(&self, shown: bool) {
        self.overview_shown.set(shown);
        let current = self.current.borrow().clone();
        self.mark(current.as_ref());
    }

    fn add_row(
        &self,
        icon: &str,
        title: &str,
        tooltip: &str,
        location: Option<Location>,
        target: Target,
    ) -> NavItem {
        let item = sidebar::nav_item(icon, title, tooltip);
        item.button
            .add_css_class(&format!("section-icon-{}", self.section.get()));
        if let Some(location) = location
            .clone()
            .filter(|l| !l.is_virtual_list() && l.uri_value() != Some("network:///"))
        {
            let l = location.clone();
            self.drop_target(&item.button, move || Some(l.clone()));
        }
        if let Some(location) = location {
            let (open, target_location) = (self.open.clone(), location.clone());
            item.button
                .connect_clicked(move |_| open(target_location.clone()));
            self.rows.borrow_mut().push((location, item.clone()));
        }
        let menu = self.menu.clone();
        nyx_ui::menu::on_right_click(&item.button, move |w, x, y| menu(target.clone(), w, x, y));
        item
    }

    fn rebuild(self: &Rc<Self>) {
        let scroll = self.sidebar.scroller.vadjustment().value();
        self.sidebar.clear();
        self.rows.borrow_mut().clear();
        self.start();
        self.places();
        self.pinned();
        self.storage();
        self.sidebar.refresh();
        self.show_rail_marks(self.sidebar.is_compact());
        let current = self.current.borrow().clone();
        self.mark(current.as_ref());
        let adj = self.sidebar.scroller.vadjustment();
        glib::idle_add_local_once(move || adj.set_value(scroll));
    }

    /// Overview, Recent and Favorites, above the groups.
    fn start(&self) {
        self.section.set("places");
        let show = self.overview.clone();
        if let Some(show) = &show {
            let title = crate::i18n::tr("Overview");
            let overview = sidebar::nav_item("go-home-symbolic", &title, &title);
            overview.button.add_css_class("section-icon-places");
            let show = show.clone();
            overview.button.connect_clicked(move |_| show(false));
            self.sidebar.append(&overview.button);
            self.overview_item.replace(Some(overview));
        }
        let prefs = PreferenceManager::shared();
        if prefs.sidebar_show_recent() && recent_available() {
            let title = crate::i18n::tr("Recent");
            let location = Location::uri("recent:///");
            let item = self.add_row(
                "document-open-recent-symbolic",
                &title,
                &title,
                Some(location.clone()),
                Target::Place("recent", location),
            );
            let week = std::time::Duration::from_secs(7 * 24 * 3600).as_secs() as i64;
            let now = glib::real_time() / 1_000_000;
            let count = super::overview::recent_files(usize::MAX)
                .iter()
                .filter(|(_, used)| now - used < week)
                .count();
            item.set_count(Some(count as u64));
            self.sidebar.append(&item.button);
        }
        let title = crate::i18n::tr("Favorites");
        let location = Location::favorites();
        let item = self.add_row(
            "starred-symbolic",
            &title,
            &crate::i18n::tr("Files and folders you starred. Drop items here to add them."),
            Some(location.clone()),
            Target::Place("favorites", location),
        );
        item.set_count(favorites::reachable().map(|n| n as u64));
        favorite_drop(&item.button);
        self.sidebar.append(&item.button);
    }

    /// Pinned folders, as rows.
    fn pinned(&self) {
        let pins = pinned_folders();
        if pins.is_empty() {
            return;
        }
        self.begin_section(
            sidebar::group_label(&crate::i18n::tr("Pinned"), false),
            "pinned",
        );
        for (location, name) in pins {
            let icon = if location.native_path().is_some() {
                "folder-symbolic"
            } else {
                "folder-remote-symbolic"
            };
            let item = self.add_row(
                icon,
                &name,
                &location.display_path(),
                Some(location.clone()),
                Target::Pin(location.clone()),
            );
            reorderable(&item.button, &location);
            self.sidebar.append(&item.button);
        }
    }

    /// Folders: Home and the standard folders, in the order Settings gives.
    fn places(&self) {
        let prefs = PreferenceManager::shared();
        let home = super::tab::home();
        self.begin_section(
            sidebar::group_label(&crate::i18n::tr("Folders"), false),
            "places",
        );
        for id in prefs.places_order() {
            let Some((id, visible)) = PLACES.iter().find(|(place, _)| *place == id) else {
                continue;
            };
            let id = *id;
            if !visible(&prefs) || matches!(id, "recent" | "network" | "trash") {
                continue;
            }
            let Some((icon, name, location)) = place(id, &home) else {
                continue;
            };
            let title = crate::i18n::tr(name);
            let item = self.add_row(
                icon,
                &title,
                &title,
                Some(location.clone()),
                Target::Place(id, location.clone()),
            );
            if id == "downloads"
                && let Some(dir) = location.native_path().map(std::path::Path::to_path_buf)
            {
                let badge = item.clone();
                glib::MainContext::default().spawn_local(async move {
                    let new = gio::spawn_blocking(move || super::overview::new_downloads(&dir))
                        .await
                        .unwrap_or(0);
                    badge.set_badge(Some(new as u64));
                });
            }
            self.sidebar.append(&item.button);
        }
    }

    /// Storage: the system disk, drives, cloud accounts, Network and Trash.
    fn storage(self: &Rc<Self>) {
        let prefs = PreferenceManager::shared();
        self.begin_section(
            sidebar::group_label(&crate::i18n::tr("Storage"), false),
            "devices",
        );
        let root_listed = self
            .monitor
            .mounts()
            .iter()
            .any(|m| m.root().path().as_deref() == Some(std::path::Path::new("/")));
        if !root_listed {
            let title = crate::i18n::tr("System");
            let location = Location::local("/");
            let item = self.add_row(
                "drive-harddisk-symbolic",
                &title,
                &crate::i18n::tr("The system disk"),
                Some(location.clone()),
                Target::Place("system", location),
            );
            self.sidebar.append(&item.button);
        }
        self.devices();
        self.cloud();
        let home = super::tab::home();
        for id in ["network", "trash"] {
            let Some((_, visible)) = PLACES.iter().find(|(place, _)| *place == id) else {
                continue;
            };
            if !visible(&prefs) {
                continue;
            }
            let Some((icon, name, location)) = place(id, &home) else {
                continue;
            };
            let title = crate::i18n::tr(name);
            let item = self.add_row(
                icon,
                &title,
                &title,
                Some(location.clone()),
                Target::Place(id, location),
            );
            if id == "trash" {
                item.set_count(Some(u64::from(TRASH_COUNT.with(Cell::get))));
            }
            self.sidebar.append(&item.button);
        }
    }

    /// Puts focus on the open place's row, or the first row.
    pub(super) fn focus(&self) {
        let current = self.current.borrow().clone();
        let rows = self.rows.borrow();
        let row = rows
            .iter()
            .find(|(l, _)| Some(l) == current.as_ref())
            .or_else(|| rows.first());
        if let Some((_, item)) = row {
            item.button.grab_focus();
        }
    }

    /// The pins in sidebar order, for the g1–g9 keys.
    pub(super) fn visible_pins(&self) -> Vec<Location> {
        self.pins
            .borrow()
            .iter()
            .filter(|(l, _)| !is_place(l))
            .map(|(l, _)| l.clone())
            .collect()
    }

    /// Folders dropped on the sidebar's empty space are pinned.
    fn pin_on_drop(&self) {
        let target = gtk::DropTarget::new(
            gtk::gdk::FileList::static_type(),
            gtk::gdk::DragAction::LINK | gtk::gdk::DragAction::COPY,
        );
        target.connect_drop(|_, value, _, _| {
            let Some(sources) = super::actions::dropped(value) else {
                return false;
            };
            let folders: Vec<Location> = sources
                .into_iter()
                .filter(|l| l.native_path().is_none_or(std::path::Path::is_dir))
                .collect();
            for folder in &folders {
                pin(folder.clone(), folder.display_name());
            }
            !folders.is_empty()
        });
        self.sidebar.list.add_controller(target);
    }

    fn devices(self: &Rc<Self>) {
        let prefs = PreferenceManager::shared();
        let is_cloud = |mount: Option<gio::Mount>| {
            mount
                .and_then(|m| m.root().path())
                .is_some_and(|p| crate::ui::window::cloud::is_cloud_mount(&p))
        };
        let volumes: Vec<gio::Volume> = self
            .monitor
            .volumes()
            .into_iter()
            .filter(|v| !is_cloud(v.get_mount()))
            .collect();
        let mounts: Vec<gio::Mount> = self
            .monitor
            .mounts()
            .into_iter()
            .filter(|m| !m.is_shadowed() && !is_cloud(Some(m.clone())))
            .filter(|m| !m.volume().is_some_and(|v| volumes.contains(&v)))
            .collect();
        self.password_drives(&volumes);
        for volume in volumes {
            let mount = volume.get_mount();
            let system_name = volume.name().to_string();
            let name = crate::ui::window::drive_dialogs::PropertiesTarget::Volume(volume.clone())
                .label_id()
                .and_then(|id| prefs.device_label(&id))
                .unwrap_or(system_name);
            let icon = icon_name(volume.symbolic_icon(), "drive-harddisk-symbolic");
            let location = mount.as_ref().map(|m| location_of_file(&m.root()));
            let pending = device_release::is_pending(&device_release::ids_for_volume(&volume));
            let item = self.add_row(
                &icon,
                &name,
                &name,
                location.clone(),
                Target::Volume(volume.clone()),
            );
            if location.is_none() {
                let (weak, v) = (Rc::downgrade(self), volume.clone());
                item.button.connect_clicked(move |b| {
                    if let Some(c) = weak.upgrade() {
                        c.mount_volume(&v, b.upcast_ref());
                    }
                });
            }
            let releasable = mount
                .as_ref()
                .is_some_and(|m| m.can_unmount() || m.can_eject())
                || volume.can_eject();
            self.append_device(
                &item,
                pending,
                releasable.then(|| Target::Volume(volume.clone())),
            );
        }
        for mount in mounts {
            let name = mount.name().to_string();
            let icon = icon_name(mount.symbolic_icon(), "drive-harddisk-symbolic");
            let location = location_of_file(&mount.root());
            let pending = device_release::is_pending(&device_release::ids_for_mount(&mount));
            let item = self.add_row(
                &icon,
                &name,
                &location.display_path(),
                Some(location),
                Target::Mount(mount.clone()),
            );
            let releasable = mount.can_unmount() || mount.can_eject();
            self.append_device(
                &item,
                pending,
                releasable.then(|| Target::Mount(mount.clone())),
            );
        }
    }

    /// A device row with its eject button, or a spinner while it is being removed.
    fn append_device(&self, item: &NavItem, pending: bool, eject: Option<Target>) {
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        line.add_css_class("device-row");
        item.button.set_hexpand(true);
        line.append(&item.button);
        if pending {
            let spinner = gtk::Spinner::new();
            spinner.start();
            spinner.add_css_class("compact-hide");
            spinner.set_margin_end(10);
            line.append(&spinner);
            item.button.set_sensitive(false);
        } else if let Some(target) = eject {
            let button =
                nyx_ui::widgets::icon_button("media-eject-symbolic", &crate::i18n::tr("Eject"));
            button.add_css_class("compact-hide");
            button.add_css_class("device-eject");
            let menu = self.menu.clone();
            button.connect_clicked(move |b| {
                let _ = &menu;
                super::devices::release(&target, b.upcast_ref());
            });
            line.append(&button);
        }
        self.sidebar.append(&line);
    }

    pub(super) fn mount_volume(self: &Rc<Self>, volume: &gio::Volume, anchor: &gtk::Widget) {
        let _ = anchor;
        let operation = super::mount::operation();
        let (open, v) = (self.open.clone(), volume.clone());
        volume.mount(
            gio::MountMountFlags::NONE,
            Some(&operation),
            gio::Cancellable::NONE,
            move |result| match result {
                Ok(()) => {
                    if let Some(mount) = v.get_mount() {
                        open(location_of_file(&mount.root()));
                    }
                }
                Err(error) if error.matches(gio::IOErrorEnum::FailedHandled) => {}
                Err(error) => {
                    if let Some(window) = nyx_ui::widgets::main_window()
                        && let Some(overlay) = window.child().and_downcast::<gtk::Overlay>()
                    {
                        nyx_ui::toast::show(&overlay, error.message());
                    }
                }
            },
        );
    }
}

/// Pins `location` under `name`, after the existing pins.
pub(super) fn pin(location: Location, name: String) {
    glib::MainContext::default().spawn_local(async move {
        let edit = move |contents: &[u8]| {
            let mut places = bookmarks::parse_pinned_places(contents);
            if places.iter().any(|(l, _)| *l == location) {
                return None;
            }
            places.push((location.clone(), name.clone()));
            Some(bookmarks::serialize_pinned_places(&places).into_bytes())
        };
        if let Err(error) = bookmarks::update(edit).await {
            tracing::warn!(%error, "unable to pin a folder");
        }
    });
}

pub(super) fn unpin(location: Location) {
    glib::MainContext::default().spawn_local(async move {
        let edit = move |contents: &[u8]| {
            let mut places = bookmarks::parse_pinned_places(contents);
            let before = places.len();
            places.retain(|(l, _)| *l != location);
            (places.len() != before)
                .then(|| bookmarks::serialize_pinned_places(&places).into_bytes())
        };
        if let Err(error) = bookmarks::update(edit).await {
            tracing::warn!(%error, "unable to unpin a folder");
        }
    });
}

/// Files dropped here become favorites; they aren't copied or moved.
pub(super) fn favorite_drop(widget: &impl IsA<gtk::Widget>) {
    let target = gtk::DropTarget::new(
        gtk::gdk::FileList::static_type(),
        gtk::gdk::DragAction::COPY,
    );
    // Copy, never move: the source then leaves its files alone.
    target.connect_enter(|t, _, _| {
        if let Some(w) = t.widget() {
            w.add_css_class("drop-target");
        }
        gtk::gdk::DragAction::COPY
    });
    target.connect_motion(|_, _, _| gtk::gdk::DragAction::COPY);
    target.connect_leave(|t| {
        if let Some(w) = t.widget() {
            w.remove_css_class("drop-target");
        }
    });
    target.connect_drop(|t, value, _, _| {
        if let Some(w) = t.widget() {
            w.remove_css_class("drop-target");
        }
        let Some(locations) = super::actions::dropped(value) else {
            return false;
        };
        favorites::add(locations);
        true
    });
    widget.add_controller(target);
}

/// The pinned folders, in their order, leaving out Home and the standard folders.
pub(super) fn pinned_folders() -> Vec<(Location, String)> {
    bookmarks::current()
        .iter()
        .filter(|(l, _)| !is_place(l))
        .cloned()
        .collect()
}

pub(super) fn is_place_location(location: &Location) -> bool {
    is_place(location)
}

pub(super) fn is_pinned(location: &Location) -> bool {
    is_place(location) || bookmarks::current().iter().any(|(l, _)| l == location)
}

impl SidebarContent {
    fn begin_section(&self, heading: gtk::Box, section: &'static str) {
        self.section.set(section);
        let mark = gtk::Separator::new(gtk::Orientation::Horizontal);
        mark.add_css_class("nav-divider");
        mark.add_css_class("compact-show");
        mark.add_css_class(&format!("section-{section}"));
        mark.set_visible(false);
        self.sidebar.append(&mark);
        self.sidebar.append(&sectioned(heading, section));
    }

    pub(super) fn on_cloud_change(&self, changed: impl Fn() + 'static) {
        self.cloud_changed.replace(Some(Rc::new(changed)));
    }

    /// The cloud accounts the sidebar last listed.
    pub(super) fn cloud_accounts(&self) -> Vec<nyx_cloud::Remote> {
        match self.cloud_listing.borrow().as_ref() {
            Some(CloudListing::Accounts(remotes)) if PreferenceManager::shared().cloud_sidebar() => {
                remotes.clone()
            }
            _ => Vec::new(),
        }
    }

    /// Connects the account `id` if needed, then opens it.
    pub(super) fn open_cloud_account(self: &Rc<Self>, id: &str) {
        self.open_cloud(id.to_string(), None);
    }
}

/// A group heading in its section's colour, as Settings colours its groups.
fn sectioned(heading: gtk::Box, section: &str) -> gtk::Box {
    if let Some(label) = heading.first_child() {
        label.add_css_class(&format!("section-{section}"));
    }
    heading
}

impl SidebarContent {
    /// Cloud accounts from nyx-cloud, when the setting shows them.
    fn cloud(self: &Rc<Self>) {
        if !PreferenceManager::shared().cloud_sidebar() {
            return;
        }
        let holder = gtk::Box::new(gtk::Orientation::Vertical, 1);
        self.sidebar.append(&holder);
        let cached = self.cloud_listing.borrow().clone();
        if let Some(listing) = cached {
            self.show_cloud(&holder, &listing);
        }
        let weak = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let remotes = gio::spawn_blocking(|| nyx_cloud::Client::connect()?.list()).await;
            let listing = CloudListing::from_result(remotes);
            let Some(content) = weak.upgrade() else {
                return;
            };
            let unchanged = content
                .cloud_listing
                .borrow()
                .as_ref()
                .is_some_and(|cached| cached.same(&listing));
            if !unchanged {
                content.cloud_listing.replace(Some(listing));
                content.queue_rebuild();
                let changed = content.cloud_changed.borrow().clone();
                if let Some(changed) = changed {
                    changed();
                }
            }
        });
        if !self.cloud_watch.get() {
            self.cloud_watch.set(true);
            let weak = Rc::downgrade(self);
            glib::MainContext::default().spawn_local(async move {
                let Ok(bus) = gio::bus_get_future(gio::BusType::Session).await else {
                    return;
                };
                let w = weak.clone();
                let subscription = bus.subscribe_to_signal(
                    Some(nyx_cloud::BUS),
                    Some(nyx_cloud::BUS),
                    Some("Changed"),
                    Some(nyx_cloud::PATH),
                    None,
                    gio::DBusSignalFlags::NONE,
                    move |_| {
                        if let Some(c) = w.upgrade() {
                            c.queue_rebuild();
                        }
                    },
                );
                if let Some(c) = weak.upgrade() {
                    c.keep.borrow_mut().push(Box::new(subscription));
                }
            });
        }
    }

    fn show_cloud(self: &Rc<Self>, holder: &gtk::Box, listing: &CloudListing) {
        let CloudListing::Accounts(remotes) = listing else {
            return;
        };
        for remote in remotes {
            let item = sidebar::nav_item("weather-overcast-symbolic", &remote.name, &remote.name);
            item.button.add_css_class("section-icon-devices");
            let (w, id) = (Rc::downgrade(self), remote.id.clone());
            item.button.connect_clicked(move |b| {
                if let Some(c) = w.upgrade() {
                    c.open_cloud(id.clone(), Some(b.clone()));
                }
            });
            holder.append(&item.button);
            // Marked like other places while its folder is the one open.
            if !remote.mount_path.is_empty() {
                self.rows
                    .borrow_mut()
                    .push((Location::local(&remote.mount_path), item.clone()));
            }
        }
    }

    /// Mounts an account if needed (up to 35 s), then opens it.
    fn open_cloud(self: &Rc<Self>, id: String, button: Option<gtk::Button>) {
        if let Some(button) = &button {
            button.set_sensitive(false);
        }
        let open = self.open.clone();
        glib::MainContext::default().spawn_local(async move {
            let result = gio::spawn_blocking(move || -> anyhow::Result<String> {
                let client = nyx_cloud::Client::connect()?;
                let find = |client: &nyx_cloud::Client| -> anyhow::Result<nyx_cloud::Remote> {
                    client.list()?.into_iter().find(|r| r.id == id).ok_or_else(|| anyhow::anyhow!("Account unavailable"))
                };
                if find(&client)?.mount_status == "mounted" {
                    return Ok(find(&client)?.mount_path);
                }
                client.mount(&id)?;
                for _ in 0..175 {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    let r = find(&client)?;
                    if r.mount_status == "mounted" {
                        return Ok(r.mount_path);
                    }
                    if r.connection == "error" || r.connection == "authentication_required" {
                        anyhow::bail!("Connection failed");
                    }
                }
                anyhow::bail!("Connection timed out")
            })
            .await;
            if let Some(button) = &button {
                button.set_sensitive(true);
            }
            match result {
                Ok(Ok(path)) => open(Location::local(path)),
                _ => {
                    if let Some(window) = nyx_ui::widgets::main_window()
                        && let Some(overlay) = window.child().and_downcast::<gtk::Overlay>()
                    {
                        nyx_ui::toast::show(
                            &overlay,
                            &crate::i18n::tr("Cloud connection failed. Retry the account or open Online Accounts in Settings."),
                        );
                    }
                }
            }
        });
    }
}

const PIN_PREFIX: &str = "nyx-files-pin:";

/// A pin can be dragged onto another to move it there.
pub(super) fn reorderable(button: &gtk::Button, location: &Location) {
    let source = gtk::DragSource::new();
    source.set_actions(gtk::gdk::DragAction::MOVE);
    let key = format!("{PIN_PREFIX}{}", location.display_path());
    source.connect_prepare(move |_, _, _| {
        Some(gtk::gdk::ContentProvider::for_value(&key.to_value()))
    });
    button.add_controller(source);
    let target = gtk::DropTarget::new(String::static_type(), gtk::gdk::DragAction::MOVE);
    let here = location.clone();
    target.connect_drop(move |_, value, _, _| {
        let Some(from) = value
            .get::<String>()
            .ok()
            .and_then(|v| v.strip_prefix(PIN_PREFIX).map(str::to_string))
        else {
            return false;
        };
        let here = here.clone();
        glib::MainContext::default().spawn_local(async move {
            let edit = move |contents: &[u8]| {
                let mut places = bookmarks::parse_pinned_places(contents);
                let source = places.iter().position(|(l, _)| l.display_path() == from)?;
                let entry = places.remove(source);
                let target = places
                    .iter()
                    .position(|(l, _)| *l == here)
                    .unwrap_or(places.len());
                places.insert(target, entry);
                Some(bookmarks::serialize_pinned_places(&places).into_bytes())
            };
            if let Err(error) = bookmarks::update(edit).await {
                tracing::warn!(%error, "unable to reorder pins");
            }
        });
        true
    });
    button.add_controller(target);
}

impl SidebarContent {
    /// Keeps Trash's icon full or empty as items come and go.
    fn watch_trash(self: &Rc<Self>) {
        let trash = gio::File::for_uri("trash:///");
        let check = {
            let (weak, trash) = (Rc::downgrade(self), trash.clone());
            move || {
                let (weak, trash) = (weak.clone(), trash.clone());
                glib::MainContext::default().spawn_local(async move {
                    let count = trash
                        .query_info_future(
                            "trash::item-count",
                            gio::FileQueryInfoFlags::NONE,
                            glib::Priority::LOW,
                        )
                        .await
                        .map_or(0, |info| info.attribute_uint32("trash::item-count"));
                    let full = count > 0;
                    TRASH_FULL.with(|t| t.set(full));
                    let recount = TRASH_COUNT.with(|t| t.replace(count)) != count;
                    if let Some(c) = weak.upgrade()
                        && (c.trash_shown_full.replace(full) != full || recount)
                    {
                        c.queue_rebuild();
                    }
                });
            }
        };
        check();
        if let Ok(monitor) =
            trash.monitor_directory(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
        {
            monitor.connect_changed(move |_, _, _, _| check());
            self.keep.borrow_mut().push(Box::new(monitor));
        }
    }

    /// Locked encrypted drives that show no volume yet: a click unlocks them.
    fn password_drives(self: &Rc<Self>, volumes: &[gio::Volume]) -> bool {
        let drives =
            crate::ui::window::orphaned_password_drives(volumes, &self.monitor.connected_drives());
        for drive in &drives {
            let name = drive.name().to_string();
            let item = self.add_row(
                "drive-harddisk-symbolic",
                &name,
                &name,
                None,
                Target::Drive(drive.clone()),
            );
            let (weak, d) = (Rc::downgrade(self), drive.clone());
            item.button.connect_clicked(move |_| {
                if let Some(c) = weak.upgrade() {
                    c.start_drive(&d);
                }
            });
            let pending = device_release::is_pending(&device_release::ids_for_drive(drive));
            self.append_device(
                &item,
                pending,
                drive.can_eject().then(|| Target::Drive(drive.clone())),
            );
        }
        !drives.is_empty()
    }

    fn start_drive(self: &Rc<Self>, drive: &gio::Drive) {
        let open = self.open.clone();
        start_drive(drive, move |l| open(l));
    }
}

/// Starts (unlocks) a drive, then mounts and opens the volume it brings up.
pub(super) fn start_drive(drive: &gio::Drive, open: impl Fn(Location) + 'static) {
    {
        let drive = drive.clone();
        glib::MainContext::default().spawn_local(async move {
            let op = super::mount::operation();
            if let Err(error) = drive
                .start_future(gio::DriveStartFlags::NONE, Some(&op))
                .await
            {
                if !error.matches(gio::IOErrorEnum::FailedHandled) {
                    super::devices::toast(error.message());
                }
                return;
            }
            for _ in 0..50 {
                if let Some(volume) = drive.volumes().into_iter().next() {
                    if volume.get_mount().is_none() {
                        let _ = volume
                            .mount_future(
                                gio::MountMountFlags::NONE,
                                Some(&super::mount::operation()),
                            )
                            .await;
                    }
                    if let Some(mount) = volume.get_mount() {
                        open(location_of_file(&mount.root()));
                    }
                    return;
                }
                glib::timeout_future(std::time::Duration::from_millis(100)).await;
            }
            super::devices::toast(&crate::i18n::tr(
                "The device started, but no mountable volume appeared.",
            ));
        });
    }
}

/// Recent needs GTK's recent-files tracking and a `recent:` backend.
pub(super) fn recent_available() -> bool {
    gtk::Settings::default().is_some_and(|s| s.is_gtk_recent_files_enabled())
        && gio::Vfs::default()
            .supported_uri_schemes()
            .iter()
            .any(|scheme| scheme.eq_ignore_ascii_case("recent"))
}
