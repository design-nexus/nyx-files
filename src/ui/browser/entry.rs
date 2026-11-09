// SPDX-License-Identifier: MIT

use crate::model::FileEntry;
use crate::services::{
    PathMatcher, PathQuery, filter_name_matches, fold_for_search,
};
use gtk::prelude::*;
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




pub(super) fn model_display_name(value: &str) -> &str {
    value.split_once('\t').map_or(value, |(_, name)| name)
}


pub(in crate::ui) fn model_is_hidden(value: &str) -> bool {
    value.as_bytes().get(1) == Some(&b'h')
}





/// `query` must already be folded through `fold_for_search` by the caller.
pub(super) fn entry_matches(value: &str, show_hidden: bool, query: &str) -> bool {
    (show_hidden || !model_is_hidden(value))
        && (query.trim().is_empty() || {
            let name = fold_for_search(model_display_name(value));
            if crate::ui::tenxer_mode::chrome_suppressed() {
                with_filter_terms(query, |terms| terms.score(&name, 0).is_some())
            } else {
                filter_name_matches(&name, query)
            }
        })
}

pub(super) fn with_filter_terms<R>(query: &str, apply: impl FnOnce(&mut PathMatcher) -> R) -> R {
    FILTER_TERMS.with_borrow_mut(|cached| {
        if cached.as_ref().is_none_or(|(cached, _)| cached != query) {
            *cached = Some((query.to_owned(), PathMatcher::new(&PathQuery::parse(query))));
        }
        let (_, matcher) = cached.as_mut().expect("filter terms cached above");
        apply(matcher)
    })
}
