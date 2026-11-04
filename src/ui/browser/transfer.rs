// SPDX-License-Identifier: MIT

use crate::model::Location;
use crate::ui::browser::ViewState;
use gtk::prelude::*;










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
