// SPDX-License-Identifier: MIT

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use gtk::{glib, prelude::*};

use super::{UdiskieIntegrationStatus, settings_row};
use crate::portal_setup::{FileManagerStatus, PortalStatus, udiskie::UdiskieStatus};
use crate::test_support::{gtk_test, gtk_test_with_env};
use crate::ui::desktop_integration::SETUP_RUNNING;
use crate::ui::portal_preferences::SettingsIntegrationStatus;

fn descendants<T: IsA<gtk::Widget> + Clone>(root: &gtk::Widget) -> Vec<T> {
    let mut widgets = Vec::new();
    if let Ok(widget) = root.clone().downcast::<T>() {
        widgets.push(widget);
    }
    let mut child = root.first_child();
    while let Some(widget) = child {
        widgets.extend(descendants::<T>(&widget));
        child = widget.next_sibling();
    }
    widgets
}

fn summary_on(row: &gtk::Box) -> std::rc::Rc<UdiskieIntegrationStatus> {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
    row.append(&content);
    UdiskieIntegrationStatus::new(&content, row)
}

fn udiskie_paths() -> (PathBuf, PathBuf) {
    (
        glib::user_config_dir().join("udiskie/config.yml"),
        glib::user_data_dir().join("strata/udiskie-install/state.toml"),
    )
}

fn seed_omarchy() {
    let directory = glib::home_dir().join(".local/share/omarchy");
    fs::create_dir_all(&directory).expect("Omarchy version directory");
    fs::write(directory.join("version"), "4.0\n").expect("Omarchy version");
}

fn write_udiskie_stub(bin: &Path) {
    let path = bin.join("udiskie");
    fs::write(&path, "#!/bin/sh\nexit 0\n").expect("udiskie stub");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("executable udiskie");
}

fn wait_until(predicate: impl Fn() -> bool, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let context = glib::MainContext::default();
    while !predicate() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        context.iteration(true);
    }
}

fn has_action_label(row: &gtk::Box, label: &str) -> bool {
    descendants::<gtk::Button>(row.upcast_ref())
        .iter()
        .any(|button| button.label().as_deref() == Some(label))
}




