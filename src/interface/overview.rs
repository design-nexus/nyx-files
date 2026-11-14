// SPDX-License-Identifier: MIT

//! The Overview Files opens on: recent files as thumbnails, pinned folders,
//! every drive and cloud account with how full it is, what's new in
//! Downloads, and the Trash with its size.

use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::{Rc, Weak},
};

use gtk::{gio, glib, prelude::*};
use nyx_ui::widgets;

use crate::{
    model::{EntryKind, FileEntry, Location, MetadataValue},
    ui::{preferences::PreferenceManager, window::drive_ops},
};

use super::{sidebar::Target, window::Window};

/// The Overview's width at most, as the mockup's page clamp.
const MAX_WIDTH: i32 = 940;
const RECENT: usize = 5;

pub(super) struct Overview {
    pub(super) root: gtk::ScrolledWindow,
    body: gtk::Box,
    pinned_head: RefCell<Option<gtk::Widget>>,
    window: RefCell<Weak<Window>>,
    narrow: Cell<bool>,
    editing_pins: Cell<bool>,
    /// The pins last seen: reading them reports them again, changed or not.
    pins: RefCell<crate::adapters::bookmarks::Pins>,
    /// Bumped by each rebuild so late results from an older one are dropped.
    generation: Cell<u64>,
    keep: RefCell<Vec<Box<dyn std::any::Any>>>,
    queued: Cell<bool>,
}

/// A file's kind: its symbolic icon and the swatch it shows in.
pub(super) fn kind_icon(name: &str, directory: bool) -> gtk::Image {
    let (icon, swatch) = if directory {
        ("folder-symbolic".to_string(), "accent")
    } else {
        let (content_type, _) = gio::content_type_guess(Some(name), None::<&[u8]>);
        let icon = icon_name(&gio::content_type_get_symbolic_icon(&content_type));
        (icon, kind_swatch(&content_type))
    };
    let image = gtk::Image::from_icon_name(&icon);
    image.add_css_class("kind-icon");
    image.add_css_class(&match swatch {
        "accent" | "dim" => format!("kind-{swatch}"),
        swatch => format!("section-swatch-{swatch}"),
    });
    image
}

fn icon_name(icon: &gio::Icon) -> String {
    icon.downcast_ref::<gio::ThemedIcon>()
        .and_then(|t| t.names().first().map(|n| n.to_string()))
        .unwrap_or_else(|| "text-x-generic-symbolic".into())
}

fn kind_swatch(content_type: &str) -> &'static str {
    let is = |t: &str| gio::content_type_is_a(content_type, t);
    if content_type.starts_with("image/") {
        "teal"
    } else if content_type.starts_with("video/") {
        "violet"
    } else if content_type.starts_with("audio/") {
        "pink"
    } else if is("application/pdf") {
        "red"
    } else if content_type.contains("spreadsheet")
        || content_type.contains("excel")
        || is("text/csv")
    {
        "green"
    } else if content_type.contains("wordprocessing")
        || content_type.contains("msword")
        || content_type.contains("opendocument.text")
        || is("application/rtf")
    {
        "blue"
    } else if is("application/zip")
        || is("application/x-tar")
        || is("application/x-compressed-tar")
        || is("application/x-7z-compressed")
        || is("application/vnd.rar")
        || is("application/gzip")
    {
        "coral"
    } else {
        "dim"
    }
}

/// An entry for a file found outside a listing, with what its thumbnail needs.
pub(super) fn entry_at(path: &Path) -> Option<FileEntry> {
    let mut entry = super::actions::entry_for(&Location::local(path))?;
    if let Ok(meta) = std::fs::metadata(path) {
        if !meta.is_dir() {
            entry.size = MetadataValue::Known(meta.len());
        }
        if let Ok(modified) = meta.modified()
            && let Ok(since) = modified.duration_since(std::time::UNIX_EPOCH)
        {
            entry.modified_unix_seconds = MetadataValue::Known(since.as_secs() as i64);
        }
        if meta.is_dir() {
            entry.kind = EntryKind::Directory;
        }
    }
    Some(entry)
}

