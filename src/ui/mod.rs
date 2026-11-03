// SPDX-License-Identifier: MIT

mod accessibility;
mod actions;
mod blur;
mod browser;
mod browser_modes;
mod chooser;
mod close_guard;
mod collection_edit;
mod collection_interaction;
mod controls;
mod decoration;
mod desktop_integration;
mod document_media;
mod document_view;
mod entry_list_model;
mod file_providers;
mod focus_navigation;
mod folder_picker;
mod frame;
mod icons_cell;
mod inline_search;
mod input_ownership;
mod jobs;
mod keys;
mod loading_skeleton;
mod marquee;
mod media;
mod missing_tools;
mod modal;
mod motion;
mod notice;
mod open_with;
mod pointer;
mod portal_preferences;
pub(crate) mod preferences;
mod presence;
mod preview;
mod progress_dock;
mod raw_details;
mod recent_apps;
mod resize_feedback;
mod scrolling;
mod search;
mod search_session;
mod settings;
mod shortcut_footer;
mod shortcut_reference;
mod sidebar_toggle;
mod special_folders;
mod table_view;
mod tabs_session;
mod tenxer_mode;
mod terminal;
mod theme;
mod thumbnail;
mod top_bar_navigation;
mod udiskie_preferences;
mod virtual_preview;
mod window;

#[cfg(feature = "qa")]
pub(crate) use accessibility::qa as accessibility_qa;
#[cfg(test)]
pub(in crate::ui) use window::offer_removable_destinations_for_test;
pub(in crate::ui) use window::{
    RemovableDestination, RemovableWatch, removable_destination_root, removable_destinations,
    send_to_recent_folders, watch_removable_destinations,
};
pub use window::{UnlockTarget, present, present_open, present_reveal, present_unlock};
#[cfg(feature = "qa")]
pub(crate) use {inline_search::SEARCH_RESULTS_LABEL, preview::PREVIEW_LABEL};

pub(crate) fn prepare_portal_ui() {
    let _theme = theme::ThemeManager::shared();
}

pub(crate) static CHOOSER_UI: crate::portal::ChooserUi = crate::portal::ChooserUi {
    prepare: prepare_portal_ui,
    present: |request, cancelled, completion| {
        chooser::present_chooser(request, cancelled, completion);
    },
    cancel: chooser::cancel_chooser,
    flush: preferences::flush_pending_folder_views,
};

pub(crate) fn read_install_facts() {
    let _startup = crate::util::ui_thread::allow(crate::util::ui_thread::Exception::Startup);
    open_with::uses_portal();
}
