// SPDX-License-Identifier: MIT

//! Helpers the interface shares for items in a listing: opening and launching
//! them, their sizes and permissions, drag content and clipboard marks, folder
//! customization, leftover recovery, and operation summaries.

mod clipboard;
pub(crate) mod customization;
pub(crate) mod desktop;
pub(crate) mod entry;
pub(crate) mod leftovers;
pub(crate) mod progress;
pub(crate) mod properties;
pub(crate) mod transfer_report;

pub(crate) use crate::ui::browser::clipboard::{ClipboardMark, clear_active_drag, drag_preview_icon};
pub(in crate::ui) use crate::ui::browser::clipboard::file_drag_content;
pub(super) use crate::ui::browser::desktop::open_location_at;
pub(super) use crate::ui::browser::entry::{format_file_size, metadata_needs_fill};

/// The narrowest a preview or a column grows to when it is resized by hand.
pub(in crate::ui) const COLUMN_WIDTH: i32 = 300;

pub(in crate::ui) fn is_trash_location(location: &crate::model::Location) -> bool {
    location
        .uri_value()
        .is_some_and(|uri| uri.starts_with("trash:"))
}