/// A thumbnail in a frame `height` tall: photos and videos fill it, pages fit in it.
pub(super) fn framed_thumbnail(
    entry: &FileEntry,
    width: i32,
    height: i32,
    icon: i32,
    radius: f32,
) -> crate::ui::thumbnail::ThumbnailSlot {
    let slot = crate::ui::thumbnail::ThumbnailSlot::new(icon);
    let (content_type, _) = gio::content_type_guess(Some(&entry.native_name), None::<&[u8]>);
    let cover = content_type.starts_with("image/") || content_type.starts_with("video/");
    slot.set_frame(Some(crate::ui::thumbnail::SlotFrame {
        min_width: 48,
        width,
        height,
        cover,
        radius,
    }));
    let name = crate::ui::thumbnail::theme_icon(
        &crate::ui::file_icons::name_for(entry),
        entry.is_directory(),
    );
    crate::ui::thumbnail::set_thumbnail_or_icon(&slot, entry, &name, icon, 320);
    slot
}

/// "Today, 10:41", "Yesterday" or "Nov 1".
fn day_text(seconds: i64) -> String {
    let (Ok(when), Ok(now)) = (
        glib::DateTime::from_unix_local(seconds),
        glib::DateTime::now_local(),
    ) else {
        return String::new();
    };
    let midnight = |d: &glib::DateTime| {
        let (y, m, day) = d.ymd();
        glib::DateTime::new(&d.timezone(), y, m, day, 0, 0, 0.0).ok()
    };
    let days = match (midnight(&now), midnight(&when)) {
        (Some(a), Some(b)) => (a.difference(&b).0 + 43_200_000_000) / 86_400_000_000,
        _ => i64::MAX,
    };
    match days {
        0 => format!(
            "{}, {}",
            crate::i18n::tr("today"),
            crate::util::localized_date(&when, "%H:%M")
        ),
        1 => crate::i18n::tr("Yesterday"),
        _ if now.year() == when.year() => crate::util::localized_date(&when, "%b %-d"),
        _ => crate::util::localized_date(&when, "%b %-d, %Y"),
    }
}

fn head(text: &str) -> (gtk::Box, gtk::Box) {
    let row = widgets::hbox(8);
    row.add_css_class("overview-head");
    let title = widgets::label(&crate::i18n::tr(text), "overview-title");
    title.set_hexpand(true);
    row.append(&title);
    let end = widgets::hbox(4);
    row.append(&end);
    (row, end)
}

fn link(text: &str, run: impl Fn() + 'static) -> gtk::Button {
    let button = gtk::Button::with_label(&crate::i18n::tr(text));
    button.add_css_class("overview-link");
    button.connect_clicked(move |_| run());
    button
}

fn grid() -> gtk::Grid {
    let grid = gtk::Grid::new();
    grid.set_column_homogeneous(true);
    grid.set_column_spacing(12);
    grid.set_row_spacing(12);
    grid
}

fn card_text(title: &str, detail: &str) -> (gtk::Box, gtk::Label) {
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    let name = widgets::label(title, "card-name");
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    text.append(&name);
    let sub = widgets::label(detail, "card-detail");
    sub.set_ellipsize(gtk::pango::EllipsizeMode::End);
    text.append(&sub);
    (text, sub)
}

/// The size of the files under `path`, not following links.
fn tree_size(path: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for item in read.flatten() {
            let Ok(meta) = item.path().symlink_metadata() else {
                continue;
            };
            if meta.is_dir() {
                stack.push(item.path());
            } else {
                total += meta.len();
            }
        }
    }
    total
}

/// Visible items in `dir`, as the folder's count.
fn child_count(dir: &Path) -> Option<usize> {
    Some(
        std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
            .count(),
    )
}

/// Downloads' newest files, newest first.
fn newest(dir: &Path, count: usize) -> Vec<PathBuf> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = read
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            (!meta.is_dir()).then(|| Some((meta.modified().ok()?, e.path())))?
        })
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    files.into_iter().take(count).map(|(_, p)| p).collect()
}

/// Files in Downloads changed in the last day, for the sidebar's badge.
pub(super) fn new_downloads(dir: &Path) -> usize {
    let Ok(read) = std::fs::read_dir(dir) else {
        return 0;
    };
    let day = std::time::Duration::from_secs(24 * 3600);
    read.flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| e.metadata().ok())
        .filter(|m| !m.is_dir())
        .filter(|m| {
            m.modified()
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age < day)
        })
        .count()
}

