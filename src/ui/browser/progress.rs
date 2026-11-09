// SPDX-License-Identifier: MIT

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
