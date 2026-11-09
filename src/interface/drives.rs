// SPDX-License-Identifier: MIT

//! Device dialogs: Properties, the label Files shows, and Format for
//! removable drives.

use std::rc::Rc;

use gtk::{gio, glib, prelude::*};
use nyx_ui::{toast, widgets};

use crate::ui::{
    preferences::PreferenceManager,
    window::{
        drive_dialogs::PropertiesTarget,
        drive_ops::{self, FilesystemType},
    },
};

use super::sidebar::Target;

fn properties_target(target: &Target) -> Option<PropertiesTarget> {
    match target {
        Target::Volume(v) => Some(PropertiesTarget::Volume(v.clone())),
        Target::Mount(m) => Some(PropertiesTarget::Mount(m.clone())),
        _ => None,
    }
}

fn name_of(target: &Target) -> String {
    match target {
        Target::Volume(v) => v.name().to_string(),
        Target::Mount(m) => m.name().to_string(),
        _ => String::new(),
    }
}

fn row(parent: &gtk::Box, title: &str, value: &str) {
    let line = widgets::hbox(12);
    line.add_css_class("properties-row");
    let key = widgets::label(&crate::i18n::tr(title), "properties-row-label");
    key.set_width_chars(12);
    let label = widgets::label(value, "properties-row-value");
    label.set_selectable(true);
    label.set_hexpand(true);
    label.set_wrap(true);
    line.append(&key);
    line.append(&label);
    parent.append(&line);
}

pub(super) fn show_properties(target: &Target) {
    let Some(props) = properties_target(target) else {
        return;
    };
    let name = name_of(target);
    let (dialog, card) = widgets::dialog(&name, 440);
    card.add_css_class("properties-card");
    let details = gtk::Box::new(gtk::Orientation::Vertical, 6);
    details.add_css_class("properties-details");
    card.append(&details);
    let volume = props.volume();
    let root = match target {
        Target::Volume(v) => v.get_mount().map(|m| m.root()),
        Target::Mount(m) => Some(m.root()),
        _ => None,
    };
    let path = root.as_ref().and_then(gio::File::path);
    let device = volume
        .as_ref()
        .and_then(drive_ops::block_device_for_volume)
        .or_else(|| path.as_deref().and_then(drive_ops::block_device_for_path));
    if let Some(path) = &path {
        row(&details, "Mounted at", &path.display().to_string());
    } else if let Some(uri) = root.as_ref().map(|r| r.uri()) {
        row(&details, "Address", &uri);
    } else {
        row(&details, "Mounted at", &crate::i18n::tr("Not mounted"));
    }
    if let Some(device) = &device {
        row(&details, "Device", &device.display().to_string());
        if let Some(label) = drive_ops::filesystem_label_for_device(device) {
            row(&details, "Filesystem label", &label);
        }
        if let Some(bytes) = drive_ops::device_size_bytes(device) {
            row(&details, "Capacity", &glib::format_size(bytes));
        }
    }
    if let Some((total, free)) = path.as_deref().and_then(drive_ops::usage_for_path) {
        let used = total.saturating_sub(free);
        row(
            &details,
            "Used",
            &format!(
                "{} of {} · {} free",
                glib::format_size(used),
                glib::format_size(total),
                glib::format_size(free)
            ),
        );
        let bar = widgets::usage_bar();
        if total > 0 {
            bar.set_value(used as f64 / total as f64);
        }
        details.append(&bar);
    }
    let buttons = widgets::hbox(8);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_top(6);
    let close = gtk::Button::with_label(&crate::i18n::tr("Close"));
    buttons.append(&close);
    card.append(&buttons);
    let d = dialog.clone();
    close.connect_clicked(move |_| d.close());
    dialog.present();
    close.grab_focus();
}

/// The name Files shows for a device; empty restores the system's name.
pub(super) fn set_label(target: &Target) {
    let Some(id) = properties_target(target).and_then(|t| t.label_id()) else {
        return;
    };
    let current = PreferenceManager::shared()
        .device_label(&id)
        .unwrap_or_else(|| name_of(target));
    widgets::ask_text(
        &crate::i18n::tr("Set label"),
        &crate::i18n::tr(
            "The name Files shows for this device. Leave it as the system name to keep that.",
        ),
        &current,
        &crate::i18n::tr("Save label"),
        move |label| PreferenceManager::shared().set_device_label(&id, &label),
    );
}

