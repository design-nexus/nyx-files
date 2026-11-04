// SPDX-License-Identifier: MIT

use std::rc::Rc;

use gtk::prelude::*;

use crate::{
    assets::icons,
    model::{SortDirection, SortKey},
    sandbox::MediaPreviewBackend,
    services::CrossVolumeDropStrategy,
    ui::{
        browser_modes::{BrowserMode, ClickActivation, ClickCount},
        controls::{segmented_control},
        preferences::PreferenceManager,
    },
};

use super::{
    ResponsiveActivationRow, append_heading,
    bindings::{bind_switch},
    page_content, scrollable_page, settings_option,
};
















pub(crate) fn abbreviate_home(path: &std::path::Path) -> String {
    let home = glib::home_dir();
    if let Ok(rest) = path.strip_prefix(&home) {
        format!("~/{}", rest.display())
    } else {
        path.display().to_string()
    }
}














