// SPDX-License-Identifier: MIT

use std::{cell::Cell, rc::Rc};

use gtk::prelude::*;

use crate::{assets, services::package_manager::PackageManager};

use super::{
    controls::{ModalTone, copyable_command, message_dialog_description, message_dialog_layout},
    modal::{ModalHost, close_on_escape, dismiss_modal_layer, modal_layer, remember_modal_focus},
};




#[cfg(test)]
mod tests;
