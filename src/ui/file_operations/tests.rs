// SPDX-License-Identifier: MIT

use std::collections::HashSet;

use super::*;
use crate::services::TransferReport;

#[derive(Default)]
struct FakeOwner {
    running: RefCell<HashSet<u64>>,
    cancelled: RefCell<Vec<u64>>,
}

impl FakeOwner {
    fn begin(self: &Rc<Self>, request: u64) -> OperationRequestId {
        self.running.borrow_mut().insert(request);
        OperationRequestId(request)
    }

    fn stop(&self, request: OperationRequestId) {
        self.running.borrow_mut().remove(&request.0);
    }
}

impl OperationOwner for FakeOwner {
    fn runs(&self, request: OperationRequestId) -> bool {
        self.running.borrow().contains(&request.0)
    }

    fn cancel(&self, request: OperationRequestId) {
        self.cancelled.borrow_mut().push(request.0);
    }
}

fn owner() -> (Rc<FakeOwner>, Rc<dyn OperationOwner>) {
    let fake = Rc::new(FakeOwner::default());
    let owner: Rc<dyn OperationOwner> = fake.clone();
    (fake, owner)
}

fn copy_started(total: usize) -> BrowserEvent {
    BrowserEvent::TransferStarted {
        total,
        moving: false,
    }
}

fn copy_progress(transferred: u64, total: u64, file: &str) -> BrowserEvent {
    BrowserEvent::TransferProgress {
        completed_items: 0,
        completed_files: 0,
        total_files: Some(2),
        current_file: Some(file.into()),
        transferred_bytes: transferred,
        total_bytes: Some(total),
    }
}

fn subject() -> String {
    "photo.jpg → ~/Pictures".into()
}

fn locked<T>(test: impl FnOnce() -> T) -> T {
    let _serial = crate::test_support::ASYNC_MAIN_CONTEXT_DEFAULT
        .lock()
        .expect("main context lock");
    let result = test();
    while glib::MainContext::default().iteration(false) {}
    result
}

#[test]
fn progress_follows_an_operation_through_to_its_outcome() {
    locked(|| {
        let operations = FileOperations::default();
        let (fake, owner) = owner();
        let request = fake.begin(1);
        operations.record(&owner, request, &copy_started(2), subject);
        let id = operations.front_of(&owner).expect("copy in front");
        operations.record(&owner, request, &copy_progress(50, 200, "a.jpg"), subject);
        let running = operations.snapshot_of(id).expect("copy listed");
        assert_eq!(running.kind, FileOperationKind::Copy);
        assert_eq!(running.status, FileOperationStatus::Running);
        assert!((running.fraction - 0.25).abs() < f64::EPSILON);
        assert_eq!(running.detail, "a.jpg");
        assert_eq!(running.subject, subject());
        assert_eq!(operations.running_count(), 1);

        fake.stop(request);
        operations.record(&owner, request, &BrowserEvent::TransferCompleted, subject);
        let done = operations.snapshot_of(id).expect("copy kept");
        assert_eq!(done.status, FileOperationStatus::Succeeded);
        assert!((done.fraction - 1.0).abs() < f64::EPSILON);
        assert_eq!(operations.running_count(), 0);
        assert_eq!(operations.finished_count(), 1);
        assert_eq!(operations.front_of(&owner), None);
    });
}

#[test]
fn a_backgrounded_operation_keeps_its_progress_and_leaves_the_front() {
    locked(|| {
        let operations = FileOperations::default();
        let (fake, owner) = owner();
        let request = fake.begin(1);
        operations.record(&owner, request, &copy_started(1), subject);
        let id = operations.front_of(&owner).expect("copy in front");
        operations.set_background(id);
        assert_eq!(operations.front_of(&owner), None);
        operations.record(&owner, request, &copy_progress(150, 200, "b.jpg"), subject);
        let snapshot = operations.snapshot_of(id).expect("copy listed");
        assert!(snapshot.background);
        assert!((snapshot.fraction - 0.75).abs() < f64::EPSILON);
        assert_eq!(snapshot.status, FileOperationStatus::Running);
    });
}

