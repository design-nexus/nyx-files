// SPDX-License-Identifier: MIT

//! Devices, drives and encrypted volumes, and the window-level questions the
//! dialogs ask (whether another modal is showing).

use std::{cell::Cell, env, path::PathBuf, rc::Rc};

use gtk::{
    gio,
    gio::prelude::EmblemedIconExt as _,
    glib,
    prelude::*,
};

use crate::app::Browser;

pub(crate) mod cloud;
pub(crate) mod device_release;
pub(crate) mod devices;
pub(crate) mod drive_dialogs;
pub(crate) mod drive_ops;
pub(crate) mod keyboard;
pub(crate) mod open_argument;
pub(crate) mod unlock_argument;
pub(crate) mod volume_password;

#[cfg(test)]
pub(super) use devices::offer_removable_destinations_for_test;
pub use unlock_argument::{UnlockTarget};

/// How a device is released: eject where the drive reports it (USB sticks,
/// optical discs); plain unmount covers mounts without ejectable media.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MediaRelease {
    EjectVolume,
    EjectMount,
    UnmountMount,
}

fn media_release_error_title(action: MediaRelease) -> &'static str {
    match action {
        MediaRelease::EjectVolume | MediaRelease::EjectMount => "Unable to eject device",
        MediaRelease::UnmountMount => "Unable to unmount device",
    }
}

fn navigate_home_if_within(browser: &Rc<Browser>, root: &gio::File) {
    let Some(device) = crate::adapters::location_for_file(root) else {
        return;
    };
    let active = browser.active_location();
    if active
        .as_ref()
        .is_some_and(|active| active == &device || active.is_within(&device))
    {
        browser.navigate(crate::model::Location::local(home_directory()));
    }
}

pub(super) fn visible_modal_layer(window: &impl IsA<gtk::Window>) -> Option<gtk::Widget> {
    topmost_modal_layer(window, &|_| false)
}

fn topmost_modal_layer(
    window: &impl IsA<gtk::Window>,
    skip: &dyn Fn(&gtk::Widget) -> bool,
) -> Option<gtk::Widget> {
    fn visible_layer(
        root: &gtk::Widget,
        skip: &dyn Fn(&gtk::Widget) -> bool,
    ) -> Option<gtk::Widget> {
        if !root.is_visible()
            || !root.is_child_visible()
            || root.opacity() == 0.0
            || root.has_css_class("tab-strip")
        {
            return None;
        }
        if root.has_css_class("app-modal-layer") {
            return (!skip(root)).then(|| root.clone());
        }
        let mut child = root.last_child();
        while let Some(widget) = child {
            child = widget.prev_sibling();
            if root.has_css_class("tab-context") && !widget.has_css_class("app-modal-layer") {
                continue;
            }
            if let Some(layer) = visible_layer(&widget, skip) {
                return Some(layer);
            }
        }
        None
    }
    let root = window.child()?;
    if root.has_css_class("tab-window") {
        visible_layer(&root, skip)
    } else {
        let mut child = root.last_child();
        while let Some(widget) = child {
            child = widget.prev_sibling();
            if widget.is_visible() && widget.has_css_class("app-modal-layer") && !skip(&widget) {
                return Some(widget);
            }
        }
        None
    }
}

/// True when the topmost visible modal layer is not `own`. An action that opens a
/// modal passes its own layer so it can still toggle that layer closed. A layer that
/// is animating out no longer owns input.
pub(super) fn foreign_modal_visible(
    window: &impl IsA<gtk::Window>,
    own: Option<&gtk::Widget>,
) -> bool {
    topmost_modal_layer(window, &crate::ui::modal::is_closing_layer)
        .is_some_and(|layer| own != Some(&layer))
}

pub(crate) fn crypto_password_uuid_for_volume(volume: &gio::Volume) -> Option<String> {
    let unix = gio_volume_unix_device(volume);
    let hint = unix
        .as_deref()
        .map(devices::block_crypto_hint)
        .unwrap_or_default();
    devices::crypto_password_uuid(
        volume.uuid().as_deref(),
        unix.as_deref(),
        hint.as_ref(),
        gio_volume_is_locked(volume),
    )
}

pub(crate) fn crypto_password_uuid_for_mount(mount: &gio::Mount) -> Option<String> {
    if let Some(volume) = mount.volume() {
        return crypto_password_uuid_for_volume(&volume);
    }
    let unix = mount
        .drive()
        .and_then(|drive| drive.identifier(gio::VOLUME_IDENTIFIER_KIND_UNIX_DEVICE.as_str()));
    let hint = unix
        .as_deref()
        .map(devices::block_crypto_hint)
        .unwrap_or_default();
    devices::crypto_password_uuid(None, unix.as_deref(), hint.as_ref(), false)
}

pub(crate) fn password_stop_drive(drive: Option<gio::Drive>) -> Option<gio::Drive> {
    drive.filter(|drive| {
        drive.start_stop_type() == gio::DriveStartStopType::Password && drive.can_stop()
    })
}

