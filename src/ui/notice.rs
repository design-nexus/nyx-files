// SPDX-License-Identifier: MIT
use gtk::prelude::*;

/// A brief confirmation as a toast over the window (or `overlay` when it is
/// not in a window), replacing any toast showing.
pub(super) fn show_notice(overlay: &gtk::Overlay, heading: Option<&str>, text: &str) {
    let overlay = super::modal::window_overlay(overlay).unwrap_or_else(|| overlay.clone());
    let message = match heading {
        Some(heading) => format!("{heading}: {text}"),
        None => text.to_owned(),
    };
    nyx_ui::toast::show(&overlay, &message);
}