#[test]
fn failures_and_cancellations_end_an_operation_once() {
    locked(|| {
        let operations = FileOperations::default();
        let (fake, owner) = owner();
        let failing = fake.begin(1);
        operations.record(&owner, failing, &copy_started(1), subject);
        let failed = operations.front_of(&owner).expect("copy");
        fake.stop(failing);
        operations.record(
            &owner,
            failing,
            &BrowserEvent::OperationFailed {
                message: "Disk full".into(),
                password_failure: None,
                report: TransferReport::default(),
            },
            subject,
        );
        operations.record(&owner, failing, &BrowserEvent::TransferCompleted, subject);
        let snapshot = operations.snapshot_of(failed).expect("failure kept");
        assert_eq!(snapshot.status, FileOperationStatus::Failed);
        assert_eq!(snapshot.detail, "Disk full");
        assert!(operations.has_failures());

        let deleting = fake.begin(2);
        operations.record(
            &owner,
            deleting,
            &BrowserEvent::DeletionStarted { total: 3 },
            subject,
        );
        let deletion = operations.front_of(&owner).expect("deletion");
        operations.cancel(deletion);
        assert_eq!(*fake.cancelled.borrow(), vec![2]);
        assert_eq!(
            operations.snapshot_of(deletion).map(|s| s.status),
            Some(FileOperationStatus::Cancelling)
        );
        fake.stop(deleting);
        operations.record(
            &owner,
            deleting,
            &BrowserEvent::OperationCancelled {
                completed: 1,
                failed: 0,
                not_attempted: 2,
                affected_locations: HashSet::new(),
                report: TransferReport::default(),
            },
            subject,
        );
        assert_eq!(
            operations.snapshot_of(deletion).map(|s| s.status),
            Some(FileOperationStatus::Cancelled)
        );
    });
}

#[test]
fn an_operation_replaced_in_front_is_marked_stopped() {
    locked(|| {
        let operations = FileOperations::default();
        let (fake, owner) = owner();
        let first = fake.begin(1);
        operations.record(&owner, first, &copy_started(1), subject);
        let replaced = operations.front_of(&owner).expect("first copy");
        fake.stop(first);
        let second = fake.begin(2);
        operations.record(&owner, second, &copy_started(4), subject);
        assert_eq!(
            operations.snapshot_of(replaced).map(|s| s.status),
            Some(FileOperationStatus::Cancelled)
        );
        let front = operations.front_of(&owner).expect("second copy");
        assert_ne!(front, replaced);
        assert_eq!(
            operations.snapshot_of(front).map(|s| s.status),
            Some(FileOperationStatus::Running)
        );
    });
}

#[test]
fn operations_of_different_owners_stay_apart() {
    locked(|| {
        let operations = FileOperations::default();
        let (first, first_owner) = owner();
        let (second, second_owner) = owner();
        let shared_request = first.begin(1);
        second.begin(1);
        operations.record(&first_owner, shared_request, &copy_started(1), subject);
        operations.record(
            &second_owner,
            shared_request,
            &BrowserEvent::DeletionStarted { total: 1 },
            subject,
        );
        let deletion = operations.front_of(&second_owner).expect("deletion");
        second.stop(shared_request);
        operations.record(
            &second_owner,
            shared_request,
            &BrowserEvent::DeletionFinished { succeeded: true },
            subject,
        );
        let copy = operations
            .front_of(&first_owner)
            .expect("copy still in front");
        assert_eq!(
            operations.snapshot_of(copy).map(|s| s.status),
            Some(FileOperationStatus::Running)
        );
        assert_eq!(
            operations.snapshot_of(deletion).map(|s| s.status),
            Some(FileOperationStatus::Succeeded)
        );
    });
}

#[test]
fn finished_operations_are_dismissed_cleared_and_bounded() {
    locked(|| {
        let operations = FileOperations::default();
        let (fake, owner) = owner();
        for request in 1..=(HISTORY_LIMIT as u64 + 3) {
            let request = fake.begin(request);
            operations.record(&owner, request, &copy_started(1), subject);
            fake.stop(request);
            operations.record(&owner, request, &BrowserEvent::TransferCompleted, subject);
        }
        assert_eq!(operations.finished_count(), HISTORY_LIMIT);
        let running = fake.begin(100);
        operations.record(&owner, running, &copy_started(1), subject);
        let newest_finished = operations
            .snapshot()
            .into_iter()
            .find(|s| !s.status.is_active())
            .expect("finished copy");
        operations.dismiss(newest_finished.id);
        assert_eq!(operations.finished_count(), HISTORY_LIMIT - 1);
        assert!(operations.clear_finished());
        assert_eq!(operations.finished_count(), 0);
        assert_eq!(operations.running_count(), 1);
        assert_eq!(
            operations.snapshot().first().map(|s| s.status),
            Some(FileOperationStatus::Running)
        );
    });
}
