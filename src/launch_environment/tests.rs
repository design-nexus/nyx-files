// SPDX-License-Identifier: MIT

use std::ffi::{OsStr, OsString};

use super::{GDK_DEBUG_BEFORE, restored_from};

#[test]
fn programs_get_the_values_files_replaced() {
    assert!(restored_from(None, None, Some(OsStr::new("cairo"))).is_empty());
    assert_eq!(
        restored_from(Some(OsStr::new("")), None, None),
        [("GDK_DEBUG", None), (GDK_DEBUG_BEFORE, None)]
    );
    assert_eq!(
        restored_from(Some(OsStr::new("events")), None, None),
        [
            ("GDK_DEBUG", Some(OsString::from("events"))),
            (GDK_DEBUG_BEFORE, None)
        ]
    );
    assert_eq!(
        restored_from(None, Some(OsStr::new("1")), Some(OsStr::new("cairo"))),
        [
            ("GSK_RENDERER", None),
            (crate::CAIRO_SELECTED_BY_STRATA, None)
        ]
    );
    // A renderer the user chose stays.
    assert!(restored_from(None, Some(OsStr::new("1")), Some(OsStr::new("vulkan"))).is_empty());
}
