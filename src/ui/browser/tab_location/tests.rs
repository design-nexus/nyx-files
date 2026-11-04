// SPDX-License-Identifier: MIT

use std::{cell::RefCell, collections::HashMap, ffi::OsStr};

use gtk::prelude::*;

use super::*;
use crate::{
    model::{EntryKind, FileEntry, MetadataValue},
    services::{
        DirectoryChange, DirectoryEvent, DirectoryRequest, FileSource, LoadHandle,
        LocationValidationError,
    },
    test_support::gtk_test,
    ui::browser::PeekBehavior,
};

type Validation = Rc<dyn Fn(Result<(), LocationValidationError>)>;
type Watch = Rc<dyn Fn(DirectoryChange)>;

#[derive(Default)]
struct HeldSource {
    validation: RefCell<Option<Validation>>,
    immediate: RefCell<Option<Result<(), LocationValidationError>>>,
    blocked: RefCell<Option<Location>>,
    cancelled: Rc<RefCell<Vec<Location>>>,
    enumerated: RefCell<Vec<Location>>,
    watchers: RefCell<HashMap<Location, Watch>>,
}

impl FileSource for HeldSource {
    fn allows_navigation(&self, location: &Location) -> bool {
        self.blocked.borrow().as_ref() != Some(location)
    }

    fn validate_location(&self, _: &Location) -> Result<(), LocationValidationError> {
        Ok(())
    }

    fn validate_location_async(&self, location: Location, emit: Validation) -> LoadHandle {
        let immediate = self.immediate.take();
        if let Some(result) = immediate {
            emit(result);
        } else {
            self.validation.replace(Some(emit));
        }
        let cancelled = self.cancelled.clone();
        LoadHandle::new(move || cancelled.borrow_mut().push(location))
    }

    fn enumerate(&self, request: DirectoryRequest, emit: Rc<dyn Fn(DirectoryEvent)>) -> LoadHandle {
        self.enumerated.borrow_mut().push(request.location);
        emit(DirectoryEvent::Finished {
            request_id: request.id,
            truncated: false,
            can_trash: None,
            can_delete: None,
        });
        LoadHandle::new(|| {})
    }

    fn watch(&self, location: Location, _: bool, notify: Watch) -> Option<LoadHandle> {
        self.watchers.borrow_mut().insert(location, notify);
        Some(LoadHandle::new(|| {}))
    }
}

fn renamed_directory(location: Location) -> FileEntry {
    let name = location.display_name();
    FileEntry {
        location,
        thumbnail_path: None,
        native_name: name.clone().into(),
        display_name: name,
        kind: EntryKind::Directory,
        size: MetadataValue::Unknown,
        modified_unix_seconds: MetadataValue::Unknown,
        recent_unix_seconds: MetadataValue::Unknown,
        recent_uri: None,
        mode: MetadataValue::Unknown,
        image_dimensions: MetadataValue::Unknown,
        child_count: MetadataValue::Unknown,
        duration_seconds: MetadataValue::Unknown,
        is_hidden: false,
    }
}




