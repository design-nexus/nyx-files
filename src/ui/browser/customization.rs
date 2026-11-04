// SPDX-License-Identifier: MIT

use crate::model::{FolderColor, FolderColorValue, Location};
use crate::services::ItemIdentity;
use crate::ui::controls::{focus_button, modal_layout};
use crate::ui::keys::{self, bindings::ESCAPE_ANY};
use crate::ui::modal::{
    ModalHost, dismiss_modal_layer, dismiss_modal_layer_then, modal_layer,
    modal_layer_with_backdrop, remember_modal_focus,
};
use crate::ui::preferences::PreferenceManager;
use gtk::prelude::*;
use gtk::{gdk, glib};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

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





