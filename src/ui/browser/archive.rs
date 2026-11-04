// SPDX-License-Identifier: MIT

//! Compress and extract dialogs for the browser view.
//!
//! These methods live on [`ViewState`] and start work through the shared
//! [`crate::app::Browser`] controller; they must not create a separate archive
//! pipeline. Password-capable extracts record a retry so a later failure can
//! reopen [`ViewState::show_extract_password_dialog`].
//!
//! # Entry points
//!
//! - [`ViewState::show_compress_dialog`]
//! - [`ViewState::extract_entry`]
//! - [`ViewState::show_extract_to_dialog`]
//! - [`ViewState::show_extract_password_dialog`]

use crate::adapters::probes::{self, Presence};
use crate::model::{FileEntry, Location};
use crate::services::{ArchiveFormat, PasswordFailure, TransferConflict, validate_basename};
use crate::ui::browser::ViewState;
use crate::ui::controls::{
    ModalTone, focus_button, form_entry, form_error_label, form_password_entry,
    message_dialog_description, message_dialog_layout, modal_layout, segmented_control,
    set_form_field_error,
};
use crate::ui::keys::{
    self,
    bindings::{ENTER_ANY, ESCAPE_ANY},
};
use crate::ui::modal::{
    ModalHost, close_on_escape, dismiss_modal_layer, modal_layer, remember_modal_focus,
    show_error_dialog, submit_on_enter,
};
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::Cell;
use std::ffi::OsStr;
use std::path::Path;
use std::rc::Rc;




impl ViewState {







}