pub(crate) fn lock_cleartext_then_stop(
    mount: Option<gio::Mount>,
    drive: Option<gio::Drive>,
    parent: &gtk::Widget,
    browser: &Rc<Browser>,
    in_flight: &Rc<Cell<bool>>,
) {
    let key = match mount.as_ref() {
        Some(mount) => device_release::key_for_mount(mount),
        None => drive.as_ref().map(device_release::key_for_drive).unwrap_or(
            device_release::ReleaseKey {
                unix_device: None,
                uuid: None,
                mount_root: None,
                display_name: String::new(),
            },
        ),
    };
    let sync_root = mount.as_ref().and_then(|mount| mount.root().path());
    let away_root = mount.as_ref().map(gio::Mount::root);
    let in_flight_for_settle = in_flight.clone();
    let parent_for_stop = parent.clone();
    let key_for_stop = key.clone();
    let started = device_release::start_release(
        parent,
        Some(browser.clone()),
        key,
        device_release::ReleaseKind::Lock,
        false,
        sync_root,
        away_root,
        move || in_flight_for_settle.set(false),
        move |unmount_operation| async move {
            let unmount_result = match mount {
                Some(mount) => {
                    mount
                        .unmount_with_operation_future(
                            gio::MountUnmountFlags::NONE,
                            Some(&unmount_operation),
                        )
                        .await
                }
                None => Ok(()),
            };
            match unmount_result {
                Ok(()) => match drive {
                    Some(drive) => {
                        let stop_operation =
                            device_release::mount_operation(&parent_for_stop, &key_for_stop);
                        drive
                            .stop_future(gio::MountUnmountFlags::NONE, Some(&stop_operation))
                            .await
                    }
                    None => Ok(()),
                },
                Err(error) => Err(error),
            }
        },
    );
    if !started {
        in_flight.set(false);
    }
}

fn gio_icon_names(icon: &gio::Icon) -> Vec<String> {
    let mut names = Vec::new();
    collect_gio_icon_names(icon, &mut names);
    names
}

fn collect_gio_icon_names(icon: &gio::Icon, names: &mut Vec<String>) {
    if let Ok(themed) = icon.clone().downcast::<gio::ThemedIcon>() {
        names.extend(themed.names().iter().map(|name| name.to_string()));
        return;
    }
    if let Ok(emblemed) = icon.clone().downcast::<gio::EmblemedIcon>() {
        collect_gio_icon_names(&emblemed.icon(), names);
        for emblem in emblemed.emblems() {
            collect_gio_icon_names(&emblem.icon(), names);
        }
    }
}

fn gio_icons_are_encrypted(
    icon: &gio::Icon,
    symbolic: &gio::Icon,
    start_stop: Option<gio::DriveStartStopType>,
    unix_device: Option<&str>,
) -> bool {
    let mut names = gio_icon_names(symbolic);
    names.extend(gio_icon_names(icon));
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let hint = unix_device
        .map(devices::block_crypto_hint)
        .unwrap_or_default();
    devices::is_encrypted_device(&names, start_stop, hint.as_ref())
}

pub(super) fn gio_volume_unix_device(volume: &gio::Volume) -> Option<glib::GString> {
    volume.identifier(gio::VOLUME_IDENTIFIER_KIND_UNIX_DEVICE.as_str())
}

pub(crate) fn gio_volume_is_encrypted(volume: &gio::Volume) -> bool {
    gio_icons_are_encrypted(
        &volume.icon(),
        &volume.symbolic_icon(),
        volume.drive().map(|drive| drive.start_stop_type()),
        gio_volume_unix_device(volume).as_deref(),
    )
}

fn gio_volume_is_locked(volume: &gio::Volume) -> bool {
    let mut names = gio_icon_names(&volume.symbolic_icon());
    names.extend(gio_icon_names(&volume.icon()));
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    devices::encrypted_device_is_locked(&names, volume.get_mount().is_some())
}

pub(crate) fn gio_mount_is_encrypted(mount: &gio::Mount) -> bool {
    if let Some(volume) = mount.volume() {
        return gio_volume_is_encrypted(&volume);
    }
    gio_icons_are_encrypted(
        &mount.icon(),
        &mount.symbolic_icon(),
        mount.drive().map(|drive| drive.start_stop_type()),
        mount
            .drive()
            .and_then(|drive| drive.identifier(gio::VOLUME_IDENTIFIER_KIND_UNIX_DEVICE.as_str()))
            .as_deref(),
    )
}

fn gio_volume_identity(volume: &gio::Volume) -> Option<String> {
    devices::device_identity(
        volume
            .identifier(gio::VOLUME_IDENTIFIER_KIND_UNIX_DEVICE.as_str())
            .as_deref(),
        volume.uuid().as_deref(),
        volume
            .drive()
            .and_then(|drive| gio_drive_identity(&drive))
            .as_deref(),
    )
}

fn gio_drive_identity(drive: &gio::Drive) -> Option<String> {
    devices::device_identity(
        drive
            .identifier(gio::VOLUME_IDENTIFIER_KIND_UNIX_DEVICE.as_str())
            .as_deref(),
        drive
            .identifier(gio::VOLUME_IDENTIFIER_KIND_UUID.as_str())
            .as_deref(),
        None,
    )
}

pub(crate) fn orphaned_password_drives(volumes: &[gio::Volume], drives: &[gio::Drive]) -> Vec<gio::Drive> {
    let identities: Vec<_> = volumes.iter().filter_map(gio_volume_identity).collect();
    drives
        .iter()
        .filter(|drive| {
            devices::password_drive_is_orphaned(
                drive.start_stop_type(),
                gio_drive_identity(drive).as_deref(),
                &identities,
                volumes
                    .iter()
                    .any(|volume| volume.drive().as_ref() == Some(*drive)),
            )
        })
        .cloned()
        .collect()
}

pub(crate) fn home_directory() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}
