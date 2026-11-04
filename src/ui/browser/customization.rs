// SPDX-License-Identifier: MIT

use crate::model::Location;
use crate::services::ItemIdentity;
use crate::ui::preferences::PreferenceManager;

/// Item customizations are keyed by native path; remote locations carry none.
pub(crate) fn relocate_item_customizations(from: &Location, to: &Location, merged: bool) {
    if let Some(from) = from.native_path() {
        PreferenceManager::shared().relocate_item_customizations(from, to.native_path(), merged);
    }
}

pub(crate) fn forget_item_customizations(
    location: &Location,
    trash_identity: Option<ItemIdentity>,
) {
    if let Some(path) = location.native_path() {
        PreferenceManager::shared().forget_item_customizations(path, trash_identity);
    }
}

pub(crate) fn restore_item_customizations(location: &Location, identity: Option<ItemIdentity>) {
    if let (Some(path), Some(identity)) = (location.native_path(), identity) {
        PreferenceManager::shared().restore_item_customizations(path, identity);
    }
}