pub(super) fn downloads_dir() -> Option<PathBuf> {
    glib::user_special_dir(glib::UserDirectory::Downloads).filter(|d| *d != super::tab::home())
}

/// Recently used files that still exist, newest first.
pub(super) fn recent_files(limit: usize) -> Vec<(PathBuf, i64)> {
    let mut items: Vec<(PathBuf, i64)> = gtk::RecentManager::default()
        .items()
        .into_iter()
        .filter_map(|info| {
            let path = gio::File::for_uri(&info.uri()).path()?;
            (path.is_file()).then(|| (path, info.modified().to_unix()))
        })
        .collect();
    items.sort_by_key(|i| std::cmp::Reverse(i.1));
    items.truncate(limit);
    items
}

fn trash_dir() -> PathBuf {
    glib::user_data_dir().join("Trash/files")
}

/// A drive, cloud account or the system disk, as a Storage card shows it.
struct Store {
    icon: String,
    name: String,
    /// What follows the free space: "USB", "Synced".
    note: String,
    path: Option<PathBuf>,
    eject: Option<Target>,
    open: Option<Location>,
    cloud: Option<String>,
    /// A drive that isn't mounted yet: a click mounts and opens it.
    volume: Option<gio::Volume>,
}

fn stores(window: &Window) -> Vec<Store> {
    let mut stores = vec![Store {
        icon: "drive-harddisk-symbolic".into(),
        name: crate::i18n::tr("System"),
        note: String::new(),
        path: Some(PathBuf::from("/")),
        eject: None,
        open: Some(Location::local("/")),
        cloud: None,
        volume: None,
    }];
    let prefs = PreferenceManager::shared();
    let monitor = gio::VolumeMonitor::get();
    for mount in monitor.mounts() {
        let Some(root) = mount.root().path() else {
            continue;
        };
        if mount.is_shadowed()
            || root == Path::new("/")
            || crate::ui::window::cloud::is_cloud_mount(&root)
            || gio_unix::functions::is_mount_path_system_internal(&root)
        {
            continue;
        }
        let target = match mount.volume() {
            Some(volume) => Target::Volume(volume),
            None => Target::Mount(mount.clone()),
        };
        let label = match &target {
            Target::Volume(v) => {
                crate::ui::window::drive_dialogs::PropertiesTarget::Volume(v.clone()).label_id()
            }
            _ => {
                crate::ui::window::drive_dialogs::PropertiesTarget::Mount(mount.clone()).label_id()
            }
        };
        let name = label
            .and_then(|id| prefs.device_label(&id))
            .unwrap_or_else(|| mount.name().to_string());
        let removable = mount
            .drive()
            .is_some_and(|d| d.is_removable() || d.is_media_removable());
        let releasable = mount.can_unmount() || mount.can_eject();
        stores.push(Store {
            icon: icon_name(&mount.symbolic_icon()),
            name,
            note: if removable {
                crate::i18n::tr("Removable")
            } else {
                String::new()
            },
            path: Some(root.clone()),
            eject: releasable.then_some(target),
            open: Some(Location::local(root)),
            cloud: None,
            volume: None,
        });
    }
    for volume in monitor.volumes() {
        if volume.get_mount().is_some() || !volume.can_mount() {
            continue;
        }
        let label =
            crate::ui::window::drive_dialogs::PropertiesTarget::Volume(volume.clone()).label_id();
        let name = label
            .and_then(|id| prefs.device_label(&id))
            .unwrap_or_else(|| volume.name().to_string());
        stores.push(Store {
            icon: icon_name(&volume.symbolic_icon()),
            name,
            note: crate::i18n::tr("Not mounted"),
            path: None,
            eject: volume.can_eject().then(|| Target::Volume(volume.clone())),
            open: None,
            cloud: None,
            volume: Some(volume),
        });
    }
    for remote in window.cloud_accounts() {
        let mounted = remote.mount_status == "mounted" && !remote.mount_path.is_empty();
        stores.push(Store {
            icon: "weather-overcast-symbolic".into(),
            name: remote.name.clone(),
            note: crate::i18n::tr(match remote.connection.as_str() {
                "error" | "authentication_required" => "Needs attention",
                _ if mounted => "Synced",
                _ => "Connects when opened",
            }),
            path: mounted.then(|| PathBuf::from(&remote.mount_path)),
            eject: None,
            open: None,
            cloud: Some(remote.id.clone()),
            volume: None,
        });
    }
    stores
}

