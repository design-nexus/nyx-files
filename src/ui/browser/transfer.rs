// SPDX-License-Identifier: MIT

use crate::adapters::probes::{self, Presence};
use crate::adapters::{gio_file_for_location, transfer_targets};
use crate::app::Browser;
use crate::model::{FileEntry, Location};
use crate::services::{
    DropCommit, MoveRecord, PasteItem, ShownName, TransferConflict, UndoMoveItem, VolumeRelation,
    transferable_drop_sources,
};
use crate::ui::browser::ViewState;
use crate::ui::browser::paths::{is_trash_location};
use crate::ui::browser::progress::FileProgressState;
use crate::ui::browser::transfer_report::shown_name;
use crate::ui::controls::{
    ModalTone, focus_button, form_check_button, message_dialog_description, message_dialog_layout,
    };
use crate::ui::keys::{
    self,
    bindings::{ENTER_ANY, ESCAPE_ANY},
};
use crate::ui::modal::{ModalHost, dismiss_modal_layer, modal_layer, show_error_dialog};
use futures_lite::future::BoxedLocal;
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;










#[derive(Clone)]
pub(super) struct PendingSendToCompletion {
    pub(super) device_name: String,
    pub(super) item_count: usize,
}

#[derive(Clone)]
pub(super) struct FinishedSendToCompletion {
    pub(super) completion: PendingSendToCompletion,
    pub(super) progress_shown: bool,
}




#[derive(PartialEq)]
pub(super) enum Preparing {
    Transfer(Location, Vec<Location>, bool),
    Replay { redo: bool, generation: u64 },
}











impl ViewState {
































}
