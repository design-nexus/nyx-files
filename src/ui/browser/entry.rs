// SPDX-License-Identifier: MIT

use crate::model::FileEntry;
use crate::services::PathMatcher;
use std::cell::RefCell;

thread_local! {
    static FILTER_TERMS: RefCell<Option<(String, PathMatcher)>> = const { RefCell::new(None) };
}

pub(in crate::ui) fn format_file_size(bytes: u64) -> String {
    crate::i18n::file_size(bytes)
}

pub(in crate::ui) fn metadata_needs_fill(entry: &FileEntry) -> bool {
    entry.modified_unix_seconds == crate::model::MetadataValue::Unknown
        || (!entry.is_directory() && entry.size == crate::model::MetadataValue::Unknown)
}