impl Overview {
    pub(super) fn new() -> Rc<Overview> {
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body.add_css_class("overview");
        let clamp = nyx_ui::clamp::Clamp::new(&body, MAX_WIDTH);
        let root = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .hexpand(true)
            .child(&clamp)
            .build();
        root.add_css_class("overview-scroll");
        Rc::new(Overview {
            root,
            body,
            pinned_head: RefCell::default(),
            window: RefCell::default(),
            narrow: Cell::new(false),
            editing_pins: Cell::new(false),
            pins: RefCell::default(),
            generation: Cell::new(0),
            keep: RefCell::default(),
            queued: Cell::new(false),
        })
    }

    pub(super) fn attach(self: &Rc<Self>, window: &Rc<Window>) {
        *self.window.borrow_mut() = Rc::downgrade(window);
        self.pins.replace(crate::adapters::bookmarks::current());
        let weak = Rc::downgrade(self);
        let pins = crate::adapters::bookmarks::watch_changes(move |result| {
            let Some(o) = weak.upgrade() else {
                return false;
            };
            if let Ok(pins) = result
                && *o.pins.borrow() != *pins
            {
                *o.pins.borrow_mut() = pins.clone();
                o.queue_refresh();
            }
            true
        });
        self.keep.borrow_mut().push(Box::new(pins));
        let monitor = gio::VolumeMonitor::get();
        let weak = Rc::downgrade(self);
        let changed = move || {
            if let Some(o) = weak.upgrade() {
                o.queue_refresh();
            }
        };
        let c = changed.clone();
        monitor.connect_mount_added(move |_, _| c());
        let c = changed.clone();
        monitor.connect_mount_removed(move |_, _| c());
        let c = changed.clone();
        monitor.connect_mount_changed(move |_, _| c());
        self.keep.borrow_mut().push(Box::new(monitor));
        for dir in [Some(trash_dir()), downloads_dir()].into_iter().flatten() {
            if let Ok(m) = gio::File::for_path(dir)
                .monitor_directory(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
            {
                let c = changed.clone();
                m.connect_changed(move |_, _, _, _| c());
                self.keep.borrow_mut().push(Box::new(m));
            }
        }
        gtk::RecentManager::default().connect_changed(move |_| changed());
    }

    /// Rebuilds once the current burst of changes is over, while showing.
    pub(super) fn queue_refresh(self: &Rc<Self>) {
        if self.queued.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
            if let Some(o) = weak.upgrade() {
                o.queued.set(false);
                if o.root.is_mapped() {
                    o.refresh();
                }
            }
        });
    }

    pub(super) fn set_narrow(self: &Rc<Self>, narrow: bool) {
        if self.narrow.replace(narrow) != narrow {
            self.refresh();
        }
    }

    /// Scrolls so the Pinned cards are at the top.
    pub(super) fn show_pinned(&self) {
        let Some(head) = self.pinned_head.borrow().clone() else {
            return;
        };
        let root = self.root.clone();
        glib::idle_add_local_once(move || {
            if let Some(point) = head.compute_point(&root, &gtk::graphene::Point::zero()) {
                let adj = root.vadjustment();
                adj.set_value(adj.value() + f64::from(point.y()) - 8.0);
            }
        });
    }

    pub(super) fn refresh(self: &Rc<Self>) {
        let Some(window) = self.window.borrow().upgrade() else {
            return;
        };
        self.generation.set(self.generation.get() + 1);
        while let Some(child) = self.body.first_child() {
            self.body.remove(&child);
        }
        let narrow = self.narrow.get();
        if narrow {
            self.body.add_css_class("narrow");
        } else {
            self.body.remove_css_class("narrow");
        }
        self.recent(&window, narrow);
        self.pinned(&window, narrow);
        self.storage(&window, narrow);
        self.downloads(&window, narrow);
        self.trash(&window);
    }

    fn recent(&self, window: &Rc<Window>, narrow: bool) {
        let count = if narrow { RECENT - 1 } else { RECENT };
        let files = recent_files(count);
        if files.is_empty() {
            return;
        }
        let (row, end) = head("Recent");
        if window.recent_available() {
            let w = Rc::downgrade(window);
            end.append(&link("Show all", move || {
                if let Some(w) = w.upgrade() {
                    w.navigate_to(Location::uri("recent:///"));
                }
            }));
        }
        self.body.append(&row);
        let cards = grid();
        for (i, (path, used)) in files.iter().enumerate() {
            let Some(entry) = entry_at(path) else {
                continue;
            };
            cards.attach(&recent_card(window, &entry, *used), i as i32, 0, 1, 1);
        }
        for i in files.len()..count {
            let filler = gtk::Box::new(gtk::Orientation::Vertical, 0);
            cards.attach(&filler, i as i32, 0, 1, 1);
        }
        self.body.append(&cards);
    }

    fn pinned(self: &Rc<Self>, window: &Rc<Window>, narrow: bool) {
        let pins = super::sidebar::pinned_folders();
        let (row, end) = head("Pinned");
        self.pinned_head.replace(Some(row.clone().upcast()));
        if !pins.is_empty() {
            let weak = Rc::downgrade(self);
            let editing = self.editing_pins.get();
            end.append(&link(if editing { "Done" } else { "Edit" }, move || {
                if let Some(o) = weak.upgrade() {
                    o.editing_pins.set(!o.editing_pins.get());
                    o.refresh();
                }
            }));
        }
        self.body.append(&row);
        if pins.is_empty() {
            let note = widgets::label(
                &crate::i18n::tr(
                    "Pin a folder from its menu, or drop it on the sidebar, to keep it here.",
                ),
                "overview-note",
            );
            note.set_wrap(true);
            self.body.append(&note);
            return;
        }
        let columns = if narrow { 2 } else { 3 };
        let cards = grid();
        let mut pending = Vec::new();
        for (i, (location, name)) in pins.iter().enumerate() {
            let (card, detail) = self.pin_card(window, location, name);
            cards.attach(&card, (i % columns) as i32, (i / columns) as i32, 1, 1);
            if let Some(path) = location.native_path() {
                pending.push((path.to_path_buf(), detail));
            }
        }
        for i in pins.len()..columns {
            cards.attach(
                &gtk::Box::new(gtk::Orientation::Vertical, 0),
                i as i32,
                0,
                1,
                1,
            );
        }
        self.body.append(&cards);
        let generation = self.generation.get();
        let weak = Rc::downgrade(self);
        let paths: Vec<PathBuf> = pending.iter().map(|(p, _)| p.clone()).collect();
        glib::MainContext::default().spawn_local(async move {
            let counts = gio::spawn_blocking(move || {
                paths.iter().map(|p| child_count(p)).collect::<Vec<_>>()
            })
            .await
            .unwrap_or_default();
            if weak
                .upgrade()
                .is_none_or(|o| o.generation.get() != generation)
            {
                return;
            }
            for ((path, label), count) in pending.iter().zip(counts) {
                let parent = path.parent().map(place_name).unwrap_or_default();
                let text = match count {
                    Some(n) => format!("{} · {parent}", crate::i18n::count("items", n)),
                    None => parent,
                };
                label.set_text(&text);
            }
        });
    }

    fn pin_card(
        &self,
        window: &Rc<Window>,
        location: &Location,
        name: &str,
    ) -> (gtk::Widget, gtk::Label) {
        let button = gtk::Button::new();
        button.add_css_class("overview-card");
        button.add_css_class("pin-card");
        let line = widgets::hbox(12);
        let icon = gtk::Image::from_icon_name("folder-symbolic");
        icon.set_pixel_size(26);
        icon.add_css_class("pin-icon");
        line.append(&icon);
        let (text, detail) = card_text(name, "");
        line.append(&text);
        button.set_child(Some(&line));
        button.set_tooltip_text(Some(&location.display_path()));
        let (w, l) = (Rc::downgrade(window), location.clone());
        button.connect_clicked(move |_| {
            if let Some(w) = w.upgrade() {
                w.navigate_to(l.clone());
            }
        });
        let (w, l) = (Rc::downgrade(window), location.clone());
        nyx_ui::menu::on_right_click(&button, move |widget, x, y| {
            let Some(w) = w.upgrade() else { return };
            let (w1, l1) = (w.clone(), l.clone());
            let (w2, l2) = (w.clone(), l.clone());
            let l3 = l.clone();
            nyx_ui::menu::popup_at(
                widget,
                x,
                y,
                vec![
                    nyx_ui::menu::item(&crate::i18n::tr("Open"), move || {
                        w1.navigate_to(l1.clone())
                    })
                    .icon("folder-open-symbolic"),
                    nyx_ui::menu::item(&crate::i18n::tr("New Tab"), move || {
                        w2.open_tab(l2.clone())
                    })
                    .icon("tab-new-symbolic"),
                    nyx_ui::menu::separator(),
                    nyx_ui::menu::item(&crate::i18n::tr("Unpin from sidebar"), move || {
                        super::sidebar::unpin(l3.clone())
                    })
                    .icon("view-pin-symbolic"),
                ],
            );
        });
        window.drop_on(&button, location.clone());
        if !self.editing_pins.get() {
            return (button.upcast(), detail);
        }
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&button));
        let remove = widgets::icon_button(
            "window-close-symbolic",
            &crate::i18n::tr("Unpin from sidebar"),
        );
        remove.add_css_class("pin-remove");
        remove.set_halign(gtk::Align::End);
        remove.set_valign(gtk::Align::Center);
        remove.set_margin_end(10);
        let l = location.clone();
        remove.connect_clicked(move |_| super::sidebar::unpin(l.clone()));
        overlay.add_overlay(&remove);
        (overlay.upcast(), detail)
    }

    fn storage(self: &Rc<Self>, window: &Rc<Window>, narrow: bool) {
        let (row, _) = head("Storage");
        self.body.append(&row);
        let stores = stores(window);
        let columns = if narrow { 2 } else { 3 };
        let cards = grid();
        let count = stores.len();
        let mut pending = Vec::new();
        for (i, store) in stores.into_iter().enumerate() {
            let last_alone = narrow && i == count - 1 && count % 2 == 1;
            let (card, bar, sub) = storage_card(window, &store);
            cards.attach(
                &card,
                (i % columns) as i32,
                (i / columns) as i32,
                if last_alone { 2 } else { 1 },
                1,
            );
            pending.push((store.path.clone(), store.note.clone(), bar, sub));
        }
        for i in count..columns {
            cards.attach(
                &gtk::Box::new(gtk::Orientation::Vertical, 0),
                i as i32,
                0,
                1,
                1,
            );
        }
        self.body.append(&cards);
        let generation = self.generation.get();
        let weak = Rc::downgrade(self);
        let paths: Vec<Option<PathBuf>> = pending.iter().map(|(p, ..)| p.clone()).collect();
        glib::MainContext::default().spawn_local(async move {
            let usage = gio::spawn_blocking(move || {
                paths
                    .iter()
                    .map(|p| p.as_deref().and_then(drive_ops::usage_for_path))
                    .collect::<Vec<_>>()
            })
            .await
            .unwrap_or_default();
            if weak
                .upgrade()
                .is_none_or(|o| o.generation.get() != generation)
            {
                return;
            }
            for ((_, note, bar, sub), usage) in pending.iter().zip(usage) {
                let mut parts = Vec::new();
                match usage {
                    Some((total, free)) if total > 0 => {
                        bar.set_value(total.saturating_sub(free) as f64 / total as f64);
                        parts.push(
                            rust_i18n::t!(
                                "%{free} free of %{total}",
                                free = glib::format_size(free),
                                total = glib::format_size(total)
                            )
                            .into_owned(),
                        );
                    }
                    _ => bar.set_visible(false),
                }
                if !note.is_empty() {
                    parts.push(note.clone());
                }
                sub.set_text(&parts.join(" · "));
            }
        });
    }

    fn downloads(self: &Rc<Self>, window: &Rc<Window>, narrow: bool) {
        let Some(dir) = downloads_dir() else { return };
        let limit = if narrow { 2 } else { 4 };
        let files = newest(&dir, limit);
        if files.is_empty() {
            return;
        }
        let (row, end) = head("New in Downloads");
        let w = Rc::downgrade(window);
        let d = dir.clone();
        end.append(&link("Open Downloads", move || {
            if let Some(w) = w.upgrade() {
                w.navigate_to(Location::local(d.clone()));
            }
        }));
        self.body.append(&row);
        let rows = grid();
        rows.set_row_spacing(8);
        for (i, path) in files.iter().enumerate() {
            let Some(entry) = entry_at(path) else {
                continue;
            };
            rows.attach(
                &download_row(window, &entry),
                (i % 2) as i32,
                (i / 2) as i32,
                1,
                1,
            );
        }
        if files.len() == 1 {
            rows.attach(&gtk::Box::new(gtk::Orientation::Vertical, 0), 1, 0, 1, 1);
        }
        self.body.append(&rows);
    }

    fn trash(self: &Rc<Self>, window: &Rc<Window>) {
        let line = widgets::hbox(10);
        line.add_css_class("overview-trash");
        let icon = gtk::Image::from_icon_name("user-trash-symbolic");
        line.append(&icon);
        let text = gtk::Label::new(None);
        text.set_xalign(0.0);
        text.set_hexpand(true);
        text.set_markup(&format!(
            "<b>{}</b>",
            glib::markup_escape_text(&crate::i18n::tr("Trash"))
        ));
        line.append(&text);
        let open = gtk::Button::with_label(&crate::i18n::tr("Open"));
        open.add_css_class("flat");
        let w = Rc::downgrade(window);
        open.connect_clicked(move |_| {
            if let Some(w) = w.upgrade() {
                w.navigate_to(Location::uri("trash:///"));
            }
        });
        line.append(&open);
        let empty = gtk::Button::with_label(&crate::i18n::tr("Empty Trash…"));
        empty.add_css_class("danger-text");
        let w = Rc::downgrade(window);
        empty.connect_clicked(move |_| {
            if let Some(w) = w.upgrade() {
                super::actions::empty_trash(&w);
            }
        });
        line.append(&empty);
        self.body.append(&line);
        let generation = self.generation.get();
        let weak = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let (count, size) = gio::spawn_blocking(|| {
                let dir = trash_dir();
                let count = std::fs::read_dir(&dir).map_or(0, |r| r.count());
                (count, tree_size(&dir))
            })
            .await
            .unwrap_or((0, 0));
            if weak
                .upgrade()
                .is_none_or(|o| o.generation.get() != generation)
            {
                return;
            }
            let title = glib::markup_escape_text(&crate::i18n::tr("Trash"));
            let detail = if count == 0 {
                crate::i18n::tr("Empty")
            } else {
                format!(
                    "{}, {}",
                    crate::i18n::count("items", count),
                    glib::format_size(size)
                )
            };
            text.set_markup(&format!(
                "<b>{title}</b> · {}",
                glib::markup_escape_text(&detail)
            ));
            empty.set_visible(count > 0);
        });
    }
}

