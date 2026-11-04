// SPDX-License-Identifier: MIT

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gtk::{gio::prelude::CancellableExt, glib};

use crate::{
    app::BrowserEvent,
    services::OperationRequestId,
    ui::{
        browser::{ViewState, transfer::PendingSendToCompletion},
        modal::show_error_dialog,
    },
};

use super::{FileProgressView, TransferProgressSnapshot};

type DismissWaiters = Rc<RefCell<Vec<Box<dyn FnOnce()>>>>;

pub(in crate::ui::browser) struct FileProgressState {
    pub(in crate::ui::browser) overlay: gtk::Overlay,
    pub(in crate::ui::browser) file_progress_view: RefCell<Option<FileProgressView>>,
    pub(super) file_progress_dismissing: Rc<Cell<usize>>,
    pub(super) file_progress_dismiss_waiters: DismissWaiters,
    pub(in crate::ui::browser) pending_file_progress: RefCell<Option<glib::SourceId>>,
    pub(super) on_present: RefCell<Option<Box<dyn FnOnce()>>>,
    pub(in crate::ui::browser) file_operation_progress: Cell<(usize, usize)>,
    pub(in crate::ui::browser) archive_progress: Cell<Option<(usize, usize)>>,
    pub(in crate::ui::browser) archive_compressing: Cell<bool>,
    pub(in crate::ui::browser) deleting: Cell<bool>,
    pub(in crate::ui::browser) transfer_progress: Cell<Option<TransferProgressSnapshot>>,
    pub(super) transfer_render_source: RefCell<Option<glib::SourceId>>,
    pub(in crate::ui::browser) transfer_current_file: RefCell<Option<String>>,
    pub(in crate::ui::browser) transfer_rate_sample: Cell<Option<(std::time::Instant, u64)>>,
    pub(in crate::ui::browser) transfer_rate_bytes_per_second: Cell<Option<f64>>,
    pub(in crate::ui::browser) flushing_to_device: Cell<bool>,
    pub(in crate::ui::browser) transfer_cancel_requested: Cell<bool>,
    pub(in crate::ui::browser) transfer_cancel_timed_out: Cell<bool>,
    pub(in crate::ui::browser) transfer_cancel_timeout: RefCell<Option<glib::SourceId>>,
    pub(super) dock_only: Cell<bool>,
    pub(super) task_description: RefCell<String>,
    pub(super) destination_description: RefCell<String>,
    pub(super) destination_label: RefCell<String>,
}

impl FileProgressState {
}

impl Drop for FileProgressState {
    fn drop(&mut self) {
        self.dismiss_file_operation_progress();
    }
}

impl crate::ui::browser::BrowserView {
}

pub(in crate::ui::browser) struct BackgroundProgress {
    pub(in crate::ui::browser) progress: Rc<FileProgressState>,
    send_to: Option<PendingSendToCompletion>,
    delete_entries: Vec<crate::model::FileEntry>,
    trash_animation: RefCell<Option<crate::ui::browser::fly_to_trash::PreparedFlight>>,
    archive_destination: Option<crate::model::Location>,
}


impl ViewState {








}
