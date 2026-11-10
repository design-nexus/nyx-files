// SPDX-License-Identifier: MIT

use super::*;
use crate::test_support::gtk_test;



#[test]
fn a_thumbnail_released_by_an_operation_is_not_remembered_as_failed() {
    gtk_test(
        "ui::thumbnail::tests::a_thumbnail_released_by_an_operation_is_not_remembered_as_failed",
        || {
            let directory = tempfile::tempdir().expect("media folder");
            let links = tempfile::tempdir().expect("link folder");
            let link = links.path().join("media");
            std::os::unix::fs::symlink(directory.path(), &link).expect("linked folder");
            for (row, shown_in, name, refused_after_end) in [
                (
                    "refused while it runs",
                    directory.path(),
                    "running.png",
                    false,
                ),
                (
                    "refused through a linked folder while it runs",
                    link.as_path(),
                    "linked.png",
                    false,
                ),
                (
                    "refused after it ended",
                    directory.path(),
                    "ended.png",
                    true,
                ),
            ] {
                let key = ThumbnailKey {
                    path: shown_in.join(name),
                    modified: None,
                    file_size: None,
                    thumbnail_size: crate::services::thumbnail_cache::CANONICAL_MAX_EDGE,
                };
                let slot = ThumbnailSlot::new(18);
                let request = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
                let target =
                    register_active_request(&slot, slot.as_ptr() as usize, request, key.clone());
                let job = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
                PENDING_THUMBNAILS.with_borrow_mut(|pending| {
                    pending.insert(
                        key.clone(),
                        PendingThumbnail {
                            id: job,
                            kind: ThumbnailKind::Image,
                            cancellation: Cancellation::default(),
                            targets: vec![target.clone()],
                        },
                    );
                });
                let operation = crate::adapters::ActiveOperation::begin([]);
                glib::MainContext::default().block_on(
                    operation.release(vec![directory.path().join(name)], &gio::Cancellable::new()),
                );
                let operation = if refused_after_end {
                    drop(operation);
                    None
                } else {
                    Some(operation)
                };

                glib::MainContext::default().block_on(finish_thumbnail_job(
                    ThumbnailJob {
                        id: job,
                        key: key.clone(),
                        resolved: key.clone(),
                        kind: ThumbnailKind::Image,
                        cancellation: Cancellation::default(),
                    },
                    Err(crate::services::file_activity::HELD_FOR_OPERATION.to_owned()),
                ));

                assert!(
                    THUMBNAIL_CACHE
                        .with_borrow_mut(|cache| cache.get(&key))
                        .is_none(),
                    "{row}: an operation's refusal is no failure to remember"
                );
                assert!(request_is_live(&target), "{row}");
                assert_eq!(
                    PENDING_THUMBNAILS.with_borrow(|pending| pending.contains_key(&key)),
                    refused_after_end,
                    "{row}: asked for again at once only when no operation will end later"
                );
                drop(operation);
                assert!(
                    PENDING_THUMBNAILS.with_borrow(|pending| pending.contains_key(&key)),
                    "{row}: the thumbnail is asked for again once the operation ends"
                );
            }
        },
    );
}
