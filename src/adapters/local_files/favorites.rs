// SPDX-License-Identifier: MIT

//! The Favorites view: each saved file and folder that can be reached now. One that
//! can't (an unplugged drive) stays saved and is listed again once it's back.

use super::*;

pub(super) fn enumerate(
    request: DirectoryRequest,
    emit: Rc<dyn Fn(DirectoryEvent)>,
    started: Instant,
) -> LoadHandle {
    let request_id = request.id;
    let task = glib::MainContext::default().spawn_local(async move {
        let deadline = started.checked_add(request.time_budget);
        let attributes = if request.include_metadata {
            FULL_ATTRIBUTES
        } else {
            LIST_ATTRIBUTES
        };
        let favorites = crate::adapters::favorites::current();
        let mut truncated = favorites.len() > request.max_entries;
        let context = glib::MainContext::default();
        // Concurrently, so one unreachable share can't hold up the rest.
        let pending: Vec<_> = favorites
            .into_iter()
            .take(request.max_entries)
            .map(|location| {
                context.spawn_local(async move {
                    let query = query_entry(location, attributes, gio::FileQueryInfoFlags::NONE);
                    match deadline {
                        Some(deadline) => glib::future_with_timeout(
                            deadline.saturating_duration_since(Instant::now()),
                            query,
                        )
                        .await
                        .map_err(|_| ()),
                        None => Ok(query.await),
                    }
                })
            })
            .collect();
        let mut entries = Vec::new();
        for handle in pending {
            match handle.await {
                Ok(Ok(Ok(entry))) => entries.push(entry),
                Ok(Err(())) => truncated = true,
                _ => {}
            }
        }
        if !entries.is_empty() {
            emit(DirectoryEvent::Batch {
                request_id,
                entries,
            });
        }
        emit(DirectoryEvent::Finished {
            request_id,
            truncated,
            can_trash: None,
            can_delete: None,
        });
    });
    LoadHandle::new(move || task.abort())
}

/// The view reloads whenever the list of favorites changes.
pub(super) fn watch(notify: Rc<dyn Fn(DirectoryChange)>, report: Rc<dyn Fn(Watching)>) -> LoadHandle {
    let watch = crate::adapters::favorites::watch_changes(move |_| notify(DirectoryChange::Rescan));
    report(Watching::Live);
    LoadHandle::new(move || drop(watch))
}
