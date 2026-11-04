// SPDX-License-Identifier: MIT

//! Properties: where an item is, its size (measured for folders), dates,
//! what opens it, media details and its permissions, which can be changed.

use std::{cell::Cell, rc::Rc, time::Duration};

use gtk::{gio, glib, prelude::*};
use nyx_ui::widgets;

use crate::{
    adapters::directory_summary::summarize_directory_with_progress,
    model::FileEntry,
    ui::browser::properties::{
        directory_counts_label, format_permissions, measurement_warning_text, media,
    },
};

use super::list::{entry_icon, kind};

fn row(parent: &gtk::Box, title: &str, value: &str) -> gtk::Label {
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    line.add_css_class("properties-row");
    let key = widgets::label(&crate::i18n::tr(title), "properties-row-label");
    key.set_width_chars(12);
    key.set_valign(gtk::Align::Start);
    let label = widgets::label(value, "properties-row-value");
    label.set_hexpand(true);
    label.set_selectable(true);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    line.append(&key);
    line.append(&label);
    parent.append(&line);
    label
}

pub(super) fn show(entry: &FileEntry) {
    let (dialog, card) = widgets::dialog(&crate::i18n::tr("Properties"), 460);
    card.add_css_class("properties-card");

    let head = widgets::hbox(14);
    let icon = gtk::Image::from_gicon(&entry_icon(entry));
    icon.set_pixel_size(56);
    head.append(&icon);
    let names = gtk::Box::new(gtk::Orientation::Vertical, 2);
    names.set_valign(gtk::Align::Center);
    let name = widgets::label(&entry.display_name, "properties-name");
    name.set_wrap(true);
    name.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    names.append(&name);
    names.append(&widgets::label(&kind(entry), "dim"));
    head.append(&names);
    card.append(&head);

    let details = gtk::Box::new(gtk::Orientation::Vertical, 6);
    details.add_css_class("properties-details");
    card.append(&details);
    let folder = entry
        .location
        .parent()
        .map(|p| p.display_path())
        .unwrap_or_default();
    row(&details, "Location", &folder);
    let size = row(&details, "Size", "—");
    let file = entry
        .location
        .native_path()
        .map(gio::File::for_path)
        .or_else(|| entry.location.uri_value().map(gio::File::for_uri));
    let mut keep = Vec::new();
    if entry.is_directory() {
        let contains = row(&details, "Contains", "—");
        let warning = widgets::label("", "dim");
        warning.set_wrap(true);
        warning.set_visible(false);
        details.append(&warning);
        if let Some(file) = file.clone() {
            size.set_text(&crate::i18n::tr("Calculating…"));
            let (size, contains, warning) =
                (size.downgrade(), contains.downgrade(), warning.downgrade());
            glib::MainContext::default().spawn_local(async move {
                let last = Rc::new(Cell::new(std::time::Instant::now()));
                let (s, c) = (size.clone(), contains.clone());
                let result = summarize_directory_with_progress(&file, move |partial| {
                    if last.get().elapsed() < Duration::from_millis(150) {
                        return;
                    }
                    last.set(std::time::Instant::now());
                    if let Some(s) = s.upgrade() {
                        s.set_text(&format!("≥ {}", glib::format_size(partial.total_size)));
                    }
                    if let Some(c) = c.upgrade() {
                        c.set_text(&directory_counts_label(&partial));
                    }
                })
                .await;
                let (Some(size), Some(contains)) = (size.upgrade(), contains.upgrade()) else {
                    return;
                };
                match result {
                    Ok(summary) => {
                        let prefix = if summary.truncated() { "≥ " } else { "" };
                        size.set_text(&format!(
                            "{prefix}{}",
                            glib::format_size(summary.total_size)
                        ));
                        contains.set_text(&directory_counts_label(&summary));
                        if let (Some(w), Some(text)) =
                            (warning.upgrade(), measurement_warning_text(&summary))
                        {
                            w.set_text(&text);
                            w.set_visible(true);
                        }
                    }
                    Err(_) => {
                        size.set_text(&crate::i18n::tr("Unavailable"));
                        contains.set_text(&crate::i18n::tr("Unavailable"));
                    }
                }
            });
        }
    } else if let crate::model::MetadataValue::Known(bytes) = entry.size {
        size.set_text(&format!("{} ({} bytes)", glib::format_size(bytes), bytes));
    }
    let modified = row(&details, "Modified", "—");
    crate::util::set_full_modified_date(
        &modified,
        match entry.modified_unix_seconds {
            crate::model::MetadataValue::Known(seconds) => Some(seconds),
            _ => None,
        },
    );
    if !entry.is_directory() {
        let (content_type, _) = gio::content_type_guess(Some(&entry.native_name), None::<&[u8]>);
        let app = gio::AppInfo::default_for_type(&content_type, false)
            .map(|a| a.display_name().to_string());
        row(
            &details,
            "Opens with",
            &app.unwrap_or_else(|| crate::i18n::tr("No app")),
        );
    }
    row(
        &details,
        "Hidden",
        &crate::i18n::tr(if entry.is_hidden { "Yes" } else { "No" }),
    );

    if let Some(path) = entry
        .location
        .native_path()
        .filter(|_| !entry.is_directory())
    {
        let section = gtk::Box::new(gtk::Orientation::Vertical, 6);
        section.add_css_class("properties-details");
        card.append(&section);
        keep.push(media::load(&section, path.to_path_buf()));
    }
    if let Some(file) = file.filter(|_| entry.location.native_path().is_some()) {
        card.append(&permissions(&file, entry.is_directory()));
    }

    let buttons = widgets::hbox(8);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_top(6);
    let close = gtk::Button::with_label(&crate::i18n::tr("Close"));
    buttons.append(&close);
    card.append(&buttons);
    let d = dialog.clone();
    close.connect_clicked(move |_| d.close());
    let held = std::cell::RefCell::new(Some(keep));
    dialog.connect_close_request(move |_| {
        held.borrow_mut().take();
        glib::Propagation::Proceed
    });
    dialog.present();
    close.grab_focus();
}

