// SPDX-License-Identifier: MIT

use std::ffi::OsString;

use super::*;
use crate::model::{Location, MetadataValue};

fn entry(kind: EntryKind) -> FileEntry {
    FileEntry {
        location: Location::local("/tmp/entry"),
        native_name: OsString::from("entry"),
        thumbnail_path: None,
        display_name: "entry".to_owned(),
        kind,
        size: MetadataValue::Unknown,
        modified_unix_seconds: MetadataValue::Unknown,
        mode: MetadataValue::Unknown,
        recent_unix_seconds: MetadataValue::Unknown,
        is_hidden: false,
        image_dimensions: MetadataValue::Unknown,
        child_count: MetadataValue::Unknown,
        duration_seconds: MetadataValue::Unknown,
        recent_uri: None,
    }
}