/// Where a folder lives, as people call it: "Documents", "Home", or its name.
pub(super) fn place_name(path: &Path) -> String {
    if path == super::tab::home() {
        return crate::i18n::tr("Home");
    }
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn recent_card(window: &Rc<Window>, entry: &FileEntry, used: i64) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("overview-card");
    button.add_css_class("recent-card");
    button.set_overflow(gtk::Overflow::Hidden);
    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let thumb = framed_thumbnail(entry, 170, 116, 40, 0.0);
    thumb.add_css_class("recent-thumb");
    thumb.set_hexpand(true);
    column.append(&thumb);
    let (content_type, _) = gio::content_type_guess(Some(&entry.native_name), None::<&[u8]>);
    let when = day_text(used);
    let when = if content_type.starts_with("image/") {
        rust_i18n::t!("Added %{when}", when = when).into_owned()
    } else {
        rust_i18n::t!("Edited %{when}", when = when).into_owned()
    };
    let (text, _) = card_text(&entry.display_name, &when);
    text.add_css_class("recent-text");
    column.append(&text);
    button.set_child(Some(&column));
    button.set_tooltip_text(Some(&entry.location.display_path()));
    let (w, e) = (Rc::downgrade(window), entry.clone());
    button.connect_clicked(move |_| {
        if let Some(w) = w.upgrade() {
            w.open_file(&e);
        }
    });
    file_menu(&button, window, entry);
    button
}

