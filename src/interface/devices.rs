// SPDX-License-Identifier: MIT

//! Ejecting and unmounting through Files' safe-removal flow, which writes
//! pending data first and moves windows away from the device.

use gtk::{gio, glib, prelude::*};

use crate::{
    adapters::{mount_can_unplug, volume_can_unplug},
    ui::window::device_release::{self, ReleaseKind},
};

use super::sidebar::Target;

#[derive(Clone, Copy)]
enum Action {
    EjectVolume,
    EjectMount,
    Unmount,
}

fn choose(volume: Option<&gio::Volume>, mount: Option<&gio::Mount>) -> Option<Action> {
    if volume.is_some_and(|v| v.can_eject()) {
        Some(Action::EjectVolume)
    } else if mount.is_some_and(|m| m.can_eject()) {
        Some(Action::EjectMount)
    } else if mount.is_some_and(|m| m.can_unmount()) {
        Some(Action::Unmount)
    } else {
        None
    }
}

/// The menu label for what releasing this device does, if anything.
pub(super) fn release_label(target: &Target) -> Option<&'static str> {
    let action = match target {
        Target::Volume(v) => choose(Some(v), v.get_mount().as_ref()),
        Target::Mount(m) => choose(None, Some(m)),
        Target::Drive(d) => d.can_eject().then_some(Action::EjectVolume),
        _ => None,
    }?;
    Some(match action {
        Action::EjectVolume | Action::EjectMount => "Eject",
        Action::Unmount => "Unmount",
    })
}

pub(super) fn toast(message: &str) {
    if let Some(w) = super::window::focused() {
        nyx_ui::toast::show(&w.overlay, message);
    }
}

pub(super) fn release(target: &Target, anchor: &gtk::Widget) {
    if let Target::Drive(drive) = target {
        let key = device_release::key_for_drive(drive);
        let drive = drive.clone();
        device_release::start_release(
            anchor,
            None,
            key,
            ReleaseKind::Eject,
            true,
            None,
            None,
            || {},
            move |operation| async move {
                drive
                    .eject_with_operation_future(gio::MountUnmountFlags::NONE, Some(&operation))
                    .await
            },
        );
        return;
    }
    let (volume, mount) = match target {
        Target::Volume(v) => (Some(v.clone()), v.get_mount()),
        Target::Mount(m) => (None, Some(m.clone())),
        _ => return,
    };
    let Some(action) = choose(volume.as_ref(), mount.as_ref()) else {
        return;
    };
    let key = match (&volume, &mount) {
        (Some(v), _) => device_release::key_for_volume(v),
        (None, Some(m)) => device_release::key_for_mount(m),
        _ => return,
    };
    let unplug = match action {
        Action::EjectVolume | Action::EjectMount => true,
        Action::Unmount => {
            mount.as_ref().is_some_and(mount_can_unplug)
                || volume.as_ref().is_some_and(volume_can_unplug)
        }
    };
    let kind = match action {
        Action::Unmount => ReleaseKind::Unmount,
        _ => ReleaseKind::Eject,
    };
    let sync_root = mount.as_ref().and_then(|m| m.root().path());
    let away_root = mount.as_ref().map(gio::Mount::root);
    let browser = super::window::focused().and_then(|w| w.current_browser());
    device_release::start_release(
        anchor,
        browser,
        key,
        kind,
        unplug,
        sync_root,
        away_root,
        || {},
        move |operation| async move {
            let flags = gio::MountUnmountFlags::NONE;
            match action {
                Action::EjectVolume => match volume {
                    Some(v) => v.eject_with_operation_future(flags, Some(&operation)).await,
                    None => Ok(()),
                },
                Action::EjectMount => match mount {
                    Some(m) => m.eject_with_operation_future(flags, Some(&operation)).await,
                    None => Ok(()),
                },
                Action::Unmount => match mount {
                    Some(m) => {
                        m.unmount_with_operation_future(flags, Some(&operation))
                            .await
                    }
                    None => Err(glib::Error::new(
                        gio::IOErrorEnum::Failed,
                        &crate::i18n::tr("The volume is not mounted."),
                    )),
                },
            }
        },
    );
}