/// Owner, group and others as r/w/x toggles, plus "executable" for files;
/// each change is a chmod, undone on screen if it fails.
fn permissions(file: &gio::File, directory: bool) -> gtk::Box {
    let section = gtk::Box::new(gtk::Orientation::Vertical, 6);
    section.add_css_class("properties-details");
    let mode_label = row(&section, "Permissions", "—");
    let mode = Rc::new(Cell::new(None::<u32>));
    let mut toggles: Vec<(gtk::ToggleButton, u32)> = Vec::new();
    let owner = widgets::label("", "dim");
    let group = widgets::label("", "dim");
    for (title, shift, identity) in [
        ("Owner", 6, Some(&owner)),
        ("Group", 3, Some(&group)),
        ("Others", 0, None),
    ] {
        let line = widgets::hbox(12);
        line.add_css_class("properties-row");
        let key = widgets::label(&crate::i18n::tr(title), "properties-row-label");
        key.set_width_chars(12);
        line.append(&key);
        let who = identity
            .cloned()
            .unwrap_or_else(|| widgets::label("", "dim"));
        who.set_hexpand(true);
        line.append(&who);
        let bits = widgets::hbox(4);
        line.append(&bits);
        for (letter, bit) in [("r", 0o4), ("w", 0o2), ("x", 0o1)] {
            let toggle = gtk::ToggleButton::with_label(letter);
            toggle.add_css_class("chip");
            toggle.add_css_class("permission-bit");
            toggle.set_sensitive(false);
            bits.append(&toggle);
            toggles.push((toggle, bit << shift));
        }
        section.append(&line);
    }
    let executable = (!directory).then(|| {
        let (r, switch) = widgets::switch_row(
            &crate::i18n::tr("Allow executing file as a program"),
            "",
            false,
            |_| {},
        );
        switch.set_sensitive(false);
        section.append(&r);
        switch
    });
    let syncing = Rc::new(Cell::new(false));
    let show = {
        let (toggles, mode_label, executable, syncing) = (
            toggles.clone(),
            mode_label.clone(),
            executable.clone(),
            syncing.clone(),
        );
        Rc::new(move |m: u32| {
            syncing.set(true);
            mode_label.set_text(&format_permissions(m));
            for (toggle, mask) in &toggles {
                toggle.set_active(m & mask != 0);
                toggle.set_sensitive(true);
            }
            if let Some(e) = &executable {
                e.set_active(m & 0o111 != 0);
                e.set_sensitive(true);
            }
            syncing.set(false);
        })
    };
    let apply = {
        let (file, mode, show, section) = (
            file.clone(),
            mode.clone(),
            show.clone(),
            section.downgrade(),
        );
        Rc::new(move |requested: u32| {
            let Some(previous) = mode.replace(Some(requested)) else {
                return;
            };
            show(requested);
            let info = gio::FileInfo::new();
            info.set_attribute_uint32("unix::mode", requested & 0o7777);
            let (file, mode, show, section) =
                (file.clone(), mode.clone(), show.clone(), section.clone());
            glib::MainContext::default().spawn_local(async move {
                if let Err(error) = file
                    .set_attributes_future(
                        &info,
                        gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
                        glib::Priority::DEFAULT,
                    )
                    .await
                {
                    mode.set(Some(previous));
                    show(previous);
                    tracing::warn!(%error, "unable to change file permissions");
                    if let Some(section) = section.upgrade() {
                        section.append(&widgets::banner(
                            &glib::markup_escape_text(&error.to_string()),
                            true,
                        ));
                    }
                }
            });
        })
    };
    for (toggle, mask) in &toggles {
        let (mode, apply, syncing, mask) = (mode.clone(), apply.clone(), syncing.clone(), *mask);
        toggle.connect_toggled(move |_| {
            if !syncing.get()
                && let Some(m) = mode.get()
            {
                apply(m ^ mask);
            }
        });
    }
    if let Some(e) = &executable {
        let (mode, apply, syncing) = (mode.clone(), apply.clone(), syncing.clone());
        e.connect_active_notify(move |s| {
            if !syncing.get()
                && let Some(m) = mode.get()
            {
                apply(if s.is_active() { m | 0o111 } else { m & !0o111 });
            }
        });
    }
    let file = file.clone();
    glib::MainContext::default().spawn_local(async move {
        let Ok(info) = file
            .query_info_future(
                "unix::mode,owner::user,owner::group",
                gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
                glib::Priority::DEFAULT,
            )
            .await
        else {
            return;
        };
        owner.set_text(&info.attribute_string("owner::user").unwrap_or_default());
        group.set_text(&info.attribute_string("owner::group").unwrap_or_default());
        let m = info.attribute_uint32("unix::mode");
        mode.set(Some(m));
        show(m);
    });
    section
}
