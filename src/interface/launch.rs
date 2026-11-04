// SPDX-License-Identifier: MIT

//! How windows open: the restored tabs, a file or folder argument, a
//! FileManager1 reveal, and `--unlock-volume`.

use std::{rc::Rc, time::Duration};

use gtk::{gio, glib, prelude::*};

use crate::{
    adapters::{RevealRequest as FileManagerReveal, location_for_file, reveal_target_for_file},
    model::Location,
    ui::{
        preferences::PreferenceManager,
        window::{
            UnlockTarget,
            open_argument::{Kind, query_kind},
            unlock_argument::{ResolvedUnlock, resolve_unlock_target},
        },
    },
};

use super::window::Window;

const UNLOCK_WAIT: Duration = Duration::from_secs(8);

/// A new window: the last session's tabs when the setting keeps them.
pub(super) fn present(application: &gtk::Application) -> Rc<Window> {
    let first = super::window::focused().is_none();
    let session = (first && PreferenceManager::shared().restore_tabs())
        .then(crate::ui::tabs_session::load_restorable)
        .flatten()
        .filter(|s| !s.tabs.is_empty());
    let Some(session) = session else {
        return Window::open(application, None);
    };
    let mut tabs = session.tabs.into_iter();
    let window = Window::open(application, tabs.next());
    for location in tabs {
        window.open_tab(location);
    }
    window.select_tab(session.active);
    window
}

/// A file or folder from the command line or a launcher: folders open, and a
/// file shows selected in its folder.
pub(super) fn present_open(application: &gtk::Application, file: &gio::File) {
    let Some(location) = location_for_file(file) else {
        return;
    };
    let window = Window::open(application, Some(location.clone()));
    let file = file.clone();
    glib::MainContext::default().spawn_local(async move {
        let operation = super::mount::operation();
        let outcome = query_kind(&file, Some(&operation)).await;
        let Some(tab) = window.current() else { return };
        match outcome {
            Ok(Kind::Directory) => {}
            Ok(Kind::File) => {
                if let Some((parent, target)) = reveal_target_for_file(&file) {
                    tab.browser.reveal_locations(parent, vec![target]);
                }
            }
            Err(error) => {
                tracing::warn!(location = %location.display_path(), %error, "target location unavailable");
                nyx_ui::toast::show(&window.overlay, error.message());
            }
        }
    });
}

/// "Show in folder" from other apps: the folder with the items selected, and
/// their Properties when asked.
pub(super) fn present_reveal(application: &gtk::Application, request: FileManagerReveal) {
    let window = Window::open(application, Some(request.directory.clone()));
    let Some(tab) = window.current() else { return };
    if !request.selection.is_empty() {
        tab.browser
            .reveal_locations(request.directory, request.selection.clone());
    }
    if request.properties {
        let entries: Vec<_> = request
            .selection
            .iter()
            .filter_map(super::actions::entry_for)
            .collect();
        if !entries.is_empty() {
            super::menus::properties(entries);
        }
    }
}

/// `--unlock-volume`: waits briefly for the encrypted volume, then asks for
/// its password and opens it.
pub(super) fn present_unlock(application: &gtk::Application, target: UnlockTarget) {
    let window = Window::open(application, None);
    glib::MainContext::default().spawn_local(async move {
        let monitor = gio::VolumeMonitor::get();
        let started = std::time::Instant::now();
        let mut waiting_shown = false;
        loop {
            match resolve_unlock_target(&target, &monitor) {
                ResolvedUnlock::Unmatched => {}
                ResolvedUnlock::NotEncrypted => {
                    toast(&window, "This is not an encrypted volume");
                    return;
                }
                ResolvedUnlock::PasswordDrive(drive) => {
                    let w = Rc::downgrade(&window);
                    super::sidebar::start_drive(&drive, move |location| {
                        if let Some(w) = w.upgrade() {
                            w.navigate_to(location);
                        }
                    });
                    return;
                }
                ResolvedUnlock::Volume(volume) => {
                    unlock(&window, &volume).await;
                    return;
                }
            }
            if started.elapsed() >= UNLOCK_WAIT {
                tracing::warn!(
                    ?target,
                    "encrypted volume was not found within the appearance wait"
                );
                toast(&window, "The encrypted volume was not found");
                return;
            }
            if !waiting_shown && started.elapsed() >= Duration::from_millis(600) {
                waiting_shown = true;
                toast(&window, "Waiting for encrypted volume…");
            }
            glib::timeout_future(Duration::from_millis(250)).await;
        }
    });
}

async fn unlock(window: &Rc<Window>, volume: &gio::Volume) {
    if volume.get_mount().is_none()
        && let Err(error) = volume
            .mount_future(gio::MountMountFlags::NONE, Some(&super::mount::operation()))
            .await
    {
        if !error.matches(gio::IOErrorEnum::FailedHandled) {
            nyx_ui::toast::show(&window.overlay, error.message());
        }
        return;
    }
    if let Some(mount) = volume.get_mount() {
        let root = mount.root();
        if let Some(location) = location_for_file(&root) {
            window.navigate_to(location);
        }
    }
}

fn toast(window: &Window, text: &str) {
    nyx_ui::toast::show(&window.overlay, &crate::i18n::tr(text));
}

/// Keeps the tabs for next time, when the setting asks for it.
pub(super) fn persist_session(tabs: &[Location], active: usize) {
    if !PreferenceManager::shared().restore_tabs() {
        crate::ui::tabs_session::remove();
        return;
    }
    if !tabs.is_empty() {
        crate::ui::tabs_session::save(tabs, active.min(tabs.len() - 1));
    }
}
