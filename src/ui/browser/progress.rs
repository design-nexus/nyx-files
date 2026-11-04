// SPDX-License-Identifier: MIT

use crate::ui::blur::BlurBin;
use crate::ui::browser::entry::{format_file_size};
use crate::ui::controls::{modal_layout};
use crate::ui::modal::{
    ModalHost, dismiss_modal_layer, modal_layer, };
use gtk::glib;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

mod context;
mod presentation;
pub(super) use context::{BackgroundProgress, FileProgressState};




#[cfg(test)]
thread_local! {
    static FILE_PROGRESS_DELAY_OVERRIDE: Cell<Option<Duration>> = const { Cell::new(None) };
}


/// Tests of fast-operation feedback must not depend on how quickly a loaded
/// CI filesystem finishes a small copy.
#[cfg(test)]
pub(super) fn set_file_progress_delay_for_test(delay: Duration) {
    FILE_PROGRESS_DELAY_OVERRIDE.with(|cell| cell.set(Some(delay)));
}





#[derive(Clone, Copy)]
pub(super) struct TransferProgressSnapshot {
    completed_items: usize,
    completed_files: usize,
    total_files: Option<usize>,
    transferred_bytes: u64,
    total_bytes: Option<u64>,
}

pub(super) struct FileProgressView {
    layer: RefCell<Option<gtk::Box>>,
    icon_name: String,
    cancel_label: String,
    compact: Option<Rc<crate::ui::progress_dock::CompactProgress>>,
    overlay: gtk::Overlay,
    blurred_root: Option<BlurBin>,
    progress: gtk::ProgressBar,
    status: gtk::Label,
    title: gtk::Label,
    subtitle: gtk::Label,
    cancel: gtk::Button,
    cancel_action: Option<Rc<dyn Fn()>>,
    status_row: gtk::Box,
    transfer_header: gtk::Box,
    transfer_footer: gtk::Box,
    transfer_percent: gtk::Label,
    transfer_bytes: gtk::Label,
    transfer_items: gtk::Label,
    transfer_rate: gtk::Label,
    archive_activity: gtk::Spinner,
    indeterminate: Rc<Cell<bool>>,
    pulse_source: Rc<RefCell<Option<glib::SourceId>>>,
    last_transfer_render: Cell<Option<Instant>>,
}






pub(crate) fn cancelled_operation_summary(
    completed: usize,
    failed: usize,
    not_attempted: usize,
) -> String {
    rust_i18n::t!(
        "%{results}.\n\nCompleted changes were not reverted.",
        results = crate::i18n::capitalize_first(crate::i18n::list([
            crate::i18n::count("items_completed", completed),
            crate::i18n::count("items_failed", failed),
            crate::i18n::count("items_not_attempted", not_attempted),
        ]))
    )
    .into_owned()
}

impl FileProgressState {














    pub(super) fn dismiss_file_operation_progress(&self) {
        self.dismiss_file_operation_progress_then(|| {});
    }

    pub(super) fn dismiss_file_operation_progress_then(
        &self,
        after_dismiss: impl FnOnce() + 'static,
    ) {
        if let Some(source) = self.pending_file_progress.take() {
            source.remove();
        }
        self.on_present.take();
        self.file_operation_progress.set((0, 0));
        if let Some(source) = self.transfer_render_source.take() {
            source.remove();
        }
        self.archive_progress.set(None);
        if let Some(source) = self.transfer_cancel_timeout.take() {
            source.remove();
        }
        self.transfer_cancel_requested.set(false);
        self.transfer_cancel_timed_out.set(false);
        self.transfer_progress.set(None);
        self.transfer_current_file.take();
        self.transfer_rate_sample.set(None);
        self.transfer_rate_bytes_per_second.set(None);
        self.flushing_to_device.set(false);
        self.file_progress_dismiss_waiters
            .borrow_mut()
            .push(Box::new(after_dismiss));
        if let Some(view) = self.file_progress_view.take() {
            view.indeterminate.set(false);
            view.archive_activity.stop();
            if let Some(source) = view.pulse_source.take() {
                source.remove();
            }
            if let Some(compact) = &view.compact {
                compact.remove();
            }
            if let Some(layer) = view.layer.take() {
                let pending = self.file_progress_dismissing.clone();
                let waiters = self.file_progress_dismiss_waiters.clone();
                pending.set(pending.get() + 1);
                let finished = Cell::new(false);
                layer.connect_parent_notify(move |layer| {
                    if layer.parent().is_none() && !finished.replace(true) {
                        pending.set(pending.get() - 1);
                        if pending.get() == 0 {
                            let callbacks = waiters.take();
                            for callback in callbacks {
                                callback();
                            }
                        }
                    }
                });
                dismiss_modal_layer(&layer, &view.overlay, view.blurred_root.as_ref());
            }
        }
        if self.file_progress_dismissing.get() == 0 {
            let callbacks = self.file_progress_dismiss_waiters.take();
            for callback in callbacks {
                callback();
            }
        }
    }


}

