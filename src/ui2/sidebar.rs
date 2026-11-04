// SPDX-License-Identifier: MIT

//! The sidebar's content: Places, Pinned folders and Devices, rebuilt when
//! pins, drives or the shown places change.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gtk::{gio, glib, prelude::*};
use nyx_ui::sidebar::{self, NavItem, Sidebar};

use crate::{
    adapters::bookmarks,
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
}

type OpenFn = Rc<dyn Fn(Location)>;
type MenuFn = Rc<dyn Fn(Target, &gtk::Widget, f64, f64)>;

pub(super) struct SidebarContent {
    pub(super) sidebar: Rc<Sidebar>,
    rows: RefCell<Vec<(Location, NavItem)>>,
    open: OpenFn,
    menu: MenuFn,
    current: RefCell<Option<Location>>,
    pins: RefCell<bookmarks::Pins>,
    queued: Cell<bool>,
    cloud_watch: Cell<bool>,
    monitor: gio::VolumeMonitor,
    keep: RefCell<Vec<Box<dyn std::any::Any>>>,
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

type Shown = fn(&PreferenceManager) -> bool;

const PLACES: [(&str, Shown); 10] = [
    ("recent", PreferenceManager::sidebar_show_recent),
    ("home", PreferenceManager::sidebar_show_home),
    ("desktop", PreferenceManager::sidebar_show_desktop),
    ("documents", PreferenceManager::sidebar_show_documents),
    ("downloads", PreferenceManager::sidebar_show_downloads),
    ("music", PreferenceManager::sidebar_show_music),
    ("pictures", PreferenceManager::sidebar_show_pictures),
    ("videos", PreferenceManager::sidebar_show_videos),
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
        "trash" => ("user-trash-symbolic", "Trash", Location::uri("trash:///")),
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
    ) -> Rc<Self> {
        let content = Rc::new(SidebarContent {
            sidebar: Sidebar::new(),
            rows: RefCell::default(),
            open: Rc::new(open),
            menu: Rc::new(menu),
            current: RefCell::default(),
            pins: RefCell::new(bookmarks::current()),
            queued: Cell::new(false),
            cloud_watch: Cell::new(false),
            monitor: gio::VolumeMonitor::get(),
            keep: RefCell::default(),
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
        content.rebuild();
        content
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

    /// Marks the row for `location` as the one showing.
    pub(super) fn mark(&self, location: Option<&Location>) {
        *self.current.borrow_mut() = location.cloned();
        for (row_location, item) in self.rows.borrow().iter() {
            item.set_active(Some(row_location) == location);
        }
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
        self.places();
        self.pinned();
        self.devices();
        self.cloud();
        self.sidebar.refresh();
        let current = self.current.borrow().clone();
        self.mark(current.as_ref());
        let adj = self.sidebar.scroller.vadjustment();
        glib::idle_add_local_once(move || adj.set_value(scroll));
    }

    fn places(&self) {
        let prefs = PreferenceManager::shared();
        let home = super::tab::home();
        self.sidebar
            .append(&sidebar::group_label(&crate::i18n::tr("Places"), true));
        for (id, visible) in PLACES {
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
            self.sidebar.append(&item.button);
        }
    }

    fn pinned(&self) {
        let pins: Vec<_> = self
            .pins
            .borrow()
            .clone()
            .into_iter()
            .filter(|(l, _)| !is_place(l))
            .collect();
        if pins.is_empty() {
            return;
        }
        self.sidebar
            .append(&sidebar::group_label(&crate::i18n::tr("Pinned"), false));
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
                Target::Pin(location),
            );
            self.sidebar.append(&item.button);
        }
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
        if volumes.is_empty() && mounts.is_empty() {
            return;
        }
        self.sidebar
            .append(&sidebar::group_label(&crate::i18n::tr("Devices"), false));
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

    fn mount_volume(self: &Rc<Self>, volume: &gio::Volume, anchor: &gtk::Widget) {
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

pub(super) fn is_place_location(location: &Location) -> bool {
    is_place(location)
}

pub(super) fn is_pinned(location: &Location) -> bool {
    is_place(location) || bookmarks::current().iter().any(|(l, _)| l == location)
}

impl SidebarContent {
    /// Cloud accounts from nyx-cloud, when the setting shows them.
    fn cloud(self: &Rc<Self>) {
        if !PreferenceManager::shared().cloud_sidebar() {
            return;
        }
        let holder = gtk::Box::new(gtk::Orientation::Vertical, 1);
        let heading = sidebar::group_label(&crate::i18n::tr("Cloud"), false);
        heading.set_visible(false);
        self.sidebar.append(&heading);
        self.sidebar.append(&holder);
        let (weak, h, head) = (Rc::downgrade(self), holder.downgrade(), heading.downgrade());
        glib::MainContext::default().spawn_local(async move {
            let remotes = gio::spawn_blocking(|| nyx_cloud::Client::connect()?.list()).await;
            let (Some(content), Some(holder), Some(heading)) =
                (weak.upgrade(), h.upgrade(), head.upgrade())
            else {
                return;
            };
            let Ok(Ok(remotes)) = remotes else { return };
            let remotes: Vec<_> = remotes.into_iter().filter(|r| r.storage_enabled).collect();
            heading.set_visible(!remotes.is_empty());
            for remote in remotes {
                let item =
                    sidebar::nav_item("weather-overcast-symbolic", &remote.name, &remote.name);
                let (w, r) = (Rc::downgrade(&content), remote.clone());
                item.button.connect_clicked(move |b| {
                    if let Some(c) = w.upgrade() {
                        c.open_cloud(r.id.clone(), b.clone());
                    }
                });
                holder.append(&item.button);
            }
            content.sidebar.refresh();
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

    /// Mounts an account if needed (up to 35 s), then opens it.
    fn open_cloud(self: &Rc<Self>, id: String, button: gtk::Button) {
        button.set_sensitive(false);
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
            button.set_sensitive(true);
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
