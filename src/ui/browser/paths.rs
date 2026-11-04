// SPDX-License-Identifier: MIT

use crate::model::{FileEntry, Location};
use crate::ui::browser::PinStatus;
use gtk::glib;
use std::path::Path;






pub(in crate::ui) fn is_trash_location(location: &Location) -> bool {
    location
        .uri_value()
        .is_some_and(|uri| uri.starts_with("trash:"))
}




