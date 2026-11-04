// SPDX-License-Identifier: MIT

mod accessibility;
pub(crate) mod actions;
mod blur;
pub(crate) mod browser;
pub(crate) mod browser_modes;
pub(crate) mod chooser;
mod close_guard;
mod collection_edit;
mod collection_interaction;
mod controls;
mod decoration;
mod desktop_integration;
mod document_media;
mod document_view;
mod entry_list_model;
pub(crate) mod file_providers;
mod focus_navigation;
mod folder_picker;
mod frame;
mod icons_cell;
mod inline_search;
mod input_ownership;
pub(crate) mod jobs;
pub(crate) mod keys;
mod loading_skeleton;
mod marquee;
mod media;
mod missing_tools;
mod modal;
mod motion;
mod notice;
pub(crate) mod open_with;
mod pointer;
pub(crate) mod portal_preferences;
pub(crate) mod preferences;
pub(crate) mod presence;
pub(crate) mod preview;
mod progress_dock;
pub(crate) mod raw_details;
pub(crate) mod recent_apps;
mod resize_feedback;
mod scrolling;
mod search;
pub(crate) mod search_session;
pub(crate) mod settings;
pub(crate) mod shortcut_footer;
pub(crate) mod shortcut_reference;
mod sidebar_toggle;
mod special_folders;
mod table_view;
pub(crate) mod tabs_session;
pub(crate) mod tenxer_mode;
mod terminal;
pub(crate) mod theme;
pub(crate) mod thumbnail;
mod top_bar_navigation;
pub(crate) mod udiskie_preferences;
mod virtual_preview;
pub(crate) mod window;

#[cfg(feature = "qa")]
pub(crate) use accessibility::qa as accessibility_qa;
#[cfg(test)]
pub(in crate::ui) use window::offer_removable_destinations_for_test;
pub use window::{UnlockTarget};
#[cfg(feature = "qa")]
pub(crate) use preview::PREVIEW_LABEL;

/// The accessible name of the filter and search results list.
pub(crate) const SEARCH_RESULTS_LABEL: &str = "Search results";



pub(crate) fn read_install_facts() {
    let _startup = crate::util::ui_thread::allow(crate::util::ui_thread::Exception::Startup);
    open_with::uses_portal();
}
