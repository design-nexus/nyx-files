// SPDX-License-Identifier: MIT

//! Connect-to-server dialog, which stays open until the location opens or fails.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::prelude::*;

use crate::{
    adapters::{gio_file_for_location, location_for_file},
    assets, i18n,
    model::Location,
    ui::{
        browser::ConnectOutcome,
        controls::{
            form_check_button, form_entry, form_error_label, keep_words_whole,
            modal_layout, set_modal_loading,
        },
        modal::{
            ModalHost, close_on_escape, dismiss_modal_layer, modal_layer, remember_modal_focus,
            submit_on_enter,
        },
    },
};

use super::{SidebarState};

#[cfg(test)]
mod tests;








