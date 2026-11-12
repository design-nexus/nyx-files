// SPDX-License-Identifier: MIT

//! Every copy, move, deletion, restore and archive the browsers run, in front
//! or in the background, for the progress card and the Jobs dashboard.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    time::{Duration, Instant},
};

use gtk::glib;

use crate::{
    app::{Browser, BrowserEvent},
    services::{ListenerGuard, Listeners, OperationRequestId},
};

#[cfg(test)]
mod tests;

/// Finished operations kept for the dashboard; older ones are dropped.
const HISTORY_LIMIT: usize = 20;
const SWEEP_INTERVAL: Duration = Duration::from_millis(500);

pub(crate) type Callback = Rc<dyn Fn()>;

/// What runs the operations: a browser, or a fake in tests.
pub(crate) trait OperationOwner {
    fn runs(&self, request: OperationRequestId) -> bool;
    fn cancel(&self, request: OperationRequestId);
}

impl OperationOwner for Browser {
    fn runs(&self, request: OperationRequestId) -> bool {
        self.runs_operation(request)
    }

    fn cancel(&self, request: OperationRequestId) {
        self.cancel_operation(request);
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub(crate) struct FileOperationId(u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FileOperationKind {
    Copy,
    Move,
    Delete,
    Restore,
    Archive,
}

impl FileOperationKind {
    pub(crate) fn verb(self) -> &'static str {
        match self {
            Self::Copy => "Copying",
            Self::Move => "Moving",
            Self::Delete => "Deleting",
            Self::Restore => "Restoring",
            Self::Archive => "Processing archive",
        }
    }

    pub(crate) fn icon(self) -> &'static str {
        match self {
            Self::Copy => crate::assets::icons::COPY,
            Self::Move => "go-jump-symbolic",
            Self::Delete => crate::assets::icons::TRASH,
            Self::Restore => "edit-undo-symbolic",
            Self::Archive => crate::assets::icons::FILE_ARCHIVE,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FileOperationStatus {
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
}

impl FileOperationStatus {
    pub(crate) const fn is_active(self) -> bool {
        matches!(self, Self::Running | Self::Cancelling)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct FileOperationSnapshot {
    pub(crate) id: FileOperationId,
    pub(crate) request: OperationRequestId,
    pub(crate) kind: FileOperationKind,
    pub(crate) status: FileOperationStatus,
    /// "Copying · 3 items".
    pub(crate) title: String,
    /// The first item and the destination, as "photo.jpg → ~/Pictures".
    pub(crate) subject: String,
    /// The file being copied and the rate, or what the operation ended with.
    pub(crate) detail: String,
    pub(crate) fraction: f64,
    pub(crate) elapsed: Duration,
    pub(crate) background: bool,
}

struct Operation {
    id: FileOperationId,
    owner: Weak<dyn OperationOwner>,
    request: OperationRequestId,
    kind: FileOperationKind,
    total: usize,
    subject: String,
    fraction: f64,
    current_file: Option<String>,
    transferred: u64,
    flushing: bool,
    status: FileOperationStatus,
    message: Option<String>,
    started: Instant,
    finished: Option<Instant>,
    background: bool,
}

impl Operation {
    fn belongs_to(&self, owner: &Rc<dyn OperationOwner>, request: OperationRequestId) -> bool {
        self.request == request && std::ptr::addr_eq(self.owner.as_ptr(), Rc::as_ptr(owner))
    }

    fn end(&mut self, status: FileOperationStatus, message: Option<String>) {
        if !self.status.is_active() {
            return;
        }
        self.status = status;
        self.message = message;
        self.finished = Some(Instant::now());
        if status == FileOperationStatus::Succeeded {
            self.fraction = 1.0;
        }
    }

    fn elapsed(&self) -> Duration {
        self.finished
            .unwrap_or_else(Instant::now)
            .saturating_duration_since(self.started)
    }

    fn rate(&self) -> Option<u64> {
        let elapsed = self.started.elapsed().as_secs_f64();
        (elapsed >= 1.0 && self.transferred > 0).then(|| (self.transferred as f64 / elapsed) as u64)
    }

    fn detail(&self) -> String {
        if self.status.is_active() {
            if self.status == FileOperationStatus::Cancelling {
                return crate::i18n::tr("Cancelling…");
            }
            if self.flushing {
                return crate::i18n::tr("Writing to device…");
            }
            let rate = self.rate().map(crate::i18n::transfer_rate);
            return match (&self.current_file, rate) {
                (Some(file), Some(rate)) => format!("{file} · {rate}"),
                (Some(file), None) => file.clone(),
                (None, Some(rate)) => rate,
                (None, None) => String::new(),
            };
        }
        self.message.clone().unwrap_or_default()
    }

    fn snapshot(&self) -> FileOperationSnapshot {
        FileOperationSnapshot {
            id: self.id,
            request: self.request,
            kind: self.kind,
            status: self.status,
            title: format!(
                "{} · {}",
                crate::i18n::tr(self.kind.verb()),
                crate::i18n::count("items", self.total)
            ),
            subject: self.subject.clone(),
            detail: self.detail(),
            fraction: self.fraction,
            elapsed: self.elapsed(),
            background: self.background,
        }
    }
}

#[derive(Default)]
pub(crate) struct FileOperations {
    operations: RefCell<Vec<Operation>>,
    next_id: Cell<u64>,
    listeners: Rc<Listeners<Callback>>,
    notify_pending: Rc<Cell<bool>>,
}

thread_local! {
    // Operations outlive the tab and window that started them.
    static SHARED: RefCell<Option<Rc<FileOperations>>> = const { RefCell::new(None) };
}

pub(crate) fn shared() -> Rc<FileOperations> {
    SHARED.with(|shared| {
        if let Some(operations) = shared.borrow().as_ref() {
            return operations.clone();
        }
        let operations = Rc::new(FileOperations::default());
        shared.replace(Some(operations.clone()));
        let weak = Rc::downgrade(&operations);
        glib::timeout_add_local(SWEEP_INTERVAL, move || {
            let Some(operations) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            operations.sweep();
            operations.notify_running();
            glib::ControlFlow::Continue
        });
        operations
    })
}

/// Records what `browser`'s file operations report, as it reports it.
pub(crate) fn follow(browser: &Rc<Browser>) {
    let weak = Rc::downgrade(browser);
    browser.observe(move |event| {
        let Some(browser) = weak.upgrade() else {
            return;
        };
        let (request, event) = match event {
            BrowserEvent::BackgroundOperation { request_id, event } => {
                (Some(*request_id), &**event)
            }
            event => (browser.observed_operation(), event),
        };
        let Some(request) = request else {
            return;
        };
        let owner: Rc<dyn OperationOwner> = browser.clone();
        shared().record(&owner, request, event, || {
            let (item, destination) = browser.operation_texts();
            [item, destination]
                .into_iter()
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        });
    });
}

impl FileOperations {
    pub(crate) fn observe(&self, callback: Callback) -> ListenerGuard<Callback> {
        self.listeners.add(callback)
    }

    pub(crate) fn record(
        &self,
        owner: &Rc<dyn OperationOwner>,
        request: OperationRequestId,
        event: &BrowserEvent,
        subject: impl FnOnce() -> String,
    ) {
        use FileOperationKind as Kind;
        use FileOperationStatus as Status;
        let started = match event {
            BrowserEvent::TransferStarted { total, moving } => {
                Some((if *moving { Kind::Move } else { Kind::Copy }, *total))
            }
            BrowserEvent::DeletionStarted { total } => Some((Kind::Delete, *total)),
            BrowserEvent::RestorationStarted { total } => Some((Kind::Restore, *total)),
            BrowserEvent::ArchiveStarted { total } => Some((Kind::Archive, *total)),
            _ => None,
        };
        if let Some((kind, total)) = started {
            self.start(owner, request, kind, total, subject());
            return;
        }
        let changed = self.update(owner, request, |operation| match event {
            BrowserEvent::TransferProgress {
                completed_items,
                total_files,
                current_file,
                transferred_bytes,
                total_bytes,
                ..
            } => {
                operation.fraction = match total_bytes {
                    Some(total) if *total > 0 => *transferred_bytes as f64 / *total as f64,
                    _ => match total_files {
                        Some(total) if *total > 0 => *completed_items as f64 / *total as f64,
                        _ => 0.0,
                    },
                }
                .clamp(0.0, 1.0);
                operation.current_file.clone_from(current_file);
                operation.transferred = *transferred_bytes;
                operation.flushing = false;
                true
            }
            BrowserEvent::DeletionProgress { completed, total }
            | BrowserEvent::RestorationProgress { completed, total }
            | BrowserEvent::ArchiveProgress { completed, total } => {
                operation.fraction = if *total == 0 {
                    0.0
                } else {
                    (*completed as f64 / *total as f64).clamp(0.0, 1.0)
                };
                true
            }
            BrowserEvent::FlushingToDevice => {
                operation.flushing = true;
                operation.fraction = 1.0;
                true
            }
            BrowserEvent::TransferCompleted
            | BrowserEvent::ArchiveCompleted { .. }
            | BrowserEvent::DeletionFinished { succeeded: true }
            | BrowserEvent::RestorationFinished { succeeded: true } => {
                operation.end(Status::Succeeded, None);
                true
            }
            BrowserEvent::OperationFailed { message, .. }
            | BrowserEvent::OperationCompletedWithErrors { message, .. } => {
                operation.end(Status::Failed, Some(message.clone()));
                true
            }
            BrowserEvent::OperationCancelled { .. } => {
                operation.end(Status::Cancelled, None);
                true
            }
            _ => false,
        });
        if changed {
            self.enforce_history_limit();
            self.notify();
        }
    }

    fn start(
        &self,
        owner: &Rc<dyn OperationOwner>,
        request: OperationRequestId,
        kind: FileOperationKind,
        total: usize,
        subject: String,
    ) {
        if self.update(owner, request, |operation| {
            operation.total = total;
            true
        }) {
            self.notify();
            return;
        }
        let id = FileOperationId(self.next_id.get().wrapping_add(1));
        self.next_id.set(id.0);
        self.operations.borrow_mut().push(Operation {
            id,
            owner: Rc::downgrade(owner),
            request,
            kind,
            total,
            subject,
            fraction: 0.0,
            current_file: None,
            transferred: 0,
            flushing: false,
            status: FileOperationStatus::Running,
            message: None,
            started: Instant::now(),
            finished: None,
            background: false,
        });
        // A new operation in front stops the one it replaces.
        self.sweep();
        self.notify();
    }

    fn update(
        &self,
        owner: &Rc<dyn OperationOwner>,
        request: OperationRequestId,
        change: impl FnOnce(&mut Operation) -> bool,
    ) -> bool {
        let mut operations = self.operations.borrow_mut();
        operations
            .iter_mut()
            .rev()
            .find(|operation| operation.belongs_to(owner, request))
            .is_some_and(change)
    }

    /// Ends operations that stopped without saying so: replaced by a newer one
    /// in front, or their browser gone.
    fn sweep(&self) {
        let mut changed = false;
        for operation in self.operations.borrow_mut().iter_mut() {
            if !operation.status.is_active() {
                continue;
            }
            let runs = operation
                .owner
                .upgrade()
                .is_some_and(|owner| owner.runs(operation.request));
            if !runs {
                operation.end(
                    FileOperationStatus::Cancelled,
                    Some(crate::i18n::tr("Stopped before it finished")),
                );
                changed = true;
            }
        }
        if changed {
            self.enforce_history_limit();
            self.notify();
        }
    }

    /// Running rows show elapsed time and rate, which change without events.
    fn notify_running(&self) {
        if self.running_count() > 0 {
            self.notify();
        }
    }

    pub(crate) fn set_background(&self, id: FileOperationId) {
        if let Some(operation) = self
            .operations
            .borrow_mut()
            .iter_mut()
            .find(|operation| operation.id == id)
        {
            operation.background = true;
        }
        self.notify();
    }

    pub(crate) fn cancel(&self, id: FileOperationId) {
        let target = {
            let mut operations = self.operations.borrow_mut();
            let Some(operation) = operations.iter_mut().find(|operation| {
                operation.id == id && operation.status == FileOperationStatus::Running
            }) else {
                return;
            };
            operation.status = FileOperationStatus::Cancelling;
            (operation.owner.clone(), operation.request)
        };
        if let Some(owner) = target.0.upgrade() {
            owner.cancel(target.1);
        }
        self.notify();
    }

    pub(crate) fn dismiss(&self, id: FileOperationId) {
        self.operations
            .borrow_mut()
            .retain(|operation| operation.id != id || operation.status.is_active());
        self.notify();
    }

    pub(crate) fn clear_finished(&self) -> bool {
        let mut operations = self.operations.borrow_mut();
        let before = operations.len();
        operations.retain(|operation| operation.status.is_active());
        let removed = operations.len() != before;
        drop(operations);
        if removed {
            self.notify();
        }
        removed
    }

    pub(crate) fn running_count(&self) -> usize {
        self.operations
            .borrow()
            .iter()
            .filter(|operation| operation.status.is_active())
            .count()
    }

    pub(crate) fn finished_count(&self) -> usize {
        self.operations.borrow().len() - self.running_count()
    }

    pub(crate) fn has_failures(&self) -> bool {
        self.operations
            .borrow()
            .iter()
            .any(|operation| operation.status == FileOperationStatus::Failed)
    }

    /// The newest operation `owner` runs in front.
    pub(crate) fn front_of(&self, owner: &Rc<dyn OperationOwner>) -> Option<FileOperationId> {
        self.operations
            .borrow()
            .iter()
            .rev()
            .find(|operation| {
                operation.status.is_active()
                    && !operation.background
                    && std::ptr::addr_eq(operation.owner.as_ptr(), Rc::as_ptr(owner))
            })
            .map(|operation| operation.id)
    }

    pub(crate) fn snapshot_of(&self, id: FileOperationId) -> Option<FileOperationSnapshot> {
        self.operations
            .borrow()
            .iter()
            .find(|operation| operation.id == id)
            .map(Operation::snapshot)
    }

    /// Running operations first, newest first, then finished ones, newest first.
    pub(crate) fn snapshot(&self) -> Vec<FileOperationSnapshot> {
        let operations = self.operations.borrow();
        let mut snapshots: Vec<_> = operations.iter().map(Operation::snapshot).collect();
        snapshots
            .sort_by_key(|snapshot| (!snapshot.status.is_active(), std::cmp::Reverse(snapshot.id)));
        snapshots
    }

    fn enforce_history_limit(&self) {
        let mut operations = self.operations.borrow_mut();
        let mut finished = operations
            .iter()
            .filter(|operation| !operation.status.is_active())
            .count();
        operations.retain(|operation| {
            if finished > HISTORY_LIMIT && !operation.status.is_active() {
                finished -= 1;
                return false;
            }
            true
        });
    }

    /// Listeners hear once per main-loop turn, however many events came in it.
    fn notify(&self) {
        if self.notify_pending.replace(true) {
            return;
        }
        let pending = self.notify_pending.clone();
        let listeners = Rc::downgrade(&self.listeners);
        glib::idle_add_local_once(move || {
            pending.set(false);
            if let Some(listeners) = listeners.upgrade() {
                listeners.notify(|callback| callback());
            }
        });
    }
}
