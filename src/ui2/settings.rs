// SPDX-License-Identifier: MIT

//! Settings as the Nyx settings card.

use gtk::prelude::*;
use nyx_ui::settings_card::{self, About, Spec, Standard};

use crate::ui::preferences::PreferenceManager;

use super::window::Window;

const GROUPS: &[(&str, &str, &str)] = &[];

const SHORTCUTS: &[(&[&str], &str)] = &[
    (&["Ctrl", "T"], "New tab"),
    (&["Ctrl", "W"], "Close tab"),
    (&["Ctrl", "Page Down"], "Next tab"),
    (&["Ctrl", "Page Up"], "Previous tab"),
    (&["Ctrl", "L"], "Type a location"),
    (&["Alt", "Left"], "Back"),
    (&["Alt", "Right"], "Forward"),
    (&["Alt", "Up"], "Enclosing folder"),
    (&["Ctrl", "B"], "Show or hide the sidebar"),
    (&["Ctrl", ","], "Settings"),
    (&["Ctrl", "Shift", "P"], "Command palette"),
    (&["Ctrl", "Q"], "Close the window"),
];

const SPEC: Spec = Spec {
    groups: GROUPS,
    search_text: None,
    standard: Some(Standard {
        app: "files",
        about: About {
            name: "Files",
            icon: crate::APPLICATION_ID,
            version: env!("CARGO_PKG_VERSION"),
            description: "A fast, keyboard-first file manager.",
            licence: "GPL-3.0-or-later",
            website: "https://github.com/design-nexus/nyx-files",
            issues: "https://github.com/design-nexus/nyx-files/issues",
            credits: &[],
        },
        shortcuts: SHORTCUTS,
        reduce_motion: || PreferenceManager::shared().reduce_motion(),
        set_reduce_motion: |on| PreferenceManager::shared().set_reduce_motion(on),
    }),
};

pub(super) fn open(window: &Window) {
    settings_card::open(&window.overlay, &window.window, SPEC, || {
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body.add_css_class("settings-page");
        let root = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&body)
            .vexpand(true)
            .build();
        (root.upcast(), body)
    });
}
