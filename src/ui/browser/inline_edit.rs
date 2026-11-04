// SPDX-License-Identifier: MIT

use crate::model::{FileEntry, Location};
use crate::services::{OperationRequestId, RequestId, validate_basename};
use crate::ui::browser::ViewState;
use crate::ui::browser::paths::is_trash_location;
use crate::ui::browser_modes::BrowserMode;
use gtk::prelude::*;
use std::rc::Rc;


enum PendingRenameState {
    Queued,
    Dispatching,
    Running(OperationRequestId),
    AwaitingRefresh { requests: Vec<(usize, RequestId)> },
}

pub(super) struct PendingRename {
    old_location: Location,
    new_location: Option<Location>,
    old_name: String,
    new_name: String,
    generation: u64,
    monitor_reported_rename: bool,
    reveal_generation: u64,
    scroll_value: Option<f64>,
    source_position: Option<usize>,
    state: PendingRenameState,
}

impl PendingRename {



}



pub(super) struct PendingEntryRename {
    depth: usize,
    parent: Location,
    reveal_generation: u64,
    move_sources: std::cell::RefCell<Vec<Location>>,
    created: std::cell::RefCell<Option<Location>>,
}




impl super::BrowserView {
}





impl ViewState {









































}