pub(super) fn can_format(target: &Target) -> bool {
    matches!(target, Target::Volume(v) if drive_ops::is_eligible(Some(v)))
}

const FILESYSTEMS: [FilesystemType; 3] = [
    FilesystemType::Exfat,
    FilesystemType::Ntfs,
    FilesystemType::Fat32,
];

/// Erases a removable drive with a new filesystem; the button needs two clicks.
pub(super) fn format(target: &Target, overlay: &gtk::Overlay) {
    let Target::Volume(volume) = target else {
        return;
    };
    let available: Vec<FilesystemType> = FILESYSTEMS
        .into_iter()
        .filter(|fs| fs.available())
        .collect();
    let name = volume.name().to_string();
    let (dialog, card) = widgets::dialog(&format!("{} {}", crate::i18n::tr("Format"), name), 460);
    card.append(&widgets::banner(
        &glib::markup_escape_text(&crate::i18n::tr("Everything on this drive will be erased.")),
        true,
    ));
    if available.is_empty() {
        let tools: Vec<&str> = FILESYSTEMS.iter().map(|fs| fs.format_tool_name()).collect();
        let note = widgets::label(
            &format!(
                "{} {}",
                crate::i18n::tr("Install a formatting tool:"),
                tools.join(", ")
            ),
            "dim",
        );
        note.set_wrap(true);
        card.append(&note);
    }
    let chosen = Rc::new(std::cell::Cell::new(
        available.first().copied().unwrap_or(FilesystemType::Exfat),
    ));
    let options: Vec<(String, String)> = available
        .iter()
        .map(|fs| (fs.label().to_string(), fs.label().to_string()))
        .collect();
    let c = chosen.clone();
    let list = available.clone();
    let switch = widgets::segmented(&options, chosen.get().label(), move |id| {
        if let Some(fs) = list.iter().find(|fs| fs.label() == id) {
            c.set(*fs);
        }
    });
    card.append(&widgets::row(
        &crate::i18n::tr("Filesystem"),
        "",
        Some(switch.upcast_ref()),
    ));
    let label = gtk::Entry::new();
    label.set_placeholder_text(Some(&crate::i18n::tr("Label")));
    card.append(&label);
    let error = widgets::label("", "error-text");
    error.set_wrap(true);
    error.set_visible(false);
    card.append(&error);
    let (quick_row, quick) = widgets::switch_row(
        &crate::i18n::tr("Quick format"),
        &crate::i18n::tr("Off, the whole drive is overwritten first, which takes much longer."),
        true,
        |_| {},
    );
    card.append(&quick_row);
    let buttons = widgets::hbox(8);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_top(6);
    let spinner = gtk::Spinner::new();
    buttons.append(&spinner);
    let cancel = gtk::Button::with_label(&crate::i18n::tr("Cancel"));
    buttons.append(&cancel);
    let (d, v, l, e, s, o) = (
        dialog.clone(),
        volume.clone(),
        label.clone(),
        error.clone(),
        spinner.clone(),
        overlay.clone(),
    );
    let running = Rc::new(std::cell::Cell::new(false));
    let form = card.clone();
    let format_button = widgets::two_click(
        &crate::i18n::tr("Format"),
        &crate::i18n::tr("Erase and format"),
        move || {
            if running.get() {
                return;
            }
            let fs = chosen.get();
            let text = l.text().to_string();
            if text.chars().count() > fs.max_label_len() {
                e.set_text(&crate::i18n::tr("Filesystem label is too long."));
                e.set_visible(true);
                return;
            }
            if let Some(message) = fs.label_character_error(&text) {
                e.set_text(&message);
                e.set_visible(true);
                return;
            }
            e.set_visible(false);
            running.set(true);
            form.set_sensitive(false);
            s.start();
            let (d, v, s, o, q) = (
                d.clone(),
                v.clone(),
                s.clone(),
                o.clone(),
                quick.is_active(),
            );
            glib::MainContext::default().spawn_local(async move {
                let result = drive_ops::format_volume(d.clone().upcast(), v, fs, text, q).await;
                s.stop();
                d.close();
                toast::show(
                    &o,
                    &match result {
                        Ok(()) => crate::i18n::tr("Drive formatted"),
                        Err(error) => error.user_message(),
                    },
                );
            });
        },
    );
    format_button.set_sensitive(!available.is_empty());
    buttons.append(&format_button);
    card.append(&buttons);
    let d = dialog.clone();
    cancel.connect_clicked(move |_| d.close());
    dialog.present();
    cancel.grab_focus();
}
