// SPDX-License-Identifier: MIT

//! What the interface builds on: preferences, key bindings, the preview
//! drawer, thumbnails, the shortcut footer and reference, custom actions,
//! device and drive helpers, and the theme colours the previews use.

mod accessibility;
pub(crate) mod actions;
pub(crate) mod browser;
pub(crate) mod browser_modes;
pub(crate) mod chooser;
mod close_guard;
mod collection_interaction;
mod controls;
mod desktop_integration;
mod document_media;
mod document_view;
pub(crate) mod file_icons;
pub(crate) mod file_providers;
mod focus_navigation;
mod frame;
mod icons_cell;
pub(crate) mod jobs;
pub(crate) mod keys;
mod media;
mod modal;
mod motion;
mod notice;
pub(crate) mod open_with;
mod pointer;
pub(crate) mod portal_preferences;
pub(crate) mod preferences;
pub(crate) mod presence;
pub(crate) mod preview;
pub(crate) mod raw_details;
pub(crate) mod recent_apps;
mod resize_feedback;
mod scrolling;
pub(crate) mod search_session;
pub(crate) mod settings;
pub(crate) mod shortcut_footer;
pub(crate) mod shortcut_reference;
mod table_view;
pub(crate) mod tabs_session;
pub(crate) mod tenxer_mode;
mod terminal;
pub(crate) mod theme;
pub(crate) mod thumbnail;
pub(crate) mod udiskie_preferences;
mod virtual_preview;
pub(crate) mod window;

#[cfg(test)]
pub(in crate::ui) use window::offer_removable_destinations_for_test;
pub use window::{UnlockTarget};

/// The accessible name of the filter and search results list.
pub(crate) const SEARCH_RESULTS_LABEL: &str = "Search results";

/// A themed symbolic icon at `size` px; it takes the colour of its widget's text.
pub(crate) fn symbolic(name: &str, size: i32) -> gtk::Image {
    let image = gtk::Image::from_icon_name(name);
    image.set_pixel_size(size);
    image
}

pub(crate) fn read_install_facts() {
    let _startup = crate::util::ui_thread::allow(crate::util::ui_thread::Exception::Startup);
    open_with::uses_portal();
}
