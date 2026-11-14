// SPDX-License-Identifier: MIT

//! Favorites: the files and folders starred to keep at hand, one URI per line in
//! `~/.config/nyx-files/favorites`, listed by `favorites:///`.

use std::{
    cell::{Cell, RefCell},
    io,
    path::{Path, PathBuf},
    rc::{Rc, Weak},
};

use gio::{glib, prelude::*};

use crate::{
    adapters::{gio_file_for_location, location_for_file, mount_lanes},
    model::Location,
    services::{Responsiveness, sanitize_uri_credentials},
    storage::{self, writer},
    util::ui_thread,
};

type Callback = dyn Fn(&[Location]);
pub(crate) type FavoritesWatch = Rc<Callback>;

thread_local! {
    static CURRENT: RefCell<Option<Vec<Location>>> = const { RefCell::new(None) };
    static WATCHERS: RefCell<Vec<Weak<Callback>>> = const { RefCell::new(Vec::new()) };
    static MONITOR: RefCell<Option<gio::FileMonitor>> = const { RefCell::new(None) };
    static VOLUMES: RefCell<Option<gio::VolumeMonitor>> = const { RefCell::new(None) };
    /// How many favorites can be reached now; `None` until the first check ends.
    static REACHABLE: Cell<Option<usize>> = const { Cell::new(None) };
    static COUNT_WATCHERS: RefCell<Vec<Weak<dyn Fn()>>> = const { RefCell::new(Vec::new()) };
    static CHECKING: Cell<bool> = const { Cell::new(false) };
    static CHECK_AGAIN: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn favorites_path() -> PathBuf {
    storage::config_directory().join("favorites")
}

/// The favorites, oldest first.
pub(crate) fn current() -> Vec<Location> {
    if let Some(list) = CURRENT.with_borrow(Clone::clone) {
        return list;
    }
    let list = {
        let _startup = ui_thread::allow(ui_thread::Exception::Startup);
        read(&favorites_path()).unwrap_or_default()
    };
    CURRENT.replace(Some(list.clone()));
    follow_file();
    follow_mounts();
    refresh_reachable();
    list
}

pub(crate) fn contains(location: &Location) -> bool {
    current().contains(location)
}

pub(crate) fn add(locations: Vec<Location>) {
    edit(|list| with_added(list, &locations));
}

pub(crate) fn remove(locations: Vec<Location>) {
    edit(|list| without(list, &locations));
}

/// Removes them when every one is a favorite already, and adds them otherwise.
pub(crate) fn toggle(locations: Vec<Location>) {
    if all_favorites(&locations) {
        remove(locations);
    } else {
        add(locations);
    }
}

pub(crate) fn all_favorites(locations: &[Location]) -> bool {
    let list = current();
    !locations.is_empty() && locations.iter().all(|l| list.contains(l))
}

/// Called with the new list whenever it changes, until the returned handle is dropped.
pub(crate) fn watch_changes(callback: impl Fn(&[Location]) + 'static) -> FavoritesWatch {
    let callback: FavoritesWatch = Rc::new(callback);
    WATCHERS.with_borrow_mut(|watchers| {
        watchers.retain(|w| w.strong_count() > 0);
        watchers.push(Rc::downgrade(&callback));
    });
    callback
}

/// How many favorites can be reached now, once the first check has finished.
pub(crate) fn reachable() -> Option<usize> {
    // Loading the list starts the first check.
    current();
    REACHABLE.get()
}

/// Called when the reachable count changes, until the returned handle is dropped.
pub(crate) fn watch_count(callback: impl Fn() + 'static) -> Rc<dyn Fn()> {
    let callback: Rc<dyn Fn()> = Rc::new(callback);
    COUNT_WATCHERS.with_borrow_mut(|watchers| {
        watchers.retain(|w| w.strong_count() > 0);
        watchers.push(Rc::downgrade(&callback));
    });
    callback
}

/// What a reachability check does with one favorite.
#[derive(Debug, PartialEq)]
enum Reach {
    /// A local path on a mount that answers: worth one look, off the UI thread.
    Look(PathBuf),
    Yes,
    No,
}

/// Decided from what's already known, without touching the item: a stalled mount
/// or a server that isn't mounted counts as unreachable.
fn reach(location: &Location, answering: impl Fn(&Path) -> bool, mounted: &[gio::File]) -> Reach {
    if let Some(path) = location.native_path() {
        return if answering(path) { Reach::Look(path.to_path_buf()) } else { Reach::No };
    }
    let file = gio_file_for_location(location);
    if mounted.iter().any(|root| file.equal(root) || file.has_prefix(root)) {
        Reach::Yes
    } else {
        Reach::No
    }
}

/// Counts the reachable favorites again, one check at a time.
fn refresh_reachable() {
    if CHECKING.replace(true) {
        CHECK_AGAIN.set(true);
        return;
    }
    let mounted: Vec<gio::File> = gio::VolumeMonitor::get()
        .mounts()
        .iter()
        .map(|m| m.root())
        .filter(|root| !root.is_native())
        .collect();
    let answering = |path: &Path| mount_lanes::responsiveness(path) == Responsiveness::Answering;
    let mut known = 0;
    let mut paths = Vec::new();
    for location in current() {
        match reach(&location, answering, &mounted) {
            Reach::Look(path) => paths.push(path),
            Reach::Yes => known += 1,
            Reach::No => {}
        }
    }
    glib::MainContext::default().spawn_local(async move {
        let found = gio::spawn_blocking(move || {
            paths.iter().filter(|p| p.symlink_metadata().is_ok()).count()
        })
        .await
        .unwrap_or(0);
        CHECKING.set(false);
        let count = Some(known + found);
        if REACHABLE.replace(count) != count {
            let watchers = COUNT_WATCHERS.with_borrow(Clone::clone);
            for watcher in watchers.iter().filter_map(Weak::upgrade) {
                watcher();
            }
        }
        if CHECK_AGAIN.take() {
            refresh_reachable();
        }
    });
}

/// A drive or server coming or going changes what can be reached.
fn follow_mounts() {
    let monitor = gio::VolumeMonitor::get();
    monitor.connect_mount_added(|_, _| refresh_reachable());
    monitor.connect_mount_removed(|_, _| refresh_reachable());
    VOLUMES.replace(Some(monitor));
}

fn edit(change: impl FnOnce(&[Location]) -> Vec<Location>) {
    let before = current();
    let Some(after) = changed(&before, change) else {
        return;
    };
    CURRENT.replace(Some(after.clone()));
    drop(writer::save(
        favorites_path(),
        Some(serialize(&after).into_bytes()),
        writer::Policy::Config,
        Some("unable to save favorites"),
    ));
    notify(&after);
    refresh_reachable();
}

/// The edited list, or `None` when the edit changed nothing.
fn changed(
    before: &[Location],
    change: impl FnOnce(&[Location]) -> Vec<Location>,
) -> Option<Vec<Location>> {
    let after = change(before);
    (after != before).then_some(after)
}

fn with_added(list: &[Location], locations: &[Location]) -> Vec<Location> {
    let mut out = list.to_vec();
    for location in locations {
        if !out.contains(location) {
            out.push(location.clone());
        }
    }
    out
}

fn without(list: &[Location], locations: &[Location]) -> Vec<Location> {
    list.iter().filter(|l| !locations.contains(l)).cloned().collect()
}

fn notify(list: &[Location]) {
    let watchers = WATCHERS.with_borrow(Clone::clone);
    for watcher in watchers {
        if let Some(callback) = watcher.upgrade() {
            callback(list);
        }
    }
}

/// Picks up edits made outside this window (another Files process, by hand).
fn follow_file() {
    let Ok(monitor) = gio::File::for_path(favorites_path())
        .monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
    else {
        return;
    };
    monitor.connect_changed(|_, _, _, event| {
        if matches!(
            event,
            gio::FileMonitorEvent::ChangesDoneHint
                | gio::FileMonitorEvent::Deleted
                | gio::FileMonitorEvent::Created
                | gio::FileMonitorEvent::Renamed
                | gio::FileMonitorEvent::MovedIn
        ) {
            reload();
        }
    });
    MONITOR.replace(Some(monitor));
}

fn reload() {
    glib::MainContext::default().spawn_local(async {
        let Ok(Ok(list)) = gio::spawn_blocking(|| read(&favorites_path())).await else {
            return;
        };
        // Its own saves come back here too; only a different list is news.
        if CURRENT.with_borrow(|c| c.as_deref() != Some(list.as_slice())) {
            CURRENT.replace(Some(list.clone()));
            notify(&list);
            refresh_reachable();
        }
    });
}

fn read(path: &Path) -> io::Result<Vec<Location>> {
    ui_thread::assert_off();
    match std::fs::read(path) {
        Ok(contents) => Ok(parse(&contents)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

fn parse(contents: &[u8]) -> Vec<Location> {
    let mut list = Vec::new();
    for line in contents.split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Ok(uri) = std::str::from_utf8(line) else {
            continue;
        };
        let uri = uri.trim();
        if uri.is_empty() {
            continue;
        }
        let Some(location) = location_for_file(&gio::File::for_uri(uri)) else {
            continue;
        };
        if !list.contains(&location) {
            list.push(location);
        }
    }
    list
}

fn serialize(list: &[Location]) -> String {
    let mut contents = String::new();
    for location in list {
        let uri = gio_file_for_location(location).uri().to_string();
        if let Ok((uri, _)) = sanitize_uri_credentials(&uri) {
            contents.push_str(&uri);
            contents.push('\n');
        }
    }
    contents
}

#[cfg(test)]
mod tests;