/// Open, and Show in folder, for a file the Overview lists.
fn file_menu(button: &gtk::Button, window: &Rc<Window>, entry: &FileEntry) {
    let (w, e) = (Rc::downgrade(window), entry.clone());
    nyx_ui::menu::on_right_click(button, move |widget, x, y| {
        let Some(w) = w.upgrade() else { return };
        let (w1, e1) = (w.clone(), e.clone());
        let (w2, l2) = (w.clone(), e.location.clone());
        nyx_ui::menu::popup_at(
            widget,
            x,
            y,
            vec![
                nyx_ui::menu::item(&crate::i18n::tr("Open"), move || w1.open_file(&e1))
                    .icon("document-open-symbolic"),
                nyx_ui::menu::item(&crate::i18n::tr("Show in folder"), move || w2.reveal(&l2))
                    .icon("folder-open-symbolic"),
            ],
        );
    });
}

fn storage_card(window: &Rc<Window>, store: &Store) -> (gtk::Box, gtk::LevelBar, gtk::Label) {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    card.add_css_class("overview-card");
    card.add_css_class("storage-card");
    card.set_cursor_from_name(Some("pointer"));
    let top = widgets::hbox(8);
    top.add_css_class("storage-top");
    let icon = gtk::Image::from_icon_name(&store.icon);
    icon.add_css_class("dim");
    top.append(&icon);
    let name = widgets::label(&store.name, "card-name");
    name.set_hexpand(true);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    top.append(&name);
    if let Some(target) = store.eject.clone() {
        let label = super::devices::release_label(&target).unwrap_or("Eject");
        let eject = widgets::labeled_button("media-eject-symbolic", &crate::i18n::tr(label));
        eject.add_css_class("storage-eject");
        eject.connect_clicked(move |b| super::devices::release(&target, b.upcast_ref()));
        top.append(&eject);
    }
    card.append(&top);
    let bar = widgets::usage_bar();
    bar.set_visible(store.path.is_some());
    card.append(&bar);
    let sub = widgets::label(&store.note, "card-detail");
    sub.set_ellipsize(gtk::pango::EllipsizeMode::End);
    card.append(&sub);
    let click = gtk::GestureClick::new();
    let (w, open, cloud, volume) = (
        Rc::downgrade(window),
        store.open.clone(),
        store.cloud.clone(),
        store.volume.clone(),
    );
    click.connect_released(move |gesture, _, x, y| {
        let Some(w) = w.upgrade() else { return };
        // The Eject button handles its own clicks.
        if let Some(widget) = gesture.widget()
            && widget.pick(x, y, gtk::PickFlags::DEFAULT).is_some_and(|p| {
                p.ancestor(gtk::Button::static_type()).is_some() || p.is::<gtk::Button>()
            })
        {
            return;
        }
        if let Some(location) = &open {
            w.navigate_to(location.clone());
        } else if let Some(id) = &cloud {
            w.open_cloud(id);
        } else if let (Some(volume), Some(widget)) = (&volume, gesture.widget()) {
            w.mount_and_open(volume, &widget);
        }
    });
    card.add_controller(click);
    if let Some(location) = &store.open {
        window.drop_on(&card, location.clone());
    }
    (card, bar, sub)
}

fn download_row(window: &Rc<Window>, entry: &FileEntry) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("overview-card");
    button.add_css_class("download-row");
    let line = widgets::hbox(12);
    line.append(&kind_icon(&entry.display_name, false));
    let size = match entry.size {
        MetadataValue::Known(bytes) => glib::format_size(bytes).to_string(),
        _ => String::new(),
    };
    let when = match entry.modified_unix_seconds {
        MetadataValue::Known(seconds) => crate::util::modified_text(seconds),
        _ => String::new(),
    };
    let detail = [size, when]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    let (text, _) = card_text(&entry.display_name, &detail);
    line.append(&text);
    button.set_child(Some(&line));
    let (w, e) = (Rc::downgrade(window), entry.clone());
    button.connect_clicked(move |_| {
        if let Some(w) = w.upgrade() {
            w.open_file(&e);
        }
    });
    file_menu(&button, window, entry);
    button
}
