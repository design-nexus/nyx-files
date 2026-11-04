// SPDX-License-Identifier: MIT

use crate::adapters::directory_summary::{DirectorySummary, summarize_directory};
use crate::adapters::trash::{EmptyTrashOutcome, empty_trash};
use crate::adapters::trash_restore::restore_destinations_for_locations;
use crate::model::{EntryKind, FileEntry, Location};
use crate::services::{LoadHandle, RestoreTrashItem};
use crate::ui::blur::BlurBin;
use crate::ui::browser::ViewState;
use crate::ui::browser::entry::{
    entry_icon, format_file_size, };
use crate::ui::controls::{
    ModalTone, focus_button, message_dialog_description, message_dialog_layout, modal_layout,
};
use crate::ui::keys::{
    self, KeyPress,
    bindings::{ENTER_ANY, ESCAPE_ANY, LEFT, RIGHT, TENXER_DELETE_CONFIRM, VIM_LEFT, VIM_RIGHT},
};
use crate::ui::modal::{
    ModalHost, close_on_escape, dismiss_modal_layer, dismiss_modal_layer_then, modal_layer,
    show_error_dialog,
};
use gtk::gdk::ModifierType as Modifiers;
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

pub(super) struct TrashLoadingView {
    layer: gtk::Box,
    overlay: gtk::Overlay,
    blurred_root: Option<BlurBin>,
}


















impl ViewState {




















}


